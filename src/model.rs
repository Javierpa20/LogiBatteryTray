#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum Transport {
    Receiver,
    Bluetooth,
}

impl Transport {
    pub fn label(self) -> &'static str {
        match self {
            Self::Receiver => "Receiver",
            Self::Bluetooth => "Bluetooth",
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BatteryState {
    pub device_key: String,
    pub display_name: String,
    pub pid: u16,
    pub device_index: u8,
    pub battery_percent: u8,
    pub is_charging: bool,
    pub transport: Transport,
}

pub fn normalized_device_name(name: &str) -> String {
    let normalized = name
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .to_lowercase();

    let mut name = normalized.as_str();
    for suffix in [
        " (bluetooth)",
        " bluetooth",
        " wireless keyboard",
        " wireless mouse",
    ] {
        if let Some(stripped) = name.strip_suffix(suffix) {
            name = stripped.trim();
            break;
        }
    }
    name.to_string()
}

/// Collapse the same physical device reported by multiple transports. Live
/// snapshots are all online, so the receiver wins ties because HID++ supplies
/// event-driven updates while Bluetooth is periodically polled.
pub fn deduplicate_devices(devices: Vec<BatteryState>) -> Vec<BatteryState> {
    use std::collections::BTreeMap;

    let mut by_name: BTreeMap<String, BatteryState> = BTreeMap::new();
    for device in devices {
        let name = normalized_device_name(&device.display_name);
        match by_name.get(&name) {
            Some(existing) if existing.transport <= device.transport => {}
            _ => {
                by_name.insert(name, device);
            }
        }
    }
    let mut devices: Vec<_> = by_name.into_values().collect();
    devices.sort_by(|a, b| a.display_name.cmp(&b.display_name));
    devices
}

#[cfg(test)]
mod tests {
    use super::{deduplicate_devices, normalized_device_name, BatteryState, Transport};

    fn state(key: &str, name: &str, transport: Transport, percent: u8) -> BatteryState {
        BatteryState {
            device_key: key.to_string(),
            display_name: name.to_string(),
            pid: 0,
            device_index: 0,
            battery_percent: percent,
            is_charging: false,
            transport,
        }
    }

    #[test]
    fn normalizes_case_whitespace_and_bluetooth_suffix() {
        assert_eq!(
            normalized_device_name("  MX   Master 3S (Bluetooth) "),
            "mx master 3s"
        );
        assert_eq!(
            normalized_device_name("MX Keys Wireless Keyboard"),
            "mx keys"
        );
    }

    #[test]
    fn receiver_wins_a_same_name_online_tie() {
        let devices = deduplicate_devices(vec![
            state("BT:1", "MX Master 3S Bluetooth", Transport::Bluetooth, 79),
            state("C548:1", "MX Master 3S", Transport::Receiver, 80),
        ]);
        assert_eq!(devices.len(), 1);
        assert_eq!(devices[0].transport, Transport::Receiver);
    }

    #[test]
    fn distinct_devices_are_preserved() {
        let devices = deduplicate_devices(vec![
            state("BT:1", "MX Keys", Transport::Bluetooth, 50),
            state("BT:2", "MX Master 3S", Transport::Bluetooth, 90),
        ]);
        assert_eq!(devices.len(), 2);
    }
}

#[derive(Clone, Debug, Default)]
pub struct PollResult {
    pub devices: Vec<BatteryState>,
    pub errors: Vec<String>,
}

impl PollResult {
    pub fn sort_devices(&mut self) {
        self.devices.sort_by(|a, b| {
            a.display_name
                .cmp(&b.display_name)
                .then_with(|| a.pid.cmp(&b.pid))
                .then_with(|| a.device_key.cmp(&b.device_key))
        });
    }
}
