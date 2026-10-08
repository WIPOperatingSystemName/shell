use std::{
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    thread,
    time::{Duration, Instant},
};
mod audio;
use crate::{background::BackgroundControl, settings_config};
use telorgon::host::application::{
    audio_mixer::{AudioMixerHandle, MixerAction},
    display_control::DisplayControl,
};
use telorgon::integrations::pipewire::{ConnectionState, ObjectHandle, RequestState};
use telorgon::services::audio::mixer::MixerTarget;
use telorgon::services::desktop_settings::{
    self as shared, Preferences, SettingsStore, ShellSnapshot, SoundChannel, SoundSettings,
};

#[derive(Clone)]
struct Endpoint(Arc<State>);
struct State {
    store: SettingsStore,
    display: DisplayControl,
    mixer: AudioMixerHandle,
    background: BackgroundControl,
    operations: Mutex<()>,
    restore: Mutex<Restore>,
    stopped: AtomicBool,
}
struct Restore {
    desired: SoundSettings,
    attempted: [Option<(u64, ObjectHandle)>; 2],
    failures: [Option<String>; 2],
    error: Option<String>,
}
fn failure(error: impl std::fmt::Display) -> zbus::fdo::Error {
    zbus::fdo::Error::Failed(error.to_string())
}

#[zbus::interface(name = "org.telorgon.TestShell.Settings1")]
impl Endpoint {
    fn snapshot(&self) -> zbus::fdo::Result<String> {
        let signal = self.0.mixer.signal();
        let mixer = signal.snapshot();
        let snapshot = ShellSnapshot {
            display: self.0.display.snapshot(),
            audio_ready: mixer.state == ConnectionState::Ready,
            audio: Some(audio::snapshot(&mixer)),
            personalization: Some(self.0.background.snapshot()),
            devices: mixer
                .nodes
                .iter()
                .filter_map(|node| {
                    let input = match node.media_class.as_str() {
                        "Audio/Sink" => false,
                        "Audio/Source" => true,
                        _ => return None,
                    };
                    Some(shared::AudioDevice {
                        name: node.name.clone(),
                        label: node.description.clone(),
                        input,
                        is_default: (if input {
                            mixer.default_input
                        } else {
                            mixer.default_output
                        }) == Some(node.handle),
                        volume: mixer.volume(&MixerTarget::Node(node.handle)),
                        muted: node.mute,
                        can_set_volume: node.can_set_volume,
                    })
                })
                .collect(),
            audio_error: self
                .0
                .restore
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .error
                .clone(),
        };
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
            {
                let mut restore = state.restore.lock().unwrap_or_else(|e| e.into_inner());
                restore.desired = preferences.sound;
                restore.attempted = [None, None];
                restore.failures = [None, None];
                restore.error = None;
            }
            let audio_result = state.restore_audio();
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
            if state.stopped.load(Ordering::Acquire) {
                return Err(failure("The shell is stopping"));
            }
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
            if state.stopped.load(Ordering::Acquire) {
                return Err(failure("The shell is stopping"));
            }
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
impl State {
    fn execute(&self, action: MixerAction) -> shared::Result<()> {
        let request = self
            .mixer
            .execute(action)
            .map_err(|error| error.to_string())?;
        let deadline = Instant::now() + Duration::from_secs(3);
        loop {
            if let RequestState::Complete(result) = request.state() {
                return result.map_err(|error| error.to_string());
            }
            if self.stopped.load(Ordering::Acquire) || Instant::now() >= deadline {
                request.cancel();
                return Err("Audio operation did not complete in time".into());
            }
            thread::sleep(Duration::from_millis(10));
        }
    }
    fn restore_audio(&self) -> shared::Result<()> {
        let signal = self.mixer.signal();
        let snapshot = signal.snapshot();
        let desired = self
            .restore
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .desired
            .clone();
        if desired == SoundSettings::default() {
            return Ok(());
        }
        if snapshot.state != ConnectionState::Ready {
            return Err("Audio server is not ready; saved preferences will be retried".into());
        }
        let mut failures = Vec::new();
        for (index, channel) in [&desired.output, &desired.input].into_iter().enumerate() {
            if channel == &SoundChannel::default() {
                continue;
            }
            let input = index == 1;
            let candidates = snapshot
                .nodes
                .iter()
                .filter(|node| {
                    node.media_class == if input { "Audio/Source" } else { "Audio/Sink" }
                        && if channel.device.is_empty() {
                            Some(node.handle)
                                == if input {
                                    snapshot.default_input
                                } else {
                                    snapshot.default_output
                                }
                        } else {
                            node.name == channel.device
                        }
                })
                .collect::<Vec<_>>();
            let [node] = candidates.as_slice() else {
                failures.push(format!(
                    "{} device '{}' is missing or ambiguous",
                    if input { "Input" } else { "Output" },
                    channel.device
                ));
                continue;
            };
            let key = (snapshot.generation, node.handle);
            {
                let mut restore = self.restore.lock().unwrap_or_else(|e| e.into_inner());
                if restore.attempted[index] == Some(key) {
                    if let Some(error) = &restore.failures[index] {
                        failures.push(error.clone());
                    }
                    continue;
                }
                // Attempt once per connected endpoint. Retrying a partially failed operation
                // continuously would overwrite subsequent taskbar or application changes.
                restore.attempted[index] = Some(key);
                restore.failures[index] = None;
            }
            let result = (|| {
                if !channel.device.is_empty() {
                    self.execute(MixerAction::SetDefault {
                        device: node.handle,
                        input,
                    })?;
                }
                if let Some(volume) = channel.volume {
                    if !node.can_set_volume {
                        return Err(format!(
                            "{} does not support volume changes",
                            node.description
                        ));
                    }
                    self.execute(MixerAction::SetVolume {
                        target: MixerTarget::Node(node.handle),
                        volume,
                    })?;
                }
                if let Some(muted) = channel.muted {
                    self.execute(MixerAction::SetMute {
                        target: MixerTarget::Node(node.handle),
                        muted,
                    })?;
                }
                Ok(())
            })();
            if let Err(error) = result {
                self.restore
                    .lock()
                    .unwrap_or_else(|e| e.into_inner())
                    .failures[index] = Some(error.clone());
                failures.push(error);
            }
        }
        let result = if failures.is_empty() {
            Ok(())
        } else {
            Err(failures.join("; "))
        };
        self.restore.lock().unwrap_or_else(|e| e.into_inner()).error =
            result.as_ref().err().cloned();
        result
    }
}

pub struct SettingsService {
    connection: Option<zbus::blocking::Connection>,
    state: Arc<State>,
    worker: Option<thread::JoinHandle<()>>,
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
            mixer,
            background,
            operations: Mutex::new(()),
            restore: Mutex::new(Restore {
                desired: preferences.sound,
                attempted: [None, None],
                failures: [None, None],
                error: None,
            }),
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
        let owner = state.clone();
        let worker = thread::Builder::new()
            .name("desktop-settings-restore".into())
            .spawn(move || {
                while !owner.stopped.load(Ordering::Acquire) {
                    if let Ok(_operation) = owner.operations.try_lock() {
                        let _ = owner.restore_audio();
                    }
                    thread::park_timeout(Duration::from_secs(2));
                }
            })
            .map_err(|error| error.to_string())?;
        Ok(Self {
            connection: Some(connection),
            state,
            worker: Some(worker),
        })
    }
}
impl Drop for SettingsService {
    fn drop(&mut self) {
        self.state.stopped.store(true, Ordering::Release);
        self.connection.take();
        if let Some(worker) = self.worker.take() {
            worker.thread().unpark();
            let _ = worker.join();
        }
    }
}
