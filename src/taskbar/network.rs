//! Read-only network presentation. The shell owns the controller and its lifetime.
mod model;
#[cfg(test)]
mod tests;

use super::constants::*;
use crate::colors::{COLOR2, COLOR5, COLOR7};
use model::*;
use telorgon::{RectF, app::*, assets::ImageSource, network::*};

pub(super) const NETWORK_BUTTON_WIDTH: f32 = ICON_WIDTH;
const PANEL_WIDTH: f32 = 320.0;
const PANEL_HEIGHT: f32 = 220.0;
const WARNING: ColorRgba8 = ColorRgba8::rgba(255, 194, 102, 255);

pub(super) fn network_anchor(output_width: f32, battery_space: f32) -> RectF {
    RectF {
        x: (output_width
            - BAR_PADDING
            - super::audio::AUDIO_BUTTON_WIDTH
            - battery_space
            - ICON_GAP
            - NETWORK_BUTTON_WIDTH)
            .max(0.0),
        y: 0.0,
        width: NETWORK_BUTTON_WIDTH,
        height: BAR_HEIGHT,
    }
}

fn icon_source(icon: NetworkIcon) -> ImageSource {
    use crate::assets::icons::*;
    match icon {
        NetworkIcon::Wifi(0) => NETWORK_WIFI_ZERO.into(),
        NetworkIcon::Wifi(1..=33) => NETWORK_WIFI_LOW.into(),
        NetworkIcon::Wifi(34..=66) => NETWORK_WIFI_MEDIUM.into(),
        NetworkIcon::Wifi(_) => NETWORK_WIFI_HIGH.into(),
        NetworkIcon::Wired => NETWORK_WIRED.into(),
        NetworkIcon::Other => NETWORK.into(),
        NetworkIcon::Offline => NETWORK_OFFLINE.into(),
    }
}

pub(super) fn indicator(snapshot: &NetworkSnapshot, open: bool) -> Button {
    let summary = summary(snapshot);
    let tint = if summary.warning {
        WARNING
    } else if summary.icon == NetworkIcon::Offline {
        COLOR7
    } else {
        COLOR5
    };
    super::taskbar::task_button(summary.label)
        .child(
            image(icon_source(summary.icon))
                .width(TASKBAR_ICON_SIZE as f32)
                .height(TASKBAR_ICON_SIZE as f32)
                .tint(tint),
        )
        .width(NETWORK_BUTTON_WIDTH)
        .height(ICON_WIDTH)
        .padding(Insets::ZERO)
        .corner_radius(TASKBAR_BUTTON_RADIUS)
        .inline_style(super::taskbar::taskbar_button_style(open))
}

#[derive(Clone)]
pub(super) struct NetworkPopupState {
    pub open: Signal<bool>,
    writer: SignalWriter<bool>,
}
impl Default for NetworkPopupState {
    fn default() -> Self {
        let (open, writer) = Signal::new(false);
        Self { open, writer }
    }
}
impl PartialEq for NetworkPopupState {
    fn eq(&self, other: &Self) -> bool {
        self.open == other.open
    }
}
impl NetworkPopupState {
    pub fn set(&self, open: bool) {
        self.writer.publish_if_changed(open);
    }
}

#[component(no_default)]
pub(super) struct NetworkPopup {
    #[input]
    network: Signal<NetworkSnapshot>,
    #[input]
    state: NetworkPopupState,
    #[input]
    anchor: RectF,
}
impl NetworkPopup {
    pub fn new(network: Signal<NetworkSnapshot>, state: NetworkPopupState, anchor: RectF) -> Self {
        Self {
            network,
            state,
            anchor,
        }
    }
}
impl ShellWidget for NetworkPopup {
    fn surface(&self) -> ShellSurfaceSpec {
        let output = self.context::<ShellContext>().output_size();
        let width = PANEL_WIDTH.min((output.width - 16.0).max(1.0));
        let height = PANEL_HEIGHT;
        ShellSurfaceSpec::new()
            .placement(
                WidgetPlacement::attached_to(self.anchor, ShellEdge::Top)
                    .width(width)
                    .height(height.min((output.height - BAR_HEIGHT - 24.0).max(1.0)))
                    .offset(NETWORK_BUTTON_WIDTH - width, -8.0)
                    .margin(8.0),
            )
            .layer(ShellSurfaceLayer::Overlay)
            .visibility_motion(crate::window::WINDOW_MINIMIZE_MOTION)
            .pointer(ShellPointer::Surface)
            .focus(ShellFocus::OnOpen)
            .visible(*self.watch(&self.state.open))
            .dismiss_on_outside_press(true)
            .outside_press_excludes_anchor(true)
            .dismiss_on_escape(true)
    }
    fn dismissed(&mut self, _: ShellDismissReason) {
        self.state.set(false);
    }
}
impl Component for NetworkPopup {
    fn view(&self) -> impl View {
        NetworkPanel::new(self.network.clone(), self.state.clone())
    }
}

#[derive(Clone, Default, PartialEq)]
struct LaunchStatus {
    busy: bool,
    error: Option<String>,
}

#[component(no_default)]
struct NetworkPanel {
    #[input]
    network: Signal<NetworkSnapshot>,
    #[input]
    popup: NetworkPopupState,
    #[state]
    launch: Signal<LaunchStatus>,
    #[state]
    launch_writer: SignalWriter<LaunchStatus>,
}
impl NetworkPanel {
    fn new(network: Signal<NetworkSnapshot>, popup: NetworkPopupState) -> Self {
        let (launch, launch_writer) = Signal::new(LaunchStatus::default());
        Self {
            network,
            popup,
            launch,
            launch_writer,
        }
    }
    fn open_settings(&mut self) {
        if self.launch.snapshot().busy {
            return;
        }
        self.launch_writer.publish(LaunchStatus {
            busy: true,
            error: None,
        });
        let writer = self.launch_writer.clone();
        let popup = self.popup.clone();
        std::thread::spawn(move || {
            let result = futures_lite::future::block_on(async {
                session::applications()?
                    .get("network-settings")?
                    .launch()
                    .await
            });
            let opened = result.as_ref().is_ok_and(|result| result.errors.is_empty());
            writer.publish(LaunchStatus {
                busy: false,
                error: (!opened).then(|| "Could not open Network settings".into()),
            });
            if opened {
                popup.set(false);
            }
        });
    }
}
fn label(value: impl ToString, size: f32, color: ColorRgba8) -> Text {
    text(value)
        .font_family("Inter")
        .size(size)
        .color(color)
        .width(Dimension::FILL)
}
impl Component for NetworkPanel {
    fn view(&self) -> impl View {
        let snapshot = self.watch(&self.network);
        let summary = summary(&snapshot);
        let launch = self.watch(&self.launch);
        let connected = (snapshot.state == NetworkServiceState::Ready
            && snapshot.networking_enabled)
            .then(|| primary_interface(&snapshot))
            .flatten();
        let (name, detail) = connected
            .map(|interface| {
                let detail = (interface.state == NetworkConnectionState::Connected)
                    .then(|| active_ap(interface))
                    .flatten()
                    .map(|ap| format!("{}% signal", ap.strength.min(100)))
                    .unwrap_or_else(|| connection_status(interface).into());
                (connection_name(&snapshot, interface), detail)
            })
            .unwrap_or_else(|| {
                (
                    "No active connection".into(),
                    if snapshot.state == NetworkServiceState::Ready && !snapshot.wifi_enabled {
                        "Wi-Fi off"
                    } else {
                        ""
                    }
                    .into(),
                )
            });
        let mut panel = column()
            .width(Dimension::FILL)
            .height(Dimension::FILL)
            .padding(12.0)
            .gap(10.0)
            .corner_radius(10.0)
            .background(Background::Color(TASKBAR_BACKGROUND_COLOR))
            .child(
                column()
                    .width(Dimension::FILL)
                    .height(40.0)
                    .gap(4.0)
                    .child(label("Network", 16.0, COLOR5).weight(600).height(20.0))
                    .child(
                        label(
                            summary.status,
                            12.0,
                            if summary.warning { WARNING } else { COLOR7 },
                        )
                        .height(16.0),
                    ),
            )
            .child(
                row()
                    .width(Dimension::FILL)
                    .height(54.0)
                    .gap(10.0)
                    .padding(8.0)
                    .corner_radius(6.0)
                    .background(Background::Color(COLOR2.with_alpha(100)))
                    .align_items(Alignment::Center)
                    .child(
                        image(icon_source(summary.icon))
                            .width(24.0)
                            .height(24.0)
                            .tint(COLOR5),
                    )
                    .child(
                        column()
                            .width(Dimension::FILL)
                            .height(38.0)
                            .gap(2.0)
                            .child(label(name, 13.0, COLOR5).height(20.0))
                            .child(label(detail, 12.0, COLOR7).height(16.0)),
                    ),
            )
            .child(column().height(Dimension::FILL));
        if let Some(error) = &launch.error {
            panel = panel.child(label(error, 12.0, WARNING).height(28.0));
        }
        panel.child(
            super::taskbar::task_button("Open Network settings")
                .width(Dimension::FILL)
                .height(36.0)
                .padding(8.0)
                .child(label(
                    if launch.busy {
                        "Opening settings…"
                    } else {
                        "Network settings"
                    },
                    13.0,
                    COLOR5,
                ))
                .enabled(!launch.busy)
                .on_press(|this: &mut Self| this.open_settings()),
        )
    }
}
