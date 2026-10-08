use telorgon::host::application::audio_mixer::MixerSnapshot;
use telorgon::integrations::pipewire::{ConnectionState, ObjectHandle};
use telorgon::services::audio::mixer::{
    self, ApplicationGroupId, MixerTarget, MuteState, StreamDirection,
};
use telorgon::services::audio::{AudioNode, DeviceChoice, Gain};
use telorgon::services::desktop_settings as shared;

fn object_id(handle: ObjectHandle) -> shared::AudioObjectId {
    shared::AudioObjectId {
        epoch: handle.epoch(),
        incarnation: handle.incarnation(),
        id: handle.id(),
    }
}

fn label(description: &str, name: &str) -> String {
    if description.is_empty() {
        name
    } else {
        description
    }
    .to_owned()
}

fn application_id(id: &ApplicationGroupId) -> shared::AudioApplicationId {
    match id {
        ApplicationGroupId::Application(id) => shared::AudioApplicationId::Application(id.clone()),
        ApplicationGroupId::Process(id) => shared::AudioApplicationId::Process(*id),
        ApplicationGroupId::Stream(id) => shared::AudioApplicationId::Stream(object_id(*id)),
    }
}

fn target(target: &MixerTarget) -> shared::AudioTarget {
    match target {
        MixerTarget::Node(id) => shared::AudioTarget::Node { id: object_id(*id) },
        MixerTarget::Application(id, direction) => shared::AudioTarget::Application {
            id: application_id(id),
            direction: match direction {
                StreamDirection::Playback => shared::AudioDirection::Output,
                StreamDirection::Recording => shared::AudioDirection::Input,
            },
        },
        MixerTarget::DefaultOutput => shared::AudioTarget::DefaultOutput,
        MixerTarget::DefaultInput => shared::AudioTarget::DefaultInput,
    }
}

fn mute(state: MuteState) -> shared::AudioMuteState {
    match state {
        MuteState::Muted => shared::AudioMuteState::Muted,
        MuteState::Unmuted => shared::AudioMuteState::Unmuted,
        MuteState::Mixed => shared::AudioMuteState::Mixed,
        MuteState::Unknown => shared::AudioMuteState::Unknown,
    }
}

fn choice(choice: &DeviceChoice) -> shared::AudioChoice {
    shared::AudioChoice {
        index: choice.index,
        name: choice.name.clone(),
        label: label(&choice.description, &choice.name),
        available: choice.available,
        priority: choice.priority,
        direction: match choice.direction {
            Some(0) => Some(shared::AudioDirection::Input),
            Some(1) => Some(shared::AudioDirection::Output),
            _ => None,
        },
        device: choice.device,
        devices: choice.devices.clone(),
        profiles: choice.profiles.clone(),
    }
}

// SPA channel positions are a stable protocol enum. Preserve unknown positions too.
fn channel_label(position: Option<u32>, index: usize) -> String {
    const LABELS: &[&str] = &[
        "Unknown",
        "Unassigned",
        "Mono",
        "Front left",
        "Front right",
        "Front center",
        "Subwoofer",
        "Side left",
        "Side right",
        "Front left center",
        "Front right center",
        "Rear center",
        "Rear left",
        "Rear right",
        "Top center",
        "Top front left",
        "Top front center",
        "Top front right",
        "Top rear left",
        "Top rear center",
        "Top rear right",
        "Rear left center",
        "Rear right center",
        "Front left wide",
        "Front right wide",
        "Subwoofer 2",
        "Front left high",
        "Front center high",
        "Front right high",
        "Top front left center",
        "Top front right center",
        "Top side left",
        "Top side right",
        "Left subwoofer",
        "Right subwoofer",
        "Bottom center",
        "Bottom left center",
        "Bottom right center",
    ];
    match position {
        Some(code) if (0x1000..0x1040).contains(&code) => format!("Aux {}", code - 0x1000 + 1),
        Some(code) => LABELS
            .get(code as usize)
            .map(|label| (*label).to_owned())
            .unwrap_or_else(|| format!("Channel {} (position {code})", index + 1)),
        None => format!("Channel {}", index + 1),
    }
}

fn channels(positions: &[u32], volumes: &[Gain]) -> Vec<shared::AudioChannelInfo> {
    (0..positions.len().max(volumes.len()))
        .map(|index| {
            let position = positions.get(index).copied();
            shared::AudioChannelInfo {
                index,
                position,
                label: channel_label(position, index),
                volume: volumes.get(index).map(|gain| gain.as_ui()),
                linear_gain: volumes.get(index).map(|gain| gain.value()),
            }
        })
        .collect()
}

fn balance(positions: &[u32], volumes: &[Gain]) -> Option<f32> {
    if positions != [3, 4] || volumes.len() != 2 {
        return None;
    }
    let left = volumes[0].value();
    let right = volumes[1].value();
    let peak = left.max(right);
    // Both channels at zero contain no information about their relative balance.
    (peak > 0.0).then(|| (right - left) / peak)
}

fn node(node: &AudioNode, mixer: &MixerSnapshot, ready: bool) -> shared::AudioNodeInfo {
    shared::AudioNodeInfo {
        id: object_id(node.handle),
        name: node.name.clone(),
        label: label(&node.description, &node.name),
        media_class: node.media_class.clone(),
        device: node.device.map(object_id),
        profile_device: node.profile_device,
        is_virtual: node.is_virtual,
        application_id: node.application_id.clone(),
        application_name: node.application_name.clone(),
        application_icon: node.application_icon.clone(),
        process_id: node.process_id,
        process_binary: node.process_binary.clone(),
        media_role: node.media_role.clone(),
        advertised_rate: node.advertised_rate,
        advertised_format: node.advertised_format.clone(),
        bluetooth_codec: node.bluetooth_codec.clone(),
        volume: mixer::node_volume(node),
        muted: node.mute,
        channels: channels(&node.channel_map, &node.channel_volumes),
        balance: balance(&node.channel_map, &node.channel_volumes),
        can_set_volume: ready && node.can_set_volume,
        can_set_mute: ready && node.can_set_volume && node.mute.is_some(),
        can_set_balance: ready
            && node.can_set_volume
            && node.channel_map == [3, 4]
            && node.channel_volumes.len() == 2,
        can_set_channel_volumes: ready && node.can_set_volume && !node.channel_volumes.is_empty(),
        destinations: mixer
            .destinations
            .get(&node.handle)
            .into_iter()
            .flatten()
            .copied()
            .map(object_id)
            .collect(),
    }
}

pub(super) fn snapshot(mixer: &MixerSnapshot) -> shared::AudioSnapshot {
    let ready = mixer.state == ConnectionState::Ready;
    shared::AudioSnapshot {
        generation: mixer.generation,
        state: match &mixer.state {
            ConnectionState::Connecting => shared::AudioConnectionState::Connecting,
            ConnectionState::Ready => shared::AudioConnectionState::Ready,
            ConnectionState::Failed(_) => shared::AudioConnectionState::Failed,
            ConnectionState::Stopping => shared::AudioConnectionState::Stopping,
            ConnectionState::Stopped => shared::AudioConnectionState::Stopped,
        },
        error: if let ConnectionState::Failed(error) = &mixer.state {
            Some(error.to_string())
        } else {
            None
        },
        default_output: mixer.default_output.map(object_id),
        default_input: mixer.default_input.map(object_id),
        nodes: mixer
            .nodes
            .iter()
            .map(|value| node(value, mixer, ready))
            .collect(),
        hardware_devices: mixer
            .devices
            .iter()
            .map(|device| shared::AudioHardwareDevice {
                id: object_id(device.handle),
                name: device.name.clone(),
                label: label(&device.description, &device.name),
                profiles: device.profiles.iter().map(choice).collect(),
                routes: device.routes.iter().map(choice).collect(),
                active_profile: device.active_profile,
                active_routes: device
                    .active_routes
                    .iter()
                    .map(|route| shared::AudioActiveRoute {
                        index: route.index,
                        device: route.device,
                    })
                    .collect(),
                can_set_profile: ready && device.can_set_profile,
                can_set_route: ready && device.can_set_route,
            })
            .collect(),
        applications: mixer
            .applications
            .iter()
            .map(|app| shared::AudioApplication {
                id: application_id(&app.id),
                name: app.name.clone(),
                icon_name: app.icon_name.clone(),
                playback: app
                    .playback
                    .iter()
                    .map(|node| object_id(node.handle))
                    .collect(),
                recording: app
                    .recording
                    .iter()
                    .map(|node| object_id(node.handle))
                    .collect(),
                playback_volume: mixer::group_volume(&app.playback),
                recording_volume: mixer::group_volume(&app.recording),
                playback_mute: mute(mixer::group_mute(&app.playback)),
                recording_mute: mute(mixer::group_mute(&app.recording)),
            })
            .collect(),
        operations: mixer
            .operations
            .iter()
            .map(|(value, operation)| shared::AudioOperation {
                target: target(value),
                pending: operation.pending,
                preview_volume: operation.preview_volume,
                error: operation.error.as_ref().map(ToString::to_string),
                applied: operation.applied,
                failed: operation.failed,
            })
            .collect(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn channels_preserve_unknowns_and_volume_units() {
        let gains = [Gain::linear(0.125).unwrap(), Gain::UNITY];
        let result = channels(&[3, 4, 0x1000, 9000], &gains);
        assert_eq!(result[0].label, "Front left");
        assert_eq!(result[0].volume, Some(0.5));
        assert_eq!(result[0].linear_gain, Some(0.125));
        assert_eq!(result[2].label, "Aux 1");
        assert_eq!(result[2].volume, None);
        assert_eq!(result[3].position, Some(9000));
        assert_eq!(channels(&[], &gains)[0].position, None);
        assert_eq!(balance(&[3, 4], &gains), Some(0.875));
        assert_eq!(balance(&[3, 4], &[Gain::SILENCE; 2]), None);
        assert_eq!(balance(&[4, 3], &gains), None);
    }
    #[test]
    fn choices_preserve_unavailable_and_unknown_states() {
        let mut source = DeviceChoice {
            index: 7,
            name: "headphones".into(),
            description: String::new(),
            available: None,
            priority: Some(100),
            direction: Some(1),
            device: Some(2),
            devices: vec![2],
            profiles: vec![1, 3],
        };
        let mapped = choice(&source);
        assert_eq!(mapped.label, "headphones");
        assert_eq!(mapped.available, None);
        assert_eq!(mapped.direction, Some(shared::AudioDirection::Output));
        assert_eq!(mapped.profiles, vec![1, 3]);
        source.available = Some(false);
        assert_eq!(choice(&source).available, Some(false));
    }
}
