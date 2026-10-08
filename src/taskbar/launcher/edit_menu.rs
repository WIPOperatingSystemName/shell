use super::*;

#[derive(Clone)]
pub(super) struct EditMenuState {
    pub open: Signal<bool>,
    writer: SignalWriter<bool>,
    editor: Arc<Mutex<ClipboardText>>,
    changed: SignalWriter<u64>,
    errors: SignalWriter<Option<String>>,
    launcher_open: Signal<bool>,
}
impl PartialEq for EditMenuState {
    fn eq(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.editor, &other.editor)
    }
}
impl EditMenuState {
    pub fn new(
        editor: Arc<Mutex<ClipboardText>>,
        changed: SignalWriter<u64>,
        errors: SignalWriter<Option<String>>,
        launcher_open: Signal<bool>,
    ) -> Self {
        let (open, writer) = Signal::new(false);
        Self {
            open,
            writer,
            editor,
            changed,
            errors,
            launcher_open,
        }
    }
    pub fn show(&self) {
        self.writer.publish(true);
    }
    pub fn close(&self) {
        self.writer.publish(false);
    }
}
#[component(no_default)]
pub(super) struct EditMenu {
    #[input]
    state: EditMenuState,
}
impl EditMenu {
    pub fn new(state: EditMenuState) -> Self {
        Self { state }
    }
}
impl ShellWidget for EditMenu {
    fn surface(&self) -> ShellSurfaceSpec {
        ShellSurfaceSpec::new()
            .visible(*self.watch(&self.state.open))
            .placement(
                WidgetPlacement::attached_to(
                    RectF {
                        x: PAD,
                        y: PAD,
                        width: 180.0,
                        height: SEARCH,
                    },
                    ShellEdge::Bottom,
                )
                .width(190.0)
                .height(160.0)
                .margin(8.0),
            )
            .layer(ShellSurfaceLayer::Overlay)
            .pointer(ShellPointer::Surface)
            .dismiss_on_outside_press(true)
            .dismiss_on_escape(true)
    }
    fn dismissed(&mut self, _: ShellDismissReason) {
        self.state.close();
    }
}
impl Component for EditMenu {
    fn view(&self) -> impl View {
        let mut menu = column()
            .width(Dimension::FILL)
            .padding(8.0)
            .gap(2.0)
            .background(Background::Color(COLOR1))
            .corner_radius(8.0);
        for (label, action) in [
            ("Copy    Ctrl+C", ClipboardEditAction::Copy),
            ("Cut       Ctrl+X", ClipboardEditAction::Cut),
            ("Paste   Ctrl+V", ClipboardEditAction::Paste),
            ("Select all  Ctrl+A", ClipboardEditAction::Changed),
        ] {
            menu = menu.child(
                task_button(label)
                    .width(Dimension::FILL)
                    .height(34.0)
                    .child(text(label).size(13.0).color(crate::colors::COLOR5))
                    .inline_style(launcher_button_style(false))
                    .on_press(move |this: &mut Self| {
                        this.state.close();
                        if action == ClipboardEditAction::Changed {
                            this.state.editor.lock().unwrap().select_all();
                            this.state.changed.publish(
                                std::time::SystemTime::now()
                                    .duration_since(std::time::UNIX_EPOCH)
                                    .unwrap_or_default()
                                    .as_nanos() as u64,
                            );
                        } else if let Ok(clipboard) = this.context::<ShellContext>().clipboard() {
                            perform_clipboard_action(
                                clipboard,
                                this.state.editor.clone(),
                                this.state.changed.clone(),
                                this.state.errors.clone(),
                                this.state.launcher_open.clone(),
                                action,
                                telorgon::ClipboardKind::System,
                            );
                        }
                    }),
            );
        }
        menu
    }
}
