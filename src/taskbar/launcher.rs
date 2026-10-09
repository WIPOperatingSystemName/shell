//! Compact installed-application launcher with OS power controls.
use super::{
    constants::*,
    taskbar::{task_button, taskbar_button_style},
};
use crate::{assets, colors::*};
mod edit_menu;
mod power;
use power::PowerAction;
use edit_menu::{EditMenu, EditMenuState};
use std::sync::{Arc, Mutex};
use telorgon::input::{ButtonState, LogicalKey, Modifiers, NamedKey};
use telorgon::services::clipboard::{ClipboardEditAction, ClipboardText};
use telorgon::{InputEvent, PointF, RectF, app::*};

#[derive(Clone)]
pub(super) struct LauncherState {
    pub open: Signal<bool>,
    writer: SignalWriter<bool>,
}
impl Default for LauncherState {
    fn default() -> Self {
        let (open, writer) = Signal::new(false);
        Self { open, writer }
    }
}
impl PartialEq for LauncherState {
    fn eq(&self, other: &Self) -> bool {
        self.open == other.open
    }
}
impl LauncherState {
    pub fn set(&self, open: bool) {
        self.writer.publish_if_changed(open);
    }
}

fn launcher_button_style(
    selected: bool,
) -> std::sync::Arc<telorgon::theme::CompiledComponentStyle> {
    use telorgon::{theme::InteractionState, ui::InteractionFlags};
    let mut style = (*taskbar_button_style(selected)).clone();
    let focus = style.states[&InteractionState::Hovered].clone();
    style.states.insert(InteractionState::Focused, focus);
    style.state_precedence.insert(0, InteractionState::Focused);
    style.relevant_states = InteractionFlags::from_bits(
        style.relevant_states.bits() | InteractionFlags::FOCUSED.bits(),
    );
    std::sync::Arc::new(style)
}

const PAD: f32 = 18.0;
const CELL: f32 = 68.0;
const GAP: f32 = 12.0;
const SEARCH: f32 = 48.0;
const GRID_TOP: f32 = PAD + SEARCH + GAP;
const ROWS: usize = 3;

fn scroll_position(current: f32, delta: f32, limit: f32) -> f32 {
    if !delta.is_finite() {
        return current.clamp(0.0, limit);
    }
    (current.clamp(0.0, limit) + delta).clamp(0.0, limit)
}
fn columns(width: f32) -> usize {
    if width >= 350.0 { 4 } else { 3 }
}
fn panel_width(output: f32) -> f32 {
    380.0_f32.min((output - 16.0).max(1.0))
}
fn filtered(mut apps: Vec<ApplicationMetadata>, query: &str) -> Vec<ApplicationMetadata> {
    let query = query.trim().to_lowercase();
    apps.retain(|app| app.name.to_lowercase().contains(&query));
    apps.sort_by_key(|app| (app.name.to_lowercase(), app.id.clone()));
    apps
}

#[component(no_default)]
pub(super) struct AppLauncher {
    #[input]
    state: LauncherState,
    #[state]
    edit_menu: EditMenuState,
    #[state]
    editor: Arc<Mutex<ClipboardText>>,
    #[state]
    edit_signal: Signal<u64>,
    #[state]
    edit_writer: SignalWriter<u64>,
    #[state]
    search_focused: bool,
    #[state]
    focus_slot: Option<usize>,
    #[state]
    scroll_offset: f32,
    #[state]
    pointer: Option<PointF>,
    #[state]
    hovered: Option<String>,
    #[state]
    selected: Option<ApplicationId>,
    #[state]
    hover_anchor: RectF,
    #[state]
    power_pending: Signal<Option<PowerAction>>,
    #[state]
    power_writer: SignalWriter<Option<PowerAction>>,
    #[state]
    error: Signal<Option<String>>,
    #[state]
    error_writer: SignalWriter<Option<String>>,
}
impl AppLauncher {
    pub fn new(state: LauncherState) -> Self {
        let (power_pending, power_writer) = Signal::new(None);
        let (error, error_writer) = Signal::new(None);
        let (edit_signal, edit_writer) = Signal::new(0);
        let editor = Arc::new(Mutex::new(ClipboardText::default()));
        let edit_menu = EditMenuState::new(
            editor.clone(),
            edit_writer.clone(),
            error_writer.clone(),
            state.open.clone(),
        );
        Self {
            state,
            edit_menu,
            editor,
            edit_signal,
            edit_writer,
            search_focused: true,
            focus_slot: None,
            scroll_offset: 0.0,
            pointer: None,
            hovered: None,
            selected: None,
            hover_anchor: RectF::default(),
            power_pending,
            power_writer,
            error,
            error_writer,
        }
    }
    fn width(&self) -> f32 {
        panel_width(self.context::<ShellContext>().output_size().width)
    }
    fn cell(&self) -> f32 {
        CELL.min(((self.width() - PAD * 2.0) / columns(self.width()) as f32 - GAP).max(1.0))
    }
    fn height(&self) -> f32 {
        (GRID_TOP + ROWS as f32 * (self.cell() + GAP) + 71.0)
            .min((self.context::<ShellContext>().output_size().height - BAR_HEIGHT - 16.0).max(1.0))
    }
    fn viewport_height(&self) -> f32 {
        (self.height() - PAD * 2.0 - SEARCH - GAP * 3.0 - 41.0).max(1.0)
    }
    fn content_height(&self, count: usize) -> f32 {
        let rows = count.div_ceil(columns(self.width()));
        rows as f32 * self.cell() + rows.saturating_sub(1) as f32 * GAP
    }
    fn scroll_limit(&self, count: usize) -> f32 {
        (self.content_height(count) - self.viewport_height()).max(0.0)
    }
    fn scroll_by(&mut self, delta: f32) -> bool {
        let next = scroll_position(
            self.scroll_offset,
            delta,
            self.scroll_limit(self.apps().len()),
        );
        let changed = next != self.scroll_offset || self.hovered.is_some();
        self.scroll_offset = next;
        self.hovered = None;
        changed
    }
    fn apps(&self) -> Vec<ApplicationMetadata> {
        // Also called by input handlers; reactive watching belongs in view().
        filtered(
            self.context::<ShellContext>().applications().installed(),
            &self.editor.lock().unwrap().text,
        )
    }
    fn launch(&mut self, id: ApplicationId) {
        self.selected = Some(id.clone());
        self.error_writer.publish(None);
        let state = self.state.clone();
        let errors = self.error_writer.clone();
        // Desktop-file parsing and activation run off the shell thread.
        std::thread::spawn(move || {
            let result = futures_lite::future::block_on(
                session::application(id.as_str())
                    .restart(session::RestartPolicy::Never)
                    .recover(false)
                    .prefer_dbus(false)
                    .launch(),
            );
            match result {
                Ok(result) if result.errors.is_empty() => state.set(false),
                Ok(result) => {
                    eprintln!("Launcher: {:?}", result.errors);
                    errors.publish(Some("Could not open app".into()));
                }
                Err(error) => {
                    eprintln!("Launcher: {error}");
                    errors.publish(Some("Could not open app".into()));
                }
            }
        });
    }
    fn clipboard_action(&mut self, action: ClipboardEditAction) {
        self.clipboard_action_kind(action, telorgon::ClipboardKind::System);
    }
    fn clipboard_action_kind(
        &mut self,
        action: ClipboardEditAction,
        kind: telorgon::ClipboardKind,
    ) {
        let Ok(clipboard) = self.context::<ShellContext>().clipboard() else {
            return;
        };
        perform_clipboard_action(
            clipboard,
            self.editor.clone(),
            self.edit_writer.clone(),
            self.error_writer.clone(),
            self.state.open.clone(),
            action,
            kind,
        );
    }

    fn power_action(&mut self, action: PowerAction) {
        if self.power_pending.snapshot().is_some() {
            return;
        }
        self.hovered = None;
        self.error_writer.publish(None);
        self.power_writer.publish(Some(action));
        let pending = self.power_writer.clone();
        let errors = self.error_writer.clone();
        // D-Bus authorization and shutdown inhibitors must never block UI rendering.
        std::thread::spawn(move || {
            if let Err(error) = power::request(action) {
                eprintln!("Power action {action:?} failed: {error}");
                errors.publish(Some(action.failure(&error)));
                pending.publish(None);
            }
        });
    }
    fn edited(&mut self) {
        self.scroll_offset = 0.0;
        self.selected = None;
        self.hovered = None;
    }
}
fn perform_clipboard_action(
    clipboard: telorgon::services::clipboard::Clipboard,
    editor: Arc<Mutex<ClipboardText>>,
    writer: SignalWriter<u64>,
    error: SignalWriter<Option<String>>,
    open: Signal<bool>,
    action: ClipboardEditAction,
    kind: telorgon::ClipboardKind,
) {
    let snapshot = editor.lock().unwrap().clone();
    let target = snapshot.target();

    std::thread::spawn(move || {
        let result = futures_lite::future::block_on(async {
            match action {
                ClipboardEditAction::Copy | ClipboardEditAction::Cut => {
                    if action == ClipboardEditAction::Cut && snapshot.read_only {
                        return Ok(());
                    }
                    if let Some(text) = snapshot.selected_text() {
                        clipboard
                            .publish(
                                kind,
                                telorgon::services::clipboard::ClipboardContent::text(
                                    text.to_owned(),
                                )?,
                                None,
                            )
                            .await?;
                        if action == ClipboardEditAction::Cut && *open.snapshot() {
                            editor.lock().unwrap().paste(target, "")?;
                        }
                    }
                }
                ClipboardEditAction::Paste => {
                    if let Some(text) = clipboard.read_text_from(kind).await? {
                        if !*open.snapshot() {
                            return Ok(());
                        }
                        let text: String = text
                            .chars()
                            .filter(|ch| !ch.is_control())
                            .take(256)
                            .collect();
                        editor.lock().unwrap().paste(target, &text)?;
                    }
                }
                _ => {}
            }
            Ok::<(), telorgon::services::clipboard::ClipboardError>(())
        });
        if let Err(cause) = result {
            if !matches!(
                cause,
                telorgon::services::clipboard::ClipboardError::Stale
                    | telorgon::services::clipboard::ClipboardError::Cancelled
            ) {
                error.publish(Some(cause.to_string()));
            }
        }
        writer.publish(
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_nanos() as u64,
        );
    });
}

impl ShellWidget for AppLauncher {
    fn surface(&self) -> ShellSurfaceSpec {
        ShellSurfaceSpec::new()
            .visible(*self.watch(&self.state.open))
            .placement(
                WidgetPlacement::attached_to(
                    RectF {
                        x: BAR_PADDING,
                        y: 0.0,
                        width: ICON_WIDTH,
                        height: BAR_HEIGHT,
                    },
                    ShellEdge::Top,
                )
                .width(self.width())
                .height(self.height())
                .offset(0.0, -8.0)
                .margin(8.0),
            )
            .layer(ShellSurfaceLayer::Overlay)
            .pointer(ShellPointer::Surface)
            .focus(ShellFocus::OnOpen)
            .dismiss_on_outside_press(true)
            .outside_press_excludes_anchor(true)
            .dismiss_on_escape(
                !(self.search_focused && !self.editor.lock().unwrap().text.is_empty()),
            )
    }
    fn children(&self) -> Vec<ShellChild> {
        if *self.watch(&self.edit_menu.open) {
            return vec![ShellChild::new(
                "search-edit-menu",
                EditMenu::new(self.edit_menu.clone()),
            )];
        }
        self.hovered
            .as_ref()
            .map(|name| {
                vec![ShellChild::new(
                    "launcher-tooltip",
                    LauncherTooltip {
                        name: name.clone(),
                        anchor: self.hover_anchor,
                    },
                )]
            })
            .unwrap_or_default()
    }
    fn dismissed(&mut self, _: ShellDismissReason) {
        self.editor.lock().unwrap().invalidate_paste();
        self.edit_menu.close();
        self.state.set(false);
    }
    fn input(&mut self, event: InputEvent) -> bool {
        match event {
            InputEvent::PointerButton {
                button,
                state: ButtonState::Pressed,
                ..
            } if button == telorgon::PointerButton::PRIMARY
                && self
                    .pointer
                    .is_some_and(|p| p.y >= PAD && p.y < PAD + SEARCH) =>
            {
                self.search_focused = true;
                self.focus_slot = Some(0);
                true
            }

            InputEvent::PointerButton {
                button,
                state: ButtonState::Pressed,
                ..
            } if button == telorgon::PointerButton::SECONDARY
                && self
                    .pointer
                    .is_some_and(|p| p.y >= PAD && p.y < PAD + SEARCH) =>
            {
                self.edit_menu.show();
                true
            }
            InputEvent::PointerButton {
                button,
                state: ButtonState::Pressed,
                ..
            } if button == telorgon::PointerButton::MIDDLE
                && self
                    .pointer
                    .is_some_and(|p| p.y >= PAD && p.y < PAD + SEARCH) =>
            {
                self.search_focused = true;
                self.clipboard_action_kind(
                    ClipboardEditAction::Paste,
                    telorgon::ClipboardKind::Selection,
                );
                true
            }
            InputEvent::PointerMoved { position, .. } => {
                self.pointer = Some(position);
                let cell = self.cell();
                let cols = columns(self.width());
                let grid_width = cols as f32 * cell + (cols - 1) as f32 * GAP;
                let x = position.x - (self.width() - grid_width) / 2.0;
                let viewport_y = position.y - GRID_TOP;
                let offset = self
                    .scroll_offset
                    .clamp(0.0, self.scroll_limit(self.apps().len()));
                let y = viewport_y + offset;
                let apps = self.apps();
                let index = (y / (cell + GAP)) as usize * cols + (x / (cell + GAP)) as usize;
                let hovered = if x >= 0.0
                    && x < grid_width
                    && viewport_y >= 0.0
                    && viewport_y < self.viewport_height()
                    && x % (cell + GAP) < cell
                    && y % (cell + GAP) < cell
                {
                    apps.get(index).map(|app| app.name.clone())
                } else {
                    None
                };
                let mut hovered = hovered;
                let mut anchor = RectF {
                    x: (self.width() - grid_width) / 2.0 + (index % cols) as f32 * (cell + GAP),
                    y: GRID_TOP + (index / cols) as f32 * (cell + GAP) - offset,
                    width: cell,
                    height: cell,
                };
                if position.y >= self.height() - PAD - 40.0
                    && position.y < self.height() - PAD
                {
                    for (offset, label) in
                        [(40.0, "Shut down"), (84.0, "Restart")]
                    {
                        let left = self.width() - PAD - offset;
                        if position.x >= left && position.x < left + 40.0 {
                            hovered = Some(label.into());
                            anchor = RectF {
                                x: left,
                                y: self.height() - PAD - 40.0,
                                width: 40.0,
                                height: 40.0,
                            };
                        }
                    }
                }
                let changed = hovered != self.hovered;
                self.hovered = hovered;
                self.hover_anchor = anchor;
                changed
            }
            InputEvent::Scroll { delta, .. } => {
                if !self.pointer.is_some_and(|point| {
                    point.x >= PAD
                        && point.x < self.width() - PAD
                        && point.y >= GRID_TOP
                        && point.y < GRID_TOP + self.viewport_height()
                }) {
                    return false;
                }
                self.scroll_by(-delta.y)
            }
            InputEvent::Key(key) if key.state == ButtonState::Pressed => {
                if matches!(
                    key.logical_key,
                    LogicalKey::Named(NamedKey::PageDown | NamedKey::PageUp)
                ) {
                    let delta = if key.logical_key == LogicalKey::Named(NamedKey::PageDown) {
                        self.viewport_height()
                    } else {
                        -self.viewport_height()
                    };
                    return self.scroll_by(delta);
                }
                if key.logical_key == LogicalKey::Named(NamedKey::Tab) {
                    let apps = self.apps().len();
                    let count = apps + 1 + 2 * usize::from(self.power_pending.snapshot().is_none());
                    let backwards = key.modifiers.contains(Modifiers::SHIFT);
                    let next = match self.focus_slot {
                        None => {
                            if backwards {
                                count - 1
                            } else {
                                0
                            }
                        }
                        Some(index) => {
                            if backwards {
                                (index + count - 1) % count
                            } else {
                                (index + 1) % count
                            }
                        }
                    };
                    self.focus_slot = Some(next);
                    self.search_focused = next == 0;
                    self.editor.lock().unwrap().invalidate_paste();
                    if next > 0 && next <= apps {
                        let top = ((next - 1) / columns(self.width())) as f32 * (self.cell() + GAP);
                        let bottom = top + self.cell();
                        if top < self.scroll_offset {
                            self.scroll_offset = top;
                        } else if bottom > self.scroll_offset + self.viewport_height() {
                            self.scroll_offset = bottom - self.viewport_height();
                        }
                        self.scroll_offset = self.scroll_offset.clamp(0.0, self.scroll_limit(apps));
                        self.hovered = None;
                    }
                    return true;
                }
                if !self.search_focused {
                    return false;
                }
                if key.logical_key == LogicalKey::Named(NamedKey::Escape) {
                    let mut editor = self.editor.lock().unwrap();
                    editor.select_all();
                    let _ = editor.replace_selection("");
                } else if key.logical_key == LogicalKey::Named(NamedKey::Enter) {
                    if let Some(app) = self.apps().first() {
                        self.launch(app.id.clone());
                    }
                    return true;
                } else {
                    let action = self.editor.lock().unwrap().key(&key);
                    match action {
                        ClipboardEditAction::None => return false,
                        ClipboardEditAction::Changed => {
                            if self.editor.lock().unwrap().selected_text().is_some() {
                                self.clipboard_action_kind(
                                    ClipboardEditAction::Copy,
                                    telorgon::ClipboardKind::Selection,
                                );
                            }
                        }
                        _ => self.clipboard_action(action),
                    }
                }
                self.edited();
                true
            }
            _ => false,
        }
    }
}
impl Component for AppLauncher {
    fn view(&self) -> impl View {
        let mut panel = column()
            .key("launcher")
            .width(self.width())
            .height(self.height())
            .padding(PAD)
            .gap(GAP)
            .corner_radius(24.0)
            .background(Background::Color(COLOR1.with_alpha(245)))
            .uniform_border(1.0, COLOR2.with_alpha(100));
        let _ = self.watch(&self.edit_signal);
        let editor = self.editor.lock().unwrap().clone();
        let selection = editor.selection();
        let mut search_text = row()
            .width(Dimension::FILL)
            .height(28.0)
            .align_items(Alignment::Center);
        if editor.text.is_empty() {
            search_text = search_text.child(text("Search apps…").size(14.0).color(COLOR7));
        } else {
            search_text = search_text.child(
                text(&editor.text[..selection.start])
                    .size(14.0)
                    .color(COLOR5),
            );
            if self.search_focused && editor.cursor == selection.start {
                search_text = search_text.child(text("│").size(14.0).color(COLOR5));
            }
            if !selection.is_empty() {
                search_text = search_text.child(
                    row().background(Background::Color(COLOR2)).child(
                        text(&editor.text[selection.clone()])
                            .size(14.0)
                            .color(COLOR5),
                    ),
                );
            }
            if self.search_focused && editor.cursor == selection.end && !selection.is_empty() {
                search_text = search_text.child(text("│").size(14.0).color(COLOR5));
            }
            search_text =
                search_text.child(text(&editor.text[selection.end..]).size(14.0).color(COLOR5));
        }
        panel = panel.child(
            row()
                .height(SEARCH)
                .gap(10.0)
                .padding(10.0)
                .align_items(Alignment::Center)
                .corner_radius(15.0)
                .uniform_border(1.0, if self.search_focused { COLOR7 } else { COLOR2 })
                .child(image(assets::icons::SEARCH).width(20.0).height(20.0))
                .child(search_text),
        );
        let apps = self.apps();
        let content_height = self.content_height(apps.len()).max(self.viewport_height());
        let mut grid = column()
            .width(Dimension::FILL)
            .gap(GAP)
            .align_items(Alignment::Center)
            .box_style(telorgon::ui::BoxStyle {
                width: telorgon::ui::SizeRule::Fill(1.0),
                height: telorgon::ui::SizeRule::Logical(content_height),
                min_size: telorgon::ui::SizeRule2D {
                    width: telorgon::ui::SizeRule::Logical(1.0),
                    height: telorgon::ui::SizeRule::Logical(content_height),
                },
                max_size: telorgon::ui::SizeRule2D {
                    width: telorgon::ui::SizeRule::Fill(1.0),
                    height: telorgon::ui::SizeRule::Logical(content_height),
                },
                ..Default::default()
            });
        if apps.is_empty() {
            grid = grid.child(text("No apps").size(12.0).color(COLOR7));
        }
        for (row_index, chunk) in apps.chunks(columns(self.width())).enumerate() {
            let mut line = row().height(self.cell()).gap(GAP).width(
                columns(self.width()) as f32 * self.cell()
                    + (columns(self.width()) - 1) as f32 * GAP,
            );
            for (column_index, app) in chunk.iter().enumerate() {
                let focus_slot = row_index * columns(self.width()) + column_index + 1;
                let id = app.id.clone();
                line = line.child(
                    task_button(&app.name)
                        .key(app.id.as_str())
                        .child(image(self.context::<ShellContext>()
                                .applications()
                                .resolve_icon(&app.id, IconRequest::new().logical_size(34))).width(34.0).height(34.0))
                        .width(self.cell())
                        .height(self.cell())
                        .corner_radius(13.0)
                        .inline_style(launcher_button_style(
                            self.selected.as_ref() == Some(&app.id),
                        ))
                        .on_press(move |this: &mut Self| {
                            this.search_focused = false;
                            this.focus_slot = Some(focus_slot);
                            this.launch(id.clone());
                        }),
                );
            }
            grid = grid.child(line);
        }
        let pending = *self.watch(&self.power_pending);
        let error = self.watch(&self.error);
        panel
            .child(
                column()
                    .width(Dimension::FILL)
                    .height(self.viewport_height())
                    .overflow(telorgon::ui::Overflow::Clip)
                    .layout_style(telorgon::ui::LayoutStyle {
                        scroll_offset: PointF {
                            x: 0.0,
                            y: self.scroll_offset.clamp(0.0, self.scroll_limit(apps.len())),
                        },
                        ..Default::default()
                    })
                    .child(grid),
            )
            .child(
                column()
                    .height(1.0)
                    .width(Dimension::FILL)
                    .background(Background::Color(COLOR2.with_alpha(100))),
            )
            .child(
                row()
                    .height(40.0)
                    .gap(4.0)
                    .align_items(Alignment::Center)
                    .child(
                        column()
                            .width(Dimension::FILL)
                            .overflow(telorgon::ui::Overflow::Clip)
                            .child(
                                text(error.as_ref().map(String::as_str).unwrap_or_else(|| {
                                    pending.map_or("", PowerAction::progress)
                                }))
                                    .size(12.0)
                                    .color(COLOR7),
                            ),
                    )
                    .child(
                        task_button("Restart")
                            .child(image(assets::icons::RESTART).width(23.0).height(23.0))
                            .width(40.0)
                            .height(40.0)
                            .inline_style(launcher_button_style(false))
                            .enabled(pending.is_none())
                            .on_press(|this: &mut Self| this.power_action(PowerAction::Restart)),
                    )
                    .child(
                        task_button("Shut down")
                            .child(image(assets::icons::POWER).width(23.0).height(23.0))
                            .width(40.0)
                            .height(40.0)
                            .inline_style(launcher_button_style(false))
                            .enabled(pending.is_none())
                            .on_press(|this: &mut Self| this.power_action(PowerAction::Shutdown)),
                    ),
            )
    }
}

#[component(no_default)]
struct LauncherTooltip {
    #[input]
    name: String,
    #[input]
    anchor: RectF,
}
impl ShellWidget for LauncherTooltip {
    fn surface(&self) -> ShellSurfaceSpec {
        ShellSurfaceSpec::new()
            .placement(
                WidgetPlacement::attached_to(self.anchor, ShellEdge::Top)
                    .width(240.0)
                    .height(34.0)
                    .offset(0.0, -4.0)
                    .margin(8.0),
            )
            .layer(ShellSurfaceLayer::Overlay)
            .pointer(ShellPointer::PassThrough)
    }
}
impl Component for LauncherTooltip {
    fn view(&self) -> impl View {
        column()
            .padding(8.0)
            .corner_radius(8.0)
            .background(Background::Color(COLOR1))
            .uniform_border(1.0, COLOR2)
            .overflow(telorgon::ui::Overflow::Clip)
            .child(text(&self.name).size(12.0).color(COLOR5))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn search_edits_unicode_and_escape_clears_without_closing() {
        use telorgon::input::{KeyEvent, PhysicalKey, PhysicalKeyCode};
        let state = LauncherState::default();
        state.set(true);
        let mut launcher = AppLauncher::new(state.clone());
        launcher
            .editor
            .lock()
            .unwrap()
            .replace_selection("Café")
            .unwrap();
        let press = |name| {
            InputEvent::Key(
                KeyEvent::new(
                    PhysicalKey::from_code(PhysicalKeyCode::Backspace),
                    ButtonState::Pressed,
                )
                .with_logical_key(LogicalKey::Named(name)),
            )
        };
        launcher.input(press(NamedKey::Backspace));
        assert_eq!(launcher.editor.lock().unwrap().text, "Caf");
        launcher.input(press(NamedKey::ArrowLeft));
        launcher.input(press(NamedKey::Delete));
        assert_eq!(launcher.editor.lock().unwrap().text, "Ca");
        launcher.input(press(NamedKey::Escape));
        assert!(launcher.editor.lock().unwrap().text.is_empty());
        assert_eq!(launcher.editor.lock().unwrap().cursor, 0);
        assert!(*state.open.snapshot());
    }

    #[test]
    fn scrolling_preserves_fractional_deltas_and_clamps_at_edges() {
        let offset = scroll_position(0.0, 7.25, 300.0);
        assert_eq!(offset, 7.25);
        assert_eq!(scroll_position(offset, 2.5, 300.0), 9.75);
        assert_eq!(scroll_position(offset, -100.0, 300.0), 0.0);
        assert_eq!(scroll_position(offset, 1000.0, 300.0), 300.0);
        assert_eq!(scroll_position(offset, f32::NAN, 300.0), offset);
        assert_eq!(scroll_position(offset, 20.0, 0.0), 0.0);
    }
    #[test]
    fn grid_adapts_without_horizontal_overflow() {
        for output in [300.0, 360.0, 1920.0] {
            let width = panel_width(output);
            let count = columns(width);
            let cell = CELL.min(((width - PAD * 2.0) / count as f32 - GAP).max(1.0));
            assert!(count as f32 * cell + (count - 1) as f32 * GAP <= width - PAD * 2.0);
        }
    }
    #[test]
    fn search_filters_names_case_insensitively_and_sorts() {
        let app = |name: &str| ApplicationMetadata {
            id: ApplicationId::new(name),
            name: name.into(),
            generic_name: None,
            description: None,
            keywords: vec![],
            categories: vec![],
            icon: None,
            visibility: ApplicationVisibility::Visible,
            startup_wm_class: None,
        };
        let found = filtered(
            vec![app("Terminal"), app("Files"), app("terminal Two")],
            " TERMINAL ",
        );
        assert_eq!(
            found
                .iter()
                .map(|app| app.name.as_str())
                .collect::<Vec<_>>(),
            ["Terminal", "terminal Two"]
        );
        assert!(filtered(found, "absent").is_empty());
    }
}
