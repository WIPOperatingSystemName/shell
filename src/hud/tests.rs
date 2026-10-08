use super::{
    SystemHuds,
    model::*,
    view::{self, HUD_HEIGHT, HUD_WIDTH},
};
use std::{
    collections::BTreeMap,
    time::{Duration, Instant},
};
use telorgon::app::*;
use telorgon::host::application::audio_mixer::{MixerOperationState, MixerSnapshot};
use telorgon::integrations::pipewire::ConnectionState;
use telorgon::platform::contracts::PermissionState;
use telorgon::services::audio::mixer::MixerTarget;

fn feedback(control: SystemShortcutControl, accepted: bool) -> SystemShortcutFeedback {
    SystemShortcutFeedback { control, accepted }
}
fn reading(level: f32) -> Reading {
    Reading {
        level: Some(level),
        muted: false,
        status: Status::Ready,
    }
}

#[test]
fn newest_control_replaces_the_hud_and_resets_its_dismissal_timer() {
    let mut model = HudModel::default();
    let now = Instant::now();
    let volume = reading(0.4);
    let brightness = reading(0.7);
    model.observe(volume, brightness, now);
    assert!(
        !model.active(),
        "background state changes must not open HUDs"
    );
    model.request(
        feedback(SystemShortcutControl::Volume, true),
        volume,
        brightness,
        now,
    );
    model.observe(volume, brightness, now);
    assert_eq!(model.state().panel.reading.level, Some(0.4));
    assert_eq!(model.state().panel.reading.status, Status::Pending);
    assert_eq!(model.state().control, Some(SystemShortcutControl::Volume));

    // A newer brightness request replaces volume and takes over the dismissal timer.
    let confirmed = now + Duration::from_millis(200);
    model.observe(reading(0.45), brightness, confirmed);
    assert_eq!(model.state().panel.reading, reading(0.45));
    let later = now + Duration::from_secs(1);
    model.request(
        feedback(SystemShortcutControl::ScreenBrightness, true),
        reading(0.45),
        brightness,
        later,
    );
    model.observe(
        reading(0.45),
        reading(0.75),
        later + Duration::from_millis(100),
    );
    model.observe(reading(0.45), reading(0.75), confirmed + DISMISS_AFTER);
    assert_eq!(
        model.state().control,
        Some(SystemShortcutControl::ScreenBrightness)
    );
    assert!(model.state().panel.visible);
    assert_eq!(model.state().panel.reading, reading(0.75));
    // Late readback from the replaced control cannot switch the HUD back.
    model.observe(reading(0.9), reading(0.75), confirmed + DISMISS_AFTER);
    assert_eq!(model.state().panel.reading, reading(0.75));
    model.request(
        feedback(SystemShortcutControl::Microphone, true),
        volume,
        brightness,
        confirmed + DISMISS_AFTER,
    );
    assert_eq!(
        model.state().control,
        Some(SystemShortcutControl::ScreenBrightness)
    );

    let hidden = later + Duration::from_secs(4);
    model.observe(reading(0.45), reading(0.75), hidden);
    assert!(!model.active());
    model.observe(reading(0.8), reading(0.9), hidden + Duration::from_secs(1));
    model.request(
        feedback(SystemShortcutControl::Microphone, true),
        volume,
        brightness,
        hidden,
    );
    assert!(!model.active());
}

#[test]
fn pending_feedback_is_bounded_and_rejected_requests_preserve_readback() {
    let now = Instant::now();
    let mut model = HudModel::default();
    let actual = reading(0.5);
    let pending = Reading {
        status: Status::Pending,
        ..actual
    };
    model.request(
        feedback(SystemShortcutControl::Volume, true),
        pending,
        actual,
        now,
    );
    for seconds in 0..=12 {
        model.observe(pending, actual, now + Duration::from_secs(seconds));
    }
    assert_eq!(model.state().panel.reading.status, Status::Unconfirmed);
    assert_eq!(model.state().panel.reading.level, Some(0.5));
    model.observe(pending, actual, now + Duration::from_secs(14));
    assert!(
        !model.active(),
        "a stalled owner must not pin the HUD indefinitely"
    );

    let later = now + Duration::from_secs(15);
    model.request(
        feedback(SystemShortcutControl::Volume, false),
        actual,
        actual,
        later,
    );
    model.observe(actual, actual, later);
    assert_eq!(model.state().panel.reading.status, Status::Failed);
    assert_eq!(model.state().panel.reading.level, Some(0.5));
    model.request(
        feedback(SystemShortcutControl::Volume, true),
        actual,
        actual,
        later,
    );
    model.observe(actual, actual, later + Duration::from_millis(200));
    assert_eq!(model.state().panel.reading.status, Status::Ready);
    model.observe(actual, actual, later + Duration::from_secs(3));
    assert!(!model.active());
}

fn mixer_snapshot() -> MixerSnapshot {
    MixerSnapshot {
        generation: 1,
        state: ConnectionState::Ready,
        default_output: None,
        default_input: None,
        applications: vec![],
        devices: vec![],
        destinations: BTreeMap::new(),
        operations: BTreeMap::new(),
        nodes: vec![],
    }
}

#[test]
fn volume_does_not_present_an_optimistic_preview_as_a_missing_output() {
    let mut snapshot = mixer_snapshot();
    snapshot.operations.insert(
        MixerTarget::DefaultOutput,
        MixerOperationState {
            pending: true,
            preview_volume: Some(0.9),
            error: None,
            applied: 0,
            failed: 0,
        },
    );
    let observed = volume_reading(&snapshot);
    assert_eq!(snapshot.volume(&MixerTarget::DefaultOutput), Some(0.9));
    assert_eq!(observed.level, None);
    assert_eq!(observed.status, Status::Unavailable);
    assert_eq!(volume_reading(&snapshot).status, Status::Unavailable);
    assert_eq!(volume_reading(&snapshot).level, None);
    snapshot.state = ConnectionState::Stopped;
    assert_eq!(volume_reading(&snapshot), Reading::default());
}

struct PanelProvider {
    level: u32,
}
impl ScreenBrightnessProvider for PanelProvider {
    fn discover(
        &mut self,
    ) -> std::result::Result<Vec<ScreenBrightnessProviderDevice>, ScreenBrightnessError> {
        Ok(vec![ScreenBrightnessProviderDevice {
            key: "panel".into(),
            name: "Panel".into(),
            maximum: 100,
            kind: ScreenBrightnessKind::InternalBacklight,
            association: ScreenBrightnessAssociation::Unknown,
            permission: PermissionState::Granted,
            verification: ScreenBrightnessVerification::Provider,
        }])
    }
    fn read(
        &mut self,
        _: &str,
    ) -> std::result::Result<ScreenBrightnessReading, ScreenBrightnessError> {
        Ok(ScreenBrightnessReading {
            configured: self.level,
            actual: None,
        })
    }
    fn set(&mut self, _: &str, value: u32) -> std::result::Result<(), ScreenBrightnessError> {
        self.level = value;
        Ok(())
    }
}
fn wait(mut predicate: impl FnMut() -> bool) {
    let deadline = Instant::now() + Duration::from_secs(2);
    while !predicate() {
        assert!(Instant::now() < deadline, "HUD observation timed out");
        std::thread::sleep(Duration::from_millis(2));
    }
}

#[test]
fn brightness_service_readback_reaches_hud_and_unavailable_controls_are_explicit() {
    // Injected provider: this test never connects to logind or real display/audio devices.
    let mut controller =
        ScreenBrightnessController::with_provider(Default::default(), PanelProvider { level: 40 })
            .unwrap();
    controller.set_host_state(ScreenBrightnessHostState {
        active: true,
        locked: false,
    });
    let handle = controller.handle();
    controller.start().unwrap();
    wait(|| handle.signal().snapshot().devices.len() == 1);
    let (volume, _) = Signal::new(mixer_snapshot());
    let huds = SystemHuds::start(volume, handle.signal()).unwrap();
    let request = handle
        .execute(
            ScreenBrightnessTarget::DefaultInternal,
            ScreenBrightnessAction::Adjust(ScreenBrightnessDelta::percentage_points(5.0).unwrap()),
        )
        .unwrap();
    huds.feedback()(feedback(SystemShortcutControl::ScreenBrightness, true));
    assert!(matches!(
        futures_lite::future::block_on(request.completion()),
        ScreenBrightnessOutcome::Applied(_)
    ));
    wait(|| huds.handle().state.snapshot().panel.reading.level == Some(0.45));
    assert!(huds.handle().state.snapshot().panel.visible);
    assert_eq!(
        huds.handle().state.snapshot().control,
        Some(SystemShortcutControl::ScreenBrightness)
    );

    let mut snapshot = handle.signal().snapshot().value.as_ref().clone();
    snapshot.pending = 1;
    snapshot.last_error = Some(ScreenBrightnessError::TimedOut);
    assert_eq!(brightness_reading(&snapshot).status, Status::Unconfirmed);
    snapshot.devices[0].permission = PermissionState::Denied;
    assert_eq!(brightness_reading(&snapshot).status, Status::Restricted);
    snapshot.devices.push(snapshot.devices[0].clone());
    assert_eq!(brightness_reading(&snapshot).status, Status::Ambiguous);
    assert_eq!(brightness_reading(&snapshot).level, None);
    snapshot.devices.clear();
    assert_eq!(brightness_reading(&snapshot), Reading::default());
    drop(huds);
    futures_lite::future::block_on(controller.shutdown()).unwrap();
}

#[component(no_default)]
struct HudFixture {
    #[input]
    state: Signal<HudState>,
}
impl Component for HudFixture {
    fn view(&self) -> impl View {
        view::content(&self.watch(&self.state))
    }
}
#[test]
fn hud_is_text_free_and_centers_icons_and_levels_when_controls_change() {
    let (state, writer) = Signal::new(HudState::default());
    let mut runtime = telorgon::application_host::AppRuntimeCore::from_composed_with_extent(
        HudFixture { state },
        telorgon::SizeI {
            width: HUD_WIDTH as i32,
            height: HUD_HEIGHT as i32,
        },
    )
    .unwrap();
    runtime.register_assets(crate::assets::bundle()).unwrap();
    runtime.register_fonts(crate::assets::bundle()).unwrap();
    for (index, (control, level, status, muted)) in [
        (SystemShortcutControl::Volume, 0.0, Status::Ready, false),
        (
            SystemShortcutControl::ScreenBrightness,
            0.65,
            Status::Pending,
            false,
        ),
        (SystemShortcutControl::Volume, 1.0, Status::Ready, false),
        (
            SystemShortcutControl::ScreenBrightness,
            0.65,
            Status::Failed,
            false,
        ),
        (SystemShortcutControl::Volume, 0.65, Status::Ready, true),
    ]
    .into_iter()
    .enumerate()
    {
        writer.publish(HudState {
            control: Some(control),
            panel: Panel {
                visible: true,
                reading: Reading {
                    status,
                    muted,
                    ..reading(level)
                },
            },
        });
        runtime
            .prepare_frame(
                telorgon::MonotonicInstant::from_nanos(index as u64 * 1_000_000),
                false,
            )
            .unwrap();
        assert!(
            runtime.ui().texts.iter().next().is_none(),
            "HUD must remain purely visual for {control:?}, {status:?}, muted={muted}"
        );
        let warning = !matches!(status, Status::Ready | Status::Pending);
        let icon = runtime.ui().images.iter().next().unwrap().0;
        let icon_rect = runtime.layout().computed(icon).unwrap().border_rect;
        assert!(
            (icon_rect.y + icon_rect.height / 2.0 - HUD_HEIGHT / 2.0).abs() < 0.01,
            "icon must be centered: {icon_rect:?}"
        );
        let track = runtime
            .ui()
            .nodes
            .alive()
            .iter()
            .copied()
            .find(|node| {
                runtime.ui().box_styles.get(*node).is_some_and(|style| {
                    style.decoration.background
                        == Background::Color(crate::colors::COLOR5.with_alpha(40))
                })
            })
            .unwrap();
        let track_rect = runtime.layout().computed(track).unwrap().border_rect;
        assert!(
            (track_rect.y + track_rect.height / 2.0 - HUD_HEIGHT / 2.0).abs() < 0.01,
            "level bar must be centered: {track_rect:?}"
        );
        let fill = runtime.ui().nodes.alive().iter().copied().find(|node| {
            runtime.ui().box_styles.get(*node).is_some_and(|style| {
                style.decoration.background
                    == Background::Color(if warning {
                        ColorRgba8::rgba(255, 199, 120, 255)
                    } else if muted {
                        crate::colors::COLOR7
                    } else {
                        ColorRgba8::rgba(124, 178, 255, 255)
                    })
            })
        });
        if level == 0.0 {
            assert!(fill.is_none());
        } else {
            let rect = runtime
                .layout()
                .computed(fill.unwrap())
                .unwrap()
                .border_rect;
            assert!(
                (rect.width - track_rect.width * level).abs() < 0.1,
                "level bar: {rect:?}"
            );
        }
    }
}
