//! OS power requests use logind's normal authorization and inhibitor handling.
use std::time::Duration;
use zbus::blocking::{connection::Builder, Connection, Proxy};

#[cfg(test)]
mod tests;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum PowerAction {
    Restart,
    Shutdown,
}

impl PowerAction {
    pub(super) fn verb(self) -> &'static str {
        match self {
            Self::Restart => "restart",
            Self::Shutdown => "shut down",
        }
    }

    pub(super) fn progress(self) -> &'static str {
        match self {
            Self::Restart => "Restarting…",
            Self::Shutdown => "Shutting down…",
        }
    }

    pub(super) fn failure(self, error: &zbus::Error) -> String {
        if let zbus::Error::MethodError(name, _, _) = error {
            if name.as_str().ends_with("OperationInhibited") {
                return "Blocked by an application".into();
            }
            if name.as_str().ends_with("AccessDenied")
                || name.as_str().ends_with("InteractiveAuthorizationRequired")
            {
                return "Permission denied".into();
            }
        }
        format!("Could not {}", self.verb())
    }
}

pub(super) fn request(action: PowerAction) -> zbus::Result<()> {
    let connection = Builder::system()?
        .method_timeout(Duration::from_secs(30))
        .build()?;
    request_on(&connection, action)
}

fn request_on(connection: &Connection, action: PowerAction) -> zbus::Result<()> {
    let manager = Proxy::new(
        connection,
        "org.freedesktop.login1",
        "/org/freedesktop/login1",
        "org.freedesktop.login1.Manager",
    )?;
    // No authentication agent is assumed. Denials stay visible in the launcher;
    // never bypass policy or inhibitors with a forced systemctl/reboot fallback.
    manager.call(
        match action {
            PowerAction::Restart => "Reboot",
            PowerAction::Shutdown => "PowerOff",
        },
        &(false,),
    )
}
