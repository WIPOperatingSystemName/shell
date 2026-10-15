use super::{ipc, pam::{Secret, Session}};
use crate::system_shell::accounts;
use std::ffi::{CString, c_char, c_int};
use std::io::Read;
use std::os::fd::{AsFd, AsRawFd, OwnedFd};
use std::os::unix::net::{UnixListener, UnixStream};
use std::os::unix::{fs::{MetadataExt, PermissionsExt}, process::CommandExt};
use std::process::{Command, Stdio};

unsafe extern "C" {
    fn geteuid() -> u32;
    fn initgroups(name: *const c_char, group: u32) -> c_int;
    fn setgid(group: u32) -> c_int;
    fn setuid(user: u32) -> c_int;
}
pub(super) fn require_root() -> Result<(), String> {
    if unsafe { geteuid() } == 0 { Ok(()) } else { Err("Session management requires root".into()) }
}

pub(super) struct Account {
    pub uid: u32,
    gid: u32,
    home: String,
}
impl Account {
    pub fn local(name: &str) -> Result<Self, String> {
        let passwd = std::fs::read_to_string("/etc/passwd").map_err(|e| e.to_string())?;
        let records: Vec<_> = passwd.lines().map(|line| line.split(':').collect::<Vec<_>>())
            .filter(|fields| fields.first() == Some(&name)).collect();
        let [fields] = records.as_slice() else { return Err("Ambiguous local account".into()) };
        if fields.len() != 7 { return Err("Invalid local account".into()); }
        Ok(Self {
            uid: fields[2].parse().map_err(|_| "Invalid UID")?,
            gid: fields[3].parse().map_err(|_| "Invalid GID")?,
            home: fields[5].into(),
        })
    }
    pub fn allowed(name: &str) -> Result<Self, String> {
        if !accounts::load()?.iter().any(|user| user.name == name) { return Err("User is not allowed to sign in".into()); }
        let account = Self::local(name)?;
        if account.uid == 0 || account.gid == 0 || !account.home.starts_with('/') { return Err("Invalid login account".into()); }
        Ok(account)
    }
}

pub(super) fn run(user: &str) -> Result<(), String> {
    require_root()?;
    let account = Account::allowed(user)?;
    let mut input = std::io::stdin().lock();
    let mut length = [0; 4];
    input.read_exact(&mut length).map_err(|e| e.to_string())?;
    let length = u32::from_be_bytes(length) as usize;
    if !(1..=512).contains(&length) { return Err("Invalid credential length".into()); }
    let mut secret = Secret(vec![0; length]);
    input.read_exact(&mut secret.0).map_err(|e| e.to_string())?;
    if secret.0.contains(&0) { return Err("Invalid credential".into()); }
    let session = Session::open(user, secret)?;
    let runtime = session.environment(c"XDG_RUNTIME_DIR")?;
    let identity = session.environment(c"XDG_SESSION_ID")?;
    if runtime != format!("/run/user/{}", account.uid)
        || identity.is_empty() || !identity.bytes().all(|byte| byte.is_ascii_alphanumeric()) {
        return Err("Unexpected PAM session identity".into());
    }
    let name = CString::new(user).map_err(|_| "Invalid user")?;
    let (mut child_channel, child_output) = UnixStream::pair().map_err(|e| e.to_string())?;
    child_channel.set_read_timeout(Some(std::time::Duration::from_secs(15))).map_err(|e| e.to_string())?;
    let mut command = Command::new("/usr/bin/telorgon-test-shell");
    command.arg("--user-session").env_clear()
        .env("PATH", "/usr/bin:/bin").env("HOME", &account.home).env("USER", user).env("LOGNAME", user)
        .env("XDG_RUNTIME_DIR", &runtime).env("DBUS_SESSION_BUS_ADDRESS", format!("unix:path={runtime}/bus"))
        .env("XDG_SESSION_ID", &identity).env("XDG_SESSION_TYPE", "wayland")
        .env("WAYLAND_DISPLAY", format!("telorgon-{identity}"))
        .env("XDG_SESSION_DESKTOP", "telorgon").env("XDG_CURRENT_DESKTOP", "telorgon")
        .env("XDG_SEAT", "seat0").env("XDG_VTNR", "1")
        .current_dir(&account.home).stdin(Stdio::piped()).stdout(Stdio::from(OwnedFd::from(child_output))).stderr(Stdio::inherit());
    // This fresh single-purpose worker never owns a renderer or a D-Bus executor.
    // Set supplementary groups before irreversibly dropping both GID and UID.
    unsafe {
        command.pre_exec(move || {
            if initgroups(name.as_ptr(), account.gid) != 0 || setgid(account.gid) != 0 || setuid(account.uid) != 0 {
                return Err(std::io::Error::last_os_error());
            }
            Ok(())
        });
    }
    let mut child = command.spawn().map_err(|e| e.to_string())?;
    let ready = (|| {
        let listener = ipc::receive_fd(&child_channel)?;
        let display = ipc::receive_fd(&child_channel)?;
        let applications = ipc::receive(&mut child_channel)?;
        let (applications, configuration): (Vec<(String, String)>, String) = serde_json::from_value(applications)?;
        Ok::<_, std::io::Error>((listener, display, applications, configuration))
    })();
    let (listener, display, applications, configuration) = match ready {
        Ok(ready) => ready,
        Err(_) => {
        drop(child.stdin.take());
        let _ = child.kill();
        let _ = child.wait();
        return Err("User session did not become ready".into());
        }
    };
    let mut output = UnixStream::from(std::io::stdout().as_fd().try_clone_to_owned().map_err(|e| e.to_string())?);
    ipc::send_fd(&output, &listener).map_err(|e| e.to_string())?;
    ipc::send_fd(&output, &display).map_err(|e| e.to_string())?;
    drop(listener);
    drop(display);
    ipc::send(&mut output, &serde_json::json!((identity, applications, configuration))).map_err(|e| e.to_string())?;
    drop(input);
    let (stop, stopped) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let mut input = std::io::stdin().lock();
        loop {
            let message = ipc::receive(&mut input);
            let end = message.is_err();
            if stop.send(message).is_err() || end { break; }
        }
    });
    loop {
        match stopped.recv_timeout(std::time::Duration::from_millis(250)) {
            Ok(Ok(command)) => {
                let result = child.stdin.as_mut().ok_or_else(|| std::io::Error::other("User session is closing"))
                    .and_then(|input| ipc::send(input, &command))
                    .and_then(|()| ipc::receive(&mut child_channel));
                if ipc::send(&mut output, &result.unwrap_or(serde_json::json!(false))).is_err() { break; }
            }
            Ok(Err(_)) | Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => break,
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {}
        }
        if !matches!(child.try_wait(), Ok(None)) { break; }
    }
    drop(child.stdin.take());
    let _ = child.wait();
    drop(session); // Close PAM before the manager releases the logind scope.
    Ok(())
}

pub(super) fn user_session() -> Result<(), String> {
    let uid = unsafe { geteuid() };
    if uid == 0 { return Err("The user session must be unprivileged".into()); }
    // Preserve the inherited protocol endpoints as close-on-exec descriptors.
    // Applications must never inherit the helper's commands or write replies.
    let mut input = std::fs::File::from(std::io::stdin().as_fd().try_clone_to_owned().map_err(|e| e.to_string())?);
    let mut output = UnixStream::from(std::io::stdout().as_fd().try_clone_to_owned().map_err(|e| e.to_string())?);
    let null = std::fs::File::open("/dev/null").map_err(|e| e.to_string())?;
    if unsafe { libc::dup2(null.as_raw_fd(), libc::STDIN_FILENO) } < 0
        || unsafe { libc::dup2(libc::STDERR_FILENO, libc::STDOUT_FILENO) } < 0 {
        return Err(std::io::Error::last_os_error().to_string());
    }
    let runtime = std::env::var("XDG_RUNTIME_DIR").map_err(|e| e.to_string())?;
    let metadata = std::fs::metadata(&runtime).map_err(|e| e.to_string())?;
    if runtime != format!("/run/user/{uid}") || !metadata.is_dir() || metadata.uid() != uid || metadata.mode() & 0o777 != 0o700 {
        return Err("Invalid user runtime directory".into());
    }
    let _bus = zbus::blocking::connection::Builder::session()
        .and_then(|builder| builder.name("org.telorgon.UserSession1"))
        .and_then(|builder| builder.build()).map_err(|e| e.to_string())?;
    let display = std::env::var("WAYLAND_DISPLAY").map_err(|e| e.to_string())?;
    let identity = std::env::var("XDG_SESSION_ID").map_err(|e| e.to_string())?;
    if identity.is_empty() || display != format!("telorgon-{identity}") || !identity.bytes().all(|byte| byte.is_ascii_alphanumeric()) {
        return Err("Invalid user display identity".into());
    }
    let path = std::path::Path::new(&runtime).join(&display);
    let listener = UnixListener::bind(&path).map_err(|e| e.to_string())?;
    struct SocketPath(std::path::PathBuf);
    impl Drop for SocketPath { fn drop(&mut self) { let _ = std::fs::remove_file(&self.0); } }
    let _path = SocketPath(path.clone());
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600)).map_err(|e| e.to_string())?;
    let mut config = telorgon::session::SessionConfig::new("telorgon-user-session")
        .applications(telorgon::session::ApplicationRegistry::new().discover_xdg_applications());
    config.recovery = false;
    config.publish_user_service_environment = true;
    let host = telorgon::session::SessionHost::start(config).map_err(|e| e.to_string())?;
    let session = host.handle();
    let store = crate::settings_config::store()?;
    let preferences = store.load().unwrap_or_else(|error| {
        eprintln!("telorgon-user-session: preferences could not be loaded; preserving the file: {error}");
        Default::default()
    });
    let configuration = telorgon::services::desktop_settings::protocol::encode(&preferences.display)?;
    let (settings, controller) = UnixStream::pair().map_err(|error| error.to_string())?;
    let mut mixer = telorgon::host::application::audio_mixer::AudioMixer::start().map_err(|error| error.to_string())?;
    let sound = crate::settings::sound::SoundService::start(preferences.sound, mixer.handle())?;
    let settings = super::settings::serve(settings, store, sound)?;
    let mut applications: Vec<_> = telorgon::app::ApplicationCatalog::system().snapshot()?
        .into_iter().filter(|app| app.visibility == telorgon::app::ApplicationVisibility::Visible)
        .map(|app| (app.id.as_str().to_owned(), app.name)).collect();
    applications.sort_by_key(|(_, name)| name.to_lowercase());
    applications.truncate(128);
    ipc::send_fd(&output, &listener).map_err(|e| e.to_string())?;
    ipc::send_fd(&output, &controller).map_err(|e| e.to_string())?;
    drop(listener); // The renderer receives the sole listening owner through the private channel.
    drop(controller);
    ipc::send(&mut output, &serde_json::json!((&applications, configuration))).map_err(|e| e.to_string())?;
    // The broker attaches this session before servicing its private settings
    // channel. Restore only after handing off readiness, without a startup cycle.
    if let Err(error) = super::audio::start(&session, std::path::Path::new(&runtime)) {
        eprintln!("telorgon-user-session: audio services could not be started: {error}");
    }
    if let Err(error) = super::portals::start(&session) {
        eprintln!("telorgon-user-session: portal services could not be started: {error}");
    }
    settings.restore_background(preferences.personalization);
    while let Ok(request) = ipc::receive(&mut input) {
        let application = request.as_str().filter(|id| applications.iter().any(|(known, _)| known == id));
        let result = match application {
            Some(id) => futures_lite::future::block_on(session.application(id)
                .prefer_dbus(false).restart(telorgon::session::RestartPolicy::Never).launch()),
            None => { ipc::send(&mut output, &serde_json::json!(false)).map_err(|e| e.to_string())?; continue; }
        };
        let success = result.as_ref().is_ok_and(|launch| launch.errors.is_empty());
        if let Err(error) = result { eprintln!("telorgon-user-session: application launch failed: {error}"); }
        ipc::send(&mut output, &serde_json::json!(success)).map_err(|e| e.to_string())?;
    }
    drop(settings);
    mixer.shutdown();
    host.close();
    Ok(())
}
