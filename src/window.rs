use telorgon::app::*;
use telorgon::{Easing, TransitionSpec};

use crate::assets;
use crate::constants::*;
use crate::colors::*;

const fn control_visual(background: ColorRgba8, icon_tint: ColorRgba8) -> WindowControlVisual {
    WindowControlVisual {
        decoration: BoxDecoration::new()
            .background(Background::Color(background))
            .corner_radius(0.0),
        icon_tint,
    }
}

const STANDARD_BUTTON: WindowControlButtonStyle = WindowControlButtonStyle {
    width: Dimension::Logical(CONTROL_BTN_WIDTH),
    height: Dimension::FILL,
    icon_size: 12.0,

    resting: control_visual(ColorRgba8::rgba(0, 0, 0, 0), ColorRgba8::rgba(232, 236, 246, 255)),
    hovered: Some(control_visual(ColorRgba8::rgba(56, 64, 84, 255), WHITE)),
    pressed: Some(control_visual(ColorRgba8::rgba(35, 40, 54, 255), WHITE)),
    focused: Some(control_visual(ColorRgba8::rgba(43, 49, 65, 255), WHITE)),
    disabled: Some(control_visual(
        ColorRgba8::rgba(36, 40, 51, 255),
        ColorRgba8::rgba(116, 122, 140, 255),
    )),

    transition: Some(TransitionSpec {
        duration_ms: 90,
        easing: Easing::EaseOut,
        repeat: false,
    }),
};

const CLOSE_BUTTON: WindowControlButtonStyle = WindowControlButtonStyle {
    hovered: Some(control_visual(ColorRgba8::rgba(183, 52, 72, 255), WHITE)),
    pressed: Some(control_visual(ColorRgba8::rgba(98, 31, 44, 255), WHITE)),
    ..STANDARD_BUTTON
};

const NORMAL_WINDOW: WindowChromeStateStyle = WindowChromeStateStyle {
    title_bar_visible: true,
    frame_radius: FRAME_RADIUS,
    shadow: Some(Shadow {
        offset: PointF { x: 0.0, y: 4.0 },
        blur: 12.0,
        spread: 2.0,
        color: ColorRgba8::rgba(0, 0, 0, 130),
    }),
    resize_regions: true,
    resize_edge: RESIZE_EDGE,
    resize_hit_slop: Insets::all(0.0),
};

pub(crate) const WINDOW_MINIMIZE_MOTION: Minimize = Minimize::shrink_and_fade(130);

pub(crate) const WINDOW_FILL: Fill = Fill::Glass(GlassStyle {
    tint: ColorRgba8::rgba(18, 27, 41, 150),
    // Soft interior, with refraction confined to a thin rounded edge.
    blur_radius: 10.0,
    // Subtle grazing-angle reflection.
    fresnel: 0.20,
    bevel_width: 52.0,
    blend_softness: 64.0,
    refraction: 16.0,
    dispersion: 4.0,
});

pub const TEST_CHROME: WindowChromeDesign = WindowChromeDesign {
    // Cutoffs/fades are milliseconds; spring frequency is radians/second.
    // Use WindowMotion::smooth() for tweens, or WindowMotion::none() to disable motion.
    motion: WindowMotion::fluid()
        .open(telorgon::tween_ms(130, Easing::EaseOut))
        .open_from_scale(0.92)
        .maximize_spring(
            Spring::new()
                .easing(Easing::EaseInOut)
                .initial_velocity(3.0)
                .damping_ratio(0.75)
                .angular_frequency(20.0)
                .settle_within_ms(300),
        )
        .restore_spring(
            Spring::new()
                .easing(Easing::EaseInOut)
                .initial_velocity(3.0)
                .damping_ratio(0.75)
                .angular_frequency(20.0)
                .settle_within_ms(300),
        )
        .maximize_content(ContentFade::new(50, 130))
        .minimize(WINDOW_MINIMIZE_MOTION)
        .resize_content(
            ContentFade::new(120, 120)
                .to_placeholder_easing(Easing::EaseInOut)
                .to_ready_easing(Easing::EaseInOut),
        ),
    resize_preview: Some(ResizePreviewDesign {
        fill: WINDOW_FILL,
        border: Border::all(FRAME_BORDER_WIDTH, COLOR7),
        corner_radius: FRAME_RADIUS,
    }),
    active: WindowChromePalette {
        frame_background: COLOR6,
        frame_border: Border::all(FRAME_BORDER_WIDTH, COLOR2),
        title_color: ColorRgba8::rgba(245, 247, 255, 255),
        title_weight: 450,
        shadow_color: Some(ColorRgba8::rgba(0, 0, 0, 210)),
    },
    inactive: WindowChromePalette {
        frame_background: COLOR1,
        frame_border: Border::all(FRAME_BORDER_WIDTH, COLOR2),
        title_color: ColorRgba8::rgba(174, 179, 193, 255),
        title_weight: 450,
        shadow_color: Some(ColorRgba8::rgba(0, 0, 0, 130)),
    },
    normal: NORMAL_WINDOW,
    maximized: WindowChromeStateStyle {
        frame_radius: 0.0,
        shadow: None,
        resize_regions: false,
        resize_edge: 0.0,
        resize_hit_slop: Insets::ZERO,
        ..NORMAL_WINDOW
    },
    tiled: WindowChromeStateStyle {
        frame_radius: 0.0,
        shadow: None,
        ..NORMAL_WINDOW
    },
    fullscreen: WindowChromeStateStyle {
        title_bar_visible: false,
        frame_radius: 0.0,
        shadow: None,
        resize_regions: false,
        resize_edge: 0.0,
        resize_hit_slop: Insets::ZERO,
    },
    title_bar: WindowTitleBarStyle {
        font_family: UI_FONT_FAMILY,
        height: TITLE_BAR_HEIGHT,
        padding: Insets::new(0.0, 0.0, 0.0, 4.0),
        gap: 7.0,
        title_size: 14.0,
        app_icon_region_size: 32.0,
        app_icon_size: 20.0,
        show_client_icon: true,
        fallback_app_icon: None,
        app_icon_opens_system_menu: true,
    },
    controls: WindowControlsDesign {
        minimize: WindowControlDesign {
            icon: assets::icons::MINIMIZE,
            style: STANDARD_BUTTON,
        },
        maximize: WindowControlDesign {
            icon: assets::icons::MAXIMIZE,
            style: STANDARD_BUTTON,
        },
        restore: WindowControlDesign {
            icon: assets::icons::RESTORE,
            style: STANDARD_BUTTON,
        },
        close: WindowControlDesign {
            icon: assets::icons::CLOSE,
            style: CLOSE_BUTTON,
        },
        gap: 0.0,
    },
    content_background: ColorRgba8::rgba(15, 18, 26, 255),
};
