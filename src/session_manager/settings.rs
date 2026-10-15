//! A private session capability, never a public system-bus permission for users.
use super::ipc;
use crate::{session_pixels, settings_config::BACKGROUND_IMAGE, settings::sound::SoundService};
use std::os::fd::OwnedFd;
use std::{net::Shutdown, os::unix::net::UnixStream, sync::{Arc, Mutex}, time::Duration};
use telorgon::services::desktop_settings::{protocol, PersonalizationSettings, SettingsStore, ShellSnapshot};

pub(super) struct Bridge(UnixStream);
impl Bridge {
    pub fn start(channel: UnixStream, session: String) -> Result<Self, String> {
        let mut worker = channel.try_clone().map_err(|error| error.to_string())?;
        worker.set_write_timeout(Some(Duration::from_secs(15))).map_err(|error| error.to_string())?;
        std::thread::Builder::new().name("session-settings".into()).spawn(move || {
            let connection = zbus::blocking::connection::Builder::system()
                .map(|builder| builder.method_timeout(Duration::from_secs(15)))
                .and_then(|builder| builder.build());
            let Ok(connection) = connection else { return };
            while let Ok(message) = ipc::receive(&mut worker) {
                let Ok((operation, configuration, has_pixels)) = serde_json::from_value::<(String, String, bool)>(message) else { break };
                let pixels = if has_pixels {
                    let Ok(fd) = ipc::receive_fd(&worker) else { break };
                    vec![zbus::zvariant::OwnedFd::from(fd)]
                } else { Vec::new() };
                let result = (|| -> Result<String, String> {
                    if operation == "background" {
                        return super::shell(&connection).and_then(|proxy| proxy.call("Background", &(&session, configuration, pixels)))
                            .map_err(|error| error.to_string());
                    }
                    if !pixels.is_empty() || !matches!(operation.as_str(), "snapshot" | "preview" | "confirm" | "revert") {
                        return Err("Unsupported display operation".into());
                    }
                    super::shell(&connection).and_then(|proxy| proxy.call("Display", &(&session, operation, configuration)))
                        .map_err(|error| error.to_string())
                })();
                if ipc::send(&mut worker, &serde_json::json!(result)).is_err() { break; }
            }
        }).map_err(|error| error.to_string())?;
        Ok(Self(channel))
    }
}
impl Drop for Bridge {
    fn drop(&mut self) {
        // Interrupt the channel immediately. An already dispatched hardware call
        // is bounded, serialized with logout, and checked against its session ID.
        let _ = self.0.shutdown(Shutdown::Both);
    }
}

#[derive(Clone)]
struct Endpoint(Arc<State>);
struct State {
    channel: Mutex<UnixStream>,
    operations: Mutex<()>,
    store: SettingsStore,
    sound: SoundService,
}
impl State {
    fn request(&self, operation: &str, configuration: String, pixels: Option<OwnedFd>) -> Result<String, String> {
        let mut channel = self.channel.lock().unwrap_or_else(|error| error.into_inner());
        let result = (|| -> Result<Result<String, String>, String> {
            ipc::send(&mut *channel, &serde_json::json!((operation, configuration, pixels.is_some()))).map_err(|error| error.to_string())?;
            if let Some(fd) = pixels { ipc::send_fd(&channel, &fd).map_err(|error| error.to_string())?; }
            let message = ipc::receive(&mut *channel).map_err(|error| error.to_string())?;
            serde_json::from_value::<Result<String, String>>(message).map_err(|error| error.to_string())
        })();
        if result.is_err() {
            // Partial replies cannot be reused; the helper's lifetime grants this capability.
            let _ = channel.shutdown(Shutdown::Both);
        }
        result.and_then(|reply| reply)
    }
    fn apply(&self, desired: PersonalizationSettings) -> Result<(), String> {
        let _operation = self.operations.try_lock().map_err(|_| "Another wallpaper operation is in progress")?;
        let prepared = self.store.load_background_validated(&desired, BACKGROUND_IMAGE)
            .and_then(|(image, _)| match image {
                Some(image) => Ok((image.extent.width, image.extent.height, Some(session_pixels::seal(&image.pixels)?))),
                None => Ok((0, 0, None)),
            });
        match prepared {
            Ok((width, height, pixels)) => {
                let configuration = serde_json::to_string(&(&desired, width, height, None::<String>)).map_err(|error| error.to_string())?;
                self.request("background", configuration, pixels).map(|_| ())
            }
            Err(error) => {
                let message: String = error.chars().take(1024).collect();
                let configuration = serde_json::to_string(&(PersonalizationSettings::default(), 0, 0, Some(message))).map_err(|error| error.to_string())?;
                let _ = self.request("background", configuration, None);
                Err(error)
            }
        }
    }
}
impl Endpoint {
    async fn request(&self, operation: &'static str, configuration: String) -> zbus::fdo::Result<String> {
        let state = self.0.clone();
        blocking::unblock(move || state.request(operation, configuration, None).map_err(zbus::fdo::Error::Failed)).await
    }
}
#[zbus::interface(name = "org.telorgon.TestShell.Settings1")]
impl Endpoint {
    async fn snapshot(&self) -> zbus::fdo::Result<String> {
        let state = self.0.clone();
        blocking::unblock(move || {
            let value = state.request("snapshot", String::new(), None).map_err(zbus::fdo::Error::Failed)?;
            let mut snapshot: ShellSnapshot = protocol::decode(&value).map_err(zbus::fdo::Error::Failed)?;
            state.sound.populate(&mut snapshot);
            protocol::encode_snapshot(&snapshot).map_err(zbus::fdo::Error::Failed)
        }).await
    }
    async fn preview_display(&self, configuration: String) -> zbus::fdo::Result<u64> {
        self.request("preview", configuration).await?.parse().map_err(|_| zbus::fdo::Error::Failed("Invalid preview token".into()))
    }
    async fn confirm_display(&self, token: u64) -> zbus::fdo::Result<()> {
        self.request("confirm", token.to_string()).await.map(|_| ())
    }
    async fn revert_display(&self, token: u64) -> zbus::fdo::Result<()> {
        self.request("revert", token.to_string()).await.map(|_| ())
    }
    async fn reload_settings(&self) -> zbus::fdo::Result<()> {
        let state = self.0.clone();
        blocking::unblock(move || {
            let preferences = state.store.load().map_err(zbus::fdo::Error::Failed)?;
            state.sound.reload(preferences.sound).map_err(zbus::fdo::Error::Failed)
        }).await
    }
    async fn apply_personalization(&self, configuration: String) -> zbus::fdo::Result<()> {
        let desired = protocol::decode(&configuration).map_err(zbus::fdo::Error::InvalidArgs)?;
        let state = self.0.clone();
        blocking::unblock(move || state.apply(desired).map_err(zbus::fdo::Error::Failed)).await
    }
    async fn delete_background(&self, configuration: String) -> zbus::fdo::Result<()> {
        let desired: PersonalizationSettings = protocol::decode(&configuration).map_err(zbus::fdo::Error::InvalidArgs)?;
        let state = self.0.clone();
        blocking::unblock(move || {
            let _operation = state.operations.try_lock().map_err(|_| zbus::fdo::Error::Failed("Another wallpaper operation is in progress".into()))?;
            let snapshot: ShellSnapshot = protocol::decode(&state.request("snapshot", String::new(), None).map_err(zbus::fdo::Error::Failed)?)
                .map_err(zbus::fdo::Error::Failed)?;
            if snapshot.personalization.is_none_or(|snapshot| snapshot.current == desired) {
                return Err(zbus::fdo::Error::Failed("Choose and save another background before deleting the current background".into()));
            }
            state.store.delete_background(&desired).map_err(zbus::fdo::Error::Failed)
        }).await
    }
}

pub(super) struct Service {
    _connection: zbus::blocking::Connection,
    state: Arc<State>,
}
impl Drop for Service {
    fn drop(&mut self) { self.state.sound.stop(); }
}
impl Service {
    pub fn restore_background(&self, desired: PersonalizationSettings) {
        if let Err(error) = self.state.apply(desired) {
            eprintln!("telorgon-user-session: saved wallpaper could not be applied: {error}");
        }
    }
}

pub(super) fn serve(channel: UnixStream, store: SettingsStore, sound: SoundService) -> Result<Service, String> {
    channel.set_read_timeout(Some(Duration::from_secs(15))).map_err(|error| error.to_string())?;
    channel.set_write_timeout(Some(Duration::from_secs(15))).map_err(|error| error.to_string())?;
    if let Err(error) = store.ensure_background_directory() {
        eprintln!("telorgon-user-session: background library could not be prepared: {error}");
    }
    let state = Arc::new(State { channel: Mutex::new(channel), operations: Mutex::new(()), store, sound });
    let endpoint = crate::settings_config::endpoint();
    let connection = zbus::blocking::connection::Builder::session()
        .and_then(|builder| builder.name(endpoint.bus_name()))
        .and_then(|builder| builder.serve_at(endpoint.object_path(), Endpoint(state.clone())))
        .and_then(|builder| builder.build()).map_err(|error| error.to_string())?;
    Ok(Service { _connection: connection, state })
}
