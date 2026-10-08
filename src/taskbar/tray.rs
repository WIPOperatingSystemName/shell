use super::constants::*;
use telorgon::{
    InputEvent, PointF, RectF,
    app::*,
    components::application::MenuStyle,
    components::shell::tray::{TrayArea, TrayAreaStyle, TrayMenuNavigation, TrayMenuView},
    tray::*,
};
#[derive(Clone)]
pub(super) struct TrayPopupState {
    pub open: Signal<bool>,
    writer: SignalWriter<bool>,
}
impl PartialEq for TrayPopupState {
    fn eq(&self, other: &Self) -> bool {
        self.open == other.open
    }
}
impl Default for TrayPopupState {
    fn default() -> Self {
        let (open, writer) = Signal::new(false);
        Self { open, writer }
    }
}
impl TrayPopupState {
    pub fn set(&self, v: bool) {
        self.writer.publish_if_changed(v);
    }
}
pub(super) const TRAY_WIDTH: f32 = 36.0;
const CELL: f32 = 44.0;
const GAP: f32 = 4.0;
const PAD: f32 = 12.0;
const COLUMNS: usize = 5;
const VISIBLE_ROWS: usize = 3;
const GRID_WIDTH: f32 = COLUMNS as f32 * CELL + (COLUMNS - 1) as f32 * GAP;
const GRID_HEIGHT: f32 = VISIBLE_ROWS as f32 * CELL + (VISIBLE_ROWS - 1) as f32 * GAP;

fn scroll_limit(items: usize) -> f32 {
    items.div_ceil(COLUMNS).saturating_sub(VISIBLE_ROWS) as f32 * (CELL + GAP)
}

fn item_at(position: PointF, offset: f32, items: usize) -> Option<usize> {
    let x = position.x - PAD;
    let viewport_y = position.y - PAD;
    if !(0.0..GRID_WIDTH).contains(&x) || !(0.0..GRID_HEIGHT).contains(&viewport_y) {
        return None;
    }
    let y = viewport_y + offset.clamp(0.0, scroll_limit(items));
    let col = (x / (CELL + GAP)) as usize;
    let row = (y / (CELL + GAP)) as usize;
    let index = row * COLUMNS + col;
    (x % (CELL + GAP) < CELL && y % (CELL + GAP) < CELL && index < items).then_some(index)
}
fn style() -> TrayAreaStyle {
    TrayAreaStyle {
        columns: COLUMNS,
        gap: GAP,
        cell_size: CELL,
        icon_size: 24.0,
        ..Default::default()
    }
}
#[component(no_default)]
pub(super) struct TrayPopup {
    #[input]
    host: TrayHandle,
    #[input]
    state: TrayPopupState,
    #[input]
    anchor: RectF,
    #[state]
    pointer: Option<PointF>,
    #[state]
    scroll_offset: f32,
}
impl TrayPopup {
    pub fn new(host: TrayHandle, state: TrayPopupState, anchor: RectF) -> Self {
        Self {
            host,
            state,
            anchor,
            pointer: None,
            scroll_offset: 0.0,
        }
    }
}
impl ShellWidget for TrayPopup {
    fn surface(&self) -> ShellSurfaceSpec {
        ShellSurfaceSpec::new()
            .placement(
                WidgetPlacement::attached_to(self.anchor, ShellEdge::Top)
                    .width(GRID_WIDTH + PAD * 2.0)
                    .height(GRID_HEIGHT + PAD * 2.0)
                    .offset(-180.0, -8.0)
                    .margin(8.0),
            )
            .layer(ShellSurfaceLayer::Overlay)
            .pointer(ShellPointer::Surface)
            .focus(ShellFocus::OnOpen)
            .dismiss_on_outside_press(true)
            .outside_press_excludes_anchor(true)
            .dismiss_on_escape(true)
    }
    fn dismissed(&mut self, _: ShellDismissReason) {
        self.state.set(false);
        let _ = self.host.close_menu();
    }
    fn input(&mut self, event: InputEvent) -> bool {
        use telorgon::input::{ButtonState, PointerButton};
        match event {
            InputEvent::PointerMoved { position, .. } => {
                self.pointer = Some(position);
                false
            }
            InputEvent::PointerButton {
                button,
                state: ButtonState::Pressed,
                ..
            } => {
                let snapshot = self.host.snapshot();
                if let Some(item) = self
                    .pointer
                    .and_then(|p| item_at(p, self.scroll_offset, snapshot.items.len()))
                    .and_then(|i| snapshot.items.get(i))
                {
                    if button == PointerButton::SECONDARY {
                        if item.has_menu {
                            let _ = self.host.request_menu(item.id.clone());
                        } else {
                            let _ = self.host.activate(item.id.clone(), 0, 0);
                        }
                        return true;
                    }
                    if button == PointerButton::MIDDLE {
                        let _ = self.host.secondary_activate(item.id.clone(), 0, 0);
                        return true;
                    }
                }
                false
            }
            InputEvent::Scroll { delta, .. } => {
                if !delta.y.is_finite() {
                    return false;
                }
                let limit = scroll_limit(self.host.snapshot().items.len());
                let next = (self.scroll_offset.clamp(0.0, limit) - delta.y).clamp(0.0, limit);
                let changed = next != self.scroll_offset;
                self.scroll_offset = next;
                changed
            }
            _ => false,
        }
    }
    fn children(&self) -> Vec<ShellChild> {
        let signal = self.host.signal();
        let snapshot = self.watch(&signal);
        if let Some(menu) = &snapshot.menu {
            vec![ShellChild::new(
                format!("tray-menu-{}", menu.owner.as_str()),
                TrayMenuPopup::new(
                    self.host.clone(),
                    RectF {
                        x: PAD,
                        y: PAD,
                        width: 240.0,
                        height: CELL,
                    },
                ),
            )]
        } else {
            vec![]
        }
    }
}
impl Component for TrayPopup {
    fn view(&self) -> impl View {
        let signal = self.host.signal();
        let snapshot = self.watch(&signal);
        let offset = self
            .scroll_offset
            .clamp(0.0, scroll_limit(snapshot.items.len()));
        column()
            .width(GRID_WIDTH + PAD * 2.0)
            .height(GRID_HEIGHT + PAD * 2.0)
            .padding(PAD)
            .corner_radius(10.0)
            .background(Background::Color(TASKBAR_BACKGROUND_COLOR))
            .child(
                column()
                    .width(GRID_WIDTH)
                    .height(GRID_HEIGHT)
                    .overflow(telorgon::ui::Overflow::Clip)
                    .layout_style(telorgon::ui::LayoutStyle {
                        scroll_offset: PointF { x: 0.0, y: offset },
                        ..Default::default()
                    })
                    .child(
                        TrayArea::new(self.host.clone())
                            .style(style())
                            .primary_opens_menu(true),
                    ),
            )
    }
}

#[component(no_default)]
struct TrayMenuPopup {
    #[input]
    host: TrayHandle,
    #[input]
    anchor: RectF,
    #[state]
    navigation: TrayMenuNavigation,
}
impl TrayMenuPopup {
    fn new(host: TrayHandle, anchor: RectF) -> Self {
        Self {
            host,
            anchor,
            navigation: TrayMenuNavigation::default(),
        }
    }
}
impl ShellWidget for TrayMenuPopup {
    fn surface(&self) -> ShellSurfaceSpec {
        let signal = self.host.signal();
        let snapshot = self.watch(&signal);
        let nav_signal = self.navigation.signal();
        self.watch(&nav_signal);
        let levels = snapshot
            .menu
            .as_ref()
            .map(|m| self.navigation.levels(&m.menu));
        let count = levels
            .as_ref()
            .and_then(|levels| levels.iter().map(|items| items.len()).max())
            .unwrap_or(1);
        let width = levels.as_ref().map(|l| l.len()).unwrap_or(1) as f32 * 284.0;
        ShellSurfaceSpec::new()
            .placement(
                WidgetPlacement::attached_to(self.anchor, ShellEdge::Top)
                    .width(width.min(self.context::<ShellContext>().output_size().width - 16.0))
                    .height(
                        (count as f32 * 34.0 + 50.0)
                            .clamp(100.0, 500.0)
                            .min(self.context::<ShellContext>().output_size().height - 32.0),
                    )
                    .margin(8.0),
            )
            .layer(ShellSurfaceLayer::Overlay)
            .pointer(ShellPointer::Surface)
            .focus(ShellFocus::OnOpen)
            .dismiss_on_outside_press(true)
            .outside_press_excludes_anchor(true)
            .dismiss_on_escape(true)
    }
    fn dismissed(&mut self, _: ShellDismissReason) {
        let _ = self.host.close_menu();
    }
    fn input(&mut self, event: InputEvent) -> bool {
        if let InputEvent::Key(event) = event {
            return self.navigation.key(&self.host, &event);
        }
        false
    }
}
impl Component for TrayMenuPopup {
    fn view(&self) -> impl View {
        TrayMenuView::new(self.host.clone())
            .navigation(self.navigation.clone())
            .style(MenuStyle {
                label_size: 13.0,
                ..Default::default()
            })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tray_scrolls_only_beyond_fifteen_icons() {
        for count in [0, 1, 14, 15] {
            assert_eq!(scroll_limit(count), 0.0);
        }
        assert_eq!(scroll_limit(16), CELL + GAP);
        assert_eq!(scroll_limit(20), CELL + GAP);
        assert_eq!(scroll_limit(21), 2.0 * (CELL + GAP));
    }

    #[test]
    fn scrolled_hit_testing_respects_padding_gaps_and_partial_rows() {
        let top_left = PointF {
            x: PAD + 1.0,
            y: PAD + 1.0,
        };
        assert_eq!(item_at(top_left, 0.0, 16), Some(0));
        assert_eq!(item_at(top_left, CELL + GAP, 16), Some(5));
        let bottom_left = PointF {
            x: PAD + 1.0,
            y: PAD + 2.0 * (CELL + GAP) + 1.0,
        };
        assert_eq!(item_at(bottom_left, CELL + GAP, 16), Some(15));
        assert_eq!(
            item_at(
                PointF {
                    x: PAD + CELL + GAP + 1.0,
                    ..bottom_left
                },
                CELL + GAP,
                16
            ),
            None
        );
        for position in [
            PointF {
                x: PAD - 1.0,
                y: PAD + 1.0,
            },
            PointF {
                x: PAD + CELL + 1.0,
                y: PAD + 1.0,
            },
            PointF {
                x: PAD + 1.0,
                y: PAD + CELL + 1.0,
            },
            PointF {
                x: PAD + 1.0,
                y: PAD + GRID_HEIGHT,
            },
        ] {
            assert_eq!(item_at(position, 0.0, 30), None);
        }
        assert_eq!(item_at(top_left, 500.0, 1), Some(0));
    }
}
