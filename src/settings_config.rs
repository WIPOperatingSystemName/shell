#[cfg(test)]
use std::path::PathBuf;
use telorgon::{
    ImageId,
    services::desktop_settings::{Result, SettingsEndpoint, SettingsStore},
};

const IDENTITY: &str = "telorgon-test-shell";
const BUS_NAME: &str = "org.telorgon.TestShell.Settings";
const OBJECT_PATH: &str = "/org/telorgon/TestShell/Settings";
const INTERFACE: &str = "org.telorgon.TestShell.Settings1";

pub(crate) const BACKGROUND_IMAGE: ImageId = ImageId(0x7fff_fffe);

pub(crate) fn store() -> Result<SettingsStore> {
    SettingsStore::for_identity(IDENTITY)
}

#[cfg(test)]
pub(crate) fn store_at(path: impl Into<PathBuf>) -> SettingsStore {
    SettingsStore::new(IDENTITY, path)
}

pub(crate) fn endpoint() -> SettingsEndpoint {
    SettingsEndpoint::new(BUS_NAME, OBJECT_PATH, INTERFACE)
        .expect("the shell settings D-Bus endpoint is valid")
}
