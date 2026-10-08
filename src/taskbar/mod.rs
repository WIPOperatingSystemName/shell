//! Taskbar and its owned window picker.
mod constants;
mod hover_delay;
mod taskbar;
mod window_picker;
mod window_peek;

pub(crate) use taskbar::TestTaskbar;

mod audio;
pub(crate) use audio::volume_icon;
mod battery;
mod network;

mod tray;

mod launcher;
