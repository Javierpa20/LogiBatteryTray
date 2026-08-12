# LogiBatteryTray

LogiBatteryTray 是面向本机 Logitech 键鼠的轻量 Windows 电量托盘程序. 它直接通过 USB 接收器读取 HID++ 电量，不依赖 Logitech Options+，不联网、不含遥测或自动更新.

当前实机目标：

- MX Keys Wireless Keyboard（Unifying 接收器）.
- MX Master 3S（Bolt 接收器）.

## 功能

- 键盘和鼠标合并为一枚汇总托盘图标，默认显示所有设备中的最低电量，优先暴露需要充电的设备.
- 悬停提示和中文右键菜单同时列出每台设备的独立电量；在线读数彩色显示，最低电量设备休眠或断连时汇总图标灰显，并明确标注“最后读数”.
- 可立即刷新并切换数字/电池图标.
- 支持低电量提醒、阈值与重复提醒间隔、HID++ 事件驱动更新及低频兜底刷新.
- 登录启动为可选项，默认关闭.
- `--once` 输出当前设备电量；`--diag` 输出 HID++ 诊断信息.

限制：只支持通过 Logitech Unifying、Bolt 或 LIGHTSPEED USB 接收器连接并实现 HID++ 2.0 电量特性的设备. 纯蓝牙连接不在支持范围内.

## 使用

直接运行 `LogiBatteryTray.exe`. 程序没有主窗口，一枚汇总图标会出现在 Windows 通知区域（必要时展开 `^`）.

命令行检查：

```powershell
& '.\LogiBatteryTray.exe' --once
& '.\LogiBatteryTray.exe' --diag
```

配置、设备能力缓存和滚动日志默认保存在 `%APPDATA%\logitray`.

默认配置：

```toml
poll_interval_seconds = 180
low_battery_threshold = 15
low_battery_cooldown_minutes = 120
selected_device_id = ""
autostart = false
log_level = "info"
view_mode = "text"
notifications_enabled = true
```

`selected_device_id` 是上游版本遗留兼容字段；定制版始终汇总所有设备，不再使用它筛选单一设备.

## 构建

```powershell
cargo fmt -- --check
cargo test --all-targets
cargo build --release
```

Windows MSVC 构建产物位于 `target\release\LogiBatteryTray.exe`. 仓库的 `dist\` 保存已验收的便携 EXE、许可和 SHA-256 清单.

## 来源与许可

本项目基于 [Ithilias/logitray v0.3.0](https://github.com/Ithilias/logitray) 定制，保留原项目 MIT 许可. 主要本地改造包括双接收器实测、单图标汇总并显示最低电量、休眠状态保留最后读数、中文菜单和 Windows GUI 子系统下的 CLI 输出修复.

完整许可见 `LICENSE`.
