use super::super::taskbar::{task_button, taskbar_button_style};
use telorgon::app::*;
use telorgon::assets::ImageSource;
use telorgon::host::application::audio_mixer::{AudioMixerHandle, MixerAction, MixerSnapshot};
use telorgon::integrations::pipewire::ConnectionState;
use telorgon::services::audio::mixer::{
    ApplicationGroupId, MixerTarget, MuteState, StreamDirection, group_mute,
};

const ROW_HEIGHT: f32 = 52.0;
const ICON_SLOT: f32 = 38.0;
const PADDING: f32 = 12.0;
const DIVIDER: f32 = 9.0;

pub(super) fn application_count(snapshot: &MixerSnapshot) -> usize {
    snapshot
        .applications
        .iter()
        .map(|app| {
            [StreamDirection::Playback, StreamDirection::Recording]
                .into_iter()
                .filter(|direction| !app.streams(*direction).is_empty())
                .count()
        })
        .sum()
}
pub(super) fn popover_height(apps: usize) -> f32 {
    2.0 + PADDING * 2.0
        + ROW_HEIGHT * (2 + apps.min(4)) as f32
        + if apps > 0 { DIVIDER } else { 0.0 }
}

pub(crate) fn volume_icon(volume: f32, muted: bool) -> ImageSource {
    use crate::assets::icons::*;
    if muted {
        VOLUME_MUTED.into()
    } else if volume <= 0.0 {
        VOLUME_ZERO.into()
    } else if volume <= 0.33 {
        VOLUME_LOW.into()
    } else if volume <= 0.66 {
        VOLUME_MEDIUM.into()
    } else {
        VOLUME_HIGH.into()
    }
}

// Identity and target are presentation inputs; levels and pending previews belong to the audio owner.
#[derive(Clone, PartialEq)]
struct ChannelIdentity {
    label: String,
    icon: ImageSource,
}
#[component(no_default)]
struct ChannelRow {
    #[input]
    mixer: AudioMixerHandle,
    #[input]
    target: MixerTarget,
    #[input]
    identity: ChannelIdentity,
    #[state]
    error: Option<String>,
}
impl ChannelRow {
    fn new(mixer: AudioMixerHandle, target: MixerTarget, label: String, icon: ImageSource) -> Self {
        Self {
            mixer,
            target,
            identity: ChannelIdentity { label, icon },
            error: None,
        }
    }
    fn execute(&mut self, action: MixerAction) {
        eprintln!("audio-mixer: requested {action:?}");
        self.error = self.mixer.execute(action).err().map(|error| {
            eprintln!("audio mixer {:?}: {error}", self.target);
            error.to_string()
        });
    }
}
impl Component for ChannelRow {
    fn view(&self) -> impl View {
        let signal = self.mixer.signal();
        let snapshot = self.watch(&signal);
        let nodes = snapshot.targets(&self.target);
        let volume = snapshot.volume(&self.target);
        let mute = group_mute(&nodes);
        let muted = mute == MuteState::Muted;
        let enabled = snapshot.state == ConnectionState::Ready && !nodes.is_empty();
        let error = self.error.clone().or_else(|| {
            snapshot
                .operations
                .get(&self.target)
                .and_then(|op| op.error.as_ref().map(ToString::to_string))
        });
        let label = format!(
            "{}: {}{}",
            self.identity.label,
            volume
                .map(|v| format!("{:.0}%", v * 100.0))
                .unwrap_or_else(|| "Unavailable".into()),
            error.map(|e| format!(", {e}")).unwrap_or_default()
        );
        channel_layout(
            image(self.identity.icon)
                .width(24.0)
                .height(24.0)
                .accessible_label(self.identity.label.clone()),
            task_button(format!(
                "{} {}",
                if muted { "Unmute" } else { "Mute" },
                label
            ))
            .width(ICON_SLOT)
            .height(ICON_SLOT)
            .child(image(volume_icon(volume.unwrap_or(0.0), muted)).width(22.0).height(22.0).tint(if muted {
                ColorRgba8::rgba(255, 100, 112, 255)
            } else {
                ColorRgba8::rgba(237, 241, 250, 255)
            }))
            .inline_style(taskbar_button_style(false))
            .enabled(enabled && mute != MuteState::Unknown)
            .on_press(|this: &mut Self| this.execute(MixerAction::ToggleMute(this.target.clone()))),
            // Keep the semantic name while omitting visible text from the compact row.
            slider("", volume.unwrap_or(0.0))
                .accessible_label(label)
                .width(Dimension::FILL)
                .enabled(enabled && volume.is_some() && nodes.iter().all(|n| n.can_set_volume))
                .on_change(|this: &mut Self, volume| {
                    this.execute(MixerAction::SetVolume {
                        target: this.target.clone(),
                        volume,
                    })
                }),
        )
    }
}

fn identity_matches(window: &str, audio: &str) -> bool {
    let window = window
        .trim()
        .strip_suffix(".desktop")
        .unwrap_or(window.trim());
    let audio = audio
        .trim()
        .strip_suffix(".desktop")
        .unwrap_or(audio.trim());
    !window.is_empty() && window.eq_ignore_ascii_case(audio)
}

fn application_identities<'a>(
    id: Option<&'a str>,
    name: &'a str,
    icon: Option<&'a str>,
    binaries: impl Iterator<Item = &'a str>,
) -> Vec<&'a str> {
    id.into_iter()
        .chain(binaries.filter_map(|binary| std::path::Path::new(binary).file_name()?.to_str()))
        .chain(icon)
        .chain(std::iter::once(name))
        .filter(|identity| !identity.trim().is_empty())
        .collect()
}

fn channel_layout(identity: Image, mute: Button, volume: Slider) -> Container {
    row()
        .width(Dimension::FILL)
        .height(ROW_HEIGHT)
        .align_items(Alignment::Center)
        .child(
            column()
                .width(ICON_SLOT)
                .height(ROW_HEIGHT)
                .align_items(Alignment::Center)
                .justify_content(Alignment::Center)
                .child(identity),
        )
        .child(mute)
        .child(volume)
}

#[component(no_default)]
pub(super) struct AudioMixerPopover {
    #[input]
    mixer: AudioMixerHandle,
}
impl AudioMixerPopover {
    pub(super) fn new(mixer: AudioMixerHandle) -> Self {
        Self { mixer }
    }
}
impl Component for AudioMixerPopover {
    fn view(&self) -> impl View {
        let signal = self.mixer.signal();
        let snapshot = self.watch(&signal);
        let mut content = column()
            .width(Dimension::FILL)
            .height(Dimension::FILL)
            .padding(PADDING)
            .background(Background::Color(ColorRgba8::rgba(25, 29, 39, 238)))
            .uniform_border(1.0, ColorRgba8::rgba(255, 255, 255, 28))
            .corner_radius(16.0)
            .shadow(telorgon::ui::Shadow {
                offset: telorgon::PointF { x: 0.0, y: 4.0 },
                blur: 16.0,
                spread: 0.0,
                color: ColorRgba8::rgba(0, 0, 0, 85),
            });
        for (target, title, icon) in [
            (
                MixerTarget::DefaultOutput,
                "Main output",
                crate::assets::icons::AUDIO_OUTPUT.into(),
            ),
            (
                MixerTarget::DefaultInput,
                "Main input",
                crate::assets::icons::AUDIO_INPUT.into(),
            ),
        ] {
            let nodes = snapshot.targets(&target);
            let label = nodes
                .first()
                .map(|n| {
                    format!(
                        "{title}: {}",
                        if n.description.is_empty() {
                            &n.name
                        } else {
                            &n.description
                        }
                    )
                })
                .unwrap_or_else(|| format!("{title}: unavailable"));
            content = content.child(ChannelRow::new(self.mixer.clone(), target, label, icon));
        }
        let count = application_count(&snapshot);
        if count == 0 {
            return content;
        }
        content = content.child(
            column()
                .height(DIVIDER)
                .justify_content(Alignment::Center)
                .child(
                    column()
                        .height(1.0)
                        .width(Dimension::FILL)
                        .background(Background::Color(ColorRgba8::rgba(255, 255, 255, 24))),
                ),
        );
        let shell = self.context::<ShellContext>();
        let catalog = shell.applications();
        let windows = shell.windows();
        let open_windows = windows.open();
        let mut applications = column()
            .width(Dimension::FILL)
            .height(ROW_HEIGHT * count.min(4) as f32)
            .scrollable();
        for app in &snapshot.applications {
            let identities = application_identities(
                match &app.id {
                    ApplicationGroupId::Application(id) => Some(id.as_str()),
                    _ => None,
                },
                &app.name,
                app.icon_name.as_deref(),
                app.playback.iter().chain(&app.recording)
                    .filter_map(|node| node.process_binary.as_deref()),
            );
            let id = identities.iter().find_map(|identity| catalog.identify(identity));
            // Use the taskbar resolver for both Wayland toplevel icons and X11 client artwork.
            // Never match window titles or substrings of unrelated application IDs.
            let request = IconRequest::new().logical_size(24);
            let icon = identities.iter().find_map(|identity| {
                let app_id = catalog.identify(identity);
                open_windows.iter()
                    .filter(|window| identity_matches(&window.application_identity, identity)
                        || app_id.as_ref().is_some_and(|id| window.application_id.as_ref() == Some(id)))
                    .find_map(|window| windows.try_resolve_icon(window.id, request))
                    .or_else(|| app_id.as_ref().and_then(|id| catalog.try_resolve_icon(id, request)))
            })
                .or_else(|| app.icon_name.as_deref()
                    .and_then(|name| catalog.try_resolve_named_icon(name, request)))
                .unwrap_or_else(|| crate::assets::icons::AUDIO_APP.into());
            let name = id
                .as_ref()
                .and_then(|id| catalog.get(id))
                .map(|m| m.name.clone())
                .unwrap_or_else(|| app.name.clone());
            for direction in [StreamDirection::Playback, StreamDirection::Recording] {
                if app.streams(direction).is_empty() {
                    continue;
                }
                let label = if direction == StreamDirection::Recording {
                    format!("{name}, recording")
                } else {
                    name.clone()
                };
                applications = applications.child(
                    column()
                        .height(ROW_HEIGHT)
                        .key(format!("{:?}-{direction:?}", app.id))
                        .child(ChannelRow::new(
                            self.mixer.clone(),
                            MixerTarget::Application(app.id.clone(), direction),
                            label,
                            icon,
                        )),
                );
            }
        }
        content.child(applications)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[component]
    struct UnlabelledMixerSlider {}
    impl Component for UnlabelledMixerSlider {
        fn view(&self) -> impl View {
            slider("", 0.72).accessible_label("Main output volume")
        }
    }

    #[component]
    struct TwoRows {
        #[state]
        output: f32,
        #[state]
        input: f32,
    }
    impl Component for TwoRows {
        fn view(&self) -> impl View {
            let mut rows = column().width(Dimension::FILL).height(104.0);
            for (label, value) in [("Output", self.output), ("Input", self.input)] {
                rows = rows.child(channel_layout(
                    image(crate::assets::icons::AUDIO_OUTPUT)
                        .width(24.0)
                        .height(24.0),
                    task_button(label)
                        .child(image(crate::assets::icons::VOLUME_HIGH).width(22.0).height(22.0))
                        .width(ICON_SLOT)
                        .height(ICON_SLOT),
                    slider("", value)
                        .accessible_label(label)
                        .width(Dimension::FILL)
                        .on_change(move |this: &mut Self, value| {
                            if label == "Output" {
                                this.output = value;
                            } else {
                                this.input = value;
                            }
                        }),
                ));
            }
            rows
        }
    }
    #[test]
    fn mixer_row_geometry() {
        let mut runtime = telorgon::application_host::AppRuntimeCore::from_composed_with_extent(
            TwoRows {
                output: 0.7,
                input: 0.2,
            },
            telorgon::SizeI {
                width: 360,
                height: 104,
            },
        )
        .unwrap();
        runtime
            .prepare_frame(telorgon::MonotonicInstant::ZERO, false)
            .unwrap();
        let sliders: Vec<_> = runtime
            .ui()
            .nodes
            .alive()
            .iter()
            .copied()
            .filter(|node| runtime.ui().kinds.get(*node) == Some(&telorgon::NodeKind::Slider))
            .collect();
        assert_eq!(sliders.len(), 2);
        for node in runtime.ui().nodes.alive().iter().copied() {
            if matches!(
                runtime.ui().kinds.get(node),
                Some(telorgon::NodeKind::Image | telorgon::NodeKind::Button)
            ) {
                let rect = runtime.layout().computed(node).unwrap().border_rect;
                if rect.width > 0.0 && rect.height > 0.0 {
                    let center = rect.y + rect.height * 0.5;
                    assert!(
                        (center - 26.0).abs() < 0.1 || (center - 78.0).abs() < 0.1,
                        "identity and volume artwork must share the slider centerline: {rect:?}"
                    );
                }
            }
        }

        for (index, node) in sliders.iter().enumerate() {
            let track = runtime
                .ui()
                .interactions
                .get(*node)
                .unwrap()
                .value_track
                .unwrap();
            let bounds = runtime.layout().computed(*node).unwrap().border_rect;
            let track = runtime.layout().computed(track).unwrap().border_rect;
            let center = ROW_HEIGHT * (index as f32 + 0.5);
            assert!(
                (track.y + track.height * 0.5 - center).abs() < 0.1,
                "track {track:?} off center {center}"
            );
            assert!(
                bounds.y >= index as f32 * ROW_HEIGHT
                    && bounds.y + bounds.height <= (index + 1) as f32 * ROW_HEIGHT
            );
        }
        for (index, requested) in [(0, 0.4), (1, 0.9)] {
            let other = 1 - index;
            let before = runtime.ui().interactions.get(sliders[other]).unwrap().value;
            let track = runtime
                .ui()
                .interactions
                .get(sliders[index])
                .unwrap()
                .value_track
                .unwrap();
            let rect = runtime.layout().computed(track).unwrap().border_rect;
            runtime.queue_input(telorgon::InputEvent::mouse_moved(telorgon::PointF {
                x: rect.x + rect.width * requested,
                y: rect.y + rect.height * 0.5,
            }));
            runtime.queue_input(telorgon::InputEvent::mouse_button(
                telorgon::PointerButton::PRIMARY,
                telorgon::ButtonState::Pressed,
            ));
            runtime.queue_input(telorgon::InputEvent::mouse_button(
                telorgon::PointerButton::PRIMARY,
                telorgon::ButtonState::Released,
            ));
            runtime.flush_input(telorgon::MonotonicInstant::from_nanos(
                (index + 1) as u64 * 1_000_000,
            ));
            runtime
                .prepare_frame(
                    telorgon::MonotonicInstant::from_nanos((index + 1) as u64 * 1_000_000),
                    false,
                )
                .unwrap();
            assert!(
                (runtime.ui().interactions.get(sliders[index]).unwrap().value - requested).abs()
                    < 0.01,
                "requested {requested}, actual {}",
                runtime.ui().interactions.get(sliders[index]).unwrap().value
            );
            assert_eq!(
                runtime.ui().interactions.get(sliders[other]).unwrap().value,
                before,
                "changing one channel must leave the other unchanged"
            );
        }
    }

    #[test]
    fn mixer_tracks_fill_remaining_width_after_resize() {
        for (output, input) in [(0.0, 1.0), (0.35, 0.72)] {
            let mut runtime =
                telorgon::application_host::AppRuntimeCore::from_composed_with_extent(
                    TwoRows { output, input },
                    telorgon::SizeI {
                        width: 360,
                        height: 104,
                    },
                )
                .unwrap();
            for width in [360, 280, 480] {
                runtime
                    .resize(telorgon::SizeI { width, height: 104 })
                    .unwrap();
                runtime
                    .prepare_frame(telorgon::MonotonicInstant::ZERO, true)
                    .unwrap();
                let mut checked = 0;
                for binding in runtime.ui().style_bindings() {
                    if runtime.ui().kinds.get(binding.state_root)
                        != Some(&telorgon::NodeKind::Slider)
                    {
                        continue;
                    }
                    if !binding
                        .slots
                        .iter()
                        .any(|slot| slot.slot == telorgon::ui::StyleSlotId::named("track"))
                    {
                        continue;
                    }
                    checked += 1;
                    let bounds = |name| {
                        let node = binding
                            .slots
                            .iter()
                            .find(|slot| slot.slot == telorgon::ui::StyleSlotId::named(name))
                            .unwrap()
                            .node;
                        runtime.layout().computed(node).unwrap().border_rect
                    };
                    let track = bounds("track");
                    let fill = bounds("fill");
                    let thumb = bounds("thumb");
                    let value = runtime
                        .ui()
                        .interactions
                        .get(binding.state_root)
                        .unwrap()
                        .value;
                    assert!((track.x - ICON_SLOT * 2.0).abs() < 0.1);
                    assert!(
                        (track.x + track.width - width as f32).abs() < 0.1,
                        "track must reach the row edge: {track:?}"
                    );
                    assert!((fill.width - track.width * value).abs() < 0.1);
                    assert!(
                        (thumb.x - (track.x + (track.width - thumb.width) * value)).abs() < 0.1,
                        "thumb {thumb:?}, track {track:?}"
                    );
                    assert!((thumb.width - 18.0).abs() < 0.1);
                }
                assert_eq!(checked, 2);
            }
        }
    }

    #[test]
    fn audio_identity_uses_executable_before_generic_electron_metadata() {
        let identities = application_identities(None, "WEBRTC VoiceEngine", Some("chromium"),
            ["/usr/share/discord/Discord"].into_iter());
        assert_eq!(identities, ["Discord", "chromium", "WEBRTC VoiceEngine"]);
        assert!(identities.iter().any(|identity| identity_matches("discord", identity)));
        assert!(!identities.iter().any(|identity| identity_matches("discord-canary", identity)));
        let identities = application_identities(Some("com.example.Chat"), "Chat", None,
            ["", "electron"].into_iter());
        assert_eq!(identities, ["com.example.Chat", "electron", "Chat"]);
    }

    #[test]
    fn application_identity_matches_protocol_names_only() {
        assert!(identity_matches("firefox", "Firefox"));
        assert!(identity_matches(
            "org.mozilla.firefox.desktop",
            "org.mozilla.firefox"
        ));
        assert!(!identity_matches("", ""));
        assert!(!identity_matches("Firefox — My document", "Firefox"));
    }

    #[test]
    #[ignore = "requires a private synthetic PipeWire server"]
    fn channel_slider_changes_observed_pipewire_volume() {
        use std::time::{Duration, Instant};
        assert!(
            std::env::var("PIPEWIRE_REMOTE")
                .unwrap()
                .starts_with("telorgon-test-")
        );
        let mut mixer = telorgon::host::application::audio_mixer::AudioMixer::start().unwrap();
        let handle = mixer.handle();
        let deadline = Instant::now() + Duration::from_secs(8);
        let node = loop {
            let snapshot = handle.signal().snapshot();
            if let Some(node) = snapshot
                .nodes
                .iter()
                .find(|node| node.name == "telorgon.test.sink" && node.can_set_volume)
            {
                break node.handle;
            }
            assert!(Instant::now() < deadline, "synthetic sink did not appear");
            std::thread::sleep(Duration::from_millis(20));
        };
        let target = MixerTarget::Node(node);
        let mut runtime = telorgon::application_host::AppRuntimeCore::from_composed_with_extent(
            ChannelRow::new(
                handle.clone(),
                target.clone(),
                "Test output".into(),
                crate::assets::icons::AUDIO_OUTPUT.into(),
            ),
            telorgon::SizeI {
                width: 334,
                height: 52,
            },
        )
        .unwrap();
        runtime
            .prepare_frame(telorgon::MonotonicInstant::ZERO, false)
            .unwrap();
        let control = runtime
            .ui()
            .nodes
            .alive()
            .iter()
            .copied()
            .find(|node| runtime.ui().kinds.get(*node) == Some(&telorgon::NodeKind::Slider))
            .unwrap();
        let track = runtime
            .ui()
            .interactions
            .get(control)
            .unwrap()
            .value_track
            .unwrap();
        let rect = runtime.layout().computed(track).unwrap().border_rect;
        runtime.queue_input(telorgon::InputEvent::mouse_moved(telorgon::PointF {
            x: rect.x + rect.width * 0.31,
            y: rect.y + rect.height * 0.5,
        }));
        runtime.queue_input(telorgon::InputEvent::mouse_button(
            telorgon::PointerButton::PRIMARY,
            telorgon::ButtonState::Pressed,
        ));
        runtime.queue_input(telorgon::InputEvent::mouse_button(
            telorgon::PointerButton::PRIMARY,
            telorgon::ButtonState::Released,
        ));
        runtime.flush_input(telorgon::MonotonicInstant::from_nanos(1_000_000));
        let deadline = Instant::now() + Duration::from_secs(8);
        loop {
            let snapshot = handle.signal().snapshot();
            let observed =
                telorgon::services::audio::mixer::group_volume(&snapshot.targets(&target)).unwrap();
            if (observed - 0.31).abs() < 0.01 {
                break;
            }
            assert!(
                Instant::now() < deadline,
                "slider failed: observed {observed}, operations {:?}",
                snapshot.operations
            );
            std::thread::sleep(Duration::from_millis(20));
        }
        mixer.shutdown();
    }

    #[test]
    fn hidden_slider_text_keeps_valid_accessibility_name() {
        slider("", 0.72)
            .accessible_label("Main output volume")
            .into_element()
            .validate()
            .expect("validation must use the accessible name, not the hidden visible label");
        let mut runtime = telorgon::application_host::AppRuntimeCore::from_composed_with_extent(
            UnlabelledMixerSlider::default(),
            telorgon::SizeI {
                width: 300,
                height: 52,
            },
        )
        .expect("icon-only mixer slider must mount without an empty-label error");
        runtime
            .prepare_frame(telorgon::MonotonicInstant::ZERO, false)
            .unwrap();
    }
}
