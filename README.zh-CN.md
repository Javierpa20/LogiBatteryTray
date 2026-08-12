# LogiBatteryTray

[English](README.md)

LogiBatteryTray 是面向 Logitech 键盘和鼠标的轻量 Windows 电量托盘程序. 它可以直接通过 USB 接收器读取 HID++ 设备，也可以通过 Windows 标准 GATT Battery Service 读取低功耗蓝牙设备. 它不依赖 Logitech Options+，不联网，不含遥测或自动更新.

## 功能

- 将所有设备合并为一枚托盘图标，显示最低电量，让最需要充电的设备保持可见.
- 悬停提示和右键菜单分别列出每台设备的电量. 设备休眠或断开后保留最后读数，并明确标记为非在线状态.
- 支持通过 Unifying、Bolt 或 LIGHTSPEED USB 接收器连接的 Logitech HID++ 2.0 设备.
- 支持 Windows 识别为 HID 外设的 Bluetooth LE 键盘和鼠标，以及名称可识别的 Logitech 输入设备，但设备必须暴露标准 Battery Service（`0x180F`）及 Battery Level characteristic（`0x2A19`）.
- 同一设备同时被 USB 接收器和蓝牙发现时自动去重；两路都在线时优先采用 HID++ 接收器读数，因为它支持事件驱动更新.
- 支持立即刷新、数字/电池图标切换、低电量提醒、阈值与重复提醒间隔，以及默认关闭的登录启动选项.
- `--once` 输出当前电量；`--diag` 输出 HID++ 诊断信息.

## 蓝牙兼容边界

蓝牙功能使用 Windows 原生 WinRT GATT API，不依赖 Options+. 只有 Windows 能够访问设备的标准 GATT Battery Service 时才可读取电量，无关的 BLE 电池设备会被过滤. 某些 Logitech 型号或固件只通过厂商私有服务提供电量，这类设备无法由本实现通过蓝牙读取，但仍可能通过受支持的 USB 接收器工作.

HID++ 接收器设备可以提供充电状态. 标准蓝牙 Battery Level characteristic 只提供百分比，没有统一的充电状态字段.

## 使用

直接运行 `LogiBatteryTray.exe`. 程序没有主窗口，一枚汇总图标会出现在 Windows 通知区域（必要时展开 `^`）.

命令行检查：

```powershell
& '.\LogiBatteryTray.exe' --once
& '.\LogiBatteryTray.exe' --diag
```

配置、HID++ 设备能力缓存和滚动日志默认保存在 `%APPDATA%\logitray`. 登录启动默认关闭，只有在托盘菜单中明确启用后才会写入当前用户启动项.

## 构建

```powershell
cargo fmt -- --check
cargo test --all-targets
cargo build --release
```

Windows MSVC 构建产物位于 `target\release\LogiBatteryTray.exe`. 仓库的 `dist\` 保存已验收的便携 EXE、许可和 SHA-256 清单.

## 来源与许可

本项目基于 [Ithilias/logitray v0.3.0](https://github.com/Ithilias/logitray) 定制并保留 MIT 许可. 主要改造包括多接收器支持、单枚最低电量汇总图标、休眠设备保留最后读数、Bluetooth GATT 支持、中文托盘界面，以及 Windows GUI 子系统构建下的 CLI 输出修复.

完整许可见 [LICENSE](LICENSE).
