//! Compositor-side sharing controls.

use telorgon::app::*;
use telorgon::ScreenCastPortalContext;

mod indicator;
pub(crate) use indicator::CaptureIndicator;
#[allow(dead_code)]
mod permissions;

/// App identifiers are untrusted presentation text, even when supplied by the portal frontend.
fn application_label(app_id: &str) -> String {
    let label: String = app_id
        .chars()
        .filter(|c| {
            !c.is_control()
                && !matches!(*c,
        '\u{061c}' | '\u{200e}' | '\u{200f}' | '\u{202a}'..='\u{202e}' | '\u{2066}'..='\u{2069}')
        })
        .take(80)
        .collect();
    if label.trim().is_empty() {
        "An application".into()
    } else {
        label
    }
}
