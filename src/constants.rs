use telorgon::app::*;
use telorgon::{Easing, TransitionSpec};

pub const TITLE_BAR_HEIGHT: f32 = 32.0;
// Preferred content size in logical units, excluding the frame. Fixed-size clients may be smaller.
pub const MINIMUM_WINDOW_SIZE: telorgon::SizeI = telorgon::SizeI {
    width: 300,
    height: 200,
};
pub const FRAME_BORDER_WIDTH: f32 = 1.5;
pub const FRAME_RADIUS: f32 = 12.0;
pub const RESIZE_EDGE: f32 = 10.0;
pub const CONTROL_BTN_WIDTH: f32 = 42.0;
pub const WHITE: ColorRgba8 = ColorRgba8::rgba(255, 255, 255, 255);
// Embedded family name of the Inter static faces used by compositor-owned titles.
pub const UI_FONT_FAMILY: &str = "Inter 18pt";
