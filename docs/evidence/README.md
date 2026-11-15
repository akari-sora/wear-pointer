# Cross-OS Live Hardware Verification Evidence (`v0.2.2`)

Every directory in `docs/evidence/` was captured from **live Bluetooth LE sessions** driven by a physical **Samsung Galaxy Watch7 (`SM-L310`, Wear OS 6 / Android 16)** running `dev.wearpointer.app` `v0.2.2` (`versionCode = 4`, default `1600 DPI` / `±15.6°` reach) against each target desktop operating system. No synthetic `--replay` substitute was used.

To prevent stray clicks on active desktop applications, all desktop pointer/button/wheel injections were captured inside dedicated centered non-activating Click-Shield windows and the user's original cursor coordinates were restored at teardown.

## Summary Matrix

| Feature / Operation | Omarchy (`Hyprland 0.56.2` Wayland + BlueZ `5.87`) | Windows 11 (`win-desktop` Session 1 + WinRT BLE) | macOS (`M4 Max` `macOS 27.0.1` + CoreBluetooth) |
| --- | --- | --- | --- |
| **Transport & Injection** | BlueZ GATT Central + `zwlr_virtual_pointer_v1` | WinRT BLE Central + Win32 `SendInput` | CoreBluetooth Central + `CGEventPost(HID)` |
| **1. `100 Hz` Live Aim Stream** | `2,022` BLE rx (`102.6/s`), `0.0%` loss, `10.6 ms` avg | `2,208` BLE rx (`102.0/s`), `0.0%` loss, `6.6–10.6 ms` avg | `1,940` BLE rx (`102.6/s`), `0.0%` loss, `7.1–11.9 ms` avg |
| **2. `Center` Snap** | `BEFORE (639,397)` / `(412,0)` → `AFTER (640,398)` on `1280x800` | `BEFORE (1375,645)` → `AFTER (1280,714)` on `2560x1440` | `BEFORE (1055,465)` → `AFTER (960,539)` on `1920x1080` |
| **3. `Left Click` (Tap)** | `button Left down/up` ×2 | `left_down=4 / left_up=4` (`os_events.jsonl`) | `left_down=4 / left_up=4` (`os_events.jsonl`) |
| **4. `Hold-to-Drag` (`≥350 ms`)** | `479.2 ms` (`8.792s → 9.271s`) | `493.2 ms` (`drag_motion=36`) | `500.3 ms` (`drag_moves=29`) |
| **5. `Right Click`** | `button Right down/up` (`1 / 1`) | `right_down=1 / right_up=1` | `right_down=1 / right_up=1` |
| **6. `Rotary Bezel Scroll ±`** | `scroll -272` ×2, `scroll +272` ×2 | `WM_MOUSEWHEEL` `up=2, down=2` | `NSEvent.scrollWheel` `up=2, down=2` |
| **7. `Live DPI Step + Bezel + Reset`** | `1600 → 1800 → 2000 → 2400 → 2200 → 1600 DPI` | `1600 → 1800 → 2000 → 2600 → 2400 → 1600 DPI` | `1600 → 1800 → 2000 → 2200 → 2600 → 2400 → 1600 DPI` |
| **8. `Privacy & Role Overlay`** | Verified live (`watch7/watch_06_privacy.png`) | Verified live (`steps.log` Op 8) | Verified live (`steps.log` Op 8) |
| **9. `Pause / Resume` + `WakeLock`** | `PAUSED: NONE_HELD` / `RESUMED: wear-pointer:sensors` | `PAUSED: NONE_HELD` / `RESUMED: wear-pointer:sensors` | `PAUSED: NONE_HELD` / `RESUMED: wear-pointer:sensors` |
| **10. `Stop / Start` Reconnect** | `disconnected 1` → auto `connected 3` (`101.0/s`) | `disconnected 1` → auto `connected 2` + post-reconnect click | `disconnected 1` → auto `connected 2` + post-reconnect click |
| **Cursor Restored** | `(717, 440) → (717, 440)` | `(0, 698) → (0, 698)` | `(545, 732) → (545, 732)` |

## Directory Contents

- `omarchy/` — HUD summary (`shield_hud.png`), timestamped step log (`steps.log`), 1 Hz receiver telemetry (`stderr.log`), trace action/DPI summary (`trace_summary.txt`), `wakelock_check.txt`, and `watch_logcat.txt`.
- `windows/` — Session 1 `NoActivateForm` HUD capture (`shield_hud.png`), Win32 OS event log (`os_events.jsonl`), `steps.log`, `stderr.log`, `trace_summary.txt`, `wakelock_check.txt`, and `watch_logcat.txt`.
- `macos/` — Non-activating `NSPanel` HUD capture (`shield_hud.png`), Cocoa `NSEvent` OS event log (`os_events.jsonl`), `steps.log`, `stderr.log`, `trace_summary.txt`, `wakelock_check.txt`, and `watch_logcat.txt`.
- `watch7/` — Live `480x480` physical `SM-L310` screenshots across all states (`watch_01_linked_1600dpi.png`, `watch_02_drag_locked.png`, `watch_03_sensitivity_tuned.png`, `watch_04_paused.png`, `watch_05_stopped.png`, `watch_06_privacy.png`).
