use std::{fs, io::{self, Write}, path::{Path, PathBuf}};

pub fn ensure_settings_entry() -> io::Result<()> {
    let data_home = std::env::var_os("XDG_DATA_HOME")
        .map(PathBuf::from)
        .filter(|path| path.is_absolute())
        .or_else(|| std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".local/share")))
        .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "Neither XDG_DATA_HOME nor HOME is available"))?;
    let binary = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent().expect("shell project has a parent directory")
        .join("telorgon-settings-app/target/debug/telorgon-settings-app");
    create_entry(&data_home.join("applications"), &binary)
}

fn create_entry(directory: &Path, binary: &Path) -> io::Result<()> {
    fs::create_dir_all(directory)?;
    let path = directory.join("org.telorgon.settings.desktop");
    // Exclusive creation preserves user edits and entries created by another startup.
    let mut file = match fs::OpenOptions::new().write(true).create_new(true).open(path) {
        Ok(file) => file,
        Err(error) if error.kind() == io::ErrorKind::AlreadyExists => return Ok(()),
        Err(error) => return Err(error),
    };
    let executable = binary.to_string_lossy()
        .replace('\\', "\\\\\\\\")
        .replace('"', "\\\\\"")
        .replace('`', "\\\\`")
        .replace('$', "\\\\$")
        .replace('%', "%%");
    write!(file, "[Desktop Entry]\nType=Application\nName=Telorgon Settings\nComment=Display, sound, and network settings\nExec=\"{executable}\"\nIcon=preferences-system\nTerminal=false\nCategories=Settings;\nKeywords=display;sound;network;wifi;monitor;resolution;scale;\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn creates_entry_and_preserves_existing_customizations() {
        let directory = std::env::temp_dir().join(format!("telorgon-desktop-entry-{}", std::process::id()));
        let applications = directory.join("applications");
        let binary = Path::new("/tmp/settings build/telorgon-settings-app");
        create_entry(&applications, binary).unwrap();
        let path = applications.join("org.telorgon.settings.desktop");
        let entry = fs::read_to_string(&path).unwrap();
        assert!(entry.contains("Exec=\"/tmp/settings build/telorgon-settings-app\"\n"));
        assert!(entry.contains("Name=Telorgon Settings\n"));
        fs::write(&path, "custom entry").unwrap();
        create_entry(&applications, binary).unwrap();
        assert_eq!(fs::read_to_string(&path).unwrap(), "custom entry");
        fs::remove_dir_all(directory).unwrap();
    }
}
