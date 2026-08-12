LogiBatteryTray 便携版

双击 LogiBatteryTray.exe 后，Windows 通知区域只显示一枚汇总图标，数字取键盘和鼠标中的最低电量.
悬停或右键图标可分别查看两台设备的电量，并可立即刷新、调整提醒或退出.

默认不会登录启动，也不会联网. 只有在托盘菜单中勾选“登录时启动”后才会写入当前用户启动项.

支持通过 Logitech USB 接收器连接的 HID++ 2.0 设备，以及 Windows 能暴露标准 GATT Battery Service 的 Bluetooth LE 设备.
某些只提供厂商私有电量服务的蓝牙设备仍无法读取；详细兼容边界见 GitHub README.
源码与构建说明：https://github.com/Javierpa20/LogiBatteryTray
许可：LICENSE（MIT，上游 Copyright (c) 2026 Ithilias）.
