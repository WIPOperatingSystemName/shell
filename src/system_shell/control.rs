use super::accounts::LoginUser;
use telorgon::app::{Signal, SignalWriter};
use telorgon::host::application::WaylandSessionControl;
use std::os::unix::net::UnixListener;
use std::sync::{Arc, Mutex};
use telorgon::host::application::display_control::DisplayControl;

pub(super) const BUS_NAME: &str = "org.telorgon.SystemShell1";
pub(super) const OBJECT_PATH: &str = "/org/telorgon/SystemShell";

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum Screen {
    Starting,
    Login,
    Desktop,
}

#[derive(Clone)]
pub(super) struct State {
    pub screen: Screen,
    pub users: Vec<LoginUser>,
    pub selected: Option<String>,
    pub session: Option<String>,
    pub busy: bool,
    pub error: Option<String>,
    pub applications: Vec<(String, String)>,
}

#[derive(Clone)]
pub(super) struct Control {
    pub signal: Signal<State>,
    writer: SignalWriter<State>,
    sessions: WaylandSessionControl,
    display: DisplayControl,
    background: super::background::Control,
    operations: Arc<Mutex<()>>,
}
impl PartialEq for Control {
    fn eq(&self, other: &Self) -> bool {
        self.signal == other.signal
    }
}
impl Control {
    pub fn new(users: Vec<LoginUser>, sessions: WaylandSessionControl, display: DisplayControl, background: super::background::Control) -> Self {
        let (signal, writer) = Signal::new(State {
            screen: Screen::Starting, users, selected: None, session: None, busy: false, error: None,
            applications: Vec::new(),
        });
        Self { signal, writer, sessions, display, background, operations: Arc::new(Mutex::new(())) }
    }

    fn show_login(&self) -> zbus::fdo::Result<()> {
        let _operation = self.operations.lock().unwrap_or_else(|error| error.into_inner());
        self.sessions.detach().map_err(|error| zbus::fdo::Error::Failed(error.to_string()))?;
        self.background.reset();
        // Boot can request login before KMS is ready; it already starts with defaults.
        if self.display.snapshot().ready {
            if let Err(error) = super::display::restore(&self.display, Default::default()) {
                eprintln!("telorgon-system-shell: login display could not be reset: {error}");
            }
        }
        let mut state = (*self.signal.snapshot()).clone();
        state.screen = Screen::Login;
        state.selected = None;
        state.session = None;
        state.busy = false;
        state.error = None;
        state.applications.clear();
        self.writer.publish(state);
        Ok(())
    }

    pub fn select(&self, name: &str) {
        let mut state = (*self.signal.snapshot()).clone();
        if state.screen == Screen::Login && !state.busy && state.users.iter().any(|user| user.name == name) {
            // Selection changes the view only; it grants no authenticated session.
            state.selected = Some(name.into());
            state.error = None;
            self.writer.publish(state);
        }
    }

    pub fn authenticate(&self, password: String) {
        let mut state = (*self.signal.snapshot()).clone();
        if state.screen != Screen::Login || state.busy { return; }
        let Some(user) = state.selected.clone() else { return };
        state.busy = true;
        state.error = None;
        self.writer.publish(state);
        let control = self.clone();
        std::thread::spawn(move || {
            if let Err(error) = crate::session_manager::login(&user, password) { control.failed(error); }
        });
    }

    pub fn logout(&self) {
        let mut state = (*self.signal.snapshot()).clone();
        if state.screen != Screen::Desktop || state.busy { return; }
        state.busy = true;
        self.writer.publish(state);
        let control = self.clone();
        std::thread::spawn(move || {
            if let Err(error) = crate::session_manager::logout() { control.failed(error); }
        });
    }

    pub fn launch(&self, application: &str) {
        let mut state = (*self.signal.snapshot()).clone();
        if state.screen != Screen::Desktop || state.busy
            || !state.applications.iter().any(|(id, _)| id == application) { return; }
        state.busy = true;
        state.error = None;
        self.writer.publish(state);
        let control = self.clone();
        let application = application.to_owned();
        std::thread::spawn(move || {
            if let Err(error) = crate::session_manager::launch(&application) { control.failed(error); }
            else {
                let mut state = (*control.signal.snapshot()).clone();
                state.busy = false;
                control.writer.publish(state);
            }
        });
    }

    fn failed(&self, error: String) {
        let mut state = (*self.signal.snapshot()).clone();
        state.busy = false;
        state.error = Some(error);
        self.writer.publish(state);
    }
}

async fn require_root(connection: &zbus::Connection, header: &zbus::message::Header<'_>) -> zbus::fdo::Result<()> {
    let sender = header.sender().ok_or_else(|| zbus::fdo::Error::AccessDenied("Missing system controller identity".into()))?;
    let bus = zbus::fdo::DBusProxy::new(connection).await?;
    if bus.get_connection_unix_user(sender.clone().into()).await? != 0 {
        return Err(zbus::fdo::Error::AccessDenied("Only the system controller may change the shell view".into()));
    }
    Ok(())
}

struct Endpoint(Control);
#[zbus::interface(name = "org.telorgon.SystemShell1")]
impl Endpoint {
    async fn show_login(
        &self,
        #[zbus(connection)] connection: &zbus::Connection,
        #[zbus(header)] header: zbus::message::Header<'_>,
    ) -> zbus::fdo::Result<()> {
        require_root(connection, &header).await?;
        let control = self.0.clone();
        blocking::unblock(move || control.show_login()).await
    }

    fn snapshot(&self) -> (String, String) {
        let state = self.0.signal.snapshot();
        (match state.screen { Screen::Starting => "starting", Screen::Login => "login", Screen::Desktop => "desktop" }.into(),
         state.selected.clone().unwrap_or_default())
    }

    async fn attach_session(&self, user: String, uid: u32, session: String,
        listener: zbus::zvariant::OwnedFd, applications: Vec<(String, String)>,
        display_configuration: String,
        #[zbus(connection)] connection: &zbus::Connection,
        #[zbus(header)] header: zbus::message::Header<'_>,
    ) -> zbus::fdo::Result<()> {
        require_root(connection, &header).await?;
        let control = self.0.clone();
        blocking::unblock(move || {
        let _operation = control.operations.lock().unwrap_or_else(|error| error.into_inner());
        let mut state = (*control.signal.snapshot()).clone();
        if state.screen != Screen::Login || !state.users.iter().any(|account| account.name == user && account.uid == uid)
            || session.is_empty() || !session.bytes().all(|byte| byte.is_ascii_alphanumeric()) {
            return Err(zbus::fdo::Error::InvalidArgs("Invalid authenticated session".into()));
        }
        let desired = telorgon::services::desktop_settings::protocol::decode(&display_configuration)
            .map_err(zbus::fdo::Error::InvalidArgs)?;
        control.sessions.attach(UnixListener::from(std::os::fd::OwnedFd::from(listener)), uid)
            .map_err(|error| zbus::fdo::Error::Failed(error.to_string()))?;
        if let Err(error) = super::display::restore(&control.display, desired) {
            eprintln!("telorgon-system-shell: saved display preferences could not be applied: {error}");
            super::display::restore(&control.display, Default::default()).map_err(zbus::fdo::Error::Failed)?;
        }
        state.screen = Screen::Desktop;
        state.selected = Some(user);
        state.session = Some(session);
        state.busy = false;
        state.error = None;
        state.applications = applications;
        control.writer.publish(state);
        Ok(())
        }).await
    }

    async fn display(&self, session: String, operation: String, configuration: String,
        #[zbus(connection)] connection: &zbus::Connection,
        #[zbus(header)] header: zbus::message::Header<'_>,
    ) -> zbus::fdo::Result<String> {
        require_root(connection, &header).await?;
        let control = self.0.clone();
        blocking::unblock(move || {
            let _operation = control.operations.lock().unwrap_or_else(|error| error.into_inner());
            let state = control.signal.snapshot();
            if state.screen != Screen::Desktop || state.session.as_deref() != Some(session.as_str()) {
                return Err(zbus::fdo::Error::AccessDenied("The display session has ended".into()));
            }
            super::display::request(&control.display, control.background.snapshot(), &operation, &configuration).map_err(zbus::fdo::Error::Failed)
        }).await
    }

    async fn background(&self, session: String, configuration: String, descriptors: Vec<zbus::zvariant::OwnedFd>,
        #[zbus(connection)] connection: &zbus::Connection,
        #[zbus(header)] header: zbus::message::Header<'_>,
    ) -> zbus::fdo::Result<String> {
        require_root(connection, &header).await?;
        let control = self.0.clone();
        blocking::unblock(move || {
            let _operation = control.operations.lock().unwrap_or_else(|error| error.into_inner());
            let state = control.signal.snapshot();
            if state.screen != Screen::Desktop || state.session.as_deref() != Some(session.as_str()) {
                return Err(zbus::fdo::Error::AccessDenied("The wallpaper session has ended".into()));
            }
            control.background.apply(&configuration, descriptors.into_iter().map(std::os::fd::OwnedFd::from).collect())
                .map_err(zbus::fdo::Error::Failed)?;
            Ok(String::new())
        }).await
    }
}

pub(super) fn serve(control: Control) -> Result<zbus::blocking::Connection, String> {
    zbus::blocking::connection::Builder::system()
        .and_then(|builder| builder.name(BUS_NAME))
        .and_then(|builder| builder.serve_at(OBJECT_PATH, Endpoint(control)))
        .and_then(|builder| builder.build())
        .map_err(|error| format!("Could not start system shell control: {error}"))
}
