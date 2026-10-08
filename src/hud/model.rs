use std::time::{Duration, Instant};
use telorgon::app::{SystemShortcutControl, SystemShortcutFeedback};
use telorgon::host::application::audio_mixer::MixerSnapshot;
use telorgon::integrations::pipewire::ConnectionState;
use telorgon::screen_brightness::{
    ScreenBrightnessError, ScreenBrightnessKind, ScreenBrightnessServiceState,
    ScreenBrightnessSnapshot,
};
use telorgon::services::audio::mixer::{MixerTarget, MuteState, group_mute, group_volume};

pub(super) const DISMISS_AFTER: Duration = Duration::from_secs(2);
const ADMISSION_GRACE: Duration = Duration::from_millis(120);
// Native owners have their own deadlines. Never leave an overlay visible forever if they stall.
const MAXIMUM_WAIT: Duration = Duration::from_secs(12);

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(super) enum Status {
    #[default]
    Ready,
    Pending,
    Unavailable,
    Restricted,
    Ambiguous,
    Failed,
    Unconfirmed,
}
impl Status {
    pub fn label(self) -> &'static str {
        match self {
            Self::Ready | Self::Pending => "",
            Self::Unavailable => "Unavailable",
            Self::Restricted => "Control unavailable",
            Self::Ambiguous => "Multiple displays",
            Self::Failed => "Couldn’t apply",
            Self::Unconfirmed => "Change not confirmed",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct Reading {
    pub level: Option<f32>,
    pub muted: bool,
    pub status: Status,
}
impl Default for Reading {
    fn default() -> Self {
        Self {
            level: None,
            muted: false,
            status: Status::Unavailable,
        }
    }
}

pub(super) fn volume_reading(snapshot: &MixerSnapshot) -> Reading {
    if snapshot.state != ConnectionState::Ready {
        return Reading::default();
    }
    let target = MixerTarget::DefaultOutput;
    let nodes = snapshot.targets(&target);
    // MixerSnapshot::volume includes an optimistic slider preview; HUDs use observed nodes.
    let level = group_volume(&nodes).filter(|level| level.is_finite());
    let operation = snapshot.operations.get(&target);
    let status = if level.is_none() {
        Status::Unavailable
    } else if operation.is_some_and(|state| state.pending) {
        Status::Pending
    } else if operation.is_some_and(|state| state.error.is_some() || state.failed > 0) {
        Status::Failed
    } else {
        Status::Ready
    };
    Reading {
        level,
        muted: group_mute(&nodes) == MuteState::Muted,
        status,
    }
}

pub(super) fn brightness_reading(snapshot: &ScreenBrightnessSnapshot) -> Reading {
    let mut devices = snapshot
        .devices
        .iter()
        .filter(|device| device.kind == ScreenBrightnessKind::InternalBacklight);
    let Some(device) = devices.next() else {
        return Reading::default();
    };
    if devices.next().is_some() {
        return Reading {
            status: Status::Ambiguous,
            ..Reading::default()
        };
    }
    let status = if snapshot.state == ScreenBrightnessServiceState::Restricted
        || !device.permission.allows_use()
    {
        Status::Restricted
    } else if snapshot.state != ScreenBrightnessServiceState::Ready {
        Status::Unavailable
    } else if snapshot.last_error == Some(ScreenBrightnessError::TimedOut) {
        Status::Unconfirmed
    } else if snapshot.pending > 0 {
        Status::Pending
    } else if snapshot.last_error.is_some() {
        Status::Failed
    } else if device.configured.is_none() {
        Status::Unavailable
    } else {
        Status::Ready
    };
    // Driver setting readback is authoritative; a pending target is not a measured level.
    Reading {
        level: device.configured.map(|level| level.as_percent() / 100.0),
        muted: false,
        status,
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
pub(super) struct Panel {
    pub visible: bool,
    pub reading: Reading,
}
#[derive(Clone, Debug, Default, PartialEq)]
pub(super) struct HudState {
    pub control: Option<SystemShortcutControl>,
    pub panel: Panel,
}

#[derive(Default)]
struct TimedPanel {
    panel: Panel,
    observed: Reading,
    expires: Option<Instant>,
    awaiting: Option<Instant>,
    wait_until: Option<Instant>,
    rejected: bool,
}
impl TimedPanel {
    fn request(&mut self, accepted: bool, reading: Reading, now: Instant) {
        self.panel.visible = true;
        self.observed = reading;
        self.expires = Some(now + DISMISS_AFTER);
        self.awaiting = accepted.then_some(now + ADMISSION_GRACE);
        self.wait_until = accepted.then_some(now + MAXIMUM_WAIT);
        self.rejected = !accepted;
    }

    fn observe(&mut self, reading: Reading, now: Instant) {
        if !self.panel.visible {
            return;
        }
        if reading != self.observed {
            self.observed = reading;
            self.awaiting = None;
            self.expires = Some(now + DISMISS_AFTER);
        }
        let mut displayed = reading;
        if self.rejected && matches!(reading.status, Status::Ready | Status::Pending) {
            displayed.status = Status::Failed;
        } else if reading.status == Status::Pending
            || (reading.status == Status::Ready && self.awaiting.is_some_and(|until| now < until))
        {
            if self.wait_until.is_some_and(|until| now >= until) {
                displayed.status = Status::Unconfirmed;
            } else {
                displayed.status = Status::Pending;
                self.expires = Some(now + DISMISS_AFTER);
            }
        }
        self.panel.reading = displayed;
        if self.expires.is_some_and(|until| now >= until) {
            self.panel.visible = false;
        }
    }
}

#[derive(Default)]
pub(super) struct HudModel {
    control: Option<SystemShortcutControl>,
    panel: TimedPanel,
}
impl HudModel {
    pub fn request(
        &mut self,
        feedback: SystemShortcutFeedback,
        volume: Reading,
        brightness: Reading,
        now: Instant,
    ) {
        let reading = match feedback.control {
            SystemShortcutControl::Volume => volume,
            SystemShortcutControl::ScreenBrightness => brightness,
            SystemShortcutControl::Microphone => return,
        };
        self.control = Some(feedback.control);
        self.panel.request(feedback.accepted, reading, now);
    }
    pub fn observe(&mut self, volume: Reading, brightness: Reading, now: Instant) {
        match self.control {
            Some(SystemShortcutControl::Volume) => self.panel.observe(volume, now),
            Some(SystemShortcutControl::ScreenBrightness) => self.panel.observe(brightness, now),
            _ => {}
        }
    }
    pub fn active(&self) -> bool {
        self.panel.panel.visible
    }
    pub fn state(&self) -> HudState {
        HudState {
            control: self.control,
            panel: self.panel.panel.clone(),
        }
    }
}
