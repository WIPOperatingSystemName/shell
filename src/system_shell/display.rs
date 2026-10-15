use telorgon::host::application::display_control::{DisplayConfiguration, DisplayControl};
use telorgon::services::desktop_settings::{protocol, PersonalizationSnapshot, ShellSnapshot};

/// Called while Control's session-operation lock is held, off the compositor thread.
pub(super) fn restore(display: &DisplayControl, configuration: DisplayConfiguration) -> Result<(), String> {
    if let Some(token) = display.snapshot().preview { display.revert(token)?; }
    let token = display.preview(configuration)?;
    display.confirm(token)
}

pub(super) fn request(display: &DisplayControl, personalization: PersonalizationSnapshot, operation: &str, configuration: &str) -> Result<String, String> {
    match operation {
        "snapshot" => protocol::encode_snapshot(&ShellSnapshot { display: display.snapshot(), personalization: Some(personalization), ..Default::default() }),
        "preview" => display.preview(protocol::decode(configuration)?).map(|token| token.to_string()),
        "confirm" | "revert" => {
            let token = configuration.parse().map_err(|_| "Invalid preview token")?;
            if operation == "confirm" { display.confirm(token)?; } else { display.revert(token)?; }
            Ok(String::new())
        }
        _ => Err("Unsupported display operation".into()),
    }
}
