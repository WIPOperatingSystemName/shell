use super::{
    model::{HudState, Panel, Status},
    owner::HudHandle,
};
use crate::colors::{COLOR1, COLOR5, COLOR7};
use telorgon::app::*;

pub(super) const HUD_WIDTH: f32 = 280.0;
pub(super) const HUD_HEIGHT: f32 = 58.0;
const MARGIN: f32 = 14.0;
const ACCENT: ColorRgba8 = ColorRgba8::rgba(124, 178, 255, 255);
const WARNING: ColorRgba8 = ColorRgba8::rgba(255, 199, 120, 255);

#[derive(Clone, Copy)]
enum Kind {
    Volume,
    ScreenBrightness,
}
impl Kind {
    fn title(self) -> &'static str {
        match self {
            Self::Volume => "Volume",
            Self::ScreenBrightness => "Screen brightness",
        }
    }
}

fn card(kind: Kind, panel: &Panel) -> telorgon::compose::Container {
    let reading = panel.reading;
    let icon = match kind {
        Kind::Volume => crate::taskbar::volume_icon(reading.level.unwrap_or(0.0), reading.muted),
        Kind::ScreenBrightness => crate::assets::icons::SCREEN_BRIGHTNESS.into(),
    };
    let value = reading
        .level
        .map_or_else(|| "—".into(), |level| format!("{:.0}%", level * 100.0));
    let status = if matches!(reading.status, Status::Ready | Status::Pending) && reading.muted {
        "Muted"
    } else {
        reading.status.label()
    };
    let warning = !matches!(reading.status, Status::Ready | Status::Pending);
    let level_color = if warning {
        WARNING
    } else if reading.muted {
        COLOR7
    } else {
        ACCENT
    };
    let accessible_label = if status.is_empty() {
        format!("{}, {value}", kind.title())
    } else {
        format!("{}, {value}, {status}", kind.title())
    };
    let mut track = stack()
        .key("hud-level-track")
        .width(Dimension::FILL)
        .height(6.0)
        .align_items(Alignment::Start)
        .justify_content(Alignment::Start)
        .background(COLOR5.with_alpha(40))
        .corner_radius(3.0)
        .overflow(telorgon::ui::Overflow::Clip);
    if let Some(level) = reading.level.filter(|level| *level > 0.0) {
        track = track.child(
            stack()
                .key("hud-level-fill")
                .width(Dimension::Percent(level.clamp(0.0, 1.0)))
                .height(6.0)
                .corner_radius(3.0)
                .background(level_color),
        );
    }
    row()
        .width(Dimension::FILL)
        .height(Dimension::FILL)
        .padding(14.0)
        .gap(12.0)
        .align_items(Alignment::Center)
        .background(COLOR1.with_alpha(240))
        .uniform_border(1.0, COLOR7.with_alpha(90))
        .corner_radius(18.0)
        .child(
            image(icon)
                .width(28.0)
                .height(28.0)
                .tint(if warning { WARNING } else { COLOR5 })
                .accessible_label(accessible_label),
        )
        .child(track)
}

fn surface(output: SizeF, visible: bool) -> ShellSurfaceSpec {
    ShellSurfaceSpec::new()
        .placement(
            WidgetPlacement::aligned(0.5, 0.0)
                .width(HUD_WIDTH.min((output.width - MARGIN * 2.0).max(1.0)))
                .height(HUD_HEIGHT)
                .margin(MARGIN),
        )
        .layer(ShellSurfaceLayer::Overlay)
        .order(i32::MAX - 2)
        .pointer(ShellPointer::PassThrough)
        .focus(ShellFocus::None)
        .visibility_motion(crate::window::WINDOW_MINIMIZE_MOTION)
        .visible(visible)
}

pub(super) fn content(state: &HudState) -> telorgon::compose::Container {
    let kind = match state.control {
        Some(SystemShortcutControl::ScreenBrightness) => Kind::ScreenBrightness,
        _ => Kind::Volume,
    };
    card(kind, &state.panel)
}

#[component(no_default)]
pub(crate) struct SystemHud {
    #[input]
    handle: HudHandle,
}
impl SystemHud {
    pub fn new(huds: &super::SystemHuds) -> Self {
        Self {
            handle: huds.handle(),
        }
    }
}
impl Component for SystemHud {
    fn view(&self) -> impl View {
        content(&self.watch(&self.handle.state))
    }
}
impl ShellWidget for SystemHud {
    fn surface(&self) -> ShellSurfaceSpec {
        let state = self.watch(&self.handle.state);
        surface(
            self.context::<ShellContext>().output_size(),
            state.panel.visible,
        )
    }
}
