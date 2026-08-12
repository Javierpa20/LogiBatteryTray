use crate::hid::client::{DeviceEvent, WorkerCommand};
use crate::model::{BatteryState, PollResult, Transport};
use std::collections::BTreeSet;
use std::sync::mpsc;
use std::thread;
use std::time::Duration;

const MIN_POLL_SECS: u64 = 60;

pub fn spawn_worker<F>(
    poll_secs: u64,
    cmd_rx: mpsc::Receiver<WorkerCommand>,
    emit: F,
) -> thread::JoinHandle<()>
where
    F: Fn(DeviceEvent) + Send + 'static,
{
    thread::spawn(move || {
        let mut poll_secs = poll_secs.max(MIN_POLL_SECS);
        let mut known = BTreeSet::new();

        loop {
            let (snapshot, complete) = poll_for_worker();
            publish_snapshot(snapshot, complete, &mut known, &emit);
            match cmd_rx.recv_timeout(Duration::from_secs(poll_secs)) {
                Ok(WorkerCommand::Refresh) => {}
                Ok(WorkerCommand::SetSafetyInterval(secs)) => {
                    poll_secs = secs.max(MIN_POLL_SECS);
                }
                Ok(WorkerCommand::Exit) | Err(mpsc::RecvTimeoutError::Disconnected) => return,
                Err(mpsc::RecvTimeoutError::Timeout) => {}
            }
        }
    })
}

fn publish_snapshot<F>(result: PollResult, complete: bool, known: &mut BTreeSet<String>, emit: &F)
where
    F: Fn(DeviceEvent),
{
    let had_errors = !result.errors.is_empty();
    for error in result.errors {
        tracing::warn!("Bluetooth battery scan: {error}");
    }

    // A radio/service failure is not evidence that every known device went
    // away. Preserve the last readings until a successful empty snapshot says
    // otherwise.
    if !complete {
        return;
    }

    let current: BTreeSet<_> = result
        .devices
        .iter()
        .map(|device| device.device_key.clone())
        .collect();
    if !had_errors {
        for gone in known.difference(&current) {
            emit(DeviceEvent::Gone(gone.clone()));
        }
    }
    for device in result.devices {
        emit(DeviceEvent::Update(device));
    }
    if had_errors {
        known.extend(current);
    } else {
        *known = current;
    }
}

fn looks_like_logitech_keyboard_or_mouse(name: &str) -> bool {
    let name = name.to_lowercase();
    [
        "logitech",
        "logi ",
        "mx keys",
        "mx master",
        "mx anywhere",
        "ergo k",
        "ergo m",
        "pebble keys",
        "pebble mouse",
        "pop keys",
        "pop mouse",
        "signature k",
        "signature m",
    ]
    .iter()
    .any(|marker| name.contains(marker))
}

fn is_keyboard_or_mouse_appearance(category: u16, subcategory: u16) -> bool {
    // Bluetooth SIG assigned numbers: Human Interface Device category 0x0F;
    // Keyboard 0x01 and Mouse 0x02 subcategories.
    category == 0x0F && matches!(subcategory, 0x01 | 0x02)
}

#[cfg(target_os = "windows")]
pub fn poll_once() -> PollResult {
    poll_for_worker().0
}

#[cfg(target_os = "windows")]
fn poll_for_worker() -> (PollResult, bool) {
    match poll_windows() {
        Ok(mut result) => {
            result.sort_devices();
            (result, true)
        }
        Err(err) => (
            PollResult {
                devices: Vec::new(),
                errors: vec![err.to_string()],
            },
            false,
        ),
    }
}

#[cfg(not(target_os = "windows"))]
pub fn poll_once() -> PollResult {
    PollResult::default()
}

#[cfg(not(target_os = "windows"))]
fn poll_for_worker() -> (PollResult, bool) {
    (PollResult::default(), true)
}

#[cfg(target_os = "windows")]
fn poll_windows() -> anyhow::Result<PollResult> {
    use anyhow::{bail, Context};
    use windows::Devices::Bluetooth::BluetoothLEDevice;
    use windows::Devices::Bluetooth::BluetoothUuidHelper;
    use windows::Devices::Bluetooth::GenericAttributeProfile::{
        GattCommunicationStatus, GattDeviceService,
    };
    use windows::Devices::Enumeration::DeviceInformation;
    use windows::Storage::Streams::DataReader;
    use windows::Win32::System::WinRT::{RoInitialize, RoUninitialize, RO_INIT_MULTITHREADED};

    struct WinRtGuard;
    impl Drop for WinRtGuard {
        fn drop(&mut self) {
            unsafe { RoUninitialize() };
        }
    }

    unsafe { RoInitialize(RO_INIT_MULTITHREADED) }.context("failed to initialize WinRT")?;
    let _guard = WinRtGuard;

    let battery_service = BluetoothUuidHelper::FromShortId(0x180F)?;
    let battery_level = BluetoothUuidHelper::FromShortId(0x2A19)?;
    let selector = GattDeviceService::GetDeviceSelectorFromUuid(battery_service)?;
    let services = DeviceInformation::FindAllAsyncAqsFilter(&selector)?.get()?;
    let mut result = PollResult::default();

    for index in 0..services.Size()? {
        let info = services.GetAt(index)?;
        let id = info.Id()?;
        let key = format!("BT:{}", id.to_string());

        let reading = (|| -> anyhow::Result<Option<BatteryState>> {
            let service = GattDeviceService::FromIdAsync(&id)?.get()?;
            let chars = service
                .GetCharacteristicsForUuidAsync(battery_level)?
                .get()?;
            if chars.Status()? != GattCommunicationStatus::Success {
                bail!("Battery Level characteristic is unavailable");
            }
            let chars = chars.Characteristics()?;
            if chars.Size()? == 0 {
                bail!("Battery Level characteristic was not found");
            }

            let read = chars.GetAt(0)?.ReadValueAsync()?.get()?;
            if read.Status()? != GattCommunicationStatus::Success {
                bail!("Battery Level read failed with status {:?}", read.Status()?);
            }
            let buffer = read.Value()?;
            if buffer.Length()? == 0 {
                bail!("Battery Level returned an empty value");
            }
            let percent = DataReader::FromBuffer(&buffer)?.ReadByte()?;
            if percent > 100 {
                bail!("Battery Level returned invalid value {percent}");
            }

            let bluetooth_device = service
                .DeviceId()
                .ok()
                .and_then(|device_id| BluetoothLEDevice::FromIdAsync(&device_id).ok())
                .and_then(|operation| operation.get().ok());
            let display_name = bluetooth_device
                .as_ref()
                .and_then(|device| device.Name().ok())
                .map(|name| name.to_string())
                .filter(|name| !name.trim().is_empty())
                .or_else(|| info.Name().ok().map(|name| name.to_string()))
                .filter(|name| !name.trim().is_empty())
                .unwrap_or_else(|| "Bluetooth device".to_string());

            let appearance_matches = bluetooth_device
                .as_ref()
                .and_then(|device| device.Appearance().ok())
                .and_then(|appearance| {
                    Some((appearance.Category().ok()?, appearance.SubCategory().ok()?))
                })
                .is_some_and(|(category, subcategory)| {
                    is_keyboard_or_mouse_appearance(category, subcategory)
                });
            if !appearance_matches && !looks_like_logitech_keyboard_or_mouse(&display_name) {
                return Ok(None);
            }

            Ok(Some(BatteryState {
                device_key: key.clone(),
                display_name,
                pid: 0,
                device_index: 0,
                battery_percent: percent,
                is_charging: false,
                transport: Transport::Bluetooth,
            }))
        })();

        match reading {
            Ok(Some(device)) => result.devices.push(device),
            Ok(None) => {}
            Err(err) => result.errors.push(format!("{key}: {err:#}")),
        }
    }

    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::{
        is_keyboard_or_mouse_appearance, looks_like_logitech_keyboard_or_mouse, publish_snapshot,
    };
    use crate::hid::client::DeviceEvent;
    use crate::model::{BatteryState, PollResult, Transport};
    use std::cell::RefCell;
    use std::collections::BTreeSet;

    fn state(key: &str) -> BatteryState {
        BatteryState {
            device_key: key.to_string(),
            display_name: "MX Keys".to_string(),
            pid: 0,
            device_index: 0,
            battery_percent: 75,
            is_charging: false,
            transport: Transport::Bluetooth,
        }
    }

    #[test]
    fn missing_bluetooth_device_is_published_as_gone() {
        let mut known = BTreeSet::from(["BT:old".to_string()]);
        let events = RefCell::new(Vec::new());
        publish_snapshot(
            PollResult {
                devices: vec![state("BT:new")],
                errors: Vec::new(),
            },
            true,
            &mut known,
            &|event| events.borrow_mut().push(event),
        );
        assert!(matches!(&events.borrow()[0], DeviceEvent::Gone(key) if key == "BT:old"));
        assert!(
            matches!(&events.borrow()[1], DeviceEvent::Update(state) if state.device_key == "BT:new")
        );
    }

    #[test]
    fn total_scan_failure_preserves_last_known_reading() {
        let mut known = BTreeSet::from(["BT:old".to_string()]);
        let events = RefCell::new(Vec::new());
        publish_snapshot(
            PollResult {
                devices: Vec::new(),
                errors: vec!["radio unavailable".to_string()],
            },
            false,
            &mut known,
            &|event| events.borrow_mut().push(event),
        );
        assert!(events.borrow().is_empty());
        assert!(known.contains("BT:old"));
    }

    #[test]
    fn partial_read_failure_does_not_mark_an_unread_device_gone() {
        let mut known = BTreeSet::from(["BT:old".to_string()]);
        let events = RefCell::new(Vec::new());
        publish_snapshot(
            PollResult {
                devices: vec![state("BT:new")],
                errors: vec!["BT:old: temporary read failure".to_string()],
            },
            true,
            &mut known,
            &|event| events.borrow_mut().push(event),
        );
        assert_eq!(events.borrow().len(), 1);
        assert!(
            matches!(&events.borrow()[0], DeviceEvent::Update(state) if state.device_key == "BT:new")
        );
        assert!(known.contains("BT:old"));
        assert!(known.contains("BT:new"));
    }

    #[test]
    fn bluetooth_filter_accepts_keyboards_and_mice_but_not_unrelated_devices() {
        assert!(is_keyboard_or_mouse_appearance(0x0F, 0x01));
        assert!(is_keyboard_or_mouse_appearance(0x0F, 0x02));
        assert!(!is_keyboard_or_mouse_appearance(0x0F, 0x03));
        assert!(looks_like_logitech_keyboard_or_mouse("MX Master 3S"));
        assert!(looks_like_logitech_keyboard_or_mouse("Logitech K380"));
        assert!(!looks_like_logitech_keyboard_or_mouse("Xiaozhu"));
    }
}
