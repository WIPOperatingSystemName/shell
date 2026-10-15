use std::sync::{Arc, Mutex, atomic::{AtomicBool, Ordering}};
mod audio;
pub(crate) mod sound;
use sound::SoundService;
use crate::{background::BackgroundControl, settings_config};
use telorgon::host::application::{
    audio_mixer::AudioMixerHandle,
    display_control::DisplayControl,
};
use telorgon::services::desktop_settings::{
    self as shared, Preferences, SettingsStore, ShellSnapshot,
};

#[derive(Clone)]
struct Endpoint(Arc<State>);
struct State {
    store: SettingsStore,
    display: DisplayControl,
    sound: SoundService,
    background: BackgroundControl,
    operations: Mutex<()>,
    stopped: AtomicBool,
}
fn failure(error: impl std::fmt::Display) -> zbus::fdo::Error {
    zbus::fdo::Error::Failed(error.to_string())
}

#[zbus::interface(name = "org.telorgon.TestShell.Settings1")]
impl Endpoint {
    fn snapshot(&self) -> zbus::fdo::Result<String> {
        let mut snapshot = ShellSnapshot {
            display: self.0.display.snapshot(),
            personalization: Some(self.0.background.snapshot()),
            ..Default::default()
        };
        self.0.sound.populate(&mut snapshot);
        shared::protocol::encode_snapshot(&snapshot).map_err(failure)
    }
    async fn reload_settings(&self) -> zbus::fdo::Result<()> {
        let state = self.0.clone();
        blocking::unblock(move || {
            let _operation = state
                .operations
                .try_lock()
                .map_err(|_| failure("Another settings operation is in progress"))?;
            let preferences = state.store.load().map_err(failure)?;
            let background_result = state.background.apply(preferences.personalization);
            let audio_result = state.sound.reload(preferences.sound);
            background_result.and(audio_result).map_err(failure)
        })
        .await
    }
    async fn apply_personalization(&self, configuration: String) -> zbus::fdo::Result<()> {
        let state = self.0.clone();
        let desired = shared::protocol::decode(&configuration).map_err(failure)?;
        blocking::unblock(move || {
            let _operation = state
                .operations
                .try_lock()
                .map_err(|_| failure("Another settings operation is in progress"))?;
            if state.stopped.load(Ordering::Acquire) { return Err(failure("The shell is stopping")); }
            state.background.apply(desired).map_err(failure)
        })
        .await
    }
    async fn delete_background(&self, configuration: String) -> zbus::fdo::Result<()> {
        let state = self.0.clone();
        let desired: shared::PersonalizationSettings =
            shared::protocol::decode(&configuration).map_err(failure)?;
        blocking::unblock(move || {
            // Serialize deletion with apply so a currently displayed background cannot vanish.
            let _operation = state
                .operations
                .try_lock()
                .map_err(|_| failure("Another settings operation is in progress"))?;
            if state.stopped.load(Ordering::Acquire) { return Err(failure("The shell is stopping")); }
            if state.background.snapshot().current == desired {
                return Err(failure(
                    "Choose and save another background before deleting the current background",
                ));
            }
            state.store.delete_background(&desired).map_err(failure)
        })
        .await
    }
    async fn preview_display(&self, configuration: String) -> zbus::fdo::Result<u64> {
        let state = self.0.clone();
        let desired = shared::protocol::decode(&configuration).map_err(failure)?;
        blocking::unblock(move || state.display.preview(desired).map_err(failure)).await
    }
    async fn confirm_display(&self, token: u64) -> zbus::fdo::Result<()> {
        let state = self.0.clone();
        blocking::unblock(move || state.display.confirm(token).map_err(failure)).await
    }
    async fn revert_display(&self, token: u64) -> zbus::fdo::Result<()> {
        let state = self.0.clone();
        blocking::unblock(move || state.display.revert(token).map_err(failure)).await
    }
}

pub struct SettingsService {
    connection: Option<zbus::blocking::Connection>,
    state: Arc<State>,
}
impl SettingsService {
    pub fn start(
        store: SettingsStore,
        preferences: Preferences,
        display: DisplayControl,
        mixer: AudioMixerHandle,
        background: BackgroundControl,
    ) -> shared::Result<Self> {
        let state = Arc::new(State {
            store,
            display,
            sound: SoundService::start(preferences.sound, mixer)?,
            background,
            operations: Mutex::new(()),
            stopped: AtomicBool::new(false),
        });
        let endpoint = settings_config::endpoint();
        let connection = zbus::blocking::connection::Builder::session()
            .map_err(|error| error.to_string())?
            .name(endpoint.bus_name())
            .map_err(|error| error.to_string())?
            .serve_at(endpoint.object_path(), Endpoint(state.clone()))
            .map_err(|error| error.to_string())?
            .build()
            .map_err(|error| error.to_string())?;
        Ok(Self {
            connection: Some(connection),
            state,
        })
    }
}
impl Drop for SettingsService {
    fn drop(&mut self) {
        self.state.stopped.store(true, Ordering::Release);
        self.state.sound.stop();
        self.connection.take();
    }
}
