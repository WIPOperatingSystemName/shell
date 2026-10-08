use super::*;
use telorgon::{Background, ColorRgba8};

#[telorgon::component]
pub(crate) struct CaptureIndicator {
    #[input]
    ui: ScreenCastPortalContext,
}
impl CaptureIndicator {
    pub fn new(ui: ScreenCastPortalContext) -> Self {
        Self { ui }
    }
}
impl Component for CaptureIndicator {
    fn view(&self) -> impl View {
        let snapshot = self.watch(self.ui.snapshot());
        let compact = self
            .try_context::<ShellContext>()
            .is_some_and(|c| c.output_size().width < 620.0);
        let mut content = column()
            .width(Dimension::Shrink)
            .height(Dimension::Shrink)
            .padding(10.0)
            .gap(8.0)
            .background(Background::Color(ColorRgba8::rgba(76, 28, 28, 255)));
        for (id, app) in &snapshot.sharing {
            let id = *id;
            let app = application_label(app);
            let app: String = app.chars().take(if compact { 16 } else { 40 }).collect();
            let remembered = snapshot.remembered.contains(&id);
            content = content.child(
                row()
                    .width(Dimension::Shrink)
                    .height(Dimension::Shrink)
                    .key(id)
                    .gap(14.0)
                    .child(
                        text(format!(
                            "{}: {app}",
                            if snapshot.starting.contains(&id) {
                                if compact {
                                    "Starting"
                                } else {
                                    "Starting screen sharing"
                                }
                            } else {
                                if compact { "Sharing" } else { "Screen sharing" }
                            }
                        ))
                        .size(14.0)
                        .color(ColorRgba8::rgba(255, 255, 255, 255)),
                    )
                    .child(
                        button().child(text(if remembered {
                            "Stop and forget"
                        } else {
                            "Stop sharing"
                        }).color(telorgon::ColorRgba8::rgba(240, 243, 250, 255)))
                        .on_press(move |this: &mut Self| {
                            if remembered {
                                let _ = this.ui.stop_and_forget(id);
                            } else {
                                let _ = this.ui.stop(id);
                            }
                        }),
                    ),
            );
        }
        if let Some(message) = &snapshot.failure {
            content = content.child(
                row()
                    .width(Dimension::Shrink)
                    .height(Dimension::Shrink)
                    .gap(12.0)
                    .child(
                        text(
                            if compact && message.contains("saved permission could not be removed")
                            {
                                "Could not forget permission.".to_owned()
                            } else if compact {
                                "Sharing failed. Try again.".to_owned()
                            } else {
                                message.clone()
                            },
                        )
                        .size(14.0)
                        .color(ColorRgba8::rgba(255, 255, 255, 255)),
                    )
                    .child(button().child(text("Dismiss").color(telorgon::ColorRgba8::rgba(240, 243, 250, 255))).on_press(|this: &mut Self| {
                        let _ = this.ui.dismiss_failure();
                    })),
            );
        }
        content
    }
}
impl ShellWidget for CaptureIndicator {
    fn surface(&self) -> ShellSurfaceSpec {
        ShellSurfaceSpec::new()
            .layer(ShellSurfaceLayer::Overlay)
            .order(i32::MAX - 1)
            .placement(WidgetPlacement::aligned(0.5, 0.0).margin(8.0))
            .visible(
                !self.ui.snapshot().snapshot().sharing.is_empty()
                    || self.ui.snapshot().snapshot().failure.is_some(),
            )
            .focus(ShellFocus::OnClick)
    }
}
