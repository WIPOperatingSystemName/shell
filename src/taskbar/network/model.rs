use telorgon::network::*;

#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) enum NetworkIcon {
    Wifi(u8),
    Wired,
    Other,
    Offline,
}

pub(super) struct NetworkSummary {
    pub status: &'static str,
    pub icon: NetworkIcon,
    pub warning: bool,
    pub label: String,
}

pub(super) fn summary(snapshot: &NetworkSnapshot) -> NetworkSummary {
    let connected = snapshot
        .interfaces
        .iter()
        .filter(|interface| interface.state == NetworkConnectionState::Connected)
        .min_by_key(|interface| match interface.kind {
            NetworkInterfaceKind::Ethernet => 0,
            NetworkInterfaceKind::Wifi => 1,
            NetworkInterfaceKind::Other => 2,
        });
    let status = match snapshot.state {
        NetworkServiceState::Unstarted => "Starting network…",
        NetworkServiceState::Unavailable => "Network unavailable",
        NetworkServiceState::Restricted => "Network access restricted",
        NetworkServiceState::Stopped => "Network monitoring stopped",
        NetworkServiceState::Ready if !snapshot.networking_enabled => "Networking off",
        NetworkServiceState::Ready
            if snapshot.connectivity == NetworkConnectivity::CaptivePortal =>
        {
            "Sign-in required"
        }
        NetworkServiceState::Ready if snapshot.connectivity == NetworkConnectivity::Internet => {
            "Internet access"
        }
        NetworkServiceState::Ready if connected.is_some() => match snapshot.connectivity {
            NetworkConnectivity::Local => "Local network only",
            NetworkConnectivity::Offline => "No Internet access",
            _ => "Connected",
        },
        NetworkServiceState::Ready
            if snapshot
                .interfaces
                .iter()
                .any(|interface| connecting(interface.state)) =>
        {
            "Connecting…"
        }
        NetworkServiceState::Ready => "Disconnected",
    };
    let current = snapshot.state == NetworkServiceState::Ready && snapshot.networking_enabled;
    let icon = if current {
        connected
            .map(interface_icon)
            .unwrap_or(NetworkIcon::Offline)
    } else {
        NetworkIcon::Offline
    };
    let warning = snapshot.state == NetworkServiceState::Restricted
        || (current
            && connected.is_some()
            && matches!(
                snapshot.connectivity,
                NetworkConnectivity::Local
                    | NetworkConnectivity::CaptivePortal
                    | NetworkConnectivity::Offline
            ));
    let label = if current {
        connected.map(|interface| {
            format!(
                "Network, {status}, {}",
                connection_name(snapshot, interface)
            )
        })
    } else {
        None
    }
    .unwrap_or_else(|| format!("Network, {status}"));
    NetworkSummary {
        status,
        icon,
        warning,
        label,
    }
}

fn connecting(state: NetworkConnectionState) -> bool {
    matches!(
        state,
        NetworkConnectionState::Preparing
            | NetworkConnectionState::Authenticating
            | NetworkConnectionState::ConfiguringIp
    )
}

pub(super) fn active_ap(interface: &NetworkInterface) -> Option<&NetworkAccessPoint> {
    interface
        .access_points
        .iter()
        .find(|ap| Some(ap.id) == interface.active_access_point)
}

pub(super) fn interface_icon(interface: &NetworkInterface) -> NetworkIcon {
    match interface.kind {
        NetworkInterfaceKind::Wifi => {
            NetworkIcon::Wifi(active_ap(interface).map_or(0, |ap| ap.strength.min(100)))
        }
        NetworkInterfaceKind::Ethernet => NetworkIcon::Wired,
        NetworkInterfaceKind::Other => NetworkIcon::Other,
    }
}

// Keep control characters from turning a network name into additional visual rows.
pub(super) fn ssid_name(ssid: Option<&Ssid>) -> String {
    ssid.map(|ssid| {
        ssid.display_name()
            .chars()
            .map(|ch| {
                if ch.is_control() || matches!(ch, '\u{2028}' | '\u{2029}') {
                    ' '
                } else {
                    ch
                }
            })
            .collect::<String>()
    })
    .filter(|name| !name.trim().is_empty())
    .unwrap_or_else(|| "Hidden network".into())
}

fn active_profile<'a>(
    snapshot: &'a NetworkSnapshot,
    interface: &NetworkInterface,
) -> Option<&'a NetworkProfile> {
    let profile = snapshot
        .connections
        .iter()
        .find(|connection| Some(connection.id) == interface.active_connection)?
        .profile?;
    snapshot
        .profiles
        .iter()
        .find(|candidate| candidate.id == profile)
}

pub(super) fn connection_name(snapshot: &NetworkSnapshot, interface: &NetworkInterface) -> String {
    if interface.kind == NetworkInterfaceKind::Wifi {
        if let Some(ssid) = active_ap(interface).and_then(|ap| ap.ssid.as_ref()) {
            return ssid_name(Some(ssid));
        }
        if let Some(profile) = active_profile(snapshot, interface) {
            if profile.ssid.is_some() {
                return ssid_name(profile.ssid.as_ref());
            }
        }
        if active_ap(interface).is_some() {
            "Hidden network".into()
        } else {
            "Wi-Fi".into()
        }
    } else {
        match interface.kind {
            NetworkInterfaceKind::Ethernet => "Ethernet".into(),
            _ => "Network connection".into(),
        }
    }
}

pub(super) fn connection_status(interface: &NetworkInterface) -> &'static str {
    if !interface.managed {
        return "Managed elsewhere";
    }
    match interface.state {
        NetworkConnectionState::Unavailable => "Unavailable",
        NetworkConnectionState::Disconnected => "Disconnected",
        NetworkConnectionState::Preparing => "Connecting…",
        NetworkConnectionState::Authenticating => "Authenticating…",
        NetworkConnectionState::ConfiguringIp => "Getting an IP address…",
        NetworkConnectionState::Connected => "Connected",
        NetworkConnectionState::Disconnecting => "Disconnecting…",
        NetworkConnectionState::Failed => "Connection failed",
        NetworkConnectionState::Unknown => "Status unknown",
    }
}

pub(super) fn primary_interface(snapshot: &NetworkSnapshot) -> Option<&NetworkInterface> {
    snapshot
        .interfaces
        .iter()
        .filter(|interface| {
            interface.state == NetworkConnectionState::Connected || connecting(interface.state)
        })
        .min_by_key(|interface| {
            (
                interface.state != NetworkConnectionState::Connected,
                match interface.kind {
                    NetworkInterfaceKind::Ethernet => 0,
                    NetworkInterfaceKind::Wifi => 1,
                    NetworkInterfaceKind::Other => 2,
                },
            )
        })
}
