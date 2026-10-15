//! Privileged authentication and session lifetime, separate from the renderer.
mod pam;
mod worker;
mod ipc;
mod settings;
mod audio;
mod portals;
use std::io::Write;
use std::os::{fd::OwnedFd, unix::net::UnixStream};
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

pub(crate) const BUS_NAME: &str = "org.telorgon.Session1";
const PATH: &str = "/org/telorgon/Session";
const SHELL_NAME: &str = "org.telorgon.SystemShell1";
const SHELL_PATH: &str = "/org/telorgon/SystemShell";

pub fn run() -> telorgon::Result<()> {
    worker::require_root().map_err(telorgon::AppError::new)?;
    // A crashed broker closes worker pipes. Also revoke any surviving scope
    // before accepting another login; only sessions from our PAM service qualify.
    let cleanup = zbus::blocking::Connection::system().map_err(|error| telorgon::AppError::new(error.to_string()))?;
    let logind = zbus::blocking::Proxy::new(&cleanup, "org.freedesktop.login1", "/org/freedesktop/login1", "org.freedesktop.login1.Manager")
        .map_err(|error| telorgon::AppError::new(error.to_string()))?;
    let sessions: Vec<(String, u32, String, String, zbus::zvariant::OwnedObjectPath)> = logind.call("ListSessions", &())
        .map_err(|error| telorgon::AppError::new(error.to_string()))?;
    for (identity, _, _, _, path) in sessions {
        let session = zbus::blocking::Proxy::new(&cleanup, "org.freedesktop.login1", path, "org.freedesktop.login1.Session")
            .map_err(|error| telorgon::AppError::new(error.to_string()))?;
        if session.get_property::<String>("Service").ok().as_deref() == Some("telorgon-session") {
            logind.call::<_, _, ()>("TerminateSession", &identity).map_err(|error| telorgon::AppError::new(error.to_string()))?;
        }
    }
    let state = Arc::new(Mutex::new(None::<Active>));
    let connection = zbus::blocking::connection::Builder::system()
        .and_then(|builder| builder.name(BUS_NAME))
        .and_then(|builder| builder.serve_at(PATH, Manager(state.clone())))
        .and_then(|builder| builder.build())
        .map_err(|error| telorgon::AppError::new(error.to_string()))?;
    let _ = shell(&connection).and_then(|proxy| proxy.call::<_, _, ()>("ShowLogin", &()));
    loop {
        std::thread::sleep(Duration::from_millis(250));
        let Ok(mut state) = state.try_lock() else { continue };
        let Some(active) = state.as_mut() else { continue };
        let bus = zbus::blocking::fdo::DBusProxy::new(&connection);
        let owner_alive = bus.and_then(|bus| Ok(bus.name_has_owner(active.owner.as_str().try_into()?)?)).unwrap_or(false);
        if !owner_alive || !matches!(active.child.try_wait(), Ok(None)) {
            if let Some(active) = state.take() { active.stop(&connection); }
            let _ = shell(&connection).and_then(|proxy| proxy.call::<_, _, ()>("ShowLogin", &()));
        }
    }
}

pub fn worker(user: &str) -> telorgon::Result<()> {
    worker::run(user).map_err(telorgon::AppError::new)
}
pub fn user_session() -> telorgon::Result<()> {
    worker::user_session().map_err(telorgon::AppError::new)
}

pub fn set_password(user: &str) -> telorgon::Result<()> {
    worker::require_root().map_err(telorgon::AppError::new)?;
    worker::Account::allowed(user).map_err(telorgon::AppError::new)?;
    use std::io::Read;
    let mut secret = pam::Secret(Vec::new());
    std::io::stdin().lock().take(514).read_to_end(&mut secret.0)
        .map_err(|error| telorgon::AppError::new(error.to_string()))?;
    if secret.0.last() == Some(&b'\n') { secret.0.pop(); }
    if !(8..=512).contains(&secret.0.len())
        || std::str::from_utf8(&secret.0).map_or(true, |value| value.chars().any(char::is_control)) {
        return Err(telorgon::AppError::new("Invalid new password"));
    }
    pam::Session::set_password(user, secret).map_err(telorgon::AppError::new)
}

struct Active {
    child: Child,
    input: Option<ChildStdin>,
    session: String,
    owner: String,
    output: UnixStream,
    applications: Vec<(String, String)>,
    settings: Option<settings::Bridge>,
}
impl Active {
    fn stop(mut self, connection: &zbus::blocking::Connection) {
        // EOF also closes the session if the broker crashes or is stopped.
        self.settings.take();
        self.input.take();
        let deadline = Instant::now() + Duration::from_secs(5);
        while matches!(self.child.try_wait(), Ok(None)) && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(25));
        }
        // logind owns the entire session scope, including any remaining children.
        if let Ok(proxy) = zbus::blocking::Proxy::new(connection, "org.freedesktop.login1", "/org/freedesktop/login1", "org.freedesktop.login1.Manager") {
            let _ = proxy.call::<_, _, ()>("TerminateSession", &self.session);
        }
        if matches!(self.child.try_wait(), Ok(None)) { let _ = self.child.kill(); }
        let _ = self.child.wait();
    }
}
struct Manager(Arc<Mutex<Option<Active>>>);

async fn authorize(connection: &zbus::Connection, header: &zbus::message::Header<'_>) -> zbus::fdo::Result<String> {
    let sender = header.sender().ok_or_else(|| denied("Missing caller identity"))?;
    let uid = zbus::fdo::DBusProxy::new(connection).await?.get_connection_unix_user(sender.clone().into()).await?;
    let renderer = worker::Account::local("telorgon-shell").map_err(|_| denied("Renderer account unavailable"))?;
    if uid != 0 && uid != renderer.uid { return Err(denied("Only the system shell may manage sessions")); }
    Ok(sender.to_string())
}
fn denied(message: &str) -> zbus::fdo::Error { zbus::fdo::Error::AccessDenied(message.into()) }
fn shell(connection: &zbus::blocking::Connection) -> zbus::Result<zbus::blocking::Proxy<'_>> {
    zbus::blocking::Proxy::new(connection, SHELL_NAME, SHELL_PATH, SHELL_NAME)
}

#[zbus::interface(name = "org.telorgon.Session1")]
impl Manager {
    async fn login(&self, user: String, password: String,
        #[zbus(connection)] connection: &zbus::Connection,
        #[zbus(header)] header: zbus::message::Header<'_>,
    ) -> zbus::fdo::Result<()> {
        let _caller = authorize(connection, &header).await?;
        let secret = pam::Secret(password.into_bytes());
        if secret.0.is_empty() || secret.0.len() > 512 || secret.0.contains(&0) { return Err(denied("Sign-in failed")); }
        let state = self.0.clone();
        blocking::unblock(move || {
            let mut state = state.try_lock().map_err(|_| denied("A session operation is already in progress"))?;
            if state.is_some() { return Err(denied("A user session is already active")); }
            let account = worker::Account::allowed(&user).map_err(|_| denied("Sign-in failed"))?;
            let connection = zbus::blocking::Connection::system().map_err(|_| denied("Session service unavailable"))?;
            let bus = zbus::blocking::fdo::DBusProxy::new(&connection).map_err(|_| denied("Session service unavailable"))?;
            let owner = bus.get_name_owner(SHELL_NAME.try_into().unwrap()).map_err(|_| denied("System shell unavailable"))?.to_string();
            let (mut output, child_output) = UnixStream::pair().map_err(|_| denied("Session channel unavailable"))?;
            output.set_read_timeout(Some(Duration::from_secs(20))).map_err(|_| denied("Session channel unavailable"))?;
            let mut child = Command::new("/usr/bin/telorgon-test-shell").args(["--session-worker", &user])
                .env_clear().env("PATH", "/usr/bin:/bin")
                .stdin(Stdio::piped()).stdout(Stdio::from(OwnedFd::from(child_output))).stderr(Stdio::inherit())
                .spawn().map_err(|_| denied("Session worker unavailable"))?;
            let mut input = child.stdin.take().unwrap();
            let sent = input.write_all(&(secret.0.len() as u32).to_be_bytes()).and_then(|()| input.write_all(&secret.0));
            drop(secret);
            let received = (|| {
                let listener = ipc::receive_fd(&output)?;
                let display = ipc::receive_fd(&output)?;
                let ready = ipc::receive(&mut output)?;
                let (session, applications, configuration): (String, Vec<(String, String)>, String) = serde_json::from_value(ready)?;
                Ok::<_, std::io::Error>((listener, display, session, applications, configuration))
            })();
            let (listener, display_channel, session, applications, configuration) = match received {
                Ok(ready) if sent.is_ok() => ready,
                _ => {
                drop(input);
                let deadline = Instant::now() + Duration::from_secs(5);
                while matches!(child.try_wait(), Ok(None)) && Instant::now() < deadline {
                    std::thread::sleep(Duration::from_millis(25));
                }
                if matches!(child.try_wait(), Ok(None)) { let _ = child.kill(); }
                let _ = child.wait();
                return Err(denied("Sign-in failed"));
                }
            };
            let mut active = Active { child, input: Some(input), session: session.clone(), owner, output, applications: applications.clone(), settings: None };
            let listener = zbus::zvariant::OwnedFd::from(listener);
            if shell(&connection).and_then(|proxy| proxy.call::<_, _, ()>("AttachSession", &(user, account.uid, &session, listener, applications, configuration))).is_err() {
                active.stop(&connection);
                let _ = shell(&connection).and_then(|proxy| proxy.call::<_, _, ()>("ShowLogin", &()));
                return Err(denied("Could not attach the authenticated desktop"));
            }
            match settings::Bridge::start(UnixStream::from(display_channel), session) {
                Ok(bridge) => active.settings = Some(bridge),
                Err(_) => {
                    active.stop(&connection);
                    let _ = shell(&connection).and_then(|proxy| proxy.call::<_, _, ()>("ShowLogin", &()));
                    return Err(denied("Could not attach desktop settings"));
                }
            }
            *state = Some(active);
            Ok(())
        }).await
    }

    async fn launch(&self, application: String,
        #[zbus(connection)] connection: &zbus::Connection,
        #[zbus(header)] header: zbus::message::Header<'_>,
    ) -> zbus::fdo::Result<()> {
        authorize(connection, &header).await?;
        let state = self.0.clone();
        blocking::unblock(move || {
            let mut state = state.try_lock().map_err(|_| denied("A session operation is already in progress"))?;
            let active = state.as_mut().ok_or_else(|| denied("No authenticated session"))?;
            if !active.applications.iter().any(|(id, _)| id == &application) { return Err(denied("Unknown application")); }
            let input = active.input.as_mut().ok_or_else(|| denied("Session is closing"))?;
            let result = ipc::send(input, &serde_json::json!(application))
                .and_then(|()| ipc::receive(&mut active.output));
            let result = match result {
                Ok(result) => result,
                Err(_) => {
                    if let Ok(connection) = zbus::blocking::Connection::system() {
                        if let Some(active) = state.take() { active.stop(&connection); }
                        let _ = shell(&connection).and_then(|proxy| proxy.call::<_, _, ()>("ShowLogin", &()));
                    }
                    return Err(denied("Session channel failed"));
                }
            };
            if result == serde_json::json!(true) { Ok(()) } else { Err(denied("Application launch failed")) }
        }).await
    }

    async fn logout(&self,
        #[zbus(connection)] connection: &zbus::Connection,
        #[zbus(header)] header: zbus::message::Header<'_>,
    ) -> zbus::fdo::Result<()> {
        authorize(connection, &header).await?;
        let state = self.0.clone();
        blocking::unblock(move || {
            let connection = zbus::blocking::Connection::system().map_err(|_| denied("Session service unavailable"))?;
            let mut state = state.try_lock().map_err(|_| denied("A session operation is already in progress"))?;
            if let Some(active) = state.take() { active.stop(&connection); }
            shell(&connection).and_then(|proxy| proxy.call::<_, _, ()>("ShowLogin", &())).map_err(|_| denied("System shell unavailable"))
        }).await
    }
}

pub(crate) fn login(user: &str, password: String) -> Result<(), String> {
    let connection = zbus::blocking::Connection::system().map_err(|error| error.to_string())?;
    let proxy = zbus::blocking::Proxy::new(&connection, BUS_NAME, PATH, BUS_NAME).map_err(|error| error.to_string())?;
    proxy.call::<_, _, ()>("Login", &(user, password)).map_err(|_| "Sign-in failed. Check your password and try again.".into())
}
pub(crate) fn logout() -> Result<(), String> {
    let connection = zbus::blocking::Connection::system().map_err(|error| error.to_string())?;
    let proxy = zbus::blocking::Proxy::new(&connection, BUS_NAME, PATH, BUS_NAME).map_err(|error| error.to_string())?;
    proxy.call::<_, _, ()>("Logout", &()).map_err(|_| "Could not finish signing out.".into())
}
pub(crate) fn launch(application: &str) -> Result<(), String> {
    let connection = zbus::blocking::Connection::system().map_err(|error| error.to_string())?;
    let proxy = zbus::blocking::Proxy::new(&connection, BUS_NAME, PATH, BUS_NAME).map_err(|error| error.to_string())?;
    proxy.call::<_, _, ()>("Launch", &(application,)).map_err(|_| "Could not open the application.".into())
}
