use crate::autostart;
use crate::config::{self, AppConfig};
use crate::hid::client::{self, DeviceEvent, WorkerCommand};
use crate::hid::scanner::scan_receivers;
use crate::icon;
use crate::model::{normalized_device_name, BatteryState, Transport};
use crate::notify::Notifier;
use crate::PRODUCT_NAME;
use anyhow::{Context, Result};
use hidapi::HidApi;
use std::collections::{BTreeMap, HashMap};
use std::sync::mpsc;
use std::thread;
use std::time::Duration;
use tao::event::Event;
use tao::event_loop::{ControlFlow, EventLoopBuilder, EventLoopProxy};
use tray_icon::menu::{CheckMenuItem, Menu, MenuEvent, MenuItem, PredefinedMenuItem, Submenu};
use tray_icon::{Icon, TrayIcon, TrayIconBuilder};

#[derive(Debug, Clone)]
enum UserEvent {
    Menu(String),
    /// An incremental device update or departure from a receiver worker.
    Device(DeviceEvent),
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct DeviceTrayProjection {
    device_key: String,
    display_name: String,
    battery_percent: u8,
    is_charging: bool,
    online: bool,
    transport: Transport,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct TraySummary {
    battery_percent: u8,
    is_charging: bool,
    online: bool,
    tooltip: String,
}

impl DeviceTrayProjection {
    fn online(state: &BatteryState) -> Self {
        Self {
            device_key: state.device_key.clone(),
            display_name: state.display_name.clone(),
            battery_percent: state.battery_percent,
            is_charging: state.is_charging,
            online: true,
            transport: state.transport,
        }
    }
}

#[cfg(test)]
fn project_device_trays(devices: &BTreeMap<String, BatteryState>) -> Vec<DeviceTrayProjection> {
    let mut projected: Vec<_> = devices.values().map(DeviceTrayProjection::online).collect();
    sort_device_trays(&mut projected);
    projected
}

fn apply_device_event(devices: &mut BTreeMap<String, DeviceTrayProjection>, event: DeviceEvent) {
    match event {
        DeviceEvent::Update(state) => {
            devices.insert(
                state.device_key.clone(),
                DeviceTrayProjection::online(&state),
            );
        }
        DeviceEvent::Gone(key) => {
            if let Some(device) = devices.get_mut(&key) {
                device.online = false;
                device.is_charging = false;
            }
        }
    }
}

fn sort_device_trays(devices: &mut [DeviceTrayProjection]) {
    devices.sort_by(|a, b| {
        a.display_name
            .cmp(&b.display_name)
            .then(a.device_key.cmp(&b.device_key))
    });
}

fn deduplicate_device_trays(devices: Vec<DeviceTrayProjection>) -> Vec<DeviceTrayProjection> {
    let mut grouped: BTreeMap<String, DeviceTrayProjection> = BTreeMap::new();
    for device in devices {
        let name = normalized_device_name(&device.display_name);
        let replace = match grouped.get(&name) {
            None => true,
            Some(existing) => {
                (device.online && !existing.online)
                    || (device.online == existing.online && device.transport < existing.transport)
            }
        };
        if replace {
            grouped.insert(name, device);
        }
    }
    let mut devices: Vec<_> = grouped.into_values().collect();
    sort_device_trays(&mut devices);
    devices
}

fn summarize_devices(devices: &[DeviceTrayProjection]) -> Option<TraySummary> {
    let lowest = devices.iter().min_by_key(|device| device.battery_percent)?;
    let tooltip = devices
        .iter()
        .map(device_tooltip)
        .collect::<Vec<_>>()
        .join("\n");
    Some(TraySummary {
        battery_percent: lowest.battery_percent,
        is_charging: lowest.is_charging,
        online: lowest.online,
        tooltip,
    })
}

/// Preset choices for the menu submenus. The numeric value is encoded into each
/// item's id (e.g. "poll:60") so the event handler can parse it back.
const POLL_PRESETS: &[(&str, u64)] = &[
    ("15 秒", 15),
    ("30 秒", 30),
    ("1 分钟", 60),
    ("2 分钟", 120),
    ("3 分钟", 180),
    ("5 分钟", 300),
    ("15 分钟", 900),
];
const THRESHOLD_PRESETS: &[u8] = &[5, 10, 15, 20, 25, 30];
const COOLDOWN_PRESETS: &[(&str, u64)] = &[
    ("30 分钟", 30),
    ("1 小时", 60),
    ("2 小时", 120),
    ("4 小时", 240),
    ("8 小时", 480),
];

struct MenuHandles {
    root: Menu,
    status_item: MenuItem,
    devices_submenu: Submenu,
    refresh_item: MenuItem,
    view_mode_item: CheckMenuItem,
    notify_item: CheckMenuItem,
    autostart_item: CheckMenuItem,
    open_config_item: MenuItem,
    exit_item: MenuItem,
    device_items: Vec<MenuItem>,
    poll_items: Vec<CheckMenuItem>,
    threshold_items: Vec<CheckMenuItem>,
    cooldown_items: Vec<CheckMenuItem>,
}

impl MenuHandles {
    fn build(cfg: &AppConfig, initial_autostart: bool, initial_text_mode: bool) -> Result<Self> {
        let root = Menu::new();
        let status_item = MenuItem::new("正在查找 Logitech 设备…", false, None);
        let devices_submenu = Submenu::new("设备电量", true);
        let refresh_item = MenuItem::with_id("refresh", "立即刷新", true, None);
        let view_mode_item = CheckMenuItem::with_id(
            "viewmode",
            "图标直接显示百分比",
            true,
            initial_text_mode,
            None,
        );

        let (poll_submenu, poll_items) = build_preset_submenu(
            "兜底刷新间隔",
            "poll",
            POLL_PRESETS
                .iter()
                .map(|&(label, value)| (label.to_string(), value)),
            cfg.poll_interval_seconds,
        )?;
        let notify_item = CheckMenuItem::with_id(
            "notify",
            "启用低电量提醒",
            true,
            cfg.notifications_enabled,
            None,
        );
        let (threshold_submenu, threshold_items) = build_preset_submenu(
            "低电量提醒阈值",
            "threshold",
            THRESHOLD_PRESETS
                .iter()
                .map(|&n| (format!("{n}%"), u64::from(n))),
            u64::from(cfg.low_battery_threshold),
        )?;
        let (cooldown_submenu, cooldown_items) = build_preset_submenu(
            "重复提醒间隔",
            "cooldown",
            COOLDOWN_PRESETS
                .iter()
                .map(|&(label, value)| (label.to_string(), value)),
            cfg.low_battery_cooldown_minutes,
        )?;

        let autostart_item =
            CheckMenuItem::with_id("autostart", "登录时启动", true, initial_autostart, None);
        let open_config_item = MenuItem::with_id("openconfig", "打开配置文件…", true, None);
        let exit_item = MenuItem::with_id("exit", "退出", true, None);

        root.append_items(&[
            &status_item,
            &devices_submenu,
            &refresh_item,
            &PredefinedMenuItem::separator(),
            &view_mode_item,
            &poll_submenu,
            &notify_item,
            &threshold_submenu,
            &cooldown_submenu,
            &autostart_item,
            &PredefinedMenuItem::separator(),
            &open_config_item,
            &PredefinedMenuItem::separator(),
            &exit_item,
        ])?;

        Ok(Self {
            root,
            status_item,
            devices_submenu,
            refresh_item,
            view_mode_item,
            notify_item,
            autostart_item,
            open_config_item,
            exit_item,
            device_items: Vec::new(),
            poll_items,
            threshold_items,
            cooldown_items,
        })
    }

    fn rebuild_device_menu(&mut self, devices: &[DeviceTrayProjection]) -> Result<()> {
        for item in self.devices_submenu.items() {
            remove_item(&self.devices_submenu, &item)?;
        }
        self.device_items.clear();

        if devices.is_empty() {
            let empty = MenuItem::new("未发现设备", false, None);
            self.devices_submenu.append(&empty)?;
            return Ok(());
        }

        for device in devices {
            let label = format!(
                "{} — {}%{}{}",
                device.display_name,
                device.battery_percent,
                if device.is_charging {
                    "（充电中）"
                } else {
                    ""
                },
                if device.online {
                    ""
                } else {
                    "（休眠/未连接，最后读数）"
                }
            );
            let item = MenuItem::new(label, false, None);
            self.devices_submenu.append(&item)?;
            self.device_items.push(item);
        }

        Ok(())
    }

    fn touch_ids(&self) {
        let _ = self.refresh_item.id();
        let _ = self.exit_item.id();
        let _ = self.open_config_item.id();
    }
}

/// Build a submenu of radio-style preset choices. Each item's id is
/// `"{prefix}:{value}"`; the item whose value equals `current` starts checked.
fn build_preset_submenu(
    title: &str,
    prefix: &str,
    presets: impl Iterator<Item = (String, u64)>,
    current: u64,
) -> Result<(Submenu, Vec<CheckMenuItem>)> {
    let submenu = Submenu::new(title, true);
    let mut items = Vec::new();
    for (label, value) in presets {
        let item = CheckMenuItem::with_id(
            format!("{prefix}:{value}"),
            label,
            true,
            value == current,
            None,
        );
        submenu.append(&item)?;
        items.push(item);
    }
    Ok((submenu, items))
}

/// Re-sync a preset submenu's checkmarks so exactly the item matching `value`
/// is checked. muda auto-toggles the clicked item, so without this, clicking the
/// already-active preset would leave it unchecked.
fn set_preset(items: &[CheckMenuItem], prefix: &str, value: u64) {
    let target = format!("{prefix}:{value}");
    for item in items {
        item.set_checked(item.id().0 == target);
    }
}

pub fn run_tray_app(mut cfg: AppConfig) -> Result<()> {
    let exe_path = std::env::current_exe().context("failed resolving executable path")?;
    if let Err(err) = autostart::set_enabled(&exe_path, cfg.autostart) {
        tracing::warn!("failed to apply autostart setting: {err}");
    }

    let autostart_enabled = autostart::is_enabled().unwrap_or(cfg.autostart);

    let event_loop = EventLoopBuilder::<UserEvent>::with_user_event().build();
    let proxy = event_loop.create_proxy();

    MenuEvent::set_event_handler(Some({
        let proxy = proxy.clone();
        move |event: MenuEvent| {
            let _ = proxy.send_event(UserEvent::Menu(event.id.0.clone()));
        }
    }));

    // Commands are sent to the supervisor, which fans them out to the per-
    // receiver workers (and spawns workers for receivers as they appear).
    let (cmd_tx, cmd_rx) = mpsc::channel::<WorkerCommand>();
    spawn_supervisor(proxy.clone(), cmd_rx, cfg.poll_interval_seconds);

    let mut text_mode = cfg.text_mode();
    let mut menu = MenuHandles::build(&cfg, autostart_enabled, text_mode)?;
    menu.touch_ids();

    let initial_icon = icon::neutral_icon()?;
    let mut summary_tray = build_tray_icon(
        &menu.root,
        initial_icon,
        &format!("{PRODUCT_NAME} — 正在查找设备…"),
    )?;

    let mut notifier = Notifier::new(
        cfg.notifications_enabled,
        cfg.low_battery_threshold,
        cfg.low_battery_cooldown_minutes,
    );
    // Keep the last valid reading when a device sleeps or disconnects. The one
    // summary tray shows the lowest known battery; if that reading is stale the
    // icon turns gray, while the tooltip and menu retain every device's status.
    let mut device_map: BTreeMap<String, DeviceTrayProjection> = BTreeMap::new();
    let mut devices: Vec<DeviceTrayProjection> = Vec::new();

    event_loop.run(move |event, _target, control_flow| {
        *control_flow = ControlFlow::Wait;

        if let Event::UserEvent(user_event) = event {
            match user_event {
                UserEvent::Menu(id) => {
                    if id == "refresh" {
                        let _ = cmd_tx.send(WorkerCommand::Refresh);
                    } else if id == "exit" {
                        let _ = cmd_tx.send(WorkerCommand::Exit);
                        *control_flow = ControlFlow::Exit;
                    } else if id == "autostart" {
                        // muda auto-toggles the check state before firing this
                        // event, so is_checked() already holds the new value.
                        let enabled = menu.autostart_item.is_checked();
                        if let Err(err) = autostart::set_enabled(&exe_path, enabled) {
                            tracing::warn!("failed to set autostart: {err}");
                        }
                        cfg.autostart = enabled;
                        if let Err(err) = config::save_config(&cfg) {
                            tracing::warn!("failed saving config: {err}");
                        }
                    } else if id == "viewmode" {
                        // muda already toggled the check mark; read it directly.
                        text_mode = menu.view_mode_item.is_checked();
                        cfg.view_mode = if text_mode { "text" } else { "icon" }.to_string();
                        if let Err(err) = config::save_config(&cfg) {
                            tracing::warn!("failed saving config: {err}");
                        }
                        if let Err(err) = sync_summary_tray(&mut summary_tray, &devices, text_mode)
                        {
                            tracing::warn!("failed updating trays: {err}");
                        }
                    } else if id == "notify" {
                        // muda already toggled the check mark; read it directly.
                        cfg.notifications_enabled = menu.notify_item.is_checked();
                        notifier.set_enabled(cfg.notifications_enabled);
                        if let Err(err) = config::save_config(&cfg) {
                            tracing::warn!("failed saving config: {err}");
                        }
                    } else if id == "openconfig" {
                        open_config_file();
                    } else if let Some(value) = id.strip_prefix("poll:") {
                        if let Ok(secs) = value.parse::<u64>() {
                            // Now controls the safety re-read interval: arrivals
                            // and battery changes are pushed, so this only bounds
                            // how often we re-read as a fallback.
                            cfg.poll_interval_seconds = secs;
                            let _ = cmd_tx.send(WorkerCommand::SetSafetyInterval(secs));
                            if let Err(err) = config::save_config(&cfg) {
                                tracing::warn!("failed saving config: {err}");
                            }
                            set_preset(&menu.poll_items, "poll", secs);
                        }
                    } else if let Some(value) = id.strip_prefix("threshold:") {
                        if let Ok(threshold) = value.parse::<u8>() {
                            cfg.low_battery_threshold = threshold;
                            notifier.set_threshold(threshold);
                            if let Err(err) = config::save_config(&cfg) {
                                tracing::warn!("failed saving config: {err}");
                            }
                            set_preset(&menu.threshold_items, "threshold", u64::from(threshold));
                        }
                    } else if let Some(value) = id.strip_prefix("cooldown:") {
                        if let Ok(minutes) = value.parse::<u64>() {
                            cfg.low_battery_cooldown_minutes = minutes;
                            notifier.set_cooldown(minutes);
                            if let Err(err) = config::save_config(&cfg) {
                                tracing::warn!("failed saving config: {err}");
                            }
                            set_preset(&menu.cooldown_items, "cooldown", minutes);
                        }
                    }
                }
                UserEvent::Device(event) => {
                    if let DeviceEvent::Update(ref state) = event {
                        notifier.maybe_notify_low_battery(state);
                    }
                    apply_device_event(&mut device_map, event);

                    devices = deduplicate_device_trays(device_map.values().cloned().collect());

                    if let Err(err) = menu.rebuild_device_menu(&devices) {
                        tracing::warn!("failed rebuilding menu: {err}");
                    }
                    update_summary(&menu.status_item, &devices);
                    if let Err(err) = sync_summary_tray(&mut summary_tray, &devices, text_mode) {
                        tracing::warn!("failed refreshing trays: {err}");
                    }
                }
            }
        }
    });
}

fn build_tray_icon(menu: &Menu, icon: Icon, tooltip: &str) -> Result<TrayIcon> {
    TrayIconBuilder::new()
        .with_menu(Box::new(menu.clone()))
        .with_tooltip(tooltip)
        .with_icon(icon)
        .build()
        .context("failed creating tray icon")
}

fn sync_summary_tray(
    tray: &mut TrayIcon,
    devices: &[DeviceTrayProjection],
    text_mode: bool,
) -> Result<()> {
    let Some(summary) = summarize_devices(devices) else {
        tray.set_icon(Some(icon::neutral_icon()?))?;
        tray.set_tooltip(Some(format!("{PRODUCT_NAME} — 未发现设备")))?;
        return Ok(());
    };

    let tray_icon = if text_mode {
        if summary.online {
            icon::text_icon(summary.battery_percent, summary.is_charging)?
        } else {
            icon::inactive_text_icon(summary.battery_percent)?
        }
    } else if summary.online {
        icon::battery_icon(summary.battery_percent, summary.is_charging)?
    } else {
        icon::inactive_battery_icon(summary.battery_percent)?
    };

    tray.set_icon(Some(tray_icon))?;
    tray.set_tooltip(Some(summary.tooltip))?;
    tracing::debug!(
        "updated summary tray for {} devices; showing {}%",
        devices.len(),
        summary.battery_percent
    );
    Ok(())
}

fn device_tooltip(device: &DeviceTrayProjection) -> String {
    format!(
        "{}：{}%{}{}",
        device.display_name,
        device.battery_percent,
        if device.is_charging {
            "（充电中）"
        } else {
            ""
        },
        if device.online {
            ""
        } else {
            "（休眠/未连接，最后读数）"
        }
    )
}

fn update_summary(status_item: &MenuItem, devices: &[DeviceTrayProjection]) {
    let online = devices.iter().filter(|d| d.online).count();
    status_item.set_text(format!("Logitech 设备：{online}/{} 在线", devices.len()));
}

/// How often the supervisor re-scans for newly attached receivers. Workers for
/// existing receivers keep running independently; this only governs detecting a
/// receiver that was just plugged in (rare), so it can be relaxed.
const RESCAN_SECS: u64 = 20;

/// Supervise the per-receiver workers: spawn one for each receiver as it appears,
/// prune workers whose receiver vanished, and fan out UI commands to all of them.
fn spawn_supervisor(
    proxy: EventLoopProxy<UserEvent>,
    cmd_rx: mpsc::Receiver<WorkerCommand>,
    safety_secs: u64,
) {
    thread::spawn(move || {
        // pid -> (command sender, join handle) for each live worker.
        let mut workers: HashMap<u16, (mpsc::Sender<WorkerCommand>, thread::JoinHandle<()>)> =
            HashMap::new();
        let mut safety_secs = safety_secs;
        let (bluetooth_tx, bluetooth_rx) = mpsc::channel::<WorkerCommand>();
        let bluetooth_proxy = proxy.clone();
        let _bluetooth_handle =
            crate::bluetooth::spawn_worker(safety_secs, bluetooth_rx, move |event| {
                let _ = bluetooth_proxy.send_event(UserEvent::Device(event));
            });

        loop {
            // Drop workers whose thread has exited (receiver unplugged), so a
            // re-plugged receiver gets a fresh worker below.
            workers.retain(|_, (_, handle)| !handle.is_finished());

            // Spawn workers for any receiver we're not already tracking.
            match HidApi::new() {
                Ok(api) => {
                    for receiver in scan_receivers(&api) {
                        if workers.contains_key(&receiver.pid) {
                            continue;
                        }
                        let (tx, rx) = mpsc::channel::<WorkerCommand>();
                        let proxy = proxy.clone();
                        let handle = client::spawn_receiver_worker(
                            receiver.clone(),
                            safety_secs,
                            rx,
                            move |event| {
                                let _ = proxy.send_event(UserEvent::Device(event));
                            },
                        );
                        workers.insert(receiver.pid, (tx, handle));
                    }
                }
                Err(err) => tracing::warn!("failed initializing hidapi for scan: {err}"),
            }

            match cmd_rx.recv_timeout(Duration::from_secs(RESCAN_SECS)) {
                Ok(WorkerCommand::Exit) => {
                    let _ = bluetooth_tx.send(WorkerCommand::Exit);
                    for (tx, _) in workers.values() {
                        let _ = tx.send(WorkerCommand::Exit);
                    }
                    return;
                }
                Ok(cmd) => {
                    if let WorkerCommand::SetSafetyInterval(secs) = cmd {
                        safety_secs = secs;
                    }
                    let _ = bluetooth_tx.send(cmd.clone());
                    for (tx, _) in workers.values() {
                        let _ = tx.send(cmd.clone());
                    }
                }
                Err(mpsc::RecvTimeoutError::Timeout) => {}
                Err(mpsc::RecvTimeoutError::Disconnected) => return,
            }
        }
    });
}

/// Open the config file in the user's default editor. The file always exists by
/// the time the tray runs (created by `load_or_create_config`).
fn open_config_file() {
    let path = config::config_path();
    #[cfg(target_os = "windows")]
    {
        // explorer hands the file to its associated editor and, unlike `cmd
        // /C start`, does so without flashing a console window.
        if let Err(err) = std::process::Command::new("explorer").arg(&path).spawn() {
            tracing::warn!("failed opening config file {}: {err}", path.display());
        }
    }
    #[cfg(not(target_os = "windows"))]
    {
        tracing::warn!(
            "opening the config file is only supported on Windows: {}",
            path.display()
        );
    }
}

fn remove_item(submenu: &Submenu, item: &tray_icon::menu::MenuItemKind) -> Result<()> {
    match item {
        tray_icon::menu::MenuItemKind::MenuItem(it) => submenu.remove(it)?,
        tray_icon::menu::MenuItemKind::Submenu(it) => submenu.remove(it)?,
        tray_icon::menu::MenuItemKind::Predefined(it) => submenu.remove(it)?,
        tray_icon::menu::MenuItemKind::Check(it) => submenu.remove(it)?,
        tray_icon::menu::MenuItemKind::Icon(it) => submenu.remove(it)?,
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{
        deduplicate_device_trays, device_tooltip, project_device_trays, summarize_devices,
        DeviceTrayProjection,
    };
    use crate::model::{BatteryState, Transport};
    use std::collections::BTreeMap;

    fn mk(id: &str) -> BatteryState {
        BatteryState {
            device_key: id.to_string(),
            display_name: "Mouse".to_string(),
            pid: 0xC52B,
            device_index: 1,
            battery_percent: 80,
            is_charging: false,
            transport: Transport::Receiver,
        }
    }

    #[test]
    fn two_devices_are_merged_into_one_lowest_battery_summary() {
        let mut keyboard = mk("keyboard");
        keyboard.display_name = "MX Keys Wireless Keyboard".to_string();
        keyboard.battery_percent = 50;
        let mut mouse = mk("mouse");
        mouse.display_name = "MX Master 3S".to_string();
        mouse.battery_percent = 95;
        let devices = BTreeMap::from([
            ("keyboard".to_string(), keyboard),
            ("mouse".to_string(), mouse),
        ]);

        let projected = project_device_trays(&devices);
        let summary = summarize_devices(&projected).unwrap();

        assert_eq!(projected.len(), 2);
        assert_eq!(summary.battery_percent, 50);
        assert!(summary.online);
        assert!(summary.tooltip.contains("MX Keys Wireless Keyboard：50%"));
        assert!(summary.tooltip.contains("MX Master 3S：95%"));
        assert_eq!(summary.tooltip.lines().count(), 2);
    }

    #[test]
    fn updating_one_device_does_not_replace_the_other() {
        let mut devices = BTreeMap::from([
            ("keyboard".to_string(), mk("keyboard")),
            ("mouse".to_string(), mk("mouse")),
        ]);
        devices.get_mut("mouse").unwrap().battery_percent = 95;

        let projected = project_device_trays(&devices);

        assert!(projected.contains(&DeviceTrayProjection {
            device_key: "keyboard".to_string(),
            display_name: "Mouse".to_string(),
            battery_percent: 80,
            is_charging: false,
            online: true,
            transport: Transport::Receiver,
        }));
        assert!(projected.contains(&DeviceTrayProjection {
            device_key: "mouse".to_string(),
            display_name: "Mouse".to_string(),
            battery_percent: 95,
            is_charging: false,
            online: true,
            transport: Transport::Receiver,
        }));
    }

    #[test]
    fn disconnected_device_marks_only_its_own_state_offline() {
        let mut devices = BTreeMap::from([
            (
                "keyboard".to_string(),
                DeviceTrayProjection::online(&mk("keyboard")),
            ),
            (
                "mouse".to_string(),
                DeviceTrayProjection::online(&mk("mouse")),
            ),
        ]);

        super::apply_device_event(
            &mut devices,
            crate::hid::client::DeviceEvent::Gone("keyboard".to_string()),
        );
        let projected: Vec<_> = devices.values().cloned().collect();

        assert_eq!(projected.len(), 2);
        assert!(
            !projected
                .iter()
                .find(|d| d.device_key == "keyboard")
                .unwrap()
                .online
        );
        assert!(
            projected
                .iter()
                .find(|d| d.device_key == "mouse")
                .unwrap()
                .online
        );
    }

    #[test]
    fn sleeping_device_tooltip_marks_the_battery_as_a_last_reading() {
        let mut device = DeviceTrayProjection::online(&mk("keyboard"));
        device.display_name = "MX Keys Wireless Keyboard".to_string();
        device.battery_percent = 50;
        device.online = false;

        let tooltip = device_tooltip(&device);

        assert_eq!(
            tooltip,
            "MX Keys Wireless Keyboard：50%（休眠/未连接，最后读数）"
        );
        assert!(!device.online);
    }

    #[test]
    fn online_bluetooth_wins_over_offline_receiver_for_same_device() {
        let mut receiver = DeviceTrayProjection::online(&mk("receiver"));
        receiver.display_name = "MX Master 3S".to_string();
        receiver.online = false;
        let mut bluetooth = DeviceTrayProjection::online(&mk("bluetooth"));
        bluetooth.display_name = "MX Master 3S Bluetooth".to_string();
        bluetooth.transport = Transport::Bluetooth;

        let devices = deduplicate_device_trays(vec![receiver, bluetooth]);

        assert_eq!(devices.len(), 1);
        assert_eq!(devices[0].transport, Transport::Bluetooth);
        assert!(devices[0].online);
    }

    #[test]
    fn online_receiver_wins_over_online_bluetooth_for_same_device() {
        let mut receiver = DeviceTrayProjection::online(&mk("receiver"));
        receiver.display_name = "MX Keys".to_string();
        let mut bluetooth = DeviceTrayProjection::online(&mk("bluetooth"));
        bluetooth.display_name = "MX Keys (Bluetooth)".to_string();
        bluetooth.transport = Transport::Bluetooth;

        let devices = deduplicate_device_trays(vec![bluetooth, receiver]);

        assert_eq!(devices.len(), 1);
        assert_eq!(devices[0].transport, Transport::Receiver);
    }
}
