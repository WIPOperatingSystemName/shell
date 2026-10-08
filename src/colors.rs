use telorgon::app::*;
use telorgon::{Easing, TransitionSpec};



pub const COLOR1: ColorRgba8 = ColorRgba8::rgba(21, 25, 34, 255);
pub const COLOR2: ColorRgba8 = COLOR1.lighten(0.15);
pub const COLOR3: ColorRgba8 = COLOR1.lighten(0.05).with_alpha(200);
pub const COLOR4: ColorRgba8 = COLOR1.lighten(0.10).with_alpha(200);
pub const COLOR5: ColorRgba8 = COLOR1.lighten(0.90);
pub const COLOR6: ColorRgba8 = COLOR1.lighten(0.02);
pub const COLOR7: ColorRgba8 = COLOR1.lighten(0.50).with_alpha(150);