use telorgon::app::{KeyBindings, KeyChord, ShortcutKey, session};

fn quit_shell() {
    telorgon::request_exit();
}

fn open_terminal() {
    if let Err(error) = session::command("foot")
        .restart(session::RestartPolicy::Never)
        .recover(false)
        .spawn()
    {
        eprintln!("Failed to launch Foot: {error}");
    }
}

fn recover_applications() {
    match session::pending_recovery() {
        Ok(entries) => {
            for entry in entries {
                if let Err(error) = entry.restore() {
                    eprintln!("Could not restore {:?}: {error}", entry.program());
                }
            }
        }
        Err(error) => eprintln!("Could not read recovery list: {error}"),
    }
}

pub fn get_keybinds() -> KeyBindings {
    KeyBindings::new()
        .bind(KeyChord::new(ShortcutKey::T).control(), open_terminal)
        .bind(KeyChord::new(ShortcutKey::Q).control(), quit_shell)
        .bind(
            KeyChord::new(ShortcutKey::R).control().shift(),
            recover_applications,
        )
}
