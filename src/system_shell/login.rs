use super::control::{Control, Screen};
use telorgon::app::*;
use telorgon::{InputEvent, input::{ButtonState, LogicalKey, NamedKey, TextInputEvent}, ui::UiEventKind};

const TEXT: ColorRgba8 = ColorRgba8::rgba(245, 247, 252, 255);
const MUTED: ColorRgba8 = ColorRgba8::rgba(184, 195, 211, 255);
const ACCENT: ColorRgba8 = ColorRgba8::rgba(108, 174, 255, 255);

#[component(no_default)]
pub(super) struct LoginShell {
    #[input]
    control: Control,
    #[state]
    password: String,
}
impl LoginShell {
    pub fn new(control: Control) -> Self {
        Self { control, password: String::new() }
    }

    fn submit(&mut self) {
        self.control.authenticate(std::mem::take(&mut self.password));
    }

    fn password_input(&mut self, event: &telorgon::ui::UiEvent) -> bool {
        if self.control.signal.snapshot().busy { return true; }
        let UiEventKind::Input(input) = &event.kind else { return false };
        let input = match input {
            InputEvent::Key(key) if key.state == ButtonState::Pressed => {
                match key.logical_key {
                    LogicalKey::Named(NamedKey::Enter) => { self.submit(); return true; }
                    LogicalKey::Named(NamedKey::Backspace) => { self.password.pop(); return true; }
                    LogicalKey::Named(NamedKey::Escape) => { self.password.clear(); return true; }
                    _ => {}
                }
                key.text.as_ref().map(|text| text.as_str()).unwrap_or("")
            }
            InputEvent::TextInput(TextInputEvent::Commit(value)) => value.as_str(),
            _ => return false,
        };
        if !input.is_empty() && !input.chars().any(char::is_control) && self.password.len() + input.len() <= 512 {
            self.password.push_str(input);
            true
        } else { false }
    }
}
impl Component for LoginShell {
    fn view(&self) -> impl View {
        let state = self.watch(&self.control.signal);
        let (rows, body_height) = if state.screen == Screen::Starting {
            (2, 66.0)
        } else if state.users.is_empty() {
            (3, 90.0)
        } else {
            let users = state.users.len();
            let selected = usize::from(state.selected.is_some());
            (3 + users + 2 * selected, 86.0 + 76.0 * users as f32 + 100.0 * selected as f32)
        };
        let panel_height = 58.0 + body_height + 18.0 * (rows - 1) as f32;
        let mut content = column()
            .width(384.0)
            .height(panel_height)
            .gap(18.0)
            .padding(28.0)
            .corner_radius(24.0)
            .background(ColorRgba8::rgba(21, 25, 34, 242))
            .uniform_border(1.0, ColorRgba8::rgba(255, 255, 255, 40))
            .child(text("Telorgon").height(26.0).size(20.0).color(MUTED));
        if state.screen == Screen::Starting {
            content = content.child(text("Starting…").height(40.0).size(32.0).color(TEXT));
        } else {
            content = content.child(text("Sign in").height(40.0).size(32.0).color(TEXT));
            if state.users.is_empty() {
                content = content.child(text("No accounts are available.").height(24.0).size(16.0).color(MUTED));
            }
            for user in &state.users {
                let selected = state.selected.as_deref() == Some(user.name.as_str());
                let name = user.name.clone();
                let initial = user.label.chars().next().unwrap_or('?').to_uppercase().to_string();
                content = content.child(
                    button()
                        .key(user.name.clone())
                        .accessible_label(format!("Select {}", user.label))
                        .enabled(!state.busy)
                        .width(Dimension::FILL)
                        .height(76.0)
                        .padding(14.0)
                        .corner_radius(14.0)
                        .background(if selected { ColorRgba8::rgba(41, 66, 97, 255) } else { ColorRgba8::rgba(35, 41, 54, 255) })
                        .uniform_border(2.0, if selected { ACCENT } else { ColorRgba8::rgba(255, 255, 255, 24) })
                        .child(
                            row().height(44.0).gap(14.0).align_items(Alignment::Center)
                                .child(column().width(44.0).height(44.0).corner_radius(22.0)
                                    .align_items(Alignment::Center).justify_content(Alignment::Center)
                                    .background(ColorRgba8::rgba(65, 87, 120, 255))
                                    .child(text(initial).size(24.0).color(TEXT)))
                                .child(column().height(44.0).gap(4.0)
                                    .child(text(user.label.clone()).height(22.0).size(18.0).color(TEXT))
                                    .child(text(user.name.clone()).height(18.0).size(14.0).color(MUTED))),
                        )
                        .on_press(move |this: &mut Self| { this.password.clear(); this.control.select(&name); }),
                );
            }
            if state.selected.is_some() {
                content = content
                    .child(text_input().key("password").accessible_label("Password").secure(true)
                        .autofocus(true).enabled(!state.busy).width(Dimension::FILL).height(50.0)
                        .padding(14.0).corner_radius(12.0).background(ColorRgba8::rgba(35, 41, 54, 255))
                        .uniform_border(1.0, ACCENT)
                        .child(text(if self.password.is_empty() { "Password".into() } else { "•".repeat(self.password.chars().count()) })
                            .height(22.0).size(18.0).color(MUTED))
                        .on_input(|this: &mut Self, event| this.password_input(event)))
                    .child(button().width(Dimension::FILL).height(50.0).padding(14.0).corner_radius(12.0)
                        .accessible_label("Sign in").enabled(!state.busy && !self.password.is_empty())
                        .background(ColorRgba8::rgba(41, 66, 97, 255))
                        .child(text(if state.busy { "Signing in…" } else { "Sign in" }).height(22.0).size(18.0).color(TEXT))
                        .on_press(|this: &mut Self| this.submit()))
                    .child(text(if state.error.is_some() { "Sign-in failed. Please try again." } else { "Enter your password." })
                        .height(20.0).size(15.0).color(MUTED));
            } else if !state.users.is_empty() {
                content = content.child(text("Choose an account to continue.").height(20.0).size(15.0).color(MUTED));
            }
        }
        column()
            .width(Dimension::FILL)
            .height(Dimension::FILL)
            .align_items(Alignment::Center)
            .justify_content(Alignment::Center)
            .background(ColorRgba8::rgba(9, 14, 24, 140))
            .child(content)
    }
}
impl ShellWidget for LoginShell {
    fn surface(&self) -> ShellSurfaceSpec {
        ShellSurfaceSpec::new()
            .visible(self.watch(&self.control.signal).screen != Screen::Desktop)
            .placement(WidgetPlacement::fill())
            .layer(ShellSurfaceLayer::Overlay)
            .pointer(ShellPointer::Modal)
            .focus(ShellFocus::OnOpen)
    }
}
