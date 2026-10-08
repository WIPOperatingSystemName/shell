//! One cancellable timer per taskbar, independent of pointer motion frequency.
use super::window_picker::PickerState;
use std::sync::{Arc, Mutex, mpsc};
use std::time::Duration;

#[derive(Default)]
pub(super) struct HoverDelay {
    key: Option<String>,
    generation: Arc<Mutex<u64>>,
    sender: Option<mpsc::Sender<Option<(u64, String, PickerState)>>>,
}
impl HoverDelay {
    pub fn update(&mut self, key: Option<String>, picker: &PickerState, delay: Duration) {
        if self.key == key {
            return;
        }
        self.key = key.clone();
        let generation = {
            let mut generation = self.generation.lock().unwrap();
            *generation = generation.wrapping_add(1);
            *generation
        };
        if self.sender.is_none() && key.is_some() {
            let (sender, receiver) = mpsc::channel::<Option<(u64, String, PickerState)>>();
            let current = self.generation.clone();
            std::thread::spawn(move || {
                let mut pending = None;
                loop {
                    let message = if pending.is_some() {
                        receiver.recv_timeout(delay)
                    } else {
                        receiver
                            .recv()
                            .map_err(|_| mpsc::RecvTimeoutError::Disconnected)
                    };
                    match message {
                        Ok(next) => pending = next,
                        Err(mpsc::RecvTimeoutError::Disconnected) => break,
                        Err(mpsc::RecvTimeoutError::Timeout) => {
                            if let Some((generation, key, picker)) = pending.take() {
                                // Cancellation and publication are serialized, so a stale timer
                                // cannot reopen the picker after a leave or click.
                                let current = current.lock().unwrap();
                                if *current == generation {
                                    picker.select(Some(key));
                                }
                            }
                        }
                    }
                }
            });
            self.sender = Some(sender);
        }
        if let Some(sender) = &self.sender {
            let _ = sender.send(key.map(|key| (generation, key, picker.clone())));
        }
    }
    pub fn cancel(&mut self) {
        self.key = None;
        let mut generation = self.generation.lock().unwrap();
        *generation = generation.wrapping_add(1);
        if let Some(sender) = &self.sender {
            let _ = sender.send(None);
        }
    }
}
impl Drop for HoverDelay {
    fn drop(&mut self) {
        self.cancel();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Instant;

    fn wait_for(picker: &PickerState, key: &str) {
        let deadline = Instant::now() + Duration::from_secs(2);
        while picker.selected.snapshot().as_deref() != Some(key) {
            assert!(Instant::now() < deadline, "hover timer did not publish");
            std::thread::sleep(Duration::from_millis(2));
        }
    }

    #[test]
    fn hover_waits_then_opens_and_motion_on_same_icon_does_not_restart() {
        let picker = PickerState::default();
        let mut timer = HoverDelay::default();
        let delay = Duration::from_millis(80);
        timer.update(Some("app".into()), &picker, delay);
        let generation = *timer.generation.lock().unwrap();
        assert!(picker.selected.snapshot().is_none());
        timer.update(Some("app".into()), &picker, delay);
        assert_eq!(*timer.generation.lock().unwrap(), generation);
        wait_for(&picker, "app");
        timer.cancel();
    }

    #[test]
    fn leave_click_and_drop_cancel_pending_picker_and_switch_retargets() {
        let picker = PickerState::default();
        let delay = Duration::from_millis(30);
        for cancel in 0..3 {
            let mut timer = HoverDelay::default();
            timer.update(Some("stale".into()), &picker, delay);
            match cancel {
                0 => timer.update(None, &picker, delay),
                1 => timer.cancel(),
                _ => drop(timer),
            }
            std::thread::sleep(Duration::from_millis(60));
            assert!(picker.selected.snapshot().is_none());
        }
        let mut timer = HoverDelay::default();
        timer.update(Some("first".into()), &picker, delay);
        timer.update(Some("second".into()), &picker, delay);
        wait_for(&picker, "second");
    }
}
