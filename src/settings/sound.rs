//! Shared user-context audio observation and saved-preference restoration.
use std::{
    sync::{Arc, Mutex, atomic::{AtomicBool, Ordering}},
    thread,
    time::{Duration, Instant},
};
use telorgon::host::application::audio_mixer::{AudioMixerHandle, MixerAction};
use telorgon::integrations::pipewire::{ConnectionState, ObjectHandle, RequestState};
use telorgon::services::audio::mixer::MixerTarget;
use telorgon::services::desktop_settings::{self as shared, ShellSnapshot, SoundChannel, SoundSettings};

struct Restore {
    desired: SoundSettings,
    attempted: [Option<(u64, ObjectHandle)>; 2],
    failures: [Option<String>; 2],
    error: Option<String>,
}
struct State {
    mixer: AudioMixerHandle,
    operations: Mutex<()>,
    restore: Mutex<Restore>,
    stopped: AtomicBool,
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

pub(crate) struct SoundService {
    state: Arc<State>,
    worker: Option<thread::JoinHandle<()>>,
}
impl SoundService {
    pub fn start(desired: SoundSettings, mixer: AudioMixerHandle) -> shared::Result<Self> {
        desired.validate()?;
        let state = Arc::new(State {
            mixer,
            operations: Mutex::new(()),
            restore: Mutex::new(Restore { desired, attempted: [None, None], failures: [None, None], error: None }),
            stopped: AtomicBool::new(false),
        });
        let owner = state.clone();
        let worker = thread::Builder::new().name("desktop-audio-restore".into()).spawn(move || {
            while !owner.stopped.load(Ordering::Acquire) {
                if let Ok(_operation) = owner.operations.try_lock() {
                    let result = owner.restore_audio();
                    owner.restore.lock().unwrap_or_else(|error| error.into_inner()).error = result.err();
                }
                thread::park_timeout(Duration::from_secs(2));
            }
        }).map_err(|error| error.to_string())?;
        Ok(Self { state, worker: Some(worker) })
    }
    pub fn reload(&self, desired: SoundSettings) -> shared::Result<()> {
        desired.validate()?;
        let _operation = self.state.operations.try_lock().map_err(|_| "Another sound operation is in progress")?;
        *self.state.restore.lock().unwrap_or_else(|error| error.into_inner()) =
            Restore { desired, attempted: [None, None], failures: [None, None], error: None };
        let result = self.state.restore_audio();
        self.state.restore.lock().unwrap_or_else(|error| error.into_inner()).error = result.as_ref().err().cloned();
        result
    }
    pub fn populate(&self, snapshot: &mut ShellSnapshot) {
        let signal = self.state.mixer.signal();
        let mixer = signal.snapshot();
        snapshot.audio_ready = mixer.state == ConnectionState::Ready;
        snapshot.audio = Some(super::audio::snapshot(&mixer));
        snapshot.devices = mixer.nodes.iter().filter_map(|node| {
            let input = match node.media_class.as_str() {
                "Audio/Sink" => false, "Audio/Source" => true, _ => return None,
            };
            Some(shared::AudioDevice {
                name: node.name.clone(), label: node.description.clone(), input,
                is_default: (if input { mixer.default_input } else { mixer.default_output }) == Some(node.handle),
                volume: mixer.volume(&MixerTarget::Node(node.handle)), muted: node.mute,
                can_set_volume: node.can_set_volume,
            })
        }).collect();
        snapshot.audio_error = self.state.restore.lock().unwrap_or_else(|error| error.into_inner()).error.clone();
    }
    pub fn stop(&self) {
        self.state.stopped.store(true, Ordering::Release);
        if let Some(worker) = &self.worker { worker.thread().unpark(); }
    }
}
impl Drop for SoundService {
    fn drop(&mut self) {
        self.stop();
        if let Some(worker) = self.worker.take() {
            worker.thread().unpark();
            let _ = worker.join();
        }
    }
}
