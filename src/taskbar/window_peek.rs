//! Temporary desktop preview using the existing shell window commands.
//! The taskbar owns the session so popup removal cannot discard pending restoration.
use super::constants::PICKER_WINDOW_PEEK_DELAY_MS;
use std::{cell::RefCell, collections::BTreeMap, rc::Rc, sync::mpsc, time::Duration};
use telorgon::app::{ShellServiceError, ShellWindows, Signal, SignalWriter};
use telorgon::shell::WindowId;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Mode {
    Idle,
    Waiting(WindowId),
    Preview(WindowId),
    Finishing(Option<WindowId>),
}

#[derive(Clone, Copy)]
struct TrackedWindow {
    original: bool,
    // Requests are FIFO but window publications arrive later. Track admission to avoid
    // duplicate commands and to undo even a minimize that has not been published yet.
    requested: bool,
}

struct Session {
    mode: Mode,
    windows: BTreeMap<WindowId, TrackedWindow>,
}
impl Default for Session {
    fn default() -> Self {
        Self {
            mode: Mode::Idle,
            windows: BTreeMap::new(),
        }
    }
}

trait WindowCommands {
    fn minimize(&self, id: WindowId, minimized: bool) -> Result<(), ShellServiceError>;
    fn activate(&self, id: WindowId) -> Result<(), ShellServiceError>;
}
impl WindowCommands for ShellWindows {
    fn minimize(&self, id: WindowId, minimized: bool) -> Result<(), ShellServiceError> {
        self.set_minimized(id, minimized).map(|_| ())
    }
    fn activate(&self, id: WindowId) -> Result<(), ShellServiceError> {
        ShellWindows::activate(self, id).map(|_| ())
    }
}

impl Session {
    fn finish(&mut self, selected: Option<WindowId>) {
        // A child's unmount must not replace the selection already committed by its click.
        if matches!(self.mode, Mode::Finishing(_)) && selected.is_none() {
            return;
        }
        if self.mode != Mode::Idle || selected.is_some() {
            self.mode = Mode::Finishing(selected);
        }
    }

    /// Returns false when queue backpressure requires another UI-thread pass.
    fn apply(&mut self, live: &[(WindowId, bool)], commands: &impl WindowCommands) -> bool {
        self.windows
            .retain(|id, _| live.iter().any(|(live_id, _)| live_id == id));
        if let Mode::Preview(target) = self.mode {
            if !live.iter().any(|(id, _)| *id == target) {
                self.finish(None);
            } else {
                // Include windows opened during the preview, preserving their initial state.
                for &(id, minimized) in live {
                    self.windows.entry(id).or_insert(TrackedWindow {
                        original: minimized,
                        requested: minimized,
                    });
                }
            }
        }
        if let Mode::Finishing(Some(selected)) = self.mode {
            if let Some(&(_, minimized)) = live.iter().find(|(id, _)| *id == selected) {
                self.windows.entry(selected).or_insert(TrackedWindow {
                    original: minimized,
                    requested: minimized,
                });
            }
        }
        let target = match self.mode {
            Mode::Preview(id) => Some(id),
            Mode::Finishing(id) => id,
            _ => return true,
        };
        // Minimize other windows before revealing the hovered window. On completion,
        // restore the original states first and activate the selection last.
        let mut ids: Vec<_> = self.windows.keys().copied().collect();
        ids.sort_by_key(|id| Some(*id) == target);
        for id in ids {
            let window = self.windows.get_mut(&id).unwrap();
            let minimized = match self.mode {
                Mode::Preview(selected) => id != selected,
                Mode::Finishing(selected) => {
                    if selected == Some(id) {
                        false
                    } else {
                        window.original
                    }
                }
                _ => unreachable!(),
            };
            if window.requested == minimized {
                continue;
            }
            match commands.minimize(id, minimized) {
                Ok(()) => window.requested = minimized,
                Err(ShellServiceError::Busy) => return false,
                Err(ShellServiceError::Stale) => {
                    self.windows.remove(&id);
                }
                Err(ShellServiceError::Unavailable) => return false,
            }
        }
        if let Mode::Finishing(selected) = self.mode {
            // Admission is asynchronous. Keep the restoration record until the host
            // publishes the restored state (including across a temporarily locked session).
            if self.windows.iter().any(|(id, window)| {
                let desired = if selected == Some(*id) {
                    false
                } else {
                    window.original
                };
                live.iter()
                    .any(|(live_id, minimized)| live_id == id && *minimized != desired)
            }) {
                return false;
            }
            if let Some(id) = selected.filter(|id| live.iter().any(|(live_id, _)| live_id == id)) {
                match commands.activate(id) {
                    Ok(()) | Err(ShellServiceError::Stale) => {}
                    Err(_) => return false,
                }
            }
            self.windows.clear();
            self.mode = Mode::Idle;
        }
        true
    }
}

// One sleeping worker publishes an epoch only. All window operations stay on the UI thread.
struct Delay {
    ready: Signal<u64>,
    writer: SignalWriter<u64>,
    epoch: u64,
    sender: Option<mpsc::Sender<Option<(u64, Duration)>>>,
}
impl Default for Delay {
    fn default() -> Self {
        let (ready, writer) = Signal::new(0);
        Self {
            ready,
            writer,
            epoch: 0,
            sender: None,
        }
    }
}
impl Delay {
    fn arm(&mut self, duration: Duration) {
        self.epoch = self.epoch.wrapping_add(1).max(1);
        if self.sender.is_none() {
            let (sender, receiver) = mpsc::channel::<Option<(u64, Duration)>>();
            let writer = self.writer.clone();
            std::thread::spawn(move || {
                let mut pending: Option<(u64, Duration)> = None;
                loop {
                    let message = match pending {
                        Some((_, delay)) => receiver.recv_timeout(delay),
                        None => receiver
                            .recv()
                            .map_err(|_| mpsc::RecvTimeoutError::Disconnected),
                    };
                    match message {
                        Ok(next) => pending = next,
                        Err(mpsc::RecvTimeoutError::Timeout) => {
                            if let Some((epoch, _)) = pending.take() {
                                writer.publish_if_changed(epoch);
                            }
                        }
                        Err(mpsc::RecvTimeoutError::Disconnected) => break,
                    }
                }
            });
            self.sender = Some(sender);
        }
        let _ = self
            .sender
            .as_ref()
            .unwrap()
            .send(Some((self.epoch, duration)));
    }
    fn cancel(&mut self) {
        self.epoch = self.epoch.wrapping_add(1).max(1);
        if let Some(sender) = &self.sender {
            let _ = sender.send(None);
        }
    }
    fn elapsed(&self) -> bool {
        *self.ready.snapshot() == self.epoch && self.epoch != 0
    }
}

struct Controller {
    session: Session,
    delay: Delay,
    wake: Signal<u64>,
    writer: SignalWriter<u64>,
    revision: u64,
    pending_hover: Option<WindowId>,
}
impl Default for Controller {
    fn default() -> Self {
        let (wake, writer) = Signal::new(0);
        Self {
            session: Session::default(),
            delay: Delay::default(),
            wake,
            writer,
            revision: 0,
            pending_hover: None,
        }
    }
}
impl Controller {
    fn notify(&mut self) {
        self.revision = self.revision.wrapping_add(1);
        self.writer.publish_if_changed(self.revision);
    }
}

#[derive(Clone, Default)]
pub(super) struct WindowPeek(Rc<RefCell<Controller>>);
impl PartialEq for WindowPeek {
    fn eq(&self, other: &Self) -> bool {
        Rc::ptr_eq(&self.0, &other.0)
    }
}
impl WindowPeek {
    pub fn signals(&self) -> (Signal<u64>, Signal<u64>) {
        let controller = self.0.borrow();
        (controller.wake.clone(), controller.delay.ready.clone())
    }
    pub fn hover(&self, target: Option<WindowId>) {
        let mut controller = self.0.borrow_mut();
        match (controller.session.mode, target) {
            (Mode::Waiting(old) | Mode::Preview(old), Some(id)) if old == id => return,
            (Mode::Idle, None) => return,
            (Mode::Finishing(_), _) => {
                if controller.pending_hover == target {
                    return;
                }
                controller.pending_hover = target;
            }
            (_, None) => {
                controller.delay.cancel();
                controller.session.finish(None);
            }
            (Mode::Preview(_), Some(id)) => controller.session.mode = Mode::Preview(id),
            (_, Some(id)) => {
                controller.session.mode = Mode::Waiting(id);
                controller
                    .delay
                    .arm(Duration::from_millis(PICKER_WINDOW_PEEK_DELAY_MS));
            }
        }
        controller.notify();
    }
    pub fn finish(&self, selected: Option<WindowId>) {
        let mut controller = self.0.borrow_mut();
        controller.pending_hover = None;
        controller.delay.cancel();
        controller.session.finish(selected);
        controller.notify();
    }
    pub fn update(&self, service: &ShellWindows) {
        let live: Vec<_> = service.open().iter().map(|w| (w.id, w.minimized)).collect();
        self.0.borrow_mut().update(&live, service);
    }
}

impl Controller {
    fn update(&mut self, live: &[(WindowId, bool)], commands: &impl WindowCommands) {
        if let Mode::Waiting(target) = self.session.mode {
            if self.delay.elapsed() {
                self.session.mode = Mode::Preview(target);
            }
        }
        if matches!(self.session.mode, Mode::Finishing(_)) && self.delay.elapsed() {
            // A request can be denied after admission, e.g. while session-locked.
            // Retry unfinished restoration from the latest authoritative publication.
            for &(id, minimized) in live {
                if let Some(window) = self.session.windows.get_mut(&id) {
                    window.requested = minimized;
                }
            }
        }
        if !self.session.apply(live, commands) {
            // Queue capacity is bounded. Keep restoration alive after the popup disappears.
            self.delay.arm(Duration::from_millis(16));
        } else if self.session.mode == Mode::Idle {
            if let Some(target) = self.pending_hover.take() {
                self.session.mode = Mode::Waiting(target);
                self.delay
                    .arm(Duration::from_millis(PICKER_WINDOW_PEEK_DELAY_MS));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::Cell;

    fn id(slot: u32) -> WindowId {
        WindowId::new(slot.try_into().unwrap(), 1.try_into().unwrap())
    }
    #[derive(Debug, PartialEq, Eq)]
    enum Command {
        Minimize(WindowId, bool),
        Activate(WindowId),
    }
    struct Fake {
        log: RefCell<Vec<Command>>,
        capacity: Cell<usize>,
    }
    impl Default for Fake {
        fn default() -> Self {
            Self {
                log: RefCell::new(Vec::new()),
                capacity: Cell::new(usize::MAX),
            }
        }
    }
    impl Fake {
        fn record(&self, command: Command) -> Result<(), ShellServiceError> {
            if self.capacity.get() == 0 {
                return Err(ShellServiceError::Busy);
            }
            self.capacity.set(self.capacity.get() - 1);
            self.log.borrow_mut().push(command);
            Ok(())
        }
        fn take(&self) -> Vec<Command> {
            std::mem::take(&mut *self.log.borrow_mut())
        }
    }
    impl WindowCommands for Fake {
        fn minimize(&self, id: WindowId, value: bool) -> Result<(), ShellServiceError> {
            self.record(Command::Minimize(id, value))
        }
        fn activate(&self, id: WindowId) -> Result<(), ShellServiceError> {
            self.record(Command::Activate(id))
        }
    }

    #[test]
    fn preview_switch_and_click_restore_original_states_before_activation() {
        use Command::*;
        let live = [
            (id(1), false),
            (id(2), false),
            (id(3), true),
            (id(4), false),
        ];
        let commands = Fake::default();
        let mut session = Session {
            mode: Mode::Preview(id(3)),
            ..Session::default()
        };
        assert!(session.apply(&live, &commands));
        assert_eq!(
            commands.take(),
            vec![
                Minimize(id(1), true),
                Minimize(id(2), true),
                Minimize(id(4), true),
                Minimize(id(3), false)
            ]
        );
        // Publications still contain the old state: repeated evaluations must be harmless.
        assert!(session.apply(&live, &commands));
        assert!(commands.take().is_empty());
        session.mode = Mode::Preview(id(2));
        assert!(session.apply(&live, &commands));
        assert_eq!(
            commands.take(),
            vec![Minimize(id(3), true), Minimize(id(2), false)]
        );
        session.finish(Some(id(2)));
        session.finish(None); // Popup unmount after the click.
        assert!(session.apply(&live, &commands));
        assert_eq!(
            commands.take(),
            vec![
                Minimize(id(1), false),
                Minimize(id(4), false),
                Activate(id(2))
            ]
        );
        assert_eq!(session.mode, Mode::Idle);
    }

    #[test]
    fn cancellation_reminimizes_an_originally_minimized_preview() {
        use Command::*;
        let live = [(id(1), false), (id(2), true)];
        let commands = Fake::default();
        let mut session = Session {
            mode: Mode::Preview(id(2)),
            ..Session::default()
        };
        session.apply(&live, &commands);
        commands.take();
        session.finish(None);
        assert!(session.apply(&live, &commands));
        assert_eq!(
            commands.take(),
            vec![Minimize(id(1), false), Minimize(id(2), true)]
        );
    }

    #[test]
    fn click_before_delay_restores_and_activates_without_minimizing_others() {
        let commands = Fake::default();
        let mut session = Session {
            mode: Mode::Waiting(id(2)),
            ..Session::default()
        };
        session.finish(Some(id(2)));
        assert!(!session.apply(&[(id(1), false), (id(2), true)], &commands));
        assert_eq!(commands.take(), vec![Command::Minimize(id(2), false)]);
        assert!(session.apply(&[(id(1), false), (id(2), false)], &commands));
        assert_eq!(commands.take(), vec![Command::Activate(id(2))]);
    }

    #[test]
    fn closing_preview_cleans_up_and_new_windows_keep_their_original_state() {
        use Command::*;
        let commands = Fake::default();
        let mut session = Session {
            mode: Mode::Preview(id(1)),
            ..Session::default()
        };
        session.apply(&[(id(1), false), (id(2), false)], &commands);
        commands.take();
        session.apply(
            &[(id(1), false), (id(2), true), (id(3), false), (id(4), true)],
            &commands,
        );
        assert_eq!(commands.take(), vec![Minimize(id(3), true)]);
        assert!(!session.apply(&[(id(2), true), (id(3), true), (id(4), true)], &commands));
        assert_eq!(
            commands.take(),
            vec![Minimize(id(2), false), Minimize(id(3), false)]
        );
        assert!(session.apply(&[(id(2), false), (id(3), false), (id(4), true)], &commands));
        assert_eq!(session.mode, Mode::Idle);
    }

    #[test]
    fn busy_queue_retries_cleanup_without_losing_original_states() {
        use Command::*;
        let live = [(id(1), false), (id(2), false), (id(3), true)];
        let commands = Fake::default();
        commands.capacity.set(1);
        let mut session = Session {
            mode: Mode::Preview(id(3)),
            ..Session::default()
        };
        assert!(!session.apply(&live, &commands));
        assert_eq!(commands.take(), vec![Minimize(id(1), true)]);
        session.finish(Some(id(3)));
        assert!(!session.apply(&live, &commands));
        commands.capacity.set(2);
        assert!(!session.apply(&live, &commands));
        assert_eq!(
            commands.take(),
            vec![Minimize(id(1), false), Minimize(id(3), false)]
        );
        commands.capacity.set(1);
        assert!(session.apply(&[(id(1), false), (id(2), false), (id(3), false)], &commands));
        assert_eq!(commands.take(), vec![Activate(id(3))]);
    }

    #[test]
    fn timer_publication_starts_preview_without_another_pointer_event() {
        let peek = WindowPeek::default();
        let commands = Fake::default();
        let live = [(id(1), false), (id(2), false)];
        peek.hover(Some(id(1)));
        peek.0.borrow_mut().update(&live, &commands);
        assert!(commands.take().is_empty());
        peek.0.borrow_mut().delay.arm(Duration::from_millis(5));
        let deadline = std::time::Instant::now() + Duration::from_secs(2);
        while !peek.0.borrow().delay.elapsed() {
            assert!(
                std::time::Instant::now() < deadline,
                "peek timer did not publish"
            );
            std::thread::sleep(Duration::from_millis(1));
        }
        peek.0.borrow_mut().update(&live, &commands);
        assert_eq!(commands.take(), vec![Command::Minimize(id(2), true)]);
        assert_eq!(peek.0.borrow().session.mode, Mode::Preview(id(1)));
    }

    #[test]
    fn restoration_survives_denial_and_reentry_waits_for_cleanup() {
        let peek = WindowPeek::default();
        let commands = Fake::default();
        peek.0.borrow_mut().session.mode = Mode::Preview(id(1));
        peek.0
            .borrow_mut()
            .update(&[(id(1), false), (id(2), false)], &commands);
        commands.take();
        peek.finish(None);
        let minimized = [(id(1), false), (id(2), true)];
        peek.0.borrow_mut().update(&minimized, &commands);
        assert_eq!(commands.take(), vec![Command::Minimize(id(2), false)]);
        peek.hover(Some(id(2)));
        {
            let mut controller = peek.0.borrow_mut();
            controller
                .delay
                .writer
                .publish_if_changed(controller.delay.epoch);
            // The admitted restore was denied: publication still reports minimized.
            controller.update(&minimized, &commands);
            assert_eq!(controller.session.mode, Mode::Finishing(None));
        }
        assert_eq!(commands.take(), vec![Command::Minimize(id(2), false)]);
        peek.0
            .borrow_mut()
            .update(&[(id(1), false), (id(2), false)], &commands);
        assert_eq!(peek.0.borrow().session.mode, Mode::Waiting(id(2)));
        assert!(commands.take().is_empty());
    }

    #[test]
    fn timer_is_stable_on_same_tile_and_not_rearmed_during_preview() {
        let peek = WindowPeek::default();
        peek.hover(Some(id(1)));
        let epoch = peek.0.borrow().delay.epoch;
        peek.hover(Some(id(1)));
        assert_eq!(peek.0.borrow().delay.epoch, epoch);
        assert_eq!(peek.0.borrow().session.mode, Mode::Waiting(id(1)));
        peek.0.borrow_mut().session.mode = Mode::Preview(id(1));
        peek.hover(Some(id(2)));
        assert_eq!(peek.0.borrow().delay.epoch, epoch);
        assert_eq!(peek.0.borrow().session.mode, Mode::Preview(id(2)));
        peek.finish(None);
        let controller = peek.0.borrow();
        controller.delay.writer.publish_if_changed(epoch); // Stale completion after leave.
        assert!(!controller.delay.elapsed());
        assert_eq!(controller.session.mode, Mode::Finishing(None));
    }
}
