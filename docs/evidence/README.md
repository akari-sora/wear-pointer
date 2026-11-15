# Cross-OS Live Hardware Verification Evidence (`v0.2.4`)

Every directory in `docs/evidence/` was captured from **live Bluetooth LE sessions** driven by a physical **Samsung Galaxy Watch7 (`SM-L310`, Wear OS 6 / Android 16)** running `dev.wearpointer.app` `v0.2.4` (`versionCode = 6`, default `1600 DPI` / `±15.6°` reach, R8-minified `1.45 MB` APK) against each target desktop operating system. No synthetic `--replay` substitute was used.

To prevent stray clicks on active desktop applications, all desktop pointer/button/wheel injections were captured inside centered non-activating Click-Shield windows and the user's original cursor coordinates and active window focus were restored at teardown.

## Summary Matrix (`LASER` · `AIR` · `TOUCH` + All Controls)

| Feature / Operation | Omarchy (`Hyprland 0.56.2` Wayland `1280x800 @ 1.5x` + BlueZ `5.87`) | Windows 11 (`win-desktop` Session 1 `2560x1440` + WinRT BLE) | macOS (`M4 Max` `macOS 27.0.1` `1920x1080` + CoreBluetooth) |
| --- | --- | --- | --- |
| **Transport & Injection** | BlueZ GATT Central + `zwlr_virtual_pointer_v1` | WinRT BLE Central + Win32 `SendInput` | CoreBluetooth Central + `CGEventPost(HID)` |
| **1. `100 Hz` Live Aim Stream** | `2,886` BLE rx (`100.6–104.0/s`), `0.0%` loss, `9.8–13.6 ms` avg | `5,148` BLE rx (`100.6–103.6/s`), `0.0%` loss, `6.9–11.8 ms` avg | `3,358` BLE rx (`101.0–103.6/s`), `0.0%` loss, `7.1–11.9 ms` avg |
| **2. `LASER` Mode + `Center`** | `BEFORE (642,399)` → `AFTER (640,398)` on `1280x800` logical layout | `BEFORE (1375,645)` → `AFTER (1281,716)` on `2560x1440` | `BEFORE (1055,465)` → `AFTER (951,539)` on `1920x1080` |
| **3. `AIR` Mode (Gyro Relative)** | Stationary 2.5 s `(640,400)→(640,400)` (`0 px` drift); external handoff click at `(710,365)` | Stationary 2.5 s `(1280,720)→(1280,720)` (`0 px` drift); external handoff click at `(1375,645)` | Stationary 3.0 s `(960,540)→(959,540)`; external handoff click at `(1055,465)` |
| **4. `TOUCH` Mode (Trackpad)** | Fast swipe `+91 px` / slow `-28 px`; tap click + `1,383 ms` hold-drag to `(720,400)` | Fast swipe `+176 px` / slow `-57 px`; tap click + `1,355 ms` hold-drag (`mv=95`) to `(1434,720)` | Fast swipe `+156 px` / slow `-42 px`; tap click + `2,008 ms` hold-drag; edge-swipe keeps `MainActivity` |
| **5. `Left` / `Right` / `Drag`** | `button Left` ×4 (`1,383 ms` drag), `button Right` `1/1` | `left_down=5 / left_up=5` (`1,355 ms` drag), `right_down=1 / right_up=1` | `left_down=4 / left_up=4` (`500 ms` & `2,008 ms` drag), `right_down=1 / right_up=1` |
| **6. `Rotary Bezel Scroll ±`** | `scroll -272` / `scroll +272` | `WM_MOUSEWHEEL` `up=1, down=1` | `NSEvent.scrollWheel` `up=2, down=2` + touch-mode bezel `-272` |
| **7. `Live DPI Step + Bezel + Reset`** | `1600 → 1800 → 2000 → 1600 DPI` (+ `v0.2.2` bezel `→ 2400`) | `1600 → 1800 → 2000 → 1600 DPI` (+ `v0.2.2` bezel `→ 2600`) | `1600 → 1800 → 2000 → 2200 → 2600 → 2400 → 1600 DPI` |
| **8. `Privacy & Role Overlay`** | Verified live (`watch7/watch_06_privacy.png`, `watch7/watch_08_mode_sheet.png`) | Verified live (`watch7/watch_06_privacy.png`) | Verified live (`steps.log` Op 8) |
| **9. `Pause / Resume` + `WakeLock`** | Clutch pause/resume + `PAUSED: NONE_HELD` / `RESUMED: wear-pointer:sensors` | Clutch pause/resume + `PAUSED: NONE_HELD` / `RESUMED: wear-pointer:sensors` | Clutch pause/resume + `PAUSED: NONE_HELD` / `RESUMED: wear-pointer:sensors` |
| **10. `Stop / Start` Reconnect** | `disconnected 1` → auto `connected 2` + post-reconnect click at `(635,399)` | `disconnected 1` → auto `connected 2` + post-reconnect click at `(1291,717)` | `disconnected 1` → auto `connected 2` + post-reconnect click |
| **Cursor Restored** | `(717, 440) → (717, 440)` | `(474, 51) → (474, 51)` | `(-1254, 935) → (-1254, 935)` |

## Directory Contents

- `omarchy/` — Compositor-verified HUD summary (`shield_hud.png`), timestamped `v0.2.4` step log (`steps.log`), 1 Hz receiver telemetry (`stderr.log`), trace action/mode/DPI summary (`trace_summary.txt`), `os_events.jsonl`, `wakelock_check.txt`, and `watch_logcat.txt`.
- `windows/` — Session 1 `NoActivateForm` HUD capture (`shield_hud.png`), Win32 OS event log (`os_events.jsonl`, `4,049` motions), `steps.log`, `stderr.log`, `trace_summary.txt`, `wakelock_check.txt`, and `watch_logcat.txt`.
- `macos/` — Non-activating `NSPanel` HUD summary (`shield_hud.png`), unlocked Cocoa `NSEvent` OS event log (`os_events.jsonl`), `steps.log`, `stderr.log`, `trace_summary.txt`, `wakelock_check.txt`, `watch_logcat.txt`, plus `v0.2.4` pointing-modes logs (`modes_steps.log`, `modes_stderr.log`, `modes_trace_summary.txt`, `modes_watch_logcat.txt`).
- `watch7/` — Live `480x480` physical `SM-L310` `v0.2.4` screenshots across all modes and states (`watch_01_linked_1600dpi.png`, `watch_02_drag_locked.png`, `watch_03_sensitivity_tuned.png`, `watch_04_paused.png`, `watch_05_stopped.png`, `watch_06_privacy.png`, `watch_07_app_icon.png`, `watch_08_mode_sheet.png`, `watch_09_air_mode.png`, `watch_10_touch_mode.png`).
