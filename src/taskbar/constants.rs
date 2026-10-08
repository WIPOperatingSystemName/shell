//! Taskbar and window picker appearance. All dimensions are logical units.
use crate::colors::*;
use telorgon::app::ColorRgba8;

// Taskbar
pub(super) const TASKBAR_BACKGROUND_COLOR: ColorRgba8 = COLOR1;
// Set width to 0.0 to hide the top border.
pub(super) const TASKBAR_TOP_BORDER_WIDTH: f32 = 1.5;
pub(super) const TASKBAR_TOP_BORDER_COLOR: ColorRgba8 = COLOR2;

// Button size is independent of the image drawn inside it.
pub(super) const ICON_WIDTH: f32 = 42.0;
// Shared artwork height; non-square icons retain their authored aspect ratio.
pub(super) const TASKBAR_ICON_SIZE: u32 = 28;
pub(super) const TASKBAR_AUDIO_ICON_WIDTH: f32 = TASKBAR_ICON_SIZE as f32 * 26.0 / 24.0;
pub(super) const TASKBAR_BUTTON_RADIUS: f32 = 5.0;
pub(super) const TASKBAR_BUTTON_HOVER_COLOR: ColorRgba8 = COLOR4;
pub(super) const TASKBAR_BUTTON_ACTIVE_COLOR: ColorRgba8 = COLOR4;
pub(super) const TASKBAR_BUTTON_IDLE_COLOR: ColorRgba8 = ColorRgba8::rgba(0, 0, 0, 0);
// Tween into active/inactive colors, including when hover or press ends.
pub(super) const TASKBAR_COLOR_DURATION_MS: u32 = 150;
pub(super) const TASKBAR_COLOR_EASING: telorgon::Easing = telorgon::Easing::EaseOut;
// Tween into hover: smaller durations are faster; 0 disables the animation.
pub(super) const TASKBAR_HOVER_DURATION_MS: u32 = 150;
pub(super) const TASKBAR_HOVER_EASING: telorgon::Easing = telorgon::Easing::EaseOut;

// Mouse-down appearance; releasing uses the hover transition above.
pub(super) const TASKBAR_BUTTON_PRESSED_COLOR: ColorRgba8 = COLOR3;
pub(super) const TASKBAR_PRESS_DURATION_MS: u32 = 90;
pub(super) const TASKBAR_PRESS_EASING: telorgon::Easing = telorgon::Easing::EaseOut;

// Indicator width = base + step per extra window, capped at MAX_WINDOWS.
pub(super) const WINDOW_COUNT_BAR_BASE_WIDTH: f32 = 6.0;
pub(super) const WINDOW_COUNT_BAR_WIDTH_STEP: f32 = 6.0;
pub(super) const WINDOW_COUNT_BAR_MAX_WINDOWS: usize = 5; // Must be at least 1.
pub(super) const WINDOW_COUNT_BAR_HEIGHT: f32 = 3.0;
pub(super) const WINDOW_COUNT_BAR_RADIUS: f32 = 1.5;
pub(super) const WINDOW_COUNT_BAR_COLOR: ColorRgba8 = ColorRgba8::rgba(143, 145, 154, 255);

pub(super) const ICON_GAP: f32 = 8.0;
pub(super) const BAR_PADDING: f32 = 8.0;
pub(super) const BAR_HEIGHT: f32 = ICON_WIDTH + BAR_PADDING * 2.0 + TASKBAR_TOP_BORDER_WIDTH;
pub(super) const IDLE: ColorRgba8 = ColorRgba8::rgba(40, 40, 45, 255);

// Window picker
pub(super) const PICKER_TITLE_COLOR: ColorRgba8 = COLOR5;
// Rectangular container behind the entire tile group.
pub(super) const PICKER_CONTAINER_BACKGROUND_COLOR: ColorRgba8 = COLOR1;
// Card and list-row appearance.
pub(super) const PICKER_CARD_BACKGROUND_COLOR: ColorRgba8 = COLOR1;
pub(super) const PICKER_CARD_RADIUS: f32 = 5.0;
pub(super) const PICKER_CARD_HOVER_COLOR: ColorRgba8 = COLOR2;
// Tween both entering and leaving hover; 0 disables the animation.
pub(super) const PICKER_CARD_HOVER_DURATION_MS: u32 = 150;
pub(super) const PICKER_CARD_HOVER_EASING: telorgon::Easing = telorgon::Easing::EaseOut;
pub(super) const PICKER_HOVER_DELAY_MS: u64 = 1000;
// Delay before a tile temporarily isolates its window on the desktop.
// Once active, switching tiles in the picker is immediate.
pub(super) const PICKER_WINDOW_PEEK_DELAY_MS: u64 = 700;
pub(super) const CARD_WIDTH: f32 = 216.0;
// Uniform outer inset and gap between each tile's header and preview.
pub(super) const PICKER_PADDING: f32 = 6.0;
pub(super) const PICKER_HEADER_HEIGHT: f32 = 26.0;
pub(super) const CARD_HEIGHT: f32 = PREVIEW_HEIGHT + PICKER_HEADER_HEIGHT + 3.0 * PICKER_PADDING;
pub(super) const PREVIEW_HEIGHT: f32 = 128.0;
pub(super) const MAX_PREVIEW_WIDTH: f32 = 320.0;
pub(super) const ROW_HEIGHT: f32 = 40.0;
// App icon size in the card header (and overflow list).
pub(super) const HEADER_ICON_SIZE: f32 = 26.0;
pub(super) const PICKER_CLOSE_BUTTON_SIZE: f32 = 26.0;
// Icon size is independent of the close button's hit area.
pub(super) const PICKER_CLOSE_ICON_SIZE: f32 = 12.0;
pub(super) const PICKER_CLOSE_ICON_COLOR: ColorRgba8 = COLOR5;
// Tween into and out of hover; 0 disables the animation.
pub(super) const PICKER_CLOSE_HOVER_DURATION_MS: u32 = 150;
pub(super) const PICKER_CLOSE_HOVER_EASING: telorgon::Easing = telorgon::Easing::EaseOut;
pub(super) const CLOSE_HOVER: ColorRgba8 = ColorRgba8::rgba(196, 43, 28, 255);
pub(super) const CLOSE_PRESSED: ColorRgba8 = ColorRgba8::rgba(160, 35, 24, 255);
