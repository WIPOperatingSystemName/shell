mod settings;
mod settings_config;
mod desktop_entry;
mod portal;
mod constants;
mod decoration;
mod cursor;
mod keybinds;
mod taskbar;
mod window;
mod colors;
mod hud;
mod background;

use crate::colors::COLOR7;
use crate::background::{BackgroundControl, ShellBackground};
use crate::decoration::DECORATION_POLICY;
use crate::constants::*;
use crate::taskbar::TestTaskbar;
use crate::window::*;
use telorgon::app::*;

asset_catalog! {
    pub mod assets = "assets";
}

fn main() -> telorgon::Result<()> {
    if let Err(error) = desktop_entry::ensure_settings_entry() {
        eprintln!("Could not create the Settings launcher entry: {error}");
    }
    let executable = std::env::current_exe().map_err(|e| telorgon::AppError::new(e.to_string()))?;
    let applications = ApplicationRegistry::new()
        .discover_xdg_applications()
        .binary_directory(executable.parent().expect("executable has a parent directory"))
        .register("capture-picker", ApplicationSpec::executable("telorgon-portal-picker"));
    let applications = applications.register(
        "network-settings",
        ApplicationSpec::desktop_entry("org.telorgon.settings.desktop").arg("--page=network"),
    );
    let mut session = session::SessionConfig::new("telorgon-test-shell")
        .applications(applications)
        .desktop_settings(session::DesktopSettings::default());
    // This shell owns the graphical session, including D-Bus-activated applications.
    session.publish_user_service_environment = true;
    let settings_store = settings_config::store().map_err(telorgon::AppError::new)?;
    let preferences = settings_store.load().unwrap_or_else(|error| {
        eprintln!("Settings could not be loaded; using defaults without overwriting the file: {error}");
        Default::default()
    });
    let background = BackgroundControl::new(settings_store.clone(), preferences.personalization.clone());
    let display_control = telorgon::host::application::display_control::DisplayControl::new(preferences.display.clone())
        .map_err(telorgon::AppError::new)?;
    let linux = LinuxShellConfig {
        display_control: Some(display_control.clone()),
        window_drag_horizontal_overflow: Some(250),
        session,
        preferred_window_minimum: MINIMUM_WINDOW_SIZE,
        // Whole-window transitions also apply when a client owns its decorations.
        window_motion: TEST_CHROME.motion,
        resize_preview: TEST_CHROME
            .resize_preview
            .unwrap_or(LinuxShellConfig::default().resize_preview),
        ..LinuxShellConfig::default()
    };

    let chrome = TEST_CHROME
        .validate()
        .expect("TEST_CHROME must contain valid finite metrics");

    let mut mixer = telorgon::host::application::audio_mixer::AudioMixer::start()
        .map_err(|error| telorgon::AppError::new(error.to_string()))?;
    let brightness = telorgon::screen_brightness::ScreenBrightnessController::new(Default::default())
        .map_err(|error| telorgon::AppError::new(error.to_string()))?;
    let brightness_handle = brightness.handle();
    let network = telorgon::network::NetworkController::new(Default::default())
        .map_err(|error| telorgon::AppError::new(error.to_string()))?;
    let network_status = network.observer().signal();
    let huds = hud::SystemHuds::start(mixer.handle().signal(), brightness_handle.signal())
        .map_err(|error| telorgon::AppError::new(error.to_string()))?;
    let keybindings = keybinds::get_keybinds()
        .mixer_media_keys(mixer.handle(), 0.05, |error| eprintln!("Audio key: {error}"))
        .map_err(|error| telorgon::AppError::new(error.to_string()))?
        .screen_brightness_keys(
            brightness_handle,
            telorgon::screen_brightness::ScreenBrightnessTarget::DefaultInternal,
            telorgon::screen_brightness::ScreenBrightnessKeyConfig::default(),
        )
        .map_err(|error| telorgon::AppError::new(error.to_string()))?
        .on_system_action(huds.feedback());
    let settings_service = settings::SettingsService::start(settings_store, preferences, display_control, mixer.handle(), background.clone())
        .map_err(telorgon::AppError::new)?;
    let mut tray = telorgon::tray::TrayHost::connect(telorgon::tray::TrayHostConfig::new())
        .map_err(|error| telorgon::AppError::new(error.to_string()))?;
    let battery_monitor = match futures_lite::future::block_on(battery::monitor(
        battery::BatteryMonitorConfig::default(),
    )) {
        Ok(monitor) => Some(monitor),
        Err(error) => {
            eprintln!("Battery monitoring could not start: {error}");
            None
        }
    };
    let result = Application::shell_environment("Telorgon Test Shell")
        .linux(linux)
        .screen_brightness(brightness)
        .network(network)
        .capture(Capture::desktop())
        .applications(ApplicationCatalog::system().watch_changes(true))
        .renderer(Renderer::Vulkan)
        .assets(assets::bundle())
        .compositor(
            Compositor::new()
                .screen_cast_portal(
                    ScreenCastPortal::new()
                        .audio(
                            ShareAudio::new()
                                .window_mode(AudioScope::SelectedApplication)
                                .monitor_mode(AudioScope::Desktop)
                                .exclude_requesting_application(true)
                                .include_microphone(false),
                        )
                        .picker_app(ApplicationRef::registered("capture-picker"))
                        .sharing(portal::CaptureIndicator::new),
                )
                .cursor_theme(cursor::cursor_theme())
                .client_cursor_mode(ClientCursorMode::Allow)
                .decoration_policy(DECORATION_POLICY)
                .keybindings(keybindings)
                .window_frame(easy_window_frame(chrome)),
        )
        .widget(ShellBackground::new(background))
        .widget(hud::SystemHud::new(&huds))
        .widget(TestTaskbar::with_services(
            mixer.handle(),
            tray.handle(),
            battery_monitor.as_ref().map(|monitor| monitor.handle()),
            network_status,
        ))
        .widget(
            WindowTiling::snap()
                .edge_threshold(32.0)
                .corner_threshold(96.0)
                .preview(TilePreviewDesign {
                    fill: WINDOW_FILL,
                    corner_radius: FRAME_RADIUS,
                    border: Border::all(FRAME_BORDER_WIDTH, COLOR7),
                    padding: Insets::all(8.0),
                    ..TilePreviewDesign::default()
                }),
        )
        .run();
    drop(huds);
    drop(settings_service);
    tray.shutdown();
    mixer.shutdown();
    if let Some(monitor) = battery_monitor {
        if let Err(error) = futures_lite::future::block_on(monitor.shutdown()) {
            eprintln!("Battery monitoring shutdown failed: {error}");
        }
    }
    result
}

#[cfg(test)]
mod font_tests {
    #[test]
    fn bundled_inter_faces_are_valid_and_loadable() {
        let bundle = super::assets::bundle().validate().unwrap();
        let mut engine = telorgon::text::TextEngine::new().unwrap();
        let fonts: Vec<_> = bundle.iter()
            .filter(|entry| entry.kind == telorgon::AssetKind::Font).collect();
        assert_eq!(fonts.len(), 4);
        for font in fonts {
            assert_eq!(font.media_type, "font/ttf");
            engine.load_font_bytes(font.bytes.to_vec()).unwrap();
        }
    }
}
