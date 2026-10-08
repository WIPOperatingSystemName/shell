use super::constants::*;
use telorgon::RectF;
use telorgon::app::*;
mod mixer;
pub(crate) use mixer::volume_icon;
use mixer::{AudioMixerPopover, application_count, popover_height};
use telorgon::host::application::audio_mixer::AudioMixerHandle;

pub(super) const AUDIO_BUTTON_WIDTH: f32 = ICON_WIDTH;
#[derive(Clone)]
pub(super) struct AudioPopupState {
    pub open: Signal<bool>,
    writer: SignalWriter<bool>,
}
impl Default for AudioPopupState {
    fn default() -> Self {
        let (open, writer) = Signal::new(false);
        Self { open, writer }
    }
}
impl PartialEq for AudioPopupState {
    fn eq(&self, other: &Self) -> bool {
        self.open == other.open
    }
}
impl AudioPopupState {
    pub fn set(&self, value: bool) {
        self.writer.publish_if_changed(value);
    }
}

#[component(no_default)]
pub(super) struct AudioPopup {
    #[input]
    mixer: AudioMixerHandle,
    #[input]
    state: AudioPopupState,
    #[input]
    anchor: RectF,
}
impl AudioPopup {
    pub fn new(mixer: AudioMixerHandle, state: AudioPopupState, anchor: RectF) -> Self {
        Self {
            mixer,
            state,
            anchor,
        }
    }
}
impl ShellWidget for AudioPopup {
    fn surface(&self) -> ShellSurfaceSpec {
        let output = self.context::<ShellContext>().output_size();
        let width = 360.0_f32.min((output.width - 16.0).max(1.0));
        let signal = self.mixer.signal();
        let snapshot = self.watch(&signal);
        let height = popover_height(application_count(&snapshot))
            .min((output.height - BAR_HEIGHT - 24.0).max(1.0));
        ShellSurfaceSpec::new()
            .placement(
                WidgetPlacement::attached_to(self.anchor, ShellEdge::Top)
                    .width(width)
                    .height(height)
                    .offset(AUDIO_BUTTON_WIDTH - width, -8.0)
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
impl Component for AudioPopup {
    fn view(&self) -> impl View {
        AudioMixerPopover::new(self.mixer.clone())
    }
}

#[component(no_default)]
pub(super) struct AudioTooltip {
    #[input]
    mixer: AudioMixerHandle,
    #[input]
    anchor: RectF,
}
impl AudioTooltip {
    pub fn new(mixer: AudioMixerHandle, anchor: RectF) -> Self {
        Self { mixer, anchor }
    }
}
impl ShellWidget for AudioTooltip {
    fn surface(&self) -> ShellSurfaceSpec {
        ShellSurfaceSpec::new()
            .placement(
                WidgetPlacement::attached_to(self.anchor, ShellEdge::Top)
                    .width(280.0)
                    .height(44.0)
                    .offset(AUDIO_BUTTON_WIDTH - 280.0, -8.0)
                    .margin(8.0),
            )
            .layer(ShellSurfaceLayer::Overlay)
            .pointer(ShellPointer::PassThrough)
    }
}
impl Component for AudioTooltip {
    fn view(&self) -> impl View {
        use telorgon::services::audio::mixer::{MixerTarget, group_mute};
        let signal = self.mixer.signal();
        let snapshot = self.watch(&signal);
        let nodes = snapshot.targets(&MixerTarget::DefaultOutput);
        let name = nodes
            .first()
            .map(|n| {
                if n.description.is_empty() {
                    n.name.as_str()
                } else {
                    n.description.as_str()
                }
            })
            .unwrap_or("Audio unavailable");
        let level = audio_status(
            &snapshot.state,
            !nodes.is_empty(),
            group_mute(&nodes),
            snapshot.volume(&MixerTarget::DefaultOutput),
        );
        column()
            .padding(10.0)
            .background(Background::Color(TASKBAR_BACKGROUND_COLOR))
            .corner_radius(6.0)
            .child(
                text(format!("{name} · {level}"))
                    .size(12.0)
                    .color(crate::colors::COLOR5),
            )
    }
}

fn audio_status(
    state: &telorgon::integrations::pipewire::ConnectionState,
    has_output: bool,
    mute: telorgon::services::audio::mixer::MuteState,
    volume: Option<f32>,
) -> String {
    use telorgon::{integrations::pipewire::ConnectionState, services::audio::mixer::MuteState};
    match state {
        ConnectionState::Connecting => "Connecting…".into(),
        ConnectionState::Failed(error) => format!("Reconnecting: {error}"),
        ConnectionState::Stopping | ConnectionState::Stopped => "Audio stopped".into(),
        ConnectionState::Ready if !has_output => "No default output".into(),
        ConnectionState::Ready if mute == MuteState::Muted => "Muted".into(),
        ConnectionState::Ready => volume
            .map(|v| format!("{:.0}%", v * 100.0))
            .unwrap_or_else(|| "Volume unavailable".into()),
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    use telorgon::{integrations::pipewire::ConnectionState, services::audio::mixer::MuteState};
    #[test]
    fn ready_without_volume_does_not_claim_reconnection() {
        assert_eq!(
            audio_status(&ConnectionState::Ready, true, MuteState::Unknown, None),
            "Volume unavailable"
        );
        assert_eq!(
            audio_status(&ConnectionState::Ready, false, MuteState::Unknown, None),
            "No default output"
        );
        assert_eq!(
            audio_status(&ConnectionState::Connecting, true, MuteState::Unknown, None),
            "Connecting…"
        );
        assert_eq!(
            audio_status(
                &ConnectionState::Ready,
                true,
                MuteState::Unmuted,
                Some(0.54)
            ),
            "54%"
        );
    }
}
