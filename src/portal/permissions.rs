//! Optional shell settings view; never opens automatically or reads storage on construction.
use super::*;
const PAGE_SIZE: usize = 6;

/// Embed with a live ScreenCastPortalContext from the host's capture chooser factory. Refresh explicitly
/// loads the bounded permission inventory; forgetting also stops the application's sharing.
#[telorgon::component]
pub struct CapturePermissions {
    #[input]
    ui: ScreenCastPortalContext,
    #[state]
    page: usize,
}
impl CapturePermissions {
    pub fn new(ui: ScreenCastPortalContext) -> Self {
        Self { ui, page: 0 }
    }
}
impl Component for CapturePermissions {
    fn view(&self) -> impl View {
        let snapshot = self.watch(self.ui.snapshot());
        let last_page = snapshot.saved_permissions.len().saturating_sub(1) / PAGE_SIZE;
        let page = self.page.min(last_page);
        let mut content = column()
            .gap(8.0)
            .child(text("Saved screen-sharing permissions"))
            .child(text(
                "Forgetting also stops the application's active sharing.",
            ))
            .child(
                button().child(text("Refresh permissions").color(telorgon::ColorRgba8::rgba(240, 243, 250, 255)))
                    .enabled(!snapshot.permissions_pending)
                    .on_press(|this: &mut Self| {
                        if !this.ui.snapshot().snapshot().permissions_pending {
                            let _ = this.ui.refresh_saved_permissions();
                        }
                    }),
            );
        if snapshot.permissions_pending {
            content = content.child(text("Updating permissions…"));
        } else if snapshot.saved_permissions.is_empty() {
            content = content.child(text(if snapshot.permissions_loaded {
                "No saved permissions."
            } else {
                "Refresh to load saved permissions."
            }));
        }
        for permission in snapshot
            .saved_permissions
            .iter()
            .skip(page * PAGE_SIZE)
            .take(PAGE_SIZE)
        {
            let app = permission.app_id.clone();
            content = content.child(
                row()
                    .key(app.clone())
                    .gap(12.0)
                    .child(text(format!(
                        "{} · {} saved",
                        application_label(&app),
                        permission.grants
                    )))
                    .child(
                        button().child(text("Forget").color(telorgon::ColorRgba8::rgba(240, 243, 250, 255)))
                            .enabled(!snapshot.permissions_pending)
                            .on_press(move |this: &mut Self| {
                                let current = this.ui.snapshot().snapshot();
                                if !current.permissions_pending
                                    && current.saved_permissions.iter().any(|p| p.app_id == app)
                                {
                                    let _ = this.ui.forget_saved_permissions(&app);
                                }
                            }),
                    ),
            );
        }
        if last_page != 0 {
            content =
                content.child(
                    row()
                        .gap(12.0)
                        .child(
                            button().child(text("Previous").color(telorgon::ColorRgba8::rgba(240, 243, 250, 255))).enabled(page > 0).on_press(
                                move |this: &mut Self| this.page = page.saturating_sub(1),
                            ),
                        )
                        .child(text(format!("{} / {}", page + 1, last_page + 1)))
                        .child(button().child(text("Next").color(telorgon::ColorRgba8::rgba(240, 243, 250, 255))).enabled(page < last_page).on_press(
                            move |this: &mut Self| this.page = (page + 1).min(last_page),
                        )),
                );
        }
        if let Some(error) = &snapshot.permission_error {
            content = content.child(text(error.clone()));
        }
        content
    }
}
