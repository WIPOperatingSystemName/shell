use super::*;
use std::num::NonZeroU32;
fn id(slot: u32, generation: u32) -> telorgon::shell::WindowId {
    telorgon::shell::WindowId::new(
        NonZeroU32::new(slot).unwrap(),
        NonZeroU32::new(generation).unwrap(),
    )
}
#[component]
struct CompactControlFixture {}
impl Component for CompactControlFixture {
    fn view(&self) -> impl View {
        task_button("×").width(24.0).height(26.0)
    }
}
#[component]
struct ColorTweenFixture {}
impl Component for ColorTweenFixture {
    fn view(&self) -> impl View {
        task_button("App")
            .background(Background::Color(TASKBAR_BUTTON_IDLE_COLOR))
            .inline_style(taskbar_button_style(false))
    }
}
#[component]
struct ReactiveColorFixture {
    #[input]
    active: Option<Signal<bool>>,
}
impl Component for ReactiveColorFixture {
    fn view(&self) -> impl View {
        stack()
            .width(50.0)
            .height(50.0)
            .child(
                task_button("App")
                    .width(50.0)
                    .height(50.0)
                    .background(Background::Color(TASKBAR_BUTTON_IDLE_COLOR))
                    .inline_style(taskbar_button_style(
                        *self.watch(self.active.as_ref().unwrap()),
                    )),
            )
            .child(
                column()
                    .width(50.0)
                    .height(50.0)
                    .align_items(Alignment::Center)
                    .child(
                        column()
                            .width(12.0)
                            .height(3.0)
                            .background(Background::Color(WINDOW_COUNT_BAR_COLOR)),
                    ),
            )
    }
}
#[component]
struct AudioStatusLayoutFixture {
    #[input]
    battery_visible: bool,
    #[input]
    network_visible: bool,
}
impl Component for AudioStatusLayoutFixture {
    fn view(&self) -> impl View {
        let mut bar = taskbar_row()
            .child(row().width(Dimension::FILL).height(ICON_WIDTH))
            .child(column().width(ICON_WIDTH).height(ICON_WIDTH))
            .child(task_button("Tray").width(TRAY_WIDTH).height(ICON_WIDTH));
        if self.network_visible {
            bar = bar.child(network_indicator(&NetworkSnapshot::default(), false));
        }
        bar = bar.child(
            task_button("Audio")
                .width(AUDIO_BUTTON_WIDTH)
                .height(ICON_WIDTH),
        );
        if self.battery_visible {
            bar = bar.child(battery_indicator(&battery::BatteryMonitorState {
                availability: battery::BatteryAvailability::Unavailable,
                batteries: vec![],
                external_power: None,
                last_error: None,
            }));
        }
        bar
    }
}
#[test]
fn battery_spacing_can_be_read_during_input_without_a_render_context() {
    struct EmptyBatteryProvider;
    impl battery::BatteryProvider for EmptyBatteryProvider {
        fn read_snapshot(
            &self,
        ) -> std::result::Result<battery::BatterySnapshot, battery::BatteryError> {
            Ok(battery::BatterySnapshot {
                observed_at: std::time::SystemTime::UNIX_EPOCH,
                batteries: vec![],
                external_supplies: vec![],
                external_power: None,
            })
        }
    }
    let monitor = futures_lite::future::block_on(battery::BatteryMonitor::start_with_provider(
        EmptyBatteryProvider,
        battery::BatteryMonitorConfig::default(),
    ))
    .unwrap();
    let taskbar = TestTaskbar {
        battery: Some(monitor.handle()),
        ..Default::default()
    };
    // Shell input callbacks have no render context; spacing must use a plain snapshot.
    assert_eq!(taskbar.battery_space(), 0.0);
    futures_lite::future::block_on(monitor.shutdown()).unwrap();
}
#[test]
fn status_widgets_keep_their_anchors_at_the_right_edge() {
    for width in [320, 800, 1920] {
        for battery_visible in [false, true] {
            for network_visible in [false, true] {
                let mut runtime =
                    telorgon::application_host::AppRuntimeCore::from_composed_with_extent(
                        AudioStatusLayoutFixture {
                            battery_visible,
                            network_visible,
                        },
                        telorgon::SizeI {
                            width,
                            height: BAR_HEIGHT.ceil() as i32,
                        },
                    )
                    .unwrap();
                runtime
                    .prepare_frame(telorgon::MonotonicInstant::ZERO, false)
                    .unwrap();
                let buttons: Vec<_> = runtime
                    .ui()
                    .nodes
                    .alive()
                    .iter()
                    .copied()
                    .filter(|node| {
                        runtime.ui().kinds.get(*node) == Some(&telorgon::NodeKind::Button)
                    })
                    .collect();
                let button = *buttons.last().unwrap();
                let rect = runtime.layout().computed(button).unwrap().border_rect;
                let battery_space = if battery_visible {
                    BATTERY_WIDGET_WIDTH + ICON_GAP
                } else {
                    0.0
                };
                let anchor = audio_anchor(width as f32, battery_space);
                assert!((rect.x - anchor.x).abs() < 0.01);
                assert!((rect.width - AUDIO_BUTTON_WIDTH).abs() < 0.01);
                assert!(audio_hit_test(
                    PointF {
                        x: rect.x + rect.width / 2.0,
                        y: rect.y + rect.height / 2.0
                    },
                    anchor
                ));
                if network_visible {
                    let network_rect = runtime
                        .layout()
                        .computed(buttons[buttons.len() - 2])
                        .unwrap()
                        .border_rect;
                    let network_anchor = network_anchor(width as f32, battery_space);
                    assert!((network_rect.x - network_anchor.x).abs() < 0.01);
                    assert!((network_rect.width - NETWORK_BUTTON_WIDTH).abs() < 0.01);
                    assert!((network_rect.right() + ICON_GAP - rect.x).abs() < 0.01);
                    let tray_rect = runtime.layout().computed(buttons[0]).unwrap().border_rect;
                    assert!((tray_rect.right() + ICON_GAP - network_rect.x).abs() < 0.01);
                }
                if battery_visible {
                    let node = runtime
                        .ui()
                        .nodes
                        .alive()
                        .iter()
                        .copied()
                        .find(|node| {
                            runtime.ui().box_styles.get(*node).is_some_and(|style| {
                                style.width == telorgon::ui::SizeRule::Logical(BATTERY_WIDGET_WIDTH)
                            })
                        })
                        .unwrap();
                    let battery_rect = runtime.layout().computed(node).unwrap().border_rect;
                    assert!((battery_rect.x - rect.right() - ICON_GAP).abs() < 0.01);
                    assert!((battery_rect.right() - (width as f32 - BAR_PADDING)).abs() < 0.01);
                    assert!(!audio_hit_test(
                        PointF {
                            x: battery_rect.x + battery_rect.width / 2.0,
                            y: battery_rect.y + battery_rect.height / 2.0
                        },
                        anchor
                    ));
                } else {
                    assert!((rect.right() - (width as f32 - BAR_PADDING)).abs() < 0.01);
                }
            }
        }
    }
}
#[test]
fn hover_after_window_loses_focus_returns_to_transparent() {
    let (active, writer) = Signal::new(true);
    let mut runtime = telorgon::application_host::AppRuntimeCore::from_composed_with_extent(
        ReactiveColorFixture {
            active: Some(active),
        },
        telorgon::SizeI {
            width: 100,
            height: 100,
        },
    )
    .unwrap();
    let at = |ms: u64| telorgon::MonotonicInstant::from_nanos(ms * 1_000_000);
    runtime.prepare_frame(at(0), false).unwrap();
    runtime.prepare_frame(at(300), false).unwrap();
    writer.publish_if_changed(false);
    runtime.prepare_frame(at(301), false).unwrap();
    runtime.prepare_frame(at(600), false).unwrap();
    let node = runtime
        .ui()
        .nodes
        .alive()
        .iter()
        .copied()
        .find(|node| runtime.ui().kinds.get(*node) == Some(&telorgon::NodeKind::Button))
        .unwrap();
    let color =
        |ui: &telorgon::ui::MountedUi| ui.box_styles.get(node).unwrap().decoration.background;
    assert_eq!(
        color(runtime.ui()),
        Background::Color(TASKBAR_BUTTON_IDLE_COLOR)
    );
    runtime.queue_input(InputEvent::mouse_moved(PointF { x: 20.0, y: 20.0 }));
    runtime.flush_input(at(601));
    runtime.prepare_frame(at(601), false).unwrap();
    runtime.prepare_frame(at(900), false).unwrap();
    assert_eq!(
        color(runtime.ui()),
        Background::Color(TASKBAR_BUTTON_HOVER_COLOR)
    );
    runtime.queue_input(InputEvent::mouse_moved(PointF {
        x: -100.0,
        y: -100.0,
    }));
    runtime.flush_input(at(901));
    runtime.prepare_frame(at(901), false).unwrap();
    runtime.prepare_frame(at(1200), false).unwrap();
    assert_eq!(
        color(runtime.ui()),
        Background::Color(TASKBAR_BUTTON_IDLE_COLOR)
    );
}

#[test]
fn real_input_and_reconciliation_tween_hover_and_active_changes() {
    use telorgon::MonotonicInstant;
    let (active, writer) = Signal::new(false);
    let mut runtime = telorgon::application_host::AppRuntimeCore::from_composed_with_extent(
        ReactiveColorFixture {
            active: Some(active),
        },
        telorgon::SizeI {
            width: 100,
            height: 100,
        },
    )
    .unwrap();
    runtime
        .prepare_frame(MonotonicInstant::ZERO, false)
        .unwrap();
    let node = runtime
        .ui()
        .nodes
        .alive()
        .iter()
        .copied()
        .find(|node| runtime.ui().kinds.get(*node) == Some(&telorgon::NodeKind::Button))
        .unwrap();
    let color =
        |ui: &telorgon::ui::MountedUi| ui.box_styles.get(node).unwrap().decoration.background;
    let at = |ms: u64| MonotonicInstant::from_nanos(ms * 1_000_000);
    runtime.queue_input(InputEvent::mouse_moved(PointF { x: 20.0, y: 20.0 }));
    runtime.flush_input(at(1));
    runtime.prepare_frame(at(1), false).unwrap();
    assert_eq!(
        color(runtime.ui()),
        Background::Color(TASKBAR_BUTTON_IDLE_COLOR)
    );
    runtime.prepare_frame(at(50), false).unwrap();
    assert_ne!(
        color(runtime.ui()),
        Background::Color(TASKBAR_BUTTON_IDLE_COLOR)
    );
    assert_ne!(
        color(runtime.ui()),
        Background::Color(TASKBAR_BUTTON_HOVER_COLOR)
    );
    runtime.prepare_frame(at(300), false).unwrap();
    assert_eq!(
        color(runtime.ui()),
        Background::Color(TASKBAR_BUTTON_HOVER_COLOR)
    );
    // Reconcile while hovered, as window and picker updates do in the shell.
    writer.publish_if_changed(true);
    runtime.prepare_frame(at(300), false).unwrap();
    writer.publish_if_changed(false);
    runtime.prepare_frame(at(300), false).unwrap();
    runtime.queue_input(InputEvent::mouse_moved(PointF {
        x: -100.0,
        y: -100.0,
    }));
    runtime.flush_input(at(301));
    runtime.prepare_frame(at(301), false).unwrap();
    runtime.prepare_frame(at(600), false).unwrap();
    assert_eq!(
        color(runtime.ui()),
        Background::Color(TASKBAR_BUTTON_IDLE_COLOR)
    );
    writer.publish_if_changed(true);
    runtime.prepare_frame(at(601), false).unwrap();
    assert_eq!(
        color(runtime.ui()),
        Background::Color(TASKBAR_BUTTON_IDLE_COLOR)
    );
    runtime.prepare_frame(at(650), false).unwrap();
    assert_ne!(
        color(runtime.ui()),
        Background::Color(TASKBAR_BUTTON_ACTIVE_COLOR)
    );
    runtime.prepare_frame(at(900), false).unwrap();
    assert_eq!(
        color(runtime.ui()),
        Background::Color(TASKBAR_BUTTON_ACTIVE_COLOR)
    );
    writer.publish_if_changed(false);
    runtime.prepare_frame(at(901), false).unwrap();
    assert_eq!(
        color(runtime.ui()),
        Background::Color(TASKBAR_BUTTON_ACTIVE_COLOR)
    );
    runtime.prepare_frame(at(1200), false).unwrap();
    assert_eq!(
        color(runtime.ui()),
        Background::Color(TASKBAR_BUTTON_IDLE_COLOR)
    );
    runtime.queue_input(InputEvent::mouse_moved(PointF { x: 20.0, y: 20.0 }));
    runtime.flush_input(at(1201));
    runtime.prepare_frame(at(1201), false).unwrap();
    runtime.prepare_frame(at(1500), false).unwrap();
    assert_eq!(
        color(runtime.ui()),
        Background::Color(TASKBAR_BUTTON_HOVER_COLOR)
    );
    runtime.deactivate_view(at(1501));
    runtime.queue_input(InputEvent::mouse_moved(PointF {
        x: -100.0,
        y: -100.0,
    }));
    runtime.flush_input(at(1501));
    runtime.prepare_frame(at(1501), false).unwrap();
    runtime.prepare_frame(at(1800), false).unwrap();
    assert_eq!(
        color(runtime.ui()),
        Background::Color(TASKBAR_BUTTON_IDLE_COLOR)
    );
}

#[test]
fn active_color_tweens_and_retargets_back_to_idle() {
    use telorgon::MonotonicInstant;
    let mut runtime = telorgon::application_host::AppRuntimeCore::from_composed_with_extent(
        ColorTweenFixture::default(),
        telorgon::SizeI {
            width: 100,
            height: 100,
        },
    )
    .unwrap();
    runtime.prepare_frame(MonotonicInstant::ZERO, true).unwrap();
    let node = runtime
        .ui()
        .nodes
        .alive()
        .iter()
        .copied()
        .find(|node| runtime.ui().kinds.get(*node) == Some(&telorgon::NodeKind::Button))
        .unwrap();
    let color = |runtime: &telorgon::ui::MountedUi| {
        runtime.box_styles.get(node).unwrap().decoration.background
    };
    let duration = u64::from(TASKBAR_COLOR_DURATION_MS) * 1_000_000;
    runtime
        .ui_mut()
        .set_local_component_style(node, Some(taskbar_button_style(true)));
    runtime
        .prepare_frame(MonotonicInstant::from_nanos(1), true)
        .unwrap();
    assert_eq!(
        color(runtime.ui()),
        Background::Color(TASKBAR_BUTTON_IDLE_COLOR)
    );
    runtime
        .prepare_frame(MonotonicInstant::from_nanos(1 + duration / 2), true)
        .unwrap();
    let middle = color(runtime.ui());
    assert_ne!(middle, Background::Color(TASKBAR_BUTTON_IDLE_COLOR));
    assert_ne!(middle, Background::Color(TASKBAR_BUTTON_ACTIVE_COLOR));
    runtime
        .ui_mut()
        .set_local_component_style(node, Some(taskbar_button_style(false)));
    runtime
        .prepare_frame(MonotonicInstant::from_nanos(1 + duration / 2), true)
        .unwrap();
    assert_eq!(color(runtime.ui()), middle);
    runtime
        .prepare_frame(MonotonicInstant::from_nanos(1 + duration * 2), true)
        .unwrap();
    assert_eq!(
        color(runtime.ui()),
        Background::Color(TASKBAR_BUTTON_IDLE_COLOR)
    );
}

#[test]
fn compact_controls_retain_declared_size_in_real_layout() {
    let mut runtime = telorgon::application_host::AppRuntimeCore::from_composed_with_extent(
        CompactControlFixture::default(),
        telorgon::SizeI {
            width: 400,
            height: 200,
        },
    )
    .unwrap();
    runtime
        .prepare_frame(telorgon::MonotonicInstant::ZERO, true)
        .unwrap();
    let node = runtime
        .ui()
        .nodes
        .alive()
        .iter()
        .copied()
        .find(|node| runtime.ui().kinds.get(*node) == Some(&telorgon::NodeKind::Button))
        .unwrap();
    let bounds = runtime.layout().computed(node).unwrap().border_rect;
    assert_eq!(bounds.width, 24.0);
    assert_eq!(bounds.height, 26.0);
}
#[test]
fn resolved_application_groups_protocol_aliases_but_unknowns_stay_separate() {
    let app = ApplicationId::new("org.mozilla.firefox.desktop");
    assert_eq!(
        identity_key(Some(&app), "firefox", id(1, 1)),
        identity_key(Some(&app), "org.mozilla.firefox", id(2, 1))
    );
    assert_eq!(
        identity_key(None, "my-app", id(1, 1)),
        identity_key(None, "my-app", id(2, 1))
    );
    assert_ne!(
        identity_key(None, "", id(1, 1)),
        identity_key(None, "", id(2, 1))
    );
    assert_ne!(
        identity_key(None, "", id(1, 1)),
        identity_key(None, "", id(1, 2))
    );
}
#[test]
fn icon_hover_ignores_spacing_and_outside_panel() {
    let y = BAR_PADDING + TASKBAR_TOP_BORDER_WIDTH + ICON_WIDTH / 2.0;
    let first = BAR_PADDING + START_SPACE + ICON_WIDTH / 2.0;
    let stride = ICON_WIDTH + ICON_GAP;
    assert_eq!(
        icon_at(
            PointF {
                x: BAR_PADDING + ICON_WIDTH / 2.0,
                y
            },
            2
        ),
        None
    );
    assert_eq!(icon_at(PointF { x: first, y }, 2), Some(0));
    assert_eq!(
        icon_at(
            PointF {
                x: first + stride,
                y
            },
            2
        ),
        Some(1)
    );
    assert_eq!(
        icon_at(
            PointF {
                x: BAR_PADDING + START_SPACE + ICON_WIDTH + ICON_GAP / 2.0,
                y
            },
            2
        ),
        None
    );
    assert_eq!(icon_at(PointF { x: first, y: -1.0 }, 2), None);
    assert_eq!(
        icon_at(
            PointF {
                x: first + 2.0 * stride,
                y
            },
            2
        ),
        None
    );
}
