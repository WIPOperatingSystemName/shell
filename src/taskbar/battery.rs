//! Read-only taskbar battery status. The shell owns observation and passes the latest state.
use telorgon::battery::{
    BatteryAvailability, BatteryKind, BatteryMonitorState, BatteryScope, BatteryState,
    BatterySummary,
};
use telorgon::ui::{BoxStyle, Overflow};
use telorgon::{RectF, app::*};

use super::constants::{BAR_PADDING, ICON_WIDTH, TASKBAR_ICON_SIZE};
use crate::colors::{COLOR1, COLOR5, COLOR7};

pub(super) const BATTERY_WIDGET_WIDTH: f32 = ICON_SIZE.width + BAR_PADDING * 2.0;
const ICON_SIZE: SizeF = SizeF {
    width: TASKBAR_ICON_SIZE as f32 * 32.0 / 20.0,
    height: TASKBAR_ICON_SIZE as f32,
};
const SVG_SCALE: f32 = ICON_SIZE.width / 32.0;
const LABEL_SCALE: f32 = ICON_SIZE.height / 28.0;
const BODY: RectF = RectF {
    x: 1.5 * SVG_SCALE,
    y: 3.0 * SVG_SCALE,
    width: 26.0 * SVG_SCALE,
    height: 14.0 * SVG_SCALE,
};
const CHARGING_COLOR: ColorRgba8 = ColorRgba8::rgba(104, 218, 137, 255);
const LOW_BATTERY_COLOR: ColorRgba8 = ColorRgba8::rgba(255, 96, 86, 255);
const BOLT_SLOT_WIDTH: f32 = 7.0 * LABEL_SCALE;

fn system_battery(state: &BatteryMonitorState) -> Option<&BatterySummary> {
    state
        .batteries
        .iter()
        .filter(|battery| {
            battery.kind == BatteryKind::Battery
                && battery.scope != BatteryScope::Device
                && battery.status.present
        })
        .min_by_key(|battery| (battery.scope != BatteryScope::System, battery.id.as_str()))
}

pub(super) fn visible(state: &BatteryMonitorState) -> bool {
    state.availability != BatteryAvailability::Ready || system_battery(state).is_some()
}

fn fill_width(percentage: u8) -> f32 {
    match percentage.min(100) {
        0 => 0.0,
        100 => ICON_SIZE.width,
        // Reveal the full authored silhouette, from the body's left edge through the tip.
        value => (1.5 + (31.25 - 1.5) * f32::from(value) / 100.0) * SVG_SCALE,
    }
}

fn canvas_style() -> BoxStyle {
    BoxStyle {
        // Layers retain their full canvas inside a narrower percentage viewport.
        max_size: SizeRule2D {
            width: SizeRule::Logical(ICON_SIZE.width),
            height: SizeRule::Logical(ICON_SIZE.height),
        },
        ..Default::default()
    }
}

fn clipped_layer(width: f32, layer: impl View) -> telorgon::compose::Container {
    stack()
        .width(width)
        .height(ICON_SIZE.height)
        .align_items(Alignment::Start)
        .justify_content(Alignment::Start)
        .overflow(Overflow::Clip)
        .child(layer)
}

fn label_layer(value: &str, charging: bool, color: ColorRgba8) -> telorgon::compose::Container {
    let mut content = row()
        .width(BODY.width)
        .height(BODY.height)
        .gap(2.0 * LABEL_SCALE)
        .center_content()
        .child(
            text(value)
                .width(Dimension::Shrink)
                .height(BODY.height)
                .font_family("Inter")
                .size(13.0 * LABEL_SCALE)
                .weight(700)
                .vertical_align(Alignment::Center)
                .color(color),
        );
    if charging {
        content = content.child(
            stack()
                .width(BOLT_SLOT_WIDTH)
                .height(BODY.height)
                .overflow(Overflow::Clip)
                .child(
                    image(crate::assets::icons::LIGHTNING_BOLT)
                        .box_style(BoxStyle {
                            transform: Transform2D {
                                translation: PointF {
                                    x: BOLT_SLOT_WIDTH / 2.0 - 14.5 * SVG_SCALE,
                                    y: BODY.height / 2.0 - ICON_SIZE.height / 2.0,
                                },
                                ..Default::default()
                            },
                            ..canvas_style()
                        })
                        .width(ICON_SIZE.width)
                        .height(ICON_SIZE.height)
                        .tint(color),
                ),
        );
    }
    row()
        .box_style(canvas_style())
        .width(ICON_SIZE.width)
        .height(ICON_SIZE.height)
        .padding_edges(EdgeInsets {
            left: BODY.x,
            right: ICON_SIZE.width - BODY.right(),
            top: BODY.y,
            bottom: ICON_SIZE.height - BODY.bottom(),
        })
        .child(content)
}

pub(super) fn indicator(state: &BatteryMonitorState) -> telorgon::compose::Container {
    let battery = system_battery(state);
    let current = state.availability == BatteryAvailability::Ready;
    let percentage = current
        .then(|| battery.and_then(|battery| battery.status.percentage))
        .flatten()
        .map(|percentage| percentage.min(100));
    let charging =
        current && battery.is_some_and(|battery| battery.status.state == BatteryState::Charging);
    let fill_color = if percentage.is_some_and(|value| value <= 20) {
        LOW_BATTERY_COLOR
    } else if charging {
        CHARGING_COLOR
    } else {
        COLOR5
    };
    let percent_text = percentage.map_or_else(|| "—".into(), |value| value.to_string());
    let percentage_label = percentage.map_or_else(
        || "percentage unavailable".into(),
        |value| format!("{value}%"),
    );
    let label = if !current {
        "Battery status unavailable".into()
    } else if charging {
        format!("Battery, {percentage_label}, charging")
    } else {
        format!("Battery, {percentage_label}")
    };

    let mut icon = stack()
        .width(ICON_SIZE.width)
        .height(ICON_SIZE.height)
        .align_items(Alignment::Start)
        .justify_content(Alignment::Start)
        .child(
            image(crate::assets::icons::BATTERY_EMPTY)
                .width(ICON_SIZE.width)
                .height(ICON_SIZE.height)
                .tint(if current { COLOR5 } else { COLOR7 })
                .accessible_label(label),
        );
    if let Some(percentage) = percentage.filter(|percentage| *percentage > 0) {
        icon = icon.child(clipped_layer(
            fill_width(percentage),
            image(crate::assets::icons::BATTERY_FILL)
                .box_style(canvas_style())
                .width(ICON_SIZE.width)
                .height(ICON_SIZE.height)
                .tint(fill_color),
        ));
    }

    icon = icon.child(label_layer(
        &percent_text,
        charging,
        if current { COLOR5 } else { COLOR7 },
    ));
    if let Some(percentage) = percentage.filter(|percentage| *percentage > 0) {
        // The same label switches ink exactly where charge passes behind its glyphs.
        icon = icon.child(clipped_layer(
            fill_width(percentage),
            label_layer(&percent_text, charging, COLOR1),
        ));
    }

    row()
        .width(BATTERY_WIDGET_WIDTH)
        .height(ICON_WIDTH)
        .padding_edges(EdgeInsets {
            left: BAR_PADDING,
            right: BAR_PADDING,
            ..Default::default()
        })
        .align_items(Alignment::Center)
        .child(icon)
}

#[cfg(test)]
mod tests {
    use super::*;
    use telorgon::application_host::{AppRuntimeCore, ComposedAppRuntime};
    use telorgon::battery::{BatteryHealth, BatteryStatus};

    #[component(no_default)]
    struct BatteryFixture {
        #[input]
        state: Signal<BatteryMonitorState>,
    }
    impl Component for BatteryFixture {
        fn view(&self) -> impl View {
            row().child(indicator(&self.watch(&self.state)))
        }
    }

    fn battery(id: &str, scope: BatteryScope, percentage: u8) -> BatterySummary {
        BatterySummary {
            id: id.into(),
            kind: BatteryKind::Battery,
            scope,
            model: None,
            status: BatteryStatus {
                present: true,
                percentage: Some(percentage),
                state: BatteryState::Discharging,
                time_to_empty: None,
                time_to_full: None,
            },
            health: BatteryHealth {
                capacity_retention_percent: None,
                cycle_count: None,
                condition: None,
            },
        }
    }

    fn state(percentage: u8, charging: bool) -> BatteryMonitorState {
        let mut battery = battery("BAT0", BatteryScope::System, percentage);
        if charging {
            battery.status.state = BatteryState::Charging;
        }
        BatteryMonitorState {
            availability: BatteryAvailability::Ready,
            batteries: vec![battery],
            external_power: None,
            last_error: None,
        }
    }

    fn runtime(state: Signal<BatteryMonitorState>) -> ComposedAppRuntime {
        let mut runtime = AppRuntimeCore::from_composed_with_extent(
            BatteryFixture { state },
            telorgon::SizeI {
                width: BATTERY_WIDGET_WIDTH.ceil() as i32,
                height: ICON_WIDTH as i32,
            },
        )
        .unwrap();
        runtime.register_assets(crate::assets::bundle()).unwrap();
        runtime.register_fonts(crate::assets::bundle()).unwrap();
        runtime
            .prepare_frame(telorgon::MonotonicInstant::ZERO, false)
            .unwrap();
        runtime
    }

    fn image_node(
        runtime: &ComposedAppRuntime,
        asset: telorgon::assets::IconAsset,
    ) -> Option<telorgon::ui::UiNodeId> {
        runtime.ui().nodes.alive().iter().copied().find(|node| {
            runtime
                .ui()
                .images
                .get(*node)
                .is_some_and(|image| image.image == asset.image_id())
        })
    }

    fn percentage_text(runtime: &ComposedAppRuntime) -> &str {
        runtime
            .ui()
            .texts
            .iter()
            .find_map(|(_, text)| runtime.ui().string(text.content))
            .unwrap()
    }

    #[test]
    fn source_selection_ignores_peripherals_ups_and_absent_batteries() {
        let mut state = state(42, false);
        let mut ups = battery("UPS0", BatteryScope::System, 90);
        ups.kind = BatteryKind::Ups;
        let mut absent = battery("BAT1", BatteryScope::System, 100);
        absent.status.present = false;
        state.batteries = vec![battery("mouse", BatteryScope::Device, 85), ups, absent];
        assert!(!visible(&state));
        state
            .batteries
            .push(battery("BAT0", BatteryScope::Unknown, 42));
        assert!(visible(&state));
        assert_eq!(system_battery(&state).unwrap().id, "BAT0");
        state
            .batteries
            .push(battery("BAT2", BatteryScope::System, 64));
        assert_eq!(system_battery(&state).unwrap().id, "BAT2");
    }

    #[test]
    fn percentage_crop_preserves_image_size_and_reveals_tip_at_full_charge() {
        for percentage in [1, 25, 50, 95, 100] {
            let (signal, _) = Signal::new(state(percentage, false));
            let runtime = runtime(signal);
            let base = image_node(&runtime, crate::assets::icons::BATTERY_EMPTY).unwrap();
            let fill = image_node(&runtime, crate::assets::icons::BATTERY_FILL).unwrap();
            let base_rect = runtime.layout().computed(base).unwrap().border_rect;
            let fill_layout = runtime.layout().computed(fill).unwrap();
            assert_eq!(fill_layout.border_rect, base_rect);
            assert_eq!(fill_layout.border_rect.width, ICON_SIZE.width);
            assert!((fill_layout.visible_rect.width - fill_width(percentage)).abs() < 0.01);
            if percentage == 95 {
                assert!(fill_layout.visible_rect.width > 29.0 * SVG_SCALE);
            }
            if percentage == 100 {
                assert_eq!(fill_layout.visible_rect, base_rect);
            }
        }
        let (signal, _) = Signal::new(state(0, false));
        let runtime = runtime(signal);
        assert_eq!(percentage_text(&runtime), "0");
        assert!(image_node(&runtime, crate::assets::icons::BATTERY_FILL).is_none());
    }

    #[test]
    fn percentage_and_charging_bolt_stay_inside_the_battery_body() {
        for percentage in [0, 9, 17, 50, 75, 100] {
            for charging in [false, true] {
                let (signal, _) = Signal::new(state(percentage, charging));
                let runtime = runtime(signal);
                let base = image_node(&runtime, crate::assets::icons::BATTERY_EMPTY).unwrap();
                let origin = runtime.layout().computed(base).unwrap().border_rect;
                let body = BODY.translate(PointF {
                    x: origin.x,
                    y: origin.y,
                });
                let within_body = |rect: RectF| {
                    assert!(
                        rect.x >= body.x - 0.01 && rect.right() <= body.right() + 0.01,
                        "{rect:?} outside {body:?}"
                    );
                    assert!(
                        rect.y >= body.y - 0.01 && rect.bottom() <= body.bottom() + 0.01,
                        "{rect:?} outside {body:?}"
                    );
                };
                let mut label_rect = None;
                for (node, _) in runtime.ui().texts.iter() {
                    let rect = runtime.layout().computed(node).unwrap().border_rect;
                    within_body(rect);
                    if let Some(previous) = label_rect {
                        // Clipped contrast layers must keep the same label position.
                        assert_eq!(rect, previous);
                    }
                    label_rect = Some(rect);
                }
                if charging {
                    let bolt = image_node(&runtime, crate::assets::icons::LIGHTNING_BOLT).unwrap();
                    let visible = runtime.layout().computed(bolt).unwrap().visible_rect;
                    assert!(visible.area() > 0.0);
                    within_body(visible);
                } else {
                    assert!(image_node(&runtime, crate::assets::icons::LIGHTNING_BOLT).is_none());
                }
            }
        }
    }

    #[test]
    fn reactive_updates_change_percent_charging_and_stale_status() {
        let (signal, writer) = Signal::new(state(25, true));
        let mut runtime = runtime(signal);
        assert_eq!(percentage_text(&runtime), "25");
        let bolt = image_node(&runtime, crate::assets::icons::LIGHTNING_BOLT).unwrap();
        let fill = image_node(&runtime, crate::assets::icons::BATTERY_FILL).unwrap();
        assert_eq!(
            runtime.ui().images.get(fill).unwrap().tint,
            Some(CHARGING_COLOR)
        );
        let bolt_layout = runtime.layout().computed(bolt).unwrap();
        assert!(bolt_layout.visible_rect.width <= BOLT_SLOT_WIDTH);

        writer.publish_if_changed(state(80, false));
        runtime
            .prepare_frame(telorgon::MonotonicInstant::from_nanos(1), false)
            .unwrap();
        assert_eq!(percentage_text(&runtime), "80");
        assert!(image_node(&runtime, crate::assets::icons::LIGHTNING_BOLT).is_none());
        let fill = image_node(&runtime, crate::assets::icons::BATTERY_FILL).unwrap();
        assert_eq!(runtime.ui().images.get(fill).unwrap().tint, Some(COLOR5));

        for (index, availability) in [
            BatteryAvailability::Unavailable,
            BatteryAvailability::Stopped,
        ]
        .into_iter()
        .enumerate()
        {
            let mut stale = state(87, true);
            stale.availability = availability;
            writer.publish_if_changed(stale);
            runtime
                .prepare_frame(
                    telorgon::MonotonicInstant::from_nanos(index as u64 + 2),
                    false,
                )
                .unwrap();
            assert_eq!(percentage_text(&runtime), "—");
            assert!(image_node(&runtime, crate::assets::icons::BATTERY_FILL).is_none());
            assert!(image_node(&runtime, crate::assets::icons::LIGHTNING_BOLT).is_none());
        }
    }

    #[test]
    fn missing_percentage_is_unknown_even_when_power_is_connected() {
        let mut unknown = state(50, false);
        unknown.external_power = Some(true);
        unknown.batteries[0].status.percentage = None;
        unknown.batteries[0].status.state = BatteryState::NotCharging;
        let (signal, _) = Signal::new(unknown);
        let runtime = runtime(signal);
        assert_eq!(percentage_text(&runtime), "—");
        assert!(image_node(&runtime, crate::assets::icons::BATTERY_FILL).is_none());
        assert!(image_node(&runtime, crate::assets::icons::LIGHTNING_BOLT).is_none());
    }
}
