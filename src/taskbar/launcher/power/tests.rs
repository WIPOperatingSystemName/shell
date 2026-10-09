use super::*;
use std::io::{BufRead, BufReader};
use std::process::{Child, Command, Stdio};
use std::sync::{Arc, Mutex, atomic::{AtomicBool, Ordering}};

struct PrivateBus(Child);
impl Drop for PrivateBus {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

struct Manager {
    calls: Arc<Mutex<Vec<(PowerAction, bool)>>>,
    denied: Arc<AtomicBool>,
}

#[zbus::interface(name = "org.freedesktop.login1.Manager")]
impl Manager {
    fn reboot(&self, interactive: bool) {
        self.calls.lock().unwrap().push((PowerAction::Restart, interactive));
    }

    fn power_off(&self, interactive: bool) -> zbus::fdo::Result<()> {
        self.calls.lock().unwrap().push((PowerAction::Shutdown, interactive));
        if self.denied.load(Ordering::SeqCst) {
            Err(zbus::fdo::Error::AccessDenied("Power action denied by policy".into()))
        } else {
            Ok(())
        }
    }
}

#[test]
fn power_requests_use_logind_methods_and_return_denials() {
    // A private bus and fake logind prevent tests from affecting the host OS.
    let mut daemon = PrivateBus(Command::new("dbus-daemon")
        .args(["--session", "--nofork", "--print-address=1"])
        .stdout(Stdio::piped()).spawn().unwrap());
    let mut address = String::new();
    BufReader::new(daemon.0.stdout.take().unwrap()).read_line(&mut address).unwrap();
    let calls = Arc::new(Mutex::new(Vec::new()));
    let denied = Arc::new(AtomicBool::new(false));
    let _server = Builder::address(address.trim()).unwrap()
        .name("org.freedesktop.login1").unwrap()
        .serve_at("/org/freedesktop/login1", Manager { calls: calls.clone(), denied: denied.clone() }).unwrap()
        .build().unwrap();
    let client = Builder::address(address.trim()).unwrap()
        .method_timeout(Duration::from_secs(3)).build().unwrap();
    request_on(&client, PowerAction::Restart).unwrap();
    request_on(&client, PowerAction::Shutdown).unwrap();
    denied.store(true, Ordering::SeqCst);
    let error = request_on(&client, PowerAction::Shutdown).unwrap_err();
    assert!(error.to_string().contains("Power action denied by policy"), "{error}");
    assert_eq!(*calls.lock().unwrap(), [
        (PowerAction::Restart, false),
        (PowerAction::Shutdown, false),
        (PowerAction::Shutdown, false),
    ]);
}
