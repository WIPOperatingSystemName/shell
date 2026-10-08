//! Application groups and the taskbar component. All dimensions are logical units.
use super::constants::*;
use super::launcher::{AppLauncher, LauncherState};
const START_SPACE: f32 = ICON_WIDTH + ICON_GAP;
use super::tray::{TrayPopup,TrayPopupState,TRAY_WIDTH};
use telorgon::tray::TrayHandle;
use super::audio::{AudioPopup, AudioPopupState, AudioTooltip, AUDIO_BUTTON_WIDTH};
use super::battery::{BATTERY_WIDGET_WIDTH, indicator as battery_indicator, visible as battery_visible};
use super::network::{
    NetworkPopup, NetworkPopupState, NETWORK_BUTTON_WIDTH, network_anchor,
    indicator as network_indicator,
};
use telorgon::network::NetworkSnapshot;
use telorgon::battery::BatteryMonitorHandle;
use telorgon::host::application::audio_mixer::AudioMixerHandle;
use telorgon::services::audio::mixer::{MixerTarget, group_mute, MuteState};
use super::hover_delay::HoverDelay;
use super::window_picker::{PickerState, WindowPicker};
use super::window_peek::WindowPeek;
use telorgon::app::*;
use telorgon::{InputEvent, RectF};

// Compact picker controls must opt out of the general button's 32-unit minimum.
pub(super) fn task_button(label: impl Into<String>) -> telorgon::compose::Button {
    button().accessible_label(label).box_style(telorgon::ui::BoxStyle {
        min_size: telorgon::ui::SizeRule2D {
            width: telorgon::ui::SizeRule::Logical(1.0),
            height: telorgon::ui::SizeRule::Logical(1.0),
        },
        decoration: telorgon::ui::BoxDecoration {
            background: Background::Color(IDLE),
            corner_radii: telorgon::ui::CornerRadii::all(4.0),
            ..Default::default()
        },
        ..Default::default()
    })
}

// Own interaction backgrounds explicitly so the global button theme cannot override them.
pub(super) fn taskbar_button_style(active: bool) -> std::sync::Arc<telorgon::theme::CompiledComponentStyle> {
    use std::{collections::BTreeMap, sync::Arc};
    use telorgon::theme::{
        CompiledComponentStyle, CompiledSlotStyle, CompiledStateStyle, InteractionState,
    };
    use telorgon::ui::{
        ComponentStyleId, InteractionFlags, StylePropertyPatch, StyleSlotId, ThemeDomainId,
    };
    let slot = StyleSlotId::named("root");
    let resting = StylePropertyPatch {
        background: Some(Background::Color(if active {
            TASKBAR_BUTTON_ACTIVE_COLOR
        } else {
            TASKBAR_BUTTON_IDLE_COLOR
        })),
        ..Default::default()
    };
    Arc::new(CompiledComponentStyle {
        id: ComponentStyleId::named(ThemeDomainId::SHELL, "taskbar", "button"),
        slots: BTreeMap::from([(
            slot,
            CompiledSlotStyle {
                patch: resting,
                font_family: None,
            },
        )]),
        variants: Default::default(),
        states: [
            (
                InteractionState::Hovered,
                TASKBAR_BUTTON_HOVER_COLOR,
                Some(telorgon::TransitionSpec {
                    duration_ms: TASKBAR_HOVER_DURATION_MS,
                    easing: TASKBAR_HOVER_EASING,
                    repeat: false,
                }),
            ),
            (
                InteractionState::Pressed,
                TASKBAR_BUTTON_PRESSED_COLOR,
                Some(telorgon::TransitionSpec {
                    duration_ms: TASKBAR_PRESS_DURATION_MS,
                    easing: TASKBAR_PRESS_EASING,
                    repeat: false,
                }),
            ),
        ]
        .into_iter()
        .map(|(state, color, transition)| {
            (
                state,
                CompiledStateStyle {
                    slots: BTreeMap::from([(
                        slot,
                        CompiledSlotStyle {
                            patch: StylePropertyPatch {
                                background: Some(Background::Color(color)),
                                ..Default::default()
                            },
                            font_family: None,
                        },
                    )]),
                    transition,
                },
            )
        })
        .collect(),
        state_precedence: vec![InteractionState::Hovered, InteractionState::Pressed],
        relevant_states: InteractionFlags::from_bits(
            InteractionFlags::HOVERED.bits() | InteractionFlags::PRESSED.bits(),
        ),
        transition: telorgon::TransitionSpec {
            duration_ms: TASKBAR_COLOR_DURATION_MS,
            easing: TASKBAR_COLOR_EASING,
            repeat: false,
        },
        controlled_slots: BTreeMap::from([(slot, resting)]),
        controlled_font_families: Default::default(),
    })
}

struct AppGroup {
    key: String,
    windows: Vec<ShellWindow>,
}
pub(super) fn group_key(window: &ShellWindow) -> String {
    identity_key(
        window.application_id.as_ref(),
        &window.application_identity,
        window.id,
    )
}
fn identity_key(
    application: Option<&ApplicationId>,
    identity: &str,
    id: telorgon::shell::WindowId,
) -> String {
    if let Some(id) = application {
        format!("app:{}", id.as_str())
    } else if !identity.trim().is_empty() {
        format!("identity:{identity}")
    } else {
        // Unknown windows must not all become one fictitious application.
        format!("window:{}:{}", id.slot(), id.generation())
    }
}
fn groups(windows: Vec<ShellWindow>) -> Vec<AppGroup> {
    let mut groups: Vec<AppGroup> = Vec::new();
    for window in windows {
        let key = group_key(&window);
        if let Some(group) = groups.iter_mut().find(|g| g.key == key) {
            group.windows.push(window);
        } else {
            groups.push(AppGroup {
                key,
                windows: vec![window],
            });
        }
    }
    groups
}
pub(super) fn window_label(window: &ShellWindow) -> String {
    if window.title.is_empty() {
        "Untitled window".into()
    } else {
        window.title.clone()
    }
}
fn icon_at(position: PointF, count: usize) -> Option<usize> {
    let button_top = BAR_PADDING + TASKBAR_TOP_BORDER_WIDTH;
    if !(button_top..button_top + ICON_WIDTH).contains(&position.y) || position.x < BAR_PADDING + START_SPACE {
        return None;
    }
    let x = position.x - BAR_PADDING - START_SPACE;
    let index = (x / (ICON_WIDTH + ICON_GAP)) as usize;
    (index < count && x % (ICON_WIDTH + ICON_GAP) < ICON_WIDTH).then_some(index)
}

fn taskbar_row() -> telorgon::compose::Container {
    row()
            .gap(ICON_GAP)
            .padding(BAR_PADDING)
            .border_sides(telorgon::ui::Border {
                top: telorgon::ui::BorderSide {
                    width: TASKBAR_TOP_BORDER_WIDTH,
                    color: TASKBAR_TOP_BORDER_COLOR,
                },
                ..Default::default()
            })
            .background(Background::Color(TASKBAR_BACKGROUND_COLOR))
}

fn audio_anchor(output_width: f32, battery_space: f32) -> RectF {
    RectF {
        x: (output_width - BAR_PADDING - AUDIO_BUTTON_WIDTH - battery_space).max(0.0),
        y: 0.0,
        width: AUDIO_BUTTON_WIDTH,
        height: BAR_HEIGHT,
    }
}

fn audio_hit_test(position: PointF, anchor: RectF) -> bool {
    RectF {
        y: BAR_PADDING + TASKBAR_TOP_BORDER_WIDTH,
        height: ICON_WIDTH,
        ..anchor
    }.contains(position)
}

#[component]
pub(crate) struct TestTaskbar {
    #[state]
    launcher_popup: LauncherState,
    #[input]
    tray: Option<TrayHandle>,
    #[state]
    tray_popup: TrayPopupState,
    #[input]
    mixer: Option<AudioMixerHandle>,
    #[input]
    battery: Option<BatteryMonitorHandle>,
    #[input]
    network: Option<Signal<NetworkSnapshot>>,
    #[state]
    network_popup: NetworkPopupState,
    #[state]
    audio_popup: AudioPopupState,
    #[state]
    application_page: usize,
    #[state]
    audio_hover: bool,
    #[state]
    picker: PickerState,
    #[state]
    hover_delay: HoverDelay,
    #[state]
    window_peek: WindowPeek,
}
impl TestTaskbar {
    pub(crate) fn with_services(
        mixer: AudioMixerHandle,
        tray: TrayHandle,
        battery: Option<BatteryMonitorHandle>,
        network: Signal<NetworkSnapshot>,
    ) -> Self {
        Self {
            mixer: Some(mixer),
            tray: Some(tray),
            battery,
            network: Some(network),
            ..Self::default()
        }
    }
    fn battery_space(&self) -> f32 {
        // Input callbacks also use this helper, outside the component's render context.
        self.battery.as_ref().map_or(0.0, |battery| {
            if battery_visible(&battery.state()) {
                BATTERY_WIDGET_WIDTH + ICON_GAP
            } else {
                0.0
            }
        })
    }
    fn application_capacity(&self) -> usize {
        let width = self.context::<ShellContext>().output_size().width;
        ((width - START_SPACE - TRAY_WIDTH - ICON_GAP - BAR_PADDING * 2.0
            - AUDIO_BUTTON_WIDTH - ICON_WIDTH - ICON_GAP * 2.0
            - self.battery_space() - self.network_space()).max(0.0)
            / (ICON_WIDTH + ICON_GAP)) as usize
    }
    fn network_space(&self) -> f32 {
        if self.network.is_some() { NETWORK_BUTTON_WIDTH + ICON_GAP } else { 0.0 }
    }
    fn page_start(&self, count: usize) -> usize {
        let capacity = self.application_capacity().max(1);
        self.application_page.min(count.saturating_sub(1) / capacity) * capacity
    }
}
impl ShellWidget for TestTaskbar {
    fn surface(&self) -> ShellSurfaceSpec {
        ShellSurfaceSpec::new()
            .placement(WidgetPlacement::edge(ShellEdge::Bottom).height(BAR_HEIGHT))
            .layer(ShellSurfaceLayer::Panel)
            .reserve_space(ShellReservation::WhenVisible)
            .exit_to(ShellEdge::Bottom)
    }
    fn children(&self) -> Vec<ShellChild> {
        if *self.watch(&self.launcher_popup.open) {
            return vec![ShellChild::new("app-launcher", AppLauncher::new(self.launcher_popup.clone()))];
        }
        if *self.watch(&self.tray_popup.open) {
            if let Some(tray)=&self.tray {
                let width=self.context::<ShellContext>().output_size().width;
                return vec![ShellChild::new("system-tray",TrayPopup::new(tray.clone(),self.tray_popup.clone(),RectF {
                    x:width-BAR_PADDING-AUDIO_BUTTON_WIDTH-self.battery_space()-self.network_space()-ICON_GAP-TRAY_WIDTH,y:0.0,width:TRAY_WIDTH,height:BAR_HEIGHT,
                }))];
            }
        }

        if *self.watch(&self.network_popup.open) {
            if let Some(network) = &self.network {
                let width = self.context::<ShellContext>().output_size().width;
                return vec![ShellChild::new(
                    "network",
                    NetworkPopup::new(
                        network.clone(), self.network_popup.clone(),
                        network_anchor(width, self.battery_space()),
                    ),
                )];
            }
        }

        if *self.watch(&self.audio_popup.open) {
            if let Some(mixer) = &self.mixer {
                let width = self.context::<ShellContext>().output_size().width;
                return vec![ShellChild::new("audio-mixer", AudioPopup::new(mixer.clone(), self.audio_popup.clone(), audio_anchor(width, self.battery_space())))];
            }
        }
        if self.audio_hover {
            if let Some(mixer) = &self.mixer {
                let width = self.context::<ShellContext>().output_size().width;
                return vec![ShellChild::new("audio-tooltip", AudioTooltip::new(mixer.clone(), audio_anchor(width, self.battery_space())))];
            }
        }
        let selection = self.watch(&self.picker.selected);
        let groups = groups(self.context::<ShellContext>().windows().open());
        let start = self.page_start(groups.len());
        let Some((index, group)) = groups
            .iter().skip(start).take(self.application_capacity())
            .enumerate()
            .find(|(_, g)| Some(&g.key) == selection.as_ref())
        else {
            return vec![];
        };
        vec![ShellChild::new(
            format!("picker-{:?}", hashed_key(&group.key)),
            WindowPicker::new(
                group.key.clone(),
                RectF {
                    x: BAR_PADDING + START_SPACE + index as f32 * (ICON_WIDTH + ICON_GAP),
                    y: 0.0,
                    width: ICON_WIDTH,
                    height: BAR_HEIGHT,
                },
                self.picker.clone(),
                self.window_peek.clone(),
            ),
        )]
    }
    fn input(&mut self, event: InputEvent) -> bool {
        let mut changed = false;
        match event {
            InputEvent::PointerMoved { position, .. } => {
                let width = self.context::<ShellContext>().output_size().width;
                let hovered = self.mixer.is_some()
                    && audio_hit_test(position, audio_anchor(width, self.battery_space()));
                changed = self.audio_hover != hovered;
                self.audio_hover = hovered;
                let groups = groups(self.context::<ShellContext>().windows().open());
                let popup_open = *self.launcher_popup.open.snapshot()
                    || *self.audio_popup.open.snapshot()
                    || *self.tray_popup.open.snapshot()
                    || *self.network_popup.open.snapshot();
                let key = if popup_open { None } else {
                    icon_at(position, (groups.len() - self.page_start(groups.len())).min(self.application_capacity())).map(|index| groups[self.page_start(groups.len()) + index].key.clone())
                };
                self.hover_delay.update(
                    key,
                    &self.picker,
                    std::time::Duration::from_millis(PICKER_HOVER_DELAY_MS),
                );
            }
            InputEvent::PointerButton { .. } => self.hover_delay.cancel(),
            _ => {}
        }
        changed // Popup signals independently schedule their updates.
    }
}
impl Component for TestTaskbar {
    fn unmounted(&mut self, _: &mut telorgon::compose::UnmountContext<Self>) {
        self.hover_delay.cancel();
        self.window_peek.finish(None);
        if let Some(context) = self.try_context::<ShellContext>() {
            self.window_peek.update(&context.windows());
        }
    }

    fn view(&self) -> impl View {
        let (wake, elapsed) = self.window_peek.signals();
        self.watch(&wake);
        self.watch(&elapsed);
        self.window_peek.update(&self.context::<ShellContext>().windows());
        let mut bar = taskbar_row().child(task_button("Start").child(image(crate::assets::icons::START).width(TASKBAR_ICON_SIZE as f32).height(TASKBAR_ICON_SIZE as f32))
            .width(ICON_WIDTH).height(ICON_WIDTH)
            .padding(Insets::ZERO)
            .inline_style(taskbar_button_style(*self.watch(&self.launcher_popup.open)))
            .on_press(|this: &mut Self| {
                this.audio_popup.set(false); this.tray_popup.set(false); this.network_popup.set(false);
                this.hover_delay.cancel(); this.picker.select(None); this.window_peek.finish(None);
                this.launcher_popup.set(!*this.launcher_popup.open.snapshot());
            }));
        let shell = self.context::<ShellContext>();
        let windows = shell.windows();
        let mut applications = row().gap(ICON_GAP).width(Dimension::FILL).height(ICON_WIDTH).scrollable();
        let all_groups = groups(windows.open());
        let start = self.page_start(all_groups.len());
        let capacity = self.application_capacity();
        let count = all_groups.len();
        for group in all_groups.into_iter().skip(start).take(capacity) {
            let window = &group.windows[0];
            let id = window.id;
            let label = window
                .application_id
                .as_ref()
                .and_then(|id| shell.applications().get(id))
                .map(|app| app.name)
                .unwrap_or_else(|| window_label(window));
            let active = group.windows.iter().any(|w| w.active && !w.minimized);
            let multiple = group.windows.len() > 1;
            let key = group.key.clone();
            applications = applications.child(
                stack()
                    .key(group.key)
                    .width(ICON_WIDTH)
                    .height(ICON_WIDTH)
                    .corner_radius(TASKBAR_BUTTON_RADIUS)
                    .child(
                        task_button(format!("{label} ({} windows)", group.windows.len()))
                            .inline_style(taskbar_button_style(active))
                            .corner_radius(TASKBAR_BUTTON_RADIUS)
                            .child(image(windows.resolve_icon(
                                id,
                                IconRequest::new().logical_size(TASKBAR_ICON_SIZE),
                            )).width(TASKBAR_ICON_SIZE as f32).height(TASKBAR_ICON_SIZE as f32).without_tint())
                            // Keep authored defaults stable: the theme owns every color change.
                            .background(Background::Color(TASKBAR_BUTTON_IDLE_COLOR))
                            .padding(Insets::ZERO)
                            .width(ICON_WIDTH)
                            .height(ICON_WIDTH)
                            .on_press(move |this: &mut Self| {
                                this.audio_popup.set(false);
                                this.network_popup.set(false);
                                this.hover_delay.cancel();
                                if multiple {
                                    this.picker.select(Some(key.clone()));
                                    return;
                                }
                                this.picker.select(None);
                                let windows = this.context::<ShellContext>().windows();
                                let active = windows
                                    .open()
                                    .iter()
                                    .any(|w| w.id == id && w.active && !w.minimized);
                                let _ = if active {
                                    windows.set_minimized(id, true)
                                } else {
                                    windows.activate(id)
                                };
                            }),
                    )
                    .child(
                        column()
                            .width(ICON_WIDTH)
                            .height(ICON_WIDTH)
                            .justify_content(Alignment::Start)
                            .align_items(Alignment::Center)
                            .child(
                                column()
                                    .width(
                                        WINDOW_COUNT_BAR_BASE_WIDTH
                                            + WINDOW_COUNT_BAR_WIDTH_STEP
                                                * (group
                                                    .windows
                                                    .len()
                                                    .clamp(1, WINDOW_COUNT_BAR_MAX_WINDOWS)
                                                    - 1)
                                                    as f32,
                                    )
                                    .height(WINDOW_COUNT_BAR_HEIGHT)
                                    .corner_radius(WINDOW_COUNT_BAR_RADIUS)
                                    .background(Background::Color(WINDOW_COUNT_BAR_COLOR)),
                            ),
                    ),
            );
        }
        bar = bar.child(applications);
        if count > capacity && capacity > 0 {
            bar = bar.child(task_button("More applications").child(image(crate::assets::icons::MORE).width(TASKBAR_ICON_SIZE as f32).height(TASKBAR_ICON_SIZE as f32))
                .width(ICON_WIDTH).height(ICON_WIDTH)
                .padding(Insets::ZERO)
                .on_press(move |this: &mut Self| {
                    this.network_popup.set(false);
                    this.hover_delay.cancel(); this.picker.select(None); this.window_peek.finish(None);
                    this.application_page = if start + capacity >= count { 0 } else { start / capacity + 1 };
                }));
        } else {
            bar = bar.child(column().width(ICON_WIDTH).height(ICON_WIDTH));
        }
        if self.tray.is_some() {
            let tray_open = *self.watch(&self.tray_popup.open);
            let tray_icon = if tray_open {
                crate::assets::icons::SYSTEM_TRAY_OPEN
            } else {
                crate::assets::icons::SYSTEM_TRAY
            };
            bar=bar.child(task_button("System tray").child(image(tray_icon).width(TASKBAR_ICON_SIZE as f32).height(TASKBAR_ICON_SIZE as f32))
                .width(TRAY_WIDTH).height(ICON_WIDTH)
                .padding(Insets::ZERO)
                .inline_style(taskbar_button_style(tray_open))
                .on_press(|this:&mut Self| {
                    this.launcher_popup.set(false);this.audio_popup.set(false);this.network_popup.set(false);this.hover_delay.cancel();this.picker.select(None);
                    this.tray_popup.set(!*this.tray_popup.open.snapshot());
                    if let Some(tray)=&this.tray {let _=tray.close_menu();}
                }));
        }
        if let Some(network) = &self.network {
            let snapshot = self.watch(network);
            bar = bar.child(network_indicator(&snapshot, *self.watch(&self.network_popup.open))
                .on_press(|this: &mut Self| {
                    this.audio_hover = false;
                    this.hover_delay.cancel(); this.picker.select(None); this.window_peek.finish(None);
                    this.launcher_popup.set(false); this.audio_popup.set(false); this.tray_popup.set(false);
                    this.network_popup.set(!*this.network_popup.open.snapshot());
                    if let Some(tray) = &this.tray { let _ = tray.close_menu(); }
                }));
        }
        if let Some(mixer) = &self.mixer {
            let signal = mixer.signal();
            let snapshot = self.watch(&signal);
            let target = MixerTarget::DefaultOutput;
            let volume = snapshot.volume(&target);
            let muted = group_mute(&snapshot.targets(&target)) == MuteState::Muted;
            let status = if muted { "Muted".into() } else { volume.map(|v| format!("{:.0}%", v * 100.0)).unwrap_or_else(|| "—".into()) };
            let label = format!("Sound mixer, {status}");
            bar = bar.child(task_button(label).child(image(super::audio::volume_icon(volume.unwrap_or(0.0), muted)).width(TASKBAR_AUDIO_ICON_WIDTH).height(TASKBAR_ICON_SIZE as f32).tint(if muted { ColorRgba8::rgba(255, 100, 112, 255) } else if volume.is_none() { crate::colors::COLOR7 } else { crate::colors::COLOR5.with_alpha((160.0 + volume.unwrap_or(0.0).clamp(0.0, 1.0) * 95.0) as u8) }))
                    .width(AUDIO_BUTTON_WIDTH).height(ICON_WIDTH)
                    .corner_radius(TASKBAR_BUTTON_RADIUS)
                    .background(Background::Color(TASKBAR_BUTTON_IDLE_COLOR))
                    .padding(Insets::ZERO)
                    .inline_style(taskbar_button_style(*self.watch(&self.audio_popup.open)))
                    .on_press(|this: &mut Self| {
                        this.hover_delay.cancel(); this.picker.select(None); this.window_peek.finish(None);
                        this.launcher_popup.set(false); this.tray_popup.set(false); this.network_popup.set(false);
                        let open = !*this.audio_popup.open.snapshot(); this.audio_popup.set(open);
                    }));
        }
        if let Some(battery) = &self.battery {
            let state = self.watch(&battery.signal());
            if battery_visible(&state) {
                bar = bar.child(battery_indicator(&state));
            }
        }
        bar
    }
}

#[cfg(test)]
#[path = "taskbar/tests.rs"]
mod tests;
