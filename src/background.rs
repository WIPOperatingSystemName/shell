#[cfg(test)]
use crate::settings_config::store_at;
use crate::{assets, settings_config::BACKGROUND_IMAGE};
use std::{
    collections::VecDeque,
    sync::{
        Arc, Mutex,
        atomic::{AtomicU64, Ordering},
    },
};
use telorgon::services::desktop_settings::{
    self as shared, PersonalizationSettings, PersonalizationSnapshot, SettingsStore,
    ValidatedBackground,
};
use telorgon::{
    BoxStyle, PointF, SizeF, SizeI, SizeRule, SizeRule2D, Transform2D, app::*,
    graphics::render::ImageResource,
};

const BACKGROUND_CACHE_BYTES: usize = 128 * 1024 * 1024;
const BACKGROUND_CACHE_ENTRIES: usize = 2;

#[derive(Clone, Debug, PartialEq)]
struct BackgroundState {
    snapshot: PersonalizationSnapshot,
    resource: Option<ImageResource>,
    validated: Option<ValidatedBackground>,
}

struct CachedBackground {
    validated: ValidatedBackground,
    resource: ImageResource,
}

#[derive(Default)]
struct BackgroundCache {
    inactive: VecDeque<CachedBackground>,
}
impl BackgroundCache {
    fn take(&mut self, preferences: &PersonalizationSettings) -> Option<CachedBackground> {
        let index = self
            .inactive
            .iter()
            .position(|entry| entry.validated.settings() == preferences)?;
        self.inactive.remove(index)
    }

    fn retain_previous(&mut self, previous: &BackgroundState, current: &BackgroundState) {
        if let (Some(resource), Some(validated)) = (&previous.resource, &previous.validated) {
            self.take(validated.settings());
            self.inactive.push_front(CachedBackground {
                validated: validated.clone(),
                resource: resource.clone(),
            });
        }
        self.trim(
            current
                .resource
                .as_ref()
                .map_or(0, |image| image.pixels.len()),
            current.resource.is_some(),
        );
    }

    fn trim(&mut self, current_bytes: usize, has_current_image: bool) {
        let budget = BACKGROUND_CACHE_BYTES.saturating_sub(current_bytes);
        let entry_limit = BACKGROUND_CACHE_ENTRIES - usize::from(has_current_image);
        // Count the current pixels once even when a reader retains its shared Arc.
        // An image larger than the budget remains usable as the current image,
        // but leaves no retained inactive images.
        while self.inactive.len() > entry_limit
            || self
                .inactive
                .iter()
                .map(|entry| entry.resource.pixels.len())
                .sum::<usize>()
                > budget
        {
            self.inactive.pop_back();
        }
    }
}

#[derive(Clone)]
pub struct BackgroundControl {
    store: Arc<SettingsStore>,
    signal: Signal<BackgroundState>,
    writer: SignalWriter<BackgroundState>,
    revision: Arc<AtomicU64>,
    cache: Arc<Mutex<BackgroundCache>>,
    default_resource: Option<ImageResource>,
}
impl PartialEq for BackgroundControl {
    fn eq(&self, other: &Self) -> bool {
        self.signal == other.signal
    }
}
impl BackgroundControl {
    pub fn new(store: SettingsStore, preferences: PersonalizationSettings) -> Self {
        let library_error = store
            .ensure_background_directory()
            .err()
            .map(|error| format!("Couldn't prepare the background library: {error}"));
        let default_resource = telorgon::assets::AssetMediaCache::new(assets::bundle())
            .and_then(|mut cache| cache.image(assets::images::WALLPAPER, None))
            .map(|image| image.render_resource())
            .ok();
        let mut state = match store.load_background_validated(&preferences, BACKGROUND_IMAGE) {
            Ok((resource, validated)) => BackgroundState {
                snapshot: PersonalizationSnapshot {
                    current: preferences,
                    error: None,
                },
                resource,
                validated,
            },
            Err(error) => BackgroundState {
                snapshot: PersonalizationSnapshot {
                    current: PersonalizationSettings::default(),
                    error: Some(format!("Using the default background: {error}")),
                },
                resource: None,
                validated: None,
            },
        };
        if let Some(error) = library_error {
            state.snapshot.error = Some(match state.snapshot.error.take() {
                Some(background_error) => format!("{error}. {background_error}"),
                None => error,
            });
        }
        let (signal, writer) = Signal::new(state);
        Self {
            store: Arc::new(store),
            signal,
            writer,
            revision: Arc::new(AtomicU64::new(1)),
            cache: Arc::new(Mutex::new(BackgroundCache::default())),
            default_resource,
        }
    }
    pub fn snapshot(&self) -> PersonalizationSnapshot {
        self.signal.snapshot().snapshot.clone()
    }
    // SettingsService serializes calls and runs decoding on a blocking worker.
    // Persisted preferences belong to the settings client; this owns applied state.
    pub fn apply(&self, preferences: PersonalizationSettings) -> shared::Result<()> {
        let mut cache = self.cache.lock().unwrap_or_else(|error| error.into_inner());
        let previous = self.signal.snapshot();
        let result = if preferences == previous.snapshot.current {
            previous
                .validated
                .as_ref()
                .map_or(Ok(()), |validated| validated.verify(&self.store))
                .map(|()| (previous.resource.clone(), previous.validated.clone()))
        } else if let Some(cached) = cache.take(&preferences) {
            cached
                .validated
                .verify(&self.store)
                .map(|()| (Some(cached.resource), Some(cached.validated)))
        } else {
            self.store
                .load_background_validated(&preferences, BACKGROUND_IMAGE)
        };
        match result {
            Ok((mut resource, validated)) => {
                // Re-selecting the applied image clears an earlier error without
                // changing its revision and causing a needless GPU upload.
                if preferences == previous.snapshot.current {
                    if previous.snapshot.error.is_some() {
                        let mut state = (*previous).clone();
                        state.snapshot.error = None;
                        self.writer.publish(state);
                    }
                    return Ok(());
                }
                if let Some(resource) = &mut resource {
                    resource.content_version = self.revision.fetch_add(1, Ordering::AcqRel) + 1;
                }
                let state = BackgroundState {
                    snapshot: PersonalizationSnapshot {
                        current: preferences,
                        error: None,
                    },
                    resource,
                    validated,
                };
                cache.retain_previous(&previous, &state);
                self.writer.publish(state);
                Ok(())
            }
            Err(error) => {
                let mut state = (*previous).clone();
                state.snapshot.error = Some(error.clone());
                self.writer.publish(state);
                Err(error)
            }
        }
    }
}

#[component(no_default)]
pub struct ShellBackground {
    #[input]
    control: BackgroundControl,
}
impl ShellBackground {
    pub fn new(control: BackgroundControl) -> Self {
        Self { control }
    }
}
impl Component for ShellBackground {
    fn view(&self) -> impl View {
        let state = self.watch(&self.control.signal);
        let resource = state
            .resource
            .as_ref()
            .or(self.control.default_resource.as_ref());
        let wallpaper = match resource {
            Some(resource) => {
                let (size, offset) = cover(resource.extent, self.viewport_size());
                Image::resource(resource.clone()).box_style(BoxStyle {
                    width: SizeRule::Logical(size.width),
                    height: SizeRule::Logical(size.height),
                    max_size: SizeRule2D {
                        width: SizeRule::Logical(f32::MAX),
                        height: SizeRule::Logical(f32::MAX),
                    },
                    // Layout centers overflowing children at zero, so translate the
                    // excess equally off both sides while the parent clips it.
                    transform: Transform2D {
                        translation: offset,
                        ..Default::default()
                    },
                    ..Default::default()
                })
            }
            None => image(assets::images::WALLPAPER)
                .width(Dimension::FILL)
                .height(Dimension::FILL),
        };
        stack()
            .width(Dimension::FILL)
            .height(Dimension::FILL)
            .overflow(telorgon::Overflow::Clip)
            .background(ColorRgba8::rgba(102, 121, 150, 255))
            .child(wallpaper)
    }
}
impl ShellWidget for ShellBackground {
    fn surface(&self) -> ShellSurfaceSpec {
        ShellSurfaceSpec::new()
            .placement(WidgetPlacement::fill())
            .layer(ShellSurfaceLayer::Background)
            .pointer(ShellPointer::PassThrough)
    }
}
fn cover(source: SizeI, viewport: SizeF) -> (SizeF, PointF) {
    let width = source.width.max(1) as f32;
    let height = source.height.max(1) as f32;
    let scale = (viewport.width.max(1.0) / width).max(viewport.height.max(1.0) / height);
    let size = SizeF {
        width: width * scale,
        height: height * scale,
    };
    let offset = PointF {
        x: (viewport.width - size.width) * 0.5,
        y: (viewport.height - size.height) * 0.5,
    };
    (size, offset)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{fs, path::PathBuf};

    fn temporary_settings(name: &str) -> PathBuf {
        std::env::temp_dir()
            .join(format!("telorgon-background-{name}-{}", std::process::id()))
            .join("settings.toml")
    }
    fn missing_background() -> PersonalizationSettings {
        PersonalizationSettings {
            background: Some(format!("background-{}.png", "0".repeat(64))),
        }
    }
    fn import_photo(path: &std::path::Path, name: &str, pixel: [u8; 3]) -> PersonalizationSettings {
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        let source = path.parent().unwrap().join(format!("{name}.bmp"));
        let mut bytes = vec![0; 58];
        bytes[..2].copy_from_slice(b"BM");
        bytes[2..6].copy_from_slice(&58u32.to_le_bytes());
        bytes[10..14].copy_from_slice(&54u32.to_le_bytes());
        bytes[14..18].copy_from_slice(&40u32.to_le_bytes());
        bytes[18..22].copy_from_slice(&1i32.to_le_bytes());
        bytes[22..26].copy_from_slice(&1i32.to_le_bytes());
        bytes[26..28].copy_from_slice(&1u16.to_le_bytes());
        bytes[28..30].copy_from_slice(&24u16.to_le_bytes());
        bytes[34..38].copy_from_slice(&4u32.to_le_bytes());
        bytes[54..57].copy_from_slice(&pixel);
        fs::write(&source, bytes).unwrap();
        store_at(path).import_background(&source).unwrap()
    }
    #[test]
    fn startup_creates_an_empty_background_library_idempotently() {
        let path = temporary_settings("empty-library");
        let directory = store_at(&path).background_directory().unwrap();
        assert!(!directory.exists());

        let first = BackgroundControl::new(store_at(&path), PersonalizationSettings::default());
        assert!(directory.is_dir());
        assert_eq!(fs::read_dir(&directory).unwrap().count(), 0);
        assert_eq!(first.snapshot().current, PersonalizationSettings::default());
        assert!(first.snapshot().error.is_none());

        let second = BackgroundControl::new(store_at(&path), PersonalizationSettings::default());
        assert_eq!(second.snapshot(), first.snapshot());
        assert_eq!(fs::read_dir(&directory).unwrap().count(), 0);
        assert!(!path.exists());
        let _ = fs::remove_dir_all(path.parent().unwrap());
    }
    #[test]
    fn library_failure_retains_the_default_background_and_reports_an_error() {
        let path = temporary_settings("library-failure");
        let directory = store_at(&path).background_directory().unwrap();
        fs::create_dir_all(directory.parent().unwrap()).unwrap();
        fs::write(&directory, b"existing file").unwrap();

        let control = BackgroundControl::new(store_at(&path), PersonalizationSettings::default());
        assert_eq!(
            control.snapshot().current,
            PersonalizationSettings::default()
        );
        assert!(control.signal.snapshot().resource.is_none());
        assert!(
            control
                .snapshot()
                .error
                .unwrap()
                .contains("background library")
        );
        assert_eq!(fs::read(&directory).unwrap(), b"existing file");
        assert!(!path.exists());
        let _ = fs::remove_dir_all(path.parent().unwrap());
    }
    #[test]
    fn missing_saved_image_falls_back_without_changing_preferences() {
        let path = temporary_settings("startup");
        let preferences = store_at(&path)
            .import_background(
                &PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("assets/images/wallpaper.webp"),
            )
            .unwrap();
        store_at(&path).save_personalization(&preferences).unwrap();
        fs::remove_file(
            store_at(&path)
                .background_path(&preferences)
                .unwrap()
                .unwrap(),
        )
        .unwrap();
        let before = fs::read(&path).unwrap();
        let control = BackgroundControl::new(store_at(&path), preferences);
        assert_eq!(
            control.snapshot().current,
            PersonalizationSettings::default()
        );
        assert!(control.snapshot().error.is_some());
        assert_eq!(fs::read(&path).unwrap(), before);
        let _ = fs::remove_dir_all(path.parent().unwrap());
    }
    #[test]
    fn failed_apply_retains_current_wallpaper_and_publishes_error() {
        let path = temporary_settings("failed-apply");
        let imported = store_at(&path)
            .import_background(
                &PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("assets/images/wallpaper.webp"),
            )
            .unwrap();
        let control = BackgroundControl::new(store_at(&path), imported);
        let before = control.signal.snapshot();
        assert!(before.resource.is_some());
        assert!(control.apply(missing_background()).is_err());
        let after = control.signal.snapshot();
        assert_eq!(after.snapshot.current, before.snapshot.current);
        assert_eq!(after.resource, before.resource);
        assert!(after.snapshot.error.is_some());
        let _ = fs::remove_dir_all(path.parent().unwrap());
    }
    #[test]
    fn switching_back_to_an_image_increases_its_resource_revision() {
        let path = temporary_settings("revision");
        let imported = store_at(&path)
            .import_background(
                &PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("assets/images/wallpaper.webp"),
            )
            .unwrap();
        let control = BackgroundControl::new(store_at(&path), PersonalizationSettings::default());
        control.apply(imported.clone()).unwrap();
        let first = control.signal.snapshot().resource.clone().unwrap();
        control.apply(PersonalizationSettings::default()).unwrap();
        control.apply(imported).unwrap();
        let second = control.signal.snapshot().resource.clone().unwrap();
        assert_eq!(first.image, second.image);
        assert!(second.content_version > first.content_version);
        assert_eq!(second.pixels, first.pixels);
        assert!(Arc::ptr_eq(&first.pixels, &second.pixels));
        let _ = fs::remove_dir_all(path.parent().unwrap());
    }
    #[test]
    fn selecting_the_applied_photo_reuses_pixels_and_does_not_upload_again() {
        let path = temporary_settings("current-cache");
        let imported = import_photo(&path, "photo", [12, 34, 56]);
        let control = BackgroundControl::new(store_at(&path), imported.clone());
        let before = control.signal.snapshot().resource.clone().unwrap();
        control.apply(imported).unwrap();
        let after = control.signal.snapshot().resource.clone().unwrap();
        assert!(Arc::ptr_eq(&before.pixels, &after.pixels));
        assert_eq!(before.content_version, after.content_version);
        let _ = fs::remove_dir_all(path.parent().unwrap());
    }
    #[test]
    fn changed_or_deleted_cached_photos_are_rejected_without_changing_the_display() {
        for deleted in [false, true] {
            let path = temporary_settings(if deleted {
                "deleted-cache"
            } else {
                "changed-cache"
            });
            let imported = import_photo(&path, "photo", [12, 34, 56]);
            let control = BackgroundControl::new(store_at(&path), imported.clone());
            control.apply(PersonalizationSettings::default()).unwrap();
            let managed = store_at(&path).background_path(&imported).unwrap().unwrap();
            if deleted {
                fs::remove_file(managed).unwrap();
            } else {
                fs::write(managed, b"changed after decoding").unwrap();
            }
            assert!(control.apply(imported).is_err());
            let state = control.signal.snapshot();
            assert_eq!(state.snapshot.current, PersonalizationSettings::default());
            assert!(state.resource.is_none());
            assert!(state.snapshot.error.is_some());
            assert!(control.cache.lock().unwrap().inactive.is_empty());
            let _ = fs::remove_dir_all(path.parent().unwrap());
        }
    }
    #[test]
    fn changed_current_photo_retains_pixels_and_can_recover_without_decoding() {
        let path = temporary_settings("changed-current");
        let imported = import_photo(&path, "photo", [12, 34, 56]);
        let control = BackgroundControl::new(store_at(&path), imported.clone());
        let before = control.signal.snapshot().resource.clone().unwrap();
        let managed = store_at(&path).background_path(&imported).unwrap().unwrap();
        let encoded = fs::read(&managed).unwrap();
        fs::write(&managed, b"changed after decoding").unwrap();
        assert!(control.apply(imported.clone()).is_err());
        let failed = control.signal.snapshot();
        assert_eq!(failed.snapshot.current, imported);
        assert!(failed.snapshot.error.is_some());
        assert_eq!(
            failed.resource.as_ref().unwrap().content_version,
            before.content_version
        );
        assert!(Arc::ptr_eq(
            &failed.resource.as_ref().unwrap().pixels,
            &before.pixels
        ));

        fs::write(&managed, encoded).unwrap();
        control.apply(imported).unwrap();
        let recovered = control.signal.snapshot();
        assert!(recovered.snapshot.error.is_none());
        assert!(Arc::ptr_eq(
            &recovered.resource.as_ref().unwrap().pixels,
            &before.pixels
        ));
        let _ = fs::remove_dir_all(path.parent().unwrap());
    }
    #[test]
    fn cache_retains_only_the_previous_photo_and_reserves_space_for_current_pixels() {
        let path = temporary_settings("cache-budget");
        let first = import_photo(&path, "first", [12, 34, 56]);
        let second = import_photo(&path, "second", [23, 45, 67]);
        let third = import_photo(&path, "third", [34, 56, 78]);
        let control = BackgroundControl::new(store_at(&path), first.clone());
        control.apply(second.clone()).unwrap();
        control.apply(third).unwrap();
        let mut cache = control.cache.lock().unwrap();
        assert_eq!(cache.inactive.len(), 1);
        assert_eq!(cache.inactive[0].validated.settings(), &second);
        assert!(cache.take(&first).is_none());

        // A one-pixel cached image needs four bytes; the applied image's
        // reservation must count toward the same byte budget.
        cache.trim(BACKGROUND_CACHE_BYTES - 3, true);
        assert!(cache.inactive.is_empty());
        drop(cache);
        let _ = fs::remove_dir_all(path.parent().unwrap());
    }
    #[test]
    fn cover_preserves_aspect_ratio_and_centers_the_crop() {
        let (size, offset) = cover(
            SizeI {
                width: 200,
                height: 100,
            },
            SizeF {
                width: 100.0,
                height: 100.0,
            },
        );
        assert_eq!(
            size,
            SizeF {
                width: 200.0,
                height: 100.0
            }
        );
        assert_eq!(offset, PointF { x: -50.0, y: 0.0 });
        let (size, offset) = cover(
            SizeI {
                width: 100,
                height: 200,
            },
            SizeF {
                width: 100.0,
                height: 100.0,
            },
        );
        assert_eq!(
            size,
            SizeF {
                width: 100.0,
                height: 200.0
            }
        );
        assert_eq!(offset, PointF { x: 0.0, y: -50.0 });
    }
}
