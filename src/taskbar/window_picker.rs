//! Hover preview cards, overflow list, sizing, and popup behavior. Dimensions are logical units.
use super::constants::*;
use super::taskbar::{group_key, task_button, window_label};
use super::window_peek::WindowPeek;
use telorgon::app::*;
use telorgon::{InputEvent, RectF, SizeF};

#[derive(Clone)]
pub(super) struct PickerState {
    pub(super) selected: Signal<Option<String>>,
    writer: SignalWriter<Option<String>>,
}
impl Default for PickerState {
    fn default() -> Self {
        let (selected, writer) = Signal::new(None);
        Self { selected, writer }
    }
}
impl PartialEq for PickerState {
    fn eq(&self, other: &Self) -> bool {
        self.selected == other.selected
    }
}
impl PickerState {
    pub(super) fn select(&self, key: Option<String>) {
        self.writer.publish_if_changed(key);
    }
}

fn short_title(title: &str, limit: usize) -> String {
    let mut chars = title.chars();
    let mut result: String = chars.by_ref().take(limit).collect();
    if chars.next().is_some() {
        result.push('…');
    }
    result
}
fn scrolled_row(current: usize, count: usize, visible: usize, down: bool) -> usize {
    let maximum = count.saturating_sub(visible);
    let current = current.min(maximum);
    if down {
        current.saturating_add(1).min(maximum)
    } else {
        current.saturating_sub(1)
    }
}

#[derive(Clone, Copy, Debug)]
struct PickerLayout {
    width: f32,
    height: f32,
    list: bool,
    rows: usize,
}
fn preview_width(size: Option<SizeF>) -> f32 {
    let Some(size) = size.filter(|size| {
        size.width.is_finite() && size.height.is_finite() && size.width > 0.0 && size.height > 0.0
    }) else {
        return CARD_WIDTH;
    };
    (PREVIEW_HEIGHT as f64 * size.width as f64 / size.height as f64)
        .round()
        .clamp(1.0, MAX_PREVIEW_WIDTH as f64) as f32
}

fn picker_layout(widths: &[f32], output: SizeF) -> PickerLayout {
    let max_width = (output.width * 0.92).max(1.0);
    let count = widths.len();
    let width = widths.iter().sum::<f32>().max(1.0);
    let max_height = (output.height - 72.0).max(1.0);
    let list = count > 64 || width > max_width || CARD_HEIGHT > max_height;
    if list {
        let rows = (((max_height.min(480.0) - 28.0) / ROW_HEIGHT)
            .floor()
            .max(1.0) as usize)
            .min(count.max(1));
        PickerLayout {
            width: 360.0f32.min(max_width),
            height: (rows as f32 * ROW_HEIGHT + 28.0).min(max_height),
            list,
            rows,
        }
    } else {
        PickerLayout {
            width,
            height: CARD_HEIGHT,
            list,
            rows: count,
        }
    }
}

fn picker_index_at(position: telorgon::PointF, widths: &[f32], layout: PickerLayout, first_row: usize) -> Option<usize> {
    if !position.x.is_finite() || !position.y.is_finite()
        || position.x < 0.0 || position.y < 0.0
        || position.x >= layout.width || position.y >= layout.height
    {
        return None;
    }
    if layout.list {
        let row = (position.y / ROW_HEIGHT) as usize;
        let start = first_row.min(widths.len().saturating_sub(layout.rows));
        return (row < layout.rows && start + row < widths.len()).then_some(start + row);
    }
    let mut right = 0.0;
    widths.iter().position(|width| {
        right += width;
        position.x < right
    })
}

// The container owns the hover tween; its selection button stays transparent.
fn card_style(
    radii: telorgon::ui::CornerRadii,
    transparent: bool,
) -> std::sync::Arc<telorgon::theme::CompiledComponentStyle> {
    use std::{collections::BTreeMap, sync::Arc};
    use telorgon::theme::{CompiledComponentStyle, CompiledSlotStyle, CompiledStateStyle, InteractionState};
    use telorgon::ui::{ComponentStyleId, InteractionFlags, StylePropertyPatch, StyleSlotId, ThemeDomainId};
    let slot = StyleSlotId::named("root");
    let clear = ColorRgba8::rgba(0, 0, 0, 0);
    let background = if transparent { clear } else { PICKER_CARD_BACKGROUND_COLOR };
    let hovered = if transparent { clear } else { PICKER_CARD_HOVER_COLOR };
    let patch = StylePropertyPatch {
        background: Some(Background::Color(background)),
        corner_radii: Some(radii),
        ..Default::default()
    };
    Arc::new(CompiledComponentStyle {
        id: ComponentStyleId::named(ThemeDomainId::SHELL, "window-picker", "card"),
        slots: BTreeMap::from([(slot, CompiledSlotStyle { patch, font_family: None })]),
        variants: Default::default(),
        states: BTreeMap::from([(
            InteractionState::Hovered,
            CompiledStateStyle {
                slots: BTreeMap::from([(
                    slot,
                    CompiledSlotStyle {
                        patch: StylePropertyPatch {
                            background: Some(Background::Color(hovered)),
                            ..Default::default()
                        },
                        font_family: None,
                    },
                )]),
                transition: None,
            },
        )]),
        state_precedence: vec![InteractionState::Hovered],
        relevant_states: InteractionFlags::HOVERED,
        transition: telorgon::TransitionSpec {
            duration_ms: PICKER_CARD_HOVER_DURATION_MS,
            easing: PICKER_CARD_HOVER_EASING,
            repeat: false,
        },
        controlled_slots: BTreeMap::from([(slot, patch)]),
        controlled_font_families: Default::default(),
    })
}

fn close_button() -> telorgon::compose::Button {
    use std::{collections::BTreeMap, sync::Arc};
    use telorgon::theme::{
        CompiledComponentStyle, CompiledSlotStyle, CompiledStateStyle, InteractionState,
    };
    use telorgon::ui::{
        ComponentStyleId, InteractionFlags, StylePropertyPatch, StyleSlotId, ThemeDomainId,
    };
    let slot = StyleSlotId::named("root");
    let resting = StylePropertyPatch {
        background: Some(Background::Color(ColorRgba8::rgba(0, 0, 0, 0))),
        ..Default::default()
    };
    let states = [
        (InteractionState::Hovered, CLOSE_HOVER),
        (InteractionState::FocusVisible, CLOSE_HOVER),
        (InteractionState::Pressed, CLOSE_PRESSED),
    ]
    .into_iter()
    .map(|(state, color)| {
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
                transition: None,
            },
        )
    })
    .collect();
    task_button("Close window")
        .child(image(crate::assets::icons::CLOSE).width(PICKER_CLOSE_ICON_SIZE).height(PICKER_CLOSE_ICON_SIZE).tint(PICKER_CLOSE_ICON_COLOR))
        // Match the resting theme before its first state evaluation, and on reconciliation.
        .background(Background::Color(ColorRgba8::rgba(0, 0, 0, 0)))
        .width(PICKER_CLOSE_BUTTON_SIZE)
        .height(PICKER_CLOSE_BUTTON_SIZE)
        .inline_style(Arc::new(CompiledComponentStyle {
            id: ComponentStyleId::named(ThemeDomainId::SHELL, "window-picker", "close"),
            slots: BTreeMap::from([(
                slot,
                CompiledSlotStyle {
                    patch: resting,
                    font_family: None,
                },
            )]),
            variants: BTreeMap::new(),
            states,
            state_precedence: vec![
                InteractionState::Hovered,
                InteractionState::FocusVisible,
                InteractionState::Pressed,
            ],
            relevant_states: InteractionFlags::from_bits(
                InteractionFlags::HOVERED.bits()
                    | InteractionFlags::FOCUS_VISIBLE.bits()
                    | InteractionFlags::PRESSED.bits(),
            ),
            transition: telorgon::TransitionSpec {
                duration_ms: PICKER_CLOSE_HOVER_DURATION_MS,
                easing: PICKER_CLOSE_HOVER_EASING,
                repeat: false,
            },
            controlled_slots: BTreeMap::from([(slot, resting)]),
            controlled_font_families: Default::default(),
        }))
}

/// One selection button owns the entire card/row. Decoration is noninteractive;
/// the close button is a sibling above it, so closing never activates the window.
fn window_entry<C: Component>(
    label: String,
    icon: telorgon::assets::ImageSource,
    width: f32,
    list: bool,
    radii: telorgon::ui::CornerRadii,
    select: impl Fn(&mut C) + 'static,
    close: impl Fn(&mut C) + 'static,
) -> telorgon::compose::Container {
    let height = if list { ROW_HEIGHT } else { CARD_HEIGHT };
    let selection = if list {
        task_button(short_title(&label, 34)).child(text(short_title(&label, 34)).font_family(crate::constants::UI_FONT_FAMILY).color(crate::colors::COLOR5))
    } else {
        task_button(&label)
            .child(image(icon).width(40.0).height(40.0).without_tint())
    };
    let selection = selection
        .width(width)
        .height(height)
        .decoration(telorgon::ui::BoxDecoration {
            background: Background::Color(ColorRgba8::rgba(0, 0, 0, 0)),
            corner_radii: radii,
            ..Default::default()
        })
        .inline_style(card_style(radii, true))
        .on_press(select);
    let header_height = if list {
        ROW_HEIGHT - 2.0 * PICKER_PADDING
    } else {
        PICKER_HEADER_HEIGHT
    };
    let mut header = row()
        .height(header_height)
        .gap(4.0)
        .align_items(Alignment::Center)
        .child(image(icon).width(HEADER_ICON_SIZE).height(HEADER_ICON_SIZE));
    if list {
        header = header.child(spacer());
    } else {
        header = header.child(
            row().center_content().child(
                text(short_title(
                    &label,
                    ((width - 2.0 * PICKER_PADDING - HEADER_ICON_SIZE - PICKER_CLOSE_BUTTON_SIZE - 8.0) / 7.0).max(0.0) as usize,
                ))
                .size(14.0)
                .font_family(crate::constants::UI_FONT_FAMILY)
                .color(PICKER_TITLE_COLOR),
            ),
        );
    }
    header = header.child(close_button().on_press(close));
    stack()
        .width(width)
        .height(height)
        .background(Background::Color(PICKER_CARD_BACKGROUND_COLOR))
        .inline_style(card_style(radii, false))
        .hover_within(true)
        .corner_radii(radii)
        .overflow(telorgon::Overflow::Clip)
        .child(selection)
        .child(column().padding(PICKER_PADDING).child(header))
}

#[component]
pub(super) struct WindowPicker {
    #[input]
    group: String,
    #[input]
    anchor: RectF,
    #[input]
    picker: PickerState,
    #[input]
    window_peek: WindowPeek,
    #[state]
    first_row: usize,
    #[state]
    pointer: Option<telorgon::PointF>,
}
impl WindowPicker {
    pub(super) fn new(group: String, anchor: RectF, picker: PickerState, window_peek: WindowPeek) -> Self {
        Self {
            group,
            anchor,
            picker,
            window_peek,
            first_row: 0,
            pointer: None,
        }
    }

    fn windows(&self) -> Vec<ShellWindow> {
        self.context::<ShellContext>()
            .windows()
            .open()
            .into_iter()
            .filter(|w| group_key(w) == self.group)
            .collect()
    }
    fn layout(&self, windows: &[ShellWindow]) -> PickerLayout {
        let widths: Vec<_> = windows
            .iter()
            .map(|window| preview_width(window.preview_size) + 2.0 * PICKER_PADDING)
            .collect();
        picker_layout(&widths, self.context::<ShellContext>().output_size())
    }
    fn scroll_rows(&mut self, down: bool) -> bool {
        let windows = self.windows();
        let layout = self.layout(&windows);
        let next = scrolled_row(self.first_row, windows.len(), layout.rows, down);
        let changed = self.first_row != next;
        self.first_row = next;
        changed
    }
    fn restore(&mut self, id: telorgon::shell::WindowId) {
        self.window_peek.finish(Some(id));
        self.picker.select(None);
    }
    fn hovered_window(&self, windows: &[ShellWindow], layout: PickerLayout) -> Option<telorgon::shell::WindowId> {
        let position = self.pointer?;
        let widths: Vec<_> = windows.iter()
            .map(|window| preview_width(window.preview_size) + 2.0 * PICKER_PADDING).collect();
        picker_index_at(position, &widths, layout, self.first_row)
            .and_then(|index| windows.get(index)).map(|window| window.id)
    }
}
impl ShellWidget for WindowPicker {
    fn surface(&self) -> ShellSurfaceSpec {
        let windows = self.windows();
        let layout = self.layout(&windows);
        ShellSurfaceSpec::new()
            .placement(
                WidgetPlacement::attached_to(self.anchor, ShellEdge::Top)
                    .width(layout.width)
                    .height(layout.height)
                    .offset((ICON_WIDTH - layout.width) * 0.5, -8.0)
                    .margin(8.0),
            )
            .layer(ShellSurfaceLayer::Overlay)
            .visibility_motion(crate::window::WINDOW_MINIMIZE_MOTION)
            .pointer(ShellPointer::Surface)
            .visible(
                !windows.is_empty()
                    && self.watch(&self.picker.selected).as_ref() == Some(&self.group),
            )
            .focus(ShellFocus::OnClick)
            .dismiss_on_outside_press(true)
            .dismiss_on_escape(true)
            .dismiss_on_pointer_leave(true)
    }
    fn window_previews(&self) -> Vec<ShellWindowPreview> {
        let windows = self.windows();
        if self.layout(&windows).list {
            return vec![];
        }
        let mut x = 0.0;
        windows
            .iter()
            .map(|window| {
                let width = preview_width(window.preview_size);
                let left = x;
                x += width + 2.0 * PICKER_PADDING;
                ShellWindowPreview::new(
                    window.id,
                    RectF {
                        x: left + PICKER_PADDING,
                        y: 2.0 * PICKER_PADDING + PICKER_HEADER_HEIGHT,
                        width,
                        height: PREVIEW_HEIGHT,
                    },
                )
            })
            .collect()
    }
    fn dismissed(&mut self, _: ShellDismissReason) {
        self.pointer = None;
        self.window_peek.finish(None);
        if self.picker.selected.snapshot().as_ref() == Some(&self.group) {
            self.picker.select(None);
        }
    }
    fn input(&mut self, event: InputEvent) -> bool {
        if let InputEvent::PointerMoved { position, .. } = event {
            self.pointer = Some(position);
            let windows = self.windows();
            self.window_peek.hover(self.hovered_window(&windows, self.layout(&windows)));
        }
        if let InputEvent::Scroll { delta, .. } = event {
            let windows = self.windows();
            let layout = self.layout(&windows);
            if layout.list && delta.y != 0.0 {
                return self.scroll_rows(delta.y > 0.0);
            }
        }
        false
    }
}
impl Component for WindowPicker {
    fn unmounted(&mut self, _: &mut telorgon::compose::UnmountContext<Self>) {
        if self.picker.selected.snapshot().as_ref().is_none_or(|group| group == &self.group) {
            self.window_peek.finish(None);
        }
    }
    fn view(&self) -> impl View {
        let windows = self.windows();
        let layout = self.layout(&windows);
        // Closing windows or scrolling can move a different tile under a stationary pointer.
        if self.pointer.is_some() {
            self.window_peek.hover(self.hovered_window(&windows, layout));
        }
        let service = self.context::<ShellContext>().windows();
        let mut content = if layout.list { column() } else { row() };
        if layout.list {
            let start = self
                .first_row
                .min(windows.len().saturating_sub(layout.rows));
            for window in windows.iter().skip(start).take(layout.rows) {
                let id = window.id;
                let label = window_label(window);
                content = content.child(
                    window_entry(
                        label,
                        service.icon(id),
                        layout.width,
                        true,
                        telorgon::ui::CornerRadii::all(PICKER_CARD_RADIUS),
                        move |this: &mut Self| this.restore(id),
                        move |this: &mut Self| {
                            let _ = this.context::<ShellContext>().windows().close(id);
                        },
                    )
                    .key(format!("{}:{}", id.slot(), id.generation())),
                );
            }
            content = content.child(
                row()
                    .height(28.0)
                    .gap(8.0)
                    .align_items(Alignment::Center)
                    .child(
                        task_button("▲")
                            .width(28.0)
                            .height(24.0)
                            .enabled(start > 0)
                            .on_press(|this: &mut Self| {
                                this.scroll_rows(false);
                            }),
                    )
                    .child(
                        text(format!(
                            "{}–{} of {}",
                            start + 1,
                            (start + layout.rows).min(windows.len()),
                            windows.len()
                        ))
                        .size(12.0),
                    )
                    .child(
                        task_button("▼")
                            .width(28.0)
                            .height(24.0)
                            .enabled(start + layout.rows < windows.len())
                            .on_press(|this: &mut Self| {
                                this.scroll_rows(true);
                            }),
                    ),
            );
        } else {
            for window in &windows {
                let id = window.id;
                let label = window_label(window);
                content = content.child(
                    window_entry(
                        label,
                        service.icon(id),
                        preview_width(window.preview_size) + 2.0 * PICKER_PADDING,
                        false,
                        telorgon::ui::CornerRadii::all(PICKER_CARD_RADIUS),
                        move |this: &mut Self| this.restore(id),
                        move |this: &mut Self| {
                            let _ = this.context::<ShellContext>().windows().close(id);
                        },
                    )
                    .key(format!("{}:{}", id.slot(), id.generation())),
                );
            }
        }
        content
            .width(layout.width)
            .height(layout.height)
            .background(Background::Color(PICKER_CONTAINER_BACKGROUND_COLOR))
            .corner_radius(PICKER_CARD_RADIUS)
            .overflow(telorgon::Overflow::Clip)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn hover_hit_testing_covers_tiles_and_excludes_list_controls() {
        let widths = [100.0, 150.0, 200.0];
        let layout = PickerLayout { width: 450.0, height: CARD_HEIGHT, list: false, rows: 3 };
        assert_eq!(picker_index_at(PointF { x: 99.0, y: 5.0 }, &widths, layout, 0), Some(0));
        assert_eq!(picker_index_at(PointF { x: 100.0, y: 5.0 }, &widths, layout, 0), Some(1));
        assert_eq!(picker_index_at(PointF { x: 300.0, y: CARD_HEIGHT - 1.0 }, &widths, layout, 0), Some(2));
        assert_eq!(picker_index_at(PointF { x: -1.0, y: 5.0 }, &widths, layout, 0), None);
        let list = PickerLayout { width: 360.0, height: ROW_HEIGHT * 2.0 + 28.0, list: true, rows: 2 };
        assert_eq!(picker_index_at(PointF { x: 10.0, y: 1.0 }, &widths, list, 1), Some(1));
        assert_eq!(picker_index_at(PointF { x: 10.0, y: ROW_HEIGHT + 1.0 }, &widths, list, 1), Some(2));
        assert_eq!(picker_index_at(PointF { x: 10.0, y: ROW_HEIGHT * 2.0 }, &widths, list, 1), None);
    }
    #[component]
    struct HoverOptionsFixture {
        #[input]
        options: Option<Signal<(bool, bool)>>,
    }
    impl Component for HoverOptionsFixture {
        fn view(&self) -> impl View {
            let (styled, hover_within) = *self.watch(self.options.as_ref().unwrap());
            let container = stack()
                .width(100.0)
                .height(50.0)
                .hover_within(hover_within)
                .child(task_button("Child").width(100.0).height(50.0));
            if styled {
                container.inline_style(card_style(telorgon::ui::CornerRadii::all(0.0), false))
            } else {
                container
            }
        }
    }

    #[test]
    fn container_style_and_hover_reconcile_independently() {
        let (options, writer) = Signal::new((false, false));
        let mut runtime = telorgon::application_host::AppRuntimeCore::from_composed_with_extent(
            HoverOptionsFixture { options: Some(options) },
            telorgon::SizeI { width: 100, height: 50 },
        ).unwrap();
        let at = |ms: u64| telorgon::MonotonicInstant::from_nanos(ms * 1_000_000);
        runtime.prepare_frame(at(0), true).unwrap();
        let child = runtime.ui().nodes.alive().iter().copied()
            .find(|node| runtime.ui().kinds.get(*node) == Some(&telorgon::NodeKind::Button)).unwrap();
        let container = runtime.ui().nodes.core(child).unwrap().parent.unwrap();
        runtime.queue_input(InputEvent::mouse_moved(PointF { x: 25.0, y: 25.0 }));
        runtime.flush_input(at(1));
        for (step, (styled, hover_within)) in [
            (true, false), (true, true), (false, true), (false, false), (true, true),
        ].into_iter().enumerate() {
            writer.publish_if_changed((styled, hover_within));
            let time = at(10 + step as u64 * 400);
            runtime.prepare_frame(time, true).unwrap();
            runtime.flush_input(time);
            runtime.prepare_frame(time, true).unwrap();
            assert_eq!(runtime.ui().nodes.core(child).unwrap().parent, Some(container));
            let hovered = runtime.ui().interactions.get(container)
                .is_some_and(|state| state.flags.contains(telorgon::ui::InteractionFlags::HOVERED));
            assert_eq!(hovered, hover_within);
            let bindings = runtime.ui().style_bindings().iter()
                .filter(|binding| binding.state_root == container).collect::<Vec<_>>();
            assert_eq!(bindings.len(), 1, "reconciliation must not duplicate style bindings");
            assert_eq!(bindings[0].local_style.is_some(), styled);
        }
    }

    #[component]
    struct EntryFixture {
        #[input]
        list: bool,
        #[state]
        selected: usize,
        #[state]
        closed: usize,
    }
    impl Component for EntryFixture {
        fn view(&self) -> impl View {
            column()
                .child(window_entry(
                    "Test window".into(),
                    telorgon::ui::ImageId(0).into(),
                    CARD_WIDTH,
                    self.list,
                    telorgon::ui::CornerRadii::all(PICKER_CARD_RADIUS),
                    |this: &mut Self| this.selected += 1,
                    |this: &mut Self| this.closed += 1,
                ))
                .child(text(format!(
                    "selected:{} closed:{}",
                    self.selected, self.closed
                )))
        }
    }
    #[test]
    fn close_hover_tweens_in_and_out() {
        let mut runtime = telorgon::application_host::AppRuntimeCore::from_composed_with_extent(
            EntryFixture { list: false, selected: 0, closed: 0 },
            telorgon::SizeI { width: CARD_WIDTH as i32, height: 220 },
        ).unwrap();
        let at = |ms: u64| telorgon::MonotonicInstant::from_nanos(ms * 1_000_000);
        runtime.prepare_frame(at(0), false).unwrap();
        let close = runtime.ui().nodes.alive().iter().copied()
            .filter(|node| runtime.ui().kinds.get(*node) == Some(&telorgon::NodeKind::Button))
            .last().unwrap();
        let bounds = runtime.layout().computed(close).unwrap().border_rect;
        let color = |ui: &telorgon::ui::MountedUi| {
            ui.box_styles.get(close).unwrap().decoration.background
        };
        let resting = Background::Color(ColorRgba8::rgba(0, 0, 0, 0));
        let hovered = Background::Color(CLOSE_HOVER);
        let duration = u64::from(PICKER_CLOSE_HOVER_DURATION_MS);
        runtime.queue_input(InputEvent::mouse_moved(PointF {
            x: bounds.x + bounds.width / 2.0,
            y: bounds.y + bounds.height / 2.0,
        }));
        runtime.flush_input(at(1));
        runtime.prepare_frame(at(1), false).unwrap();
        if duration > 1 {
            runtime.prepare_frame(at(1 + duration / 2), false).unwrap();
            assert_ne!(color(runtime.ui()), resting);
            assert_ne!(color(runtime.ui()), hovered);
        }
        runtime.prepare_frame(at(1 + duration), false).unwrap();
        assert_eq!(color(runtime.ui()), hovered);
        let leave = 2 + duration;
        runtime.queue_input(InputEvent::mouse_moved(PointF { x: -10.0, y: -10.0 }));
        runtime.flush_input(at(leave));
        runtime.prepare_frame(at(leave), false).unwrap();
        if duration > 1 {
            runtime.prepare_frame(at(leave + duration / 2), false).unwrap();
            assert_ne!(color(runtime.ui()), resting);
            assert_ne!(color(runtime.ui()), hovered);
        }
        runtime.prepare_frame(at(leave + duration), false).unwrap();
        assert_eq!(color(runtime.ui()), resting);
    }

    #[test]
    fn close_is_transparent_on_mount_before_theme_evaluation() {
        for list in [false, true] {
            let runtime = telorgon::ViewRuntime::from_composed(EntryFixture {
                list,
                selected: 0,
                closed: 0,
            })
            .unwrap();
            let close = runtime
                .ui()
                .nodes
                .alive()
                .iter()
                .copied()
                .filter(|node| runtime.ui().kinds.get(*node) == Some(&telorgon::NodeKind::Button))
                .last()
                .unwrap();
            assert_eq!(
                runtime
                    .ui()
                    .box_styles
                    .get(close)
                    .unwrap()
                    .decoration
                    .background,
                Background::Color(ColorRgba8::rgba(0, 0, 0, 0))
            );
        }
    }
    #[test]
    fn card_hover_survives_close_button_and_clears_on_exit() {
        for list in [false, true] {
            let mut runtime = telorgon::application_host::AppRuntimeCore::from_composed_with_extent(
                EntryFixture { list, selected: 0, closed: 0 },
                telorgon::SizeI { width: CARD_WIDTH as i32, height: 220 },
            ).unwrap();
            let at = |ms: u64| telorgon::MonotonicInstant::from_nanos(ms * 1_000_000);
            runtime.prepare_frame(at(0), true).unwrap();
            let card = runtime.ui().style_bindings().iter()
                .find(|binding| binding.local_style.is_some()
                    && runtime.ui().kinds.get(binding.state_root) == Some(&telorgon::NodeKind::Box))
                .unwrap().state_root;
            let close = runtime.ui().nodes.alive().iter().copied()
                .filter(|node| runtime.ui().kinds.get(*node) == Some(&telorgon::NodeKind::Button))
                .last().unwrap();
            let close = runtime.layout().computed(close).unwrap().border_rect;
            for (ms, point, color) in [
                (1, PointF { x: 50.0, y: 15.0 }, PICKER_CARD_HOVER_COLOR),
                (401, PointF { x: close.x + close.width / 2.0, y: close.y + close.height / 2.0 }, PICKER_CARD_HOVER_COLOR),
                (801, PointF { x: -10.0, y: -10.0 }, PICKER_CARD_BACKGROUND_COLOR),
            ] {
                runtime.queue_input(InputEvent::mouse_moved(point));
                runtime.flush_input(at(ms));
                runtime.prepare_frame(at(ms), false).unwrap();
                runtime.prepare_frame(at(ms + 300), false).unwrap();
                assert_eq!(runtime.ui().box_styles.get(card).unwrap().decoration.background,
                    Background::Color(color));
            }
            runtime.queue_input(InputEvent::mouse_moved(PointF {
                x: close.x + close.width / 2.0,
                y: close.y + close.height / 2.0,
            }));
            runtime.flush_input(at(1201));
            runtime.prepare_frame(at(1201), false).unwrap();
            runtime.prepare_frame(at(1501), false).unwrap();
            runtime.deactivate_view(at(1502));
            runtime.prepare_frame(at(1502), false).unwrap();
            runtime.prepare_frame(at(1802), false).unwrap();
            assert_eq!(runtime.ui().box_styles.get(card).unwrap().decoration.background,
                Background::Color(PICKER_CARD_BACKGROUND_COLOR));
        }
    }

    #[test]
    fn whole_entries_share_one_selection_target_and_close_is_independent() {
        for list in [false, true] {
            let mut runtime =
                telorgon::application_host::AppRuntimeCore::from_composed_with_extent(
                    EntryFixture {
                        list,
                        selected: 0,
                        closed: 0,
                    },
                    telorgon::SizeI {
                        width: CARD_WIDTH as i32,
                        height: 220,
                    },
                )
                .unwrap();
            runtime
                .prepare_frame(telorgon::MonotonicInstant::ZERO, true)
                .unwrap();
            let buttons = runtime
                .ui()
                .nodes
                .alive()
                .iter()
                .copied()
                .filter(|node| runtime.ui().kinds.get(*node) == Some(&telorgon::NodeKind::Button))
                .collect::<Vec<_>>();
            assert_eq!(
                buttons.len(),
                2,
                "one selection target and one close control"
            );
            let selection = runtime.layout().computed(buttons[0]).unwrap().border_rect;
            let close = runtime.layout().computed(buttons[1]).unwrap().border_rect;
            let icon_bounds = runtime
                .ui()
                .nodes
                .alive()
                .iter()
                .copied()
                .filter(|node| runtime.ui().kinds.get(*node) == Some(&telorgon::NodeKind::Image))
                .filter_map(|node| runtime.layout().computed(node))
                .map(|layout| layout.border_rect)
                .find(|bounds| {
                    bounds.width == HEADER_ICON_SIZE && bounds.height == HEADER_ICON_SIZE
                })
                .unwrap();
            assert_eq!(icon_bounds.x - selection.x, PICKER_PADDING);
            let header_height = if list {
                ROW_HEIGHT - 2.0 * PICKER_PADDING
            } else {
                PICKER_HEADER_HEIGHT
            };
            assert_eq!(icon_bounds.y - selection.y,
                PICKER_PADDING + (header_height - HEADER_ICON_SIZE) / 2.0);
            assert_eq!(selection.x + selection.width - close.x - close.width, PICKER_PADDING);
            if !list {
                assert_eq!(close.y - selection.y, PICKER_PADDING);
                assert_eq!(
                    CARD_HEIGHT - (2.0 * PICKER_PADDING + PICKER_HEADER_HEIGHT + PREVIEW_HEIGHT),
                    PICKER_PADDING,
                );
            }
            assert_eq!(
                runtime
                    .ui()
                    .box_styles
                    .get(buttons[1])
                    .unwrap()
                    .decoration
                    .background,
                Background::Color(ColorRgba8::rgba(0, 0, 0, 0))
            );
            runtime.queue_input(InputEvent::mouse_moved(PointF {
                x: close.x + 12.0,
                y: close.y + 13.0,
            }));
            runtime.flush_input(telorgon::MonotonicInstant::ZERO);
            runtime
                .prepare_frame(telorgon::MonotonicInstant::ZERO, true)
                .unwrap();
            assert!(runtime.ui().interactions.get(buttons[1]).unwrap().flags
                .contains(telorgon::ui::InteractionFlags::HOVERED));
            runtime.queue_input(InputEvent::mouse_moved(PointF { x: 100.0, y: 200.0 }));
            runtime.flush_input(telorgon::MonotonicInstant::ZERO);
            runtime
                .prepare_frame(telorgon::MonotonicInstant::ZERO, true)
                .unwrap();
            assert_eq!(
                runtime
                    .ui()
                    .box_styles
                    .get(buttons[1])
                    .unwrap()
                    .decoration
                    .background,
                Background::Color(ColorRgba8::rgba(0, 0, 0, 0))
            );
            let height = if list { ROW_HEIGHT } else { CARD_HEIGHT };
            assert_eq!(selection.width, CARD_WIDTH);
            assert_eq!(selection.height, height);
            let click = |runtime: &mut telorgon::application_host::AppRuntimeCore<_>, point| {
                runtime.queue_input(InputEvent::mouse_moved(point));
                runtime.queue_input(InputEvent::mouse_button(
                    telorgon::PointerButton::PRIMARY,
                    telorgon::ButtonState::Pressed,
                ));
                runtime.queue_input(InputEvent::mouse_button(
                    telorgon::PointerButton::PRIMARY,
                    telorgon::ButtonState::Released,
                ));
                runtime.flush_input(telorgon::MonotonicInstant::ZERO);
                runtime
                    .prepare_frame(telorgon::MonotonicInstant::ZERO, true)
                    .unwrap();
            };
            // Icon, title, and body/edge all activate the same control.
            for point in [
                PointF { x: 10.0, y: 15.0 },
                PointF { x: 100.0, y: 15.0 },
                PointF {
                    x: 100.0,
                    y: height - 2.0,
                },
            ] {
                click(&mut runtime, point);
            }
            click(
                &mut runtime,
                PointF {
                    x: close.x + close.width / 2.0,
                    y: close.y + close.height / 2.0,
                },
            );
            assert!(
                runtime
                    .ui()
                    .texts
                    .values()
                    .iter()
                    .any(|value| runtime.ui().string(value.content) == Some("selected:3 closed:1"))
            );
        }
    }
    #[test]
    fn preview_width_tracks_aspect_ratio_and_caps_wide_windows() {
        let width = |w, h| {
            preview_width(Some(SizeF {
                width: w,
                height: h,
            }))
        };
        assert_eq!(width(1920.0, 1080.0), 228.0);
        assert_eq!(width(800.0, 600.0), 171.0);
        assert_eq!(width(600.0, 900.0), 85.0);
        assert_eq!(width(3840.0, 2160.0), width(1920.0, 1080.0));
        assert_eq!(width(5000.0, 500.0), MAX_PREVIEW_WIDTH);
        assert_eq!(width(0.0, 100.0), CARD_WIDTH);
        assert_eq!(width(f32::NAN, 100.0), CARD_WIDTH);
        assert_eq!(preview_width(None), CARD_WIDTH);
        let output = SizeF {
            width: 320.0,
            height: 800.0,
        };
        let narrow = picker_layout(&[171.0, 85.0], output);
        assert_eq!(narrow.width, 256.0);
        assert!(!narrow.list);
        assert!(picker_layout(&[228.0, 85.0], output).list);
        assert!(picker_layout(&[], output).width > 0.0); // Valid while the last window closes.
    }
    #[test]
    fn equal_aspect_cards_keep_their_size_until_the_output_width_threshold() {
        let output = SizeF {
            width: 1920.0,
            height: 1080.0,
        };
        let one = picker_layout(&[CARD_WIDTH; 1], output);
        let two = picker_layout(&[CARD_WIDTH; 2], output);
        assert!(!one.list && !two.list);
        assert_eq!(two.width - one.width, CARD_WIDTH);
        assert_eq!(one.height, two.height);
        assert!(!picker_layout(&[CARD_WIDTH; 8], output).list);
        assert!(picker_layout(&[CARD_WIDTH; 9], output).list);
        let threshold = (2.0 * CARD_WIDTH) / 0.92;
        assert!(
            !picker_layout(
                &[CARD_WIDTH; 2],
                SizeF {
                    width: threshold,
                    ..output
                }
            )
            .list
        );
        assert!(
            picker_layout(
                &[CARD_WIDTH; 2],
                SizeF {
                    width: threshold - 1.0,
                    ..output
                }
            )
            .list
        );
    }
    #[test]
    fn scrolling_clamps_after_windows_close_and_at_either_end() {
        assert_eq!(scrolled_row(90, 15, 10, false), 4);
        assert_eq!(scrolled_row(90, 15, 10, true), 5);
        assert_eq!(scrolled_row(0, 15, 10, false), 0);
        assert_eq!(scrolled_row(5, 15, 10, true), 5);
        assert_eq!(scrolled_row(9, 0, 10, true), 0);
    }
    #[test]
    fn overflow_list_is_bounded_in_both_dimensions() {
        for output in [
            SizeF {
                width: 800.0,
                height: 600.0,
            },
            SizeF {
                width: 320.0,
                height: 240.0,
            },
        ] {
            let layout = picker_layout(&[CARD_WIDTH; 500], output);
            assert!(layout.list);
            assert!(layout.width <= output.width * 0.92);
            assert!(layout.height <= output.height - 72.0);
            assert!(layout.rows > 0 && layout.rows < 500);
        }
    }
    #[test]
    fn long_unicode_titles_truncate_on_character_boundaries() {
        assert_eq!(short_title("é日🙂abc", 3), "é日🙂…");
        assert_eq!(short_title("短い", 3), "短い");
    }
}
