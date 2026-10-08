use super::*;

fn interface(kind: NetworkInterfaceKind, state: NetworkConnectionState) -> NetworkInterface {
    NetworkInterface {
        device: Default::default(),
        id: NetworkInterfaceId::new(),
        name: "wlan0".into(),
        kind,
        managed: true,
        state,
        failure: None,
        capabilities: Default::default(),
        active_connection: None,
        ipv4: Default::default(),
        ipv6: Default::default(),
        access_points: vec![],
        active_access_point: None,
    }
}

fn ap(name: Option<Vec<u8>>, strength: u8, security: WifiSecurity) -> NetworkAccessPoint {
    NetworkAccessPoint {
        id: NetworkAccessPointId::new(),
        ssid: name.map(|name| Ssid::new(name).unwrap()),
        bssid: format!("00:11:22:33:44:{strength:02x}"),
        strength,
        frequency_mhz: 5180,
        security,
    }
}

fn connected() -> NetworkSnapshot {
    let mut wifi = interface(
        NetworkInterfaceKind::Wifi,
        NetworkConnectionState::Connected,
    );
    let current = ap(Some(b"Home Wi-Fi".to_vec()), 74, WifiSecurity::WpaPersonal);
    wifi.active_access_point = Some(current.id);
    wifi.access_points = vec![current];
    wifi.ipv4
        .addresses
        .push(IpAddress::new("192.0.2.10".parse().unwrap(), 24).unwrap());
    wifi.ipv4.gateway = Some("192.0.2.1".parse().unwrap());
    wifi.ipv4.dns.push("192.0.2.53".parse().unwrap());
    NetworkSnapshot {
        state: NetworkServiceState::Ready,
        connectivity: NetworkConnectivity::Internet,
        networking_enabled: true,
        wifi_enabled: true,
        wifi_hardware_enabled: true,
        interfaces: vec![wifi],
        ..Default::default()
    }
}

#[test]
fn network_summary_distinguishes_connection_from_internet_access() {
    let mut snapshot = connected();
    assert_eq!(summary(&snapshot).icon, NetworkIcon::Wifi(74));
    assert!(summary(&snapshot).label.contains("Home Wi-Fi"));
    for (connectivity, expected) in [
        (NetworkConnectivity::CaptivePortal, "Sign-in required"),
        (NetworkConnectivity::Local, "Local network only"),
        (NetworkConnectivity::Offline, "No Internet access"),
    ] {
        snapshot.connectivity = connectivity;
        assert_eq!(summary(&snapshot).status, expected);
        assert!(summary(&snapshot).warning);
    }
    snapshot.networking_enabled = false;
    assert_eq!(summary(&snapshot).status, "Networking off");
    assert_eq!(summary(&snapshot).icon, NetworkIcon::Offline);
    snapshot.state = NetworkServiceState::Restricted;
    assert_eq!(summary(&snapshot).status, "Network access restricted");
    assert!(!summary(&snapshot).label.contains("Home Wi-Fi"));
    snapshot.state = NetworkServiceState::Unavailable;
    assert_eq!(summary(&snapshot).status, "Network unavailable");
}

#[test]
fn connected_wifi_name_falls_back_to_profile_when_ap_is_not_reported() {
    let mut snapshot = connected();
    snapshot.interfaces[0].access_points.clear();
    let connection = NetworkConnectionId::new();
    let profile = NetworkProfileId::new();
    snapshot.interfaces[0].active_connection = Some(connection);
    snapshot.connections.push(NetworkConnection {
        id: connection,
        profile: Some(profile),
        interfaces: vec![snapshot.interfaces[0].id],
        state: NetworkConnectionState::Connected,
    });
    snapshot.profiles.push(NetworkProfile {
        interface_name: None,
        id: profile,
        name: "Saved home profile".into(),
        ssid: Some(Ssid::new(b"Hidden home".to_vec()).unwrap()),
        kind: NetworkInterfaceKind::Wifi,
        autoconnect: true,
        persistence: NetworkPersistence::Saved,
        ip: Default::default(),
    });
    assert_eq!(
        connection_name(&snapshot, &snapshot.interfaces[0]),
        "Hidden home"
    );
}

fn visible_text(ui: &telorgon::ui::MountedUi) -> Vec<String> {
    ui.texts
        .iter()
        .map(|(_, visual)| ui.string(visual.content).unwrap().to_owned())
        .collect()
}

#[test]
fn panel_reacts_to_status_updates_and_removes_stale_connection_details() {
    let (signal, writer) = Signal::new(connected());
    let mut runtime = telorgon::application_host::AppRuntimeCore::from_composed_with_extent(
        NetworkPanel::new(signal, NetworkPopupState::default()),
        SizeI {
            width: 320,
            height: 220,
        },
    )
    .unwrap();
    runtime
        .prepare_frame(telorgon::MonotonicInstant::ZERO, false)
        .unwrap();
    let contents = visible_text(runtime.ui());
    assert!(contents.contains(&"Internet access".into()));
    assert!(contents.contains(&"Home Wi-Fi".into()));
    assert!(contents.contains(&"Network settings".into()));
    assert!(
        !contents.iter().any(|text| text.contains("192.0.2.")
            || text.contains("DNS")
            || text.contains("Nearby"))
    );
    writer.publish(NetworkSnapshot {
        state: NetworkServiceState::Unavailable,
        ..Default::default()
    });
    runtime
        .prepare_frame(telorgon::MonotonicInstant::from_nanos(1_000_000), false)
        .unwrap();
    let contents = visible_text(runtime.ui());
    assert!(contents.contains(&"Network unavailable".into()));
    assert!(
        !contents
            .iter()
            .any(|text| text.contains("192.0.2.") || text.contains("Home Wi-Fi"))
    );
}
