//! The persistent compositor starts without attaching a personal desktop session.
use crate::{assets, cursor, decoration::DECORATION_POLICY, window::TEST_CHROME};
use telorgon::app::*;
pub(crate) mod accounts;
mod control;
mod login;
mod desktop;
mod display;
mod background;

pub fn run() -> telorgon::Result<()> {
    let users = accounts::load().map_err(telorgon::AppError::new)?;
    let sessions = telorgon::host::application::WaylandSessionControl::default();
    let display = telorgon::host::application::display_control::DisplayControl::new(Default::default())
        .map_err(telorgon::AppError::new)?;
    let background = background::Control::new();
    let control = control::Control::new(users, sessions.clone(), display.clone(), background.clone());
    // Keep the system-bus connection alive for the compositor's entire lifetime.
    let _service = control::serve(control.clone()).map_err(telorgon::AppError::new)?;
    let mut session = session::SessionConfig::new("telorgon-system-shell")
        .applications(ApplicationRegistry::new());
    // No personal settings, recovery journal, user bus publication or application discovery.
    session.recovery = false;
    Application::shell_environment("Telorgon")
        .linux(LinuxShellConfig {
            session,
            socket_name: Some("wayland-system".into()),
            client_sessions: Some(sessions),
            display_control: Some(display),
            ..Default::default()
        })
        .renderer(Renderer::Auto)
        .assets(assets::bundle())
        .compositor(Compositor::new().cursor_theme(cursor::cursor_theme())
            .client_cursor_mode(ClientCursorMode::Allow)
            .decoration_policy(DECORATION_POLICY)
            .window_frame(easy_window_frame(TEST_CHROME.validate().expect("valid window chrome"))))
        .widget(background::SystemBackground::new(background))
        .widget(login::LoginShell::new(control.clone()))
        .widget(desktop::DesktopPanel::new(control))
        .run()
}
