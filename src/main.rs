use std::path::PathBuf;

use telorgon::app::*;
use telorgon::{
    ClientCursorMode, CursorAsset, Easing, PointerGraphic, PointerIcon, PointerThemeOverrides,
    TransitionSpec,
};

asset_catalog! {
    pub mod assets = "assets";
}

const TITLE_BAR_HEIGHT: f32 = 32.0;
const FRAME_BORDER_WIDTH: f32 = 2.0;
const FRAME_RADIUS: f32 = 14.0;
const RESIZE_EDGE: f32 = 10.0;

fn cursor(asset: CursorAsset, hotspot_x: u16, hotspot_y: u16) -> PointerGraphic {
    PointerGraphic::new(asset)
        .size(32)
        .hotspot(hotspot_x, hotspot_y)
        .tint(ColorRgba8::rgba(255, 255, 255, 255))
}

fn pointer_overrides() -> PointerThemeOverrides {
    PointerThemeOverrides::new()
        .set(PointerIcon::Default, cursor(assets::cursors::ARROW, 3, 3))
        .set(
            PointerIcon::Pointer,
            cursor(assets::cursors::POINTER, 12, 4),
        )
        .set(PointerIcon::Text, cursor(assets::cursors::TEXT, 16, 16))
        .set(PointerIcon::Move, cursor(assets::cursors::MOVE, 16, 16))
        .set(
            PointerIcon::AllResize,
            cursor(assets::cursors::MOVE, 16, 16),
        )
        .set(
            PointerIcon::EResize,
            cursor(assets::cursors::RESIZE_EW, 16, 16),
        )
        .set(
            PointerIcon::WResize,
            cursor(assets::cursors::RESIZE_EW, 16, 16),
        )
        .set(
            PointerIcon::EwResize,
            cursor(assets::cursors::RESIZE_EW, 16, 16),
        )
        .set(
            PointerIcon::ColResize,
            cursor(assets::cursors::RESIZE_EW, 16, 16),
        )
        .set(
            PointerIcon::NResize,
            cursor(assets::cursors::RESIZE_NS, 16, 16),
        )
        .set(
            PointerIcon::SResize,
            cursor(assets::cursors::RESIZE_NS, 16, 16),
        )
        .set(
            PointerIcon::NsResize,
            cursor(assets::cursors::RESIZE_NS, 16, 16),
        )
        .set(
            PointerIcon::RowResize,
            cursor(assets::cursors::RESIZE_NS, 16, 16),
        )
}

const WHITE: ColorRgba8 = ColorRgba8::rgba(255, 255, 255, 255);

// Shared appearance for every control and interaction state.
const fn control_visual(
    background: ColorRgba8,
    icon_tint: ColorRgba8,
) -> WindowControlVisual {
    WindowControlVisual {
        decoration: BoxDecoration::new()
            .background(Background::Color(background))
            .corner_radius(0.0),
        icon_tint,
    }
}

const STANDARD_BUTTON: WindowControlButtonStyle = WindowControlButtonStyle {
    width: Dimension::Pixels(38.0),
    height: Dimension::FILL,
    icon_size: 15.0,

    resting: control_visual(
        ColorRgba8::rgba(43, 49, 65, 255),
        ColorRgba8::rgba(232, 236, 246, 255),
    ),
    hovered: Some(control_visual(
        ColorRgba8::rgba(56, 64, 84, 255),
        WHITE,
    )),
    pressed: Some(control_visual(
        ColorRgba8::rgba(35, 40, 54, 255),
        WHITE,
    )),
    focused: Some(control_visual(
        ColorRgba8::rgba(43, 49, 65, 255),
        WHITE,
    )),
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
    resting: control_visual(
        ColorRgba8::rgba(121, 42, 55, 255),
        ColorRgba8::rgba(255, 230, 234, 255),
    ),
    hovered: Some(control_visual(
        ColorRgba8::rgba(183, 52, 72, 255),
        WHITE,
    )),
    pressed: Some(control_visual(
        ColorRgba8::rgba(98, 31, 44, 255),
        WHITE,
    )),
    ..STANDARD_BUTTON
};

const NORMAL_WINDOW: WindowChromeStateStyle = WindowChromeStateStyle {
    title_bar_visible: true,
    frame_radius: FRAME_RADIUS,
    shadow: Some(Shadow {
        offset: PointF { x: 0.0, y: 12.0 },
        blur: 30.0,
        spread: 0.0,
        color: ColorRgba8::rgba(0, 0, 0, 128),
    }),
    resize_regions: true,
    resize_edge: RESIZE_EDGE,
    resize_hit_slop: Insets::all(0.0),
};

const TEST_CHROME: WindowChromeDesign = WindowChromeDesign {
    resize_preview_color: Some(ColorRgba8::rgba(23, 27, 37, 150)),
    active: WindowChromePalette {
        frame_background: ColorRgba8::rgba(16, 201, 44, 255),
        frame_border: ColorRgba8::rgba(101, 119, 184, 255),
        frame_border_width: FRAME_BORDER_WIDTH,
        title_color: ColorRgba8::rgba(245, 247, 255, 255),
        title_weight: 650,
    },
    inactive: WindowChromePalette {
        frame_background: ColorRgba8::rgba(16, 201, 44, 255),
        frame_border: ColorRgba8::rgba(65, 70, 85, 255),
        frame_border_width: FRAME_BORDER_WIDTH,
        title_color: ColorRgba8::rgba(174, 179, 193, 255),
        title_weight: 450,
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
        height: TITLE_BAR_HEIGHT,
        padding: Insets::new(0.0, 0.0, 0.0, 8.8),
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
        gap: 7.0,
    },
    content_background: ColorRgba8::rgba(15, 18, 26, 255),
};

#[component]
struct DesktopBackground {}

impl Component for DesktopBackground {
    fn view(&self) -> impl View {
        stack()
            .decoration(
                BoxDecoration::new()
                    .background(Background::Color(ColorRgba8::rgba(10, 12, 18, 255))),
            )
            .child(
                image(assets::images::WALLPAPER)
                    .width(Dimension::FILL)
                    .height(Dimension::FILL),
            )
    }
}

#[component]
struct TestPanel {}

impl Component for TestPanel {
    fn view(&self) -> impl View {
        row()
            .height(42.0)
            .padding((7.0, 14.0))
            .gap(10.0)
            .align_items(Alignment::Center)
            .decoration(
                BoxDecoration::new()
                    .background(Background::Color(ColorRgba8::rgba(17, 20, 29, 100)))
                    .uniform_border(1.0, ColorRgba8::rgba(58, 65, 82, 255)),
            )
            .child(text("TELORGON TEST DESKTOP").size(13.0).weight(700))
            .child(spacer())
            .child(
                text("custom chrome • client icons • code-defined pointers")
                    .size(12.0)
                    .color(ColorRgba8::rgba(155, 164, 188, 255))
                    .pointer_icon(PointerIcon::Pointer),
            )
    }
}

fn main() -> telorgon::Result<()> {
    let linux = LinuxDesktopConfig {
        drm_device: PathBuf::from("/dev/dri/card1"),
        socket_name: Some("telorgon-0".into()),
        ..LinuxDesktopConfig::default()
    };

    let chrome = TEST_CHROME
        .validate()
        .expect("TEST_CHROME must contain valid finite metrics");

    Application::desktop_environment("Telorgon Test Compositor")
        .linux(linux)
        .renderer(Renderer::Vulkan)
        .assets(assets::bundle())
        .pointer_overrides(pointer_overrides())
        .client_cursor_mode(ClientCursorMode::Allow)
        .compositor(
            Compositor::new()
                .window_frame(easy_window_frame(chrome))
                .background(DesktopBackground::default()),
        )
        .shell_widget(
            ShellWidget::new("panel")
                .anchor(ShellWidgetAnchor::Top)
                .height(ShellWidgetExtent::Pixels(42.0))
                .reserve_space(42.0)
                .content(TestPanel::default()),
        )
        .run()
}
