use crate::{assets, session_pixels, settings_config::BACKGROUND_IMAGE};
use std::os::fd::OwnedFd;
use std::sync::{Arc, atomic::{AtomicU64, Ordering}};
use telorgon::app::*;
use telorgon::{BoxStyle, SizeI, SizeRule, SizeRule2D, Transform2D};
use telorgon::graphics::render::{ImageAlphaMode, ImageColorEncoding, ImagePixelFormat, ImageResource};
use telorgon::services::desktop_settings::{PersonalizationSettings, PersonalizationSnapshot};

#[derive(Clone, Default, PartialEq)]
struct State {
    snapshot: PersonalizationSnapshot,
    image: Option<ImageResource>,
}

#[derive(Clone)]
pub(super) struct Control {
    signal: Signal<State>,
    writer: SignalWriter<State>,
    revision: Arc<AtomicU64>,
}
impl PartialEq for Control {
    fn eq(&self, other: &Self) -> bool { self.signal == other.signal }
}
impl Control {
    pub fn new() -> Self {
        let (signal, writer) = Signal::new(State::default());
        Self { signal, writer, revision: Arc::new(AtomicU64::new(1)) }
    }
    pub fn snapshot(&self) -> PersonalizationSnapshot { self.signal.snapshot().snapshot.clone() }
    pub fn reset(&self) { self.writer.publish(State::default()); }

    // Called under the renderer's session-operation lock. No user path or image
    // decoder enters this process, and logout releases all personal pixel data.
    pub fn apply(&self, configuration: &str, mut descriptors: Vec<OwnedFd>) -> std::result::Result<(), String> {
        if configuration.len() > 8192 { return Err("Wallpaper message exceeds its limit".into()); }
        let (desired, width, height, error): (PersonalizationSettings, i32, i32, Option<String>) =
            serde_json::from_str(configuration).map_err(|error| error.to_string())?;
        desired.validate()?;
        if let Some(error) = error {
            if !descriptors.is_empty() || width != 0 || height != 0 || error.len() > 4096 {
                return Err("Invalid wallpaper error message".into());
            }
            let mut state = (*self.signal.snapshot()).clone();
            state.snapshot.error = Some(error);
            self.writer.publish(state);
            return Ok(());
        }
        let image = if desired.background.is_some() {
            if descriptors.len() != 1 { return Err("Missing wallpaper pixels".into()); }
            Some(ImageResource {
                image: BACKGROUND_IMAGE,
                content_version: self.revision.fetch_add(1, Ordering::AcqRel) + 1,
                extent: SizeI { width, height },
                color_encoding: ImageColorEncoding::Srgb,
                alpha_mode: ImageAlphaMode::Straight,
                pixel_format: ImagePixelFormat::Rgba8,
                pixels: session_pixels::read(descriptors.pop().unwrap(), width, height)?.into(),
            })
        } else {
            if !descriptors.is_empty() || width != 0 || height != 0 { return Err("Unexpected default wallpaper pixels".into()); }
            None
        };
        self.writer.publish(State { snapshot: PersonalizationSnapshot { current: desired, error: None }, image });
        Ok(())
    }
}

#[component(no_default)]
pub(super) struct SystemBackground {
    #[input]
    control: Control,
}
impl SystemBackground {
    pub fn new(control: Control) -> Self { Self { control } }
}
impl Component for SystemBackground {
    fn view(&self) -> impl View {
        let state = self.watch(&self.control.signal);
        let wallpaper = match &state.image {
            Some(resource) => {
                let (size, offset) = crate::background::cover(resource.extent, self.viewport_size());
                Image::resource(resource.clone()).box_style(BoxStyle {
                    width: SizeRule::Logical(size.width), height: SizeRule::Logical(size.height),
                    max_size: SizeRule2D { width: SizeRule::Logical(f32::MAX), height: SizeRule::Logical(f32::MAX) },
                    transform: Transform2D { translation: offset, ..Default::default() },
                    ..Default::default()
                })
            }
            None => image(assets::images::WALLPAPER).width(Dimension::FILL).height(Dimension::FILL),
        };
        stack().width(Dimension::FILL).height(Dimension::FILL)
            .overflow(telorgon::Overflow::Clip)
            .background(ColorRgba8::rgba(38, 42, 48, 255)).child(wallpaper)
    }
}
impl ShellWidget for SystemBackground {
    fn surface(&self) -> ShellSurfaceSpec {
        ShellSurfaceSpec::new().placement(WidgetPlacement::fill())
            .layer(ShellSurfaceLayer::Background).pointer(ShellPointer::PassThrough)
    }
}
