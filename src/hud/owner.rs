use super::model::{HudModel, HudState, brightness_reading, volume_reading};
use std::{
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
        mpsc::{self, SyncSender},
    },
    thread::{self, JoinHandle},
    time::{Duration, Instant},
};
use telorgon::app::{Signal, SystemShortcutFeedback};
use telorgon::host::application::audio_mixer::MixerSnapshot;
use telorgon::screen_brightness::ScreenBrightnessSnapshot;

const SAMPLE_INTERVAL: Duration = Duration::from_millis(30);
enum Message {
    Request(SystemShortcutFeedback),
    Stop,
}

#[derive(Clone)]
pub(crate) struct HudHandle {
    pub(super) state: Signal<HudState>,
    sender: SyncSender<Message>,
}
impl PartialEq for HudHandle {
    fn eq(&self, other: &Self) -> bool {
        self.state == other.state
    }
}
impl HudHandle {
    pub fn notify(&self, feedback: SystemShortcutFeedback) {
        // The compositor never waits for presentation work or for a native request to finish.
        let _ = self.sender.try_send(Message::Request(feedback));
    }
}

pub(crate) struct SystemHuds {
    handle: HudHandle,
    stopped: Arc<AtomicBool>,
    worker: Option<JoinHandle<()>>,
}
impl SystemHuds {
    pub fn start(
        volume: Signal<MixerSnapshot>,
        brightness: Signal<ScreenBrightnessSnapshot>,
    ) -> std::io::Result<Self> {
        let (state, writer) = Signal::new(HudState::default());
        let (sender, receiver) = mpsc::sync_channel(32);
        let stopped = Arc::new(AtomicBool::new(false));
        let worker_stop = stopped.clone();
        let worker = thread::Builder::new()
            .name("test-shell-system-huds".into())
            .spawn(move || {
                let mut model = HudModel::default();
                while !worker_stop.load(Ordering::Acquire) {
                    // Sample existing snapshots only while feedback is visible; idle has no timer.
                    let message = if model.active() {
                        match receiver.recv_timeout(SAMPLE_INTERVAL) {
                            Ok(message) => Some(message),
                            Err(mpsc::RecvTimeoutError::Timeout) => None,
                            Err(mpsc::RecvTimeoutError::Disconnected) => break,
                        }
                    } else {
                        match receiver.recv() {
                            Ok(message) => Some(message),
                            Err(_) => break,
                        }
                    };
                    if worker_stop.load(Ordering::Acquire) || matches!(message, Some(Message::Stop))
                    {
                        break;
                    }
                    let now = Instant::now();
                    let volume = volume_reading(&volume.snapshot());
                    let brightness = brightness_reading(&brightness.snapshot());
                    if let Some(Message::Request(feedback)) = message {
                        model.request(feedback, volume, brightness, now);
                    }
                    // Coalesce bursts without an unbounded queue or a thread per key press.
                    for _ in 0..32 {
                        match receiver.try_recv() {
                            Ok(Message::Request(feedback)) => {
                                model.request(feedback, volume, brightness, now)
                            }
                            Ok(Message::Stop) => return,
                            Err(_) => break,
                        }
                    }
                    model.observe(volume, brightness, now);
                    writer.publish_if_changed(model.state());
                }
            })?;
        Ok(Self {
            handle: HudHandle { state, sender },
            stopped,
            worker: Some(worker),
        })
    }
    pub(super) fn handle(&self) -> HudHandle {
        self.handle.clone()
    }
    pub fn feedback(&self) -> impl Fn(SystemShortcutFeedback) + Send + Sync + 'static {
        let handle = self.handle();
        move |feedback| handle.notify(feedback)
    }
}
impl Drop for SystemHuds {
    fn drop(&mut self) {
        self.stopped.store(true, Ordering::Release);
        let _ = self.handle.sender.try_send(Message::Stop);
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}
