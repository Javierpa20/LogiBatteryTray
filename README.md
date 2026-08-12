# LogiBatteryTray

[简体中文](README.zh-CN.md)

LogiBatteryTray is a lightweight Windows tray battery monitor for Logitech keyboards and mice. It reads HID++ devices directly through USB receivers and can also read Bluetooth Low Energy devices through Windows' standard GATT Battery Service. It does not require Logitech Options+, connect to the Internet, collect telemetry, or auto-update.

## Features

- Combines all devices into a single tray icon and displays the lowest battery level, so the device that needs attention stays visible.
- Lists each device and its individual battery level in the tooltip and right-click menu. A sleeping or disconnected device keeps its last reading and is clearly marked inactive.
- Supports Logitech HID++ 2.0 devices connected through Unifying, Bolt, or LIGHTSPEED USB receivers.
- Supports Bluetooth LE keyboards and mice that Windows identifies as HID peripherals, or recognizable Logitech input devices, when they expose the standard Battery Service (`0x180F`) and Battery Level characteristic (`0x2A19`).
- Deduplicates a device visible through both a receiver and Bluetooth; a live receiver reading wins because HID++ provides event-driven updates.
- Provides manual refresh, percentage/battery icon modes, low-battery notifications, configurable thresholds and cooldowns, and an opt-in launch-at-login option.
- Includes `--once` for current battery readings and `--diag` for HID++ diagnostics.

## Bluetooth compatibility

Bluetooth support uses Windows' native WinRT GATT APIs and does not depend on Options+. It works only when Windows exposes the device's standard GATT Battery Service. Unrelated BLE battery devices are filtered out. Some Logitech models or firmware expose battery information only through proprietary services; those devices cannot be read over Bluetooth by this implementation and may still work through a supported USB receiver.

Charging state is currently available from HID++ receiver devices. The standard Bluetooth Battery Level characteristic supplies a percentage but no universal charging-state value.

## Usage

Run `LogiBatteryTray.exe`. The app has no main window; one summary icon appears in the Windows notification area (expand `^` if necessary).

Command-line checks:

```powershell
& '.\LogiBatteryTray.exe' --once
& '.\LogiBatteryTray.exe' --diag
```

Configuration, the HID++ capability cache, and rotating logs are stored in `%APPDATA%\logitray` by default. Launch at login is disabled until explicitly enabled from the tray menu.

## Build

```powershell
cargo fmt -- --check
cargo test --all-targets
cargo build --release
```

The Windows MSVC output is `target\release\LogiBatteryTray.exe`. The repository's `dist\` directory contains the verified portable executable, license, and SHA-256 manifest.

## Origin and license

This project is based on [Ithilias/logitray v0.3.0](https://github.com/Ithilias/logitray) and retains its MIT license. Major customizations include multi-receiver support, a single lowest-battery summary icon, retained last readings for sleeping devices, Bluetooth GATT support, a Chinese tray interface, and repaired CLI output in a Windows GUI-subsystem build.

See [LICENSE](LICENSE) for the full license text.
