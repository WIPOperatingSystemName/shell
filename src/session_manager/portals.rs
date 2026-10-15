//! Portal processes inherit the authenticated user's display and logind scope.
use std::time::{Duration, Instant};
use telorgon::session::{RestartPolicy, SessionHandle, Stream};

pub(super) fn start(session: &SessionHandle) -> Result<(), String> {
    let connection = zbus::blocking::connection::Builder::session()
        .map(|builder| builder.method_timeout(Duration::from_secs(2)))
        .and_then(|builder| builder.build()).map_err(|error| error.to_string())?;
    let bus = zbus::blocking::fdo::DBusProxy::new(&connection).map_err(|error| error.to_string())?;
    // Start the backends first so D-Bus activation cannot move them outside
    // this login's scope. A subsequent restart retains the same environment.
    for (name, program, args) in [
        ("org.freedesktop.impl.portal.PermissionStore", "/usr/libexec/xdg-permission-store", &[][..]),
        ("org.freedesktop.portal.Documents", "/usr/libexec/xdg-document-portal", &[][..]),
        ("org.freedesktop.impl.portal.desktop.telorgon.FileExplorer", "/usr/bin/telorgon-file-explorer", &["--portal"][..]),
        ("org.freedesktop.portal.Desktop", "/usr/libexec/xdg-desktop-portal", &[][..]),
    ] {
        let name = zbus::names::BusName::try_from(name).map_err(|error| error.to_string())?;
        if bus.name_has_owner(name.clone()).map_err(|error| error.to_string())? {
            // A user bus may be shared with another login. Never replace or
            // silently use a chooser attached to that login's display.
            let pid = bus.get_connection_unix_process_id(name.clone()).map_err(|error| error.to_string())?;
            let environment = std::fs::read(format!("/proc/{pid}/environ")).map_err(|error| error.to_string())?;
            let display = format!("WAYLAND_DISPLAY={}", std::env::var("WAYLAND_DISPLAY").map_err(|error| error.to_string())?);
            if !environment.split(|byte| *byte == 0).any(|entry| entry == display.as_bytes()) {
                return Err(format!("{name} belongs to a different graphical session"));
            }
            continue;
        }
        session.command(program).args(args).stdin(Stream::Null)
            .restart(RestartPolicy::OnExit).spawn().map_err(|error| error.to_string())?;
        let deadline = Instant::now() + Duration::from_secs(3);
        while !bus.name_has_owner(name.clone()).map_err(|error| error.to_string())? {
            if Instant::now() >= deadline { return Err(format!("{name} did not become ready")); }
            std::thread::sleep(Duration::from_millis(25));
        }
    }
    Ok(())
}
