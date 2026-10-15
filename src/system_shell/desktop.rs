use super::control::{Control, Screen};
use telorgon::app::*;

const TEXT: ColorRgba8 = ColorRgba8::rgba(245, 247, 252, 255);

#[component(no_default)]
pub(super) struct DesktopPanel {
    #[input]
    control: Control,
    #[state]
    page: usize,
}
impl DesktopPanel {
    pub fn new(control: Control) -> Self { Self { control, page: 0 } }
    fn capacity(&self) -> usize {
        ((self.context::<ShellContext>().output_size().width - 460.0).max(190.0) / 190.0) as usize
    }
}
impl Component for DesktopPanel {
    fn view(&self) -> impl View {
        let state = self.watch(&self.control.signal);
        let capacity = self.capacity().max(1);
        let last = state.applications.len().saturating_sub(1) / capacity;
        let page = self.page.min(last);
        let mut applications = row().width(Dimension::FILL).height(40.0).gap(10.0);
        for (id, name) in state.applications.iter().skip(page * capacity).take(capacity) {
            let id = id.clone();
            applications = applications.child(button().key(id.clone()).accessible_label(name.clone())
                .width(180.0).height(40.0).corner_radius(10.0).enabled(!state.busy)
                .background(ColorRgba8::rgba(41, 66, 97, 255))
                .child(text(name.clone()).size(16.0).color(TEXT))
                .on_press(move |this: &mut Self| this.control.launch(&id)));
        }
        if last > 0 {
            applications = applications
                .child(button().width(34.0).height(40.0).enabled(page > 0)
                    .accessible_label("Previous applications").child(text("‹").size(24.0).color(TEXT))
                    .on_press(move |this: &mut Self| this.page = page.saturating_sub(1)))
                .child(button().width(34.0).height(40.0).enabled(page < last)
                    .accessible_label("Next applications").child(text("›").size(24.0).color(TEXT))
                    .on_press(move |this: &mut Self| this.page = (page + 1).min(last)));
        }
        row().width(Dimension::FILL).height(72.0).padding(16.0).gap(16.0)
            .align_items(Alignment::Center).background(ColorRgba8::rgba(21, 25, 34, 242))
            .child(column().width(180.0).height(40.0)
                .child(text("Telorgon").height(22.0).size(20.0).color(TEXT))
                .child(text(state.error.as_deref().unwrap_or_else(|| state.selected.as_deref().unwrap_or("")))
                    .height(18.0).size(13.0).color(TEXT)))
            .child(applications)
            .child(button().width(130.0).height(40.0).corner_radius(10.0)
                .accessible_label("Sign out").enabled(!state.busy)
                .background(ColorRgba8::rgba(41, 66, 97, 255))
                .child(text("Sign out").size(16.0).color(TEXT))
                .on_press(|this: &mut Self| this.control.logout()))
    }
}
impl ShellWidget for DesktopPanel {
    fn surface(&self) -> ShellSurfaceSpec {
        ShellSurfaceSpec::new()
            .visible(self.watch(&self.control.signal).screen == Screen::Desktop)
            .placement(WidgetPlacement::edge(ShellEdge::Top).height(72.0))
            .layer(ShellSurfaceLayer::Panel)
            .reserve_space(ShellReservation::WhenVisible)
    }
}
