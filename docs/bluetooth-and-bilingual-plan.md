# Bluetooth support and bilingual GitHub plan

## Goal

1. Make the public GitHub landing page bilingual: English `README.md` and Chinese `README.zh-CN.md`, with reciprocal language links.
2. Read battery levels from Logitech devices connected directly through Windows Bluetooth Low Energy, while retaining receiver-based HID++ support.
3. Keep one summary tray icon and avoid duplicate entries if the same physical device is visible through both transports.

## Design

- Add a Windows-only Bluetooth backend based on the standard GATT Battery Service (`0000180f-0000-1000-8000-00805f9b34fb`) and Battery Level characteristic (`00002a19-0000-1000-8000-00805f9b34fb`).
- Poll paired/visible GATT battery services on a conservative timer and emit the same device events used by HID++ receiver workers.
- Extend battery state with a transport marker (`Receiver` or `Bluetooth`) and stable source key.
- Before rendering, group states by normalized display name. Prefer an online state over a stale one; if both are online, prefer the receiver reading because it already has event-driven HID++ updates. Preserve the other source internally so transport switching is seamless.
- Keep the summary icon rule unchanged: show the lowest battery among deduplicated devices; tooltip/menu retain individual device readings and identify the active transport.
- Bluetooth failures are isolated: unavailable radio, denied access, sleeping device, missing Battery Service, and malformed characteristic values must not terminate receiver monitoring or the tray process.

## Tests and acceptance

- Unit tests: transport-aware keys, same-name deduplication, online-over-stale preference, receiver tie-break, distinct-device preservation, summary minimum.
- Existing HID++ and tray tests remain green.
- Build and run `--once`; receiver readings must remain unchanged.
- Bluetooth diagnostic output must distinguish: supported devices found, no active Battery Service, and backend/API failure.
- Update portable binary and SHA-256 only after release build passes.
- Review staged Git diff, secrets, personal paths, encoding/NUL, binary size, MIT license, and GitHub Actions before push.

## Honest support boundary

Bluetooth support means Windows BLE devices that expose the standard GATT Battery Service. Some Logitech firmware/Windows pairings may hide that service or expose battery only through a vendor path; those cases will be reported as unsupported rather than guessed. Real Bluetooth hardware acceptance requires switching at least one target device from its receiver channel to a paired Bluetooth channel.
