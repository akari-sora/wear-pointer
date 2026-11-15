# Acceptance metrics

Targets below were validated on physical hardware in `v0.2.0`–`v0.2.4` (2026-11-15), with full auditable logs, OS event captures, and Watch7 screenshots stored in [`docs/evidence/`](evidence/README.md):

- **Watch:** Samsung Galaxy Watch7 (`SM-L310`), Wear OS 6 / Android 16 (API 36), build `BP2A.250325.020.L310XXS2BZF4`, `dev.wearpointer.app` `v0.2.4` (`versionCode = 6`, default `1600 DPI` / `±15.6°` reach, R8-shrunk `1.45 MB` APK).
- **Linux / Omarchy:** BlueZ 5.87 + Hyprland 0.56.2 (`zwlr_virtual_pointer_v1`, `1920x1200@60` scaled `1.5x` = `1280x800` logical layout).
- **Windows 11:** `x86_64-pc-windows-msvc` receiver (`2560x1440` Session 1) over WinRT BLE central + Win32 `SendInput`.
- **macOS (`M4 Max`):** `aarch64-apple-darwin` `WearPointer.app` (`1920x1080`, `macOS 27.0.1`) over CoreBluetooth central + `CGEventPost(HID)`.

| Metric | Target | Omarchy (`BlueZ + Hyprland`) | Windows 11 (`WinRT + SendInput`) | macOS (`CoreBluetooth + CGEventPost`) | Status |
| --- | --- | --- | --- | --- | --- |
| Sensor rate (screen on & screen off w/ wake lock) | ≥ 90 Hz sustained 60 s | `99.7–99.9 Hz` (`watch_samples = 100`, `gyroGapsOver50ms = 0`) | `99.7–99.9 Hz` (`watch_samples = 100`) | `99.7–100.0 Hz` (`watch_samples = 100`) | **PASS** |
| Desktop `rx` throughput | Within ~10% of `aim_hz` | `100.4–104.0 pkt/s` | `100.6–103.6 pkt/s` | `100.0–104.0 pkt/s` | **PASS** |
| Sequence loss (single link) | < 2% | `0` lost (`0.0%`) | `0` lost (`0.0%`) | `0` lost (`0.0%`) | **PASS** |
| GATT `notify_drop` | Near 0 | `0` | `0` | `0` | **PASS** |
| Relative latency (`--debug` avg / max) | avg < 30 ms, max < 60 ms | **`9.8–13.6 ms` avg** (typical max `20–49 ms`) | **`6.6–11.8 ms` avg** (`15.6–33.8 ms` max) | **`7.1–12.9 ms` avg** (`21.7–43.6 ms` max) | **PASS** |
| Pointing Modes (`LASER` / `AIR` / `TOUCH`) | Live switch (`KIND_MODE`), zero drift in `AIR`, ballistics & hold-drag in `TOUCH` | `LASER` `(640,398)`, `AIR` `0 px` drift + `(710,365)` handoff, `TOUCH` `+91/-28 px` + `1383 ms` drag | `LASER` `(1281,716)`, `AIR` `0 px` drift + `(1375,645)` handoff, `TOUCH` `+176/-57 px` + `1355 ms` drag (`mv=95`) | `LASER` `(951,539)`, `AIR` rest `(960,540)→(959,540)` + `(1055,465)` handoff, `TOUCH` `+156/-42 px` + `2008 ms` drag | **PASS** |
| Controls (Tap, 350 ms Hold-Drag, Right, Center, Pause, Rotary Scroll) | Deterministic, zero stuck buttons | Verified live (`746`–`1,461` moves, `479–1,383 ms` drag, `±272` scroll, `Center → 640,398`) | Verified live (`4,049` OS moves, `5/5` left, `1,355 ms` drag w/ `95` moves, `1/1` right, `±1` wheel, `Center → 1281,716`) | Verified live (`1,810` OS moves, `4/4` left, `500–2,008 ms` drag, `1/1` right, `±2` wheel, `Center → 951,539`) | **PASS** |
| Live Sensitivity / DPI (`KIND_SENSITIVITY`, `1600 DPI` default) | Ergonomic default + live step, bezel & reset | `1600 → 1800 → 2000 → 2400 → 2200 → 1600 DPI` | `1600 → 1800 → 2000 → 2600 → 2400 → 1600 DPI` | `1600 → 1800 → 2000 → 2200 → 2600 → 2400 → 1600 DPI` | **PASS** |
| Stop / Start & mid-drag auto-reconnect | Release `Left` within 300 ms, auto-reconnect w/ `0` loss | Verified (`epoch=1 → epoch=2`, post-reconnect click `(635,399)`) | Verified (`epoch=1 → epoch=2`, post-reconnect click `(1291,717)`) | Verified (`epoch=1 → epoch=2` in `4.8 s` + post-reconnect click) | **PASS** |
| Sensor gap rebase (> 500 ms) & Pause WakeLock | Auto-emit `Center`; release WakeLock on Pause | `NONE_HELD` on Pause, `wear-pointer:sensors` on Resume; `Sensor gap — recentered` verified | `NONE_HELD` on Pause, `wear-pointer:sensors` on Resume | `NONE_HELD` on Pause, `wear-pointer:sensors` on Resume | **PASS** |

## End-to-end latency

The desktop `--debug` line reports latency relative to the fastest packet in the session, not an absolute clock. The first packet reads about 0 ms by construction. A useful number is the average and max after the link has been up for a minute, while moving the watch.

Target: average under 30 ms, max under 60 ms, in the same room with the watch within 2 m of the adapter. If the max sits near a multiple of the connection interval (7.5, 15, 30, 45 ms are typical), the interval is the bottleneck, not the filter.

The filter adds its own delay on slow motion (`--steady`, default 0.8 Hz). Latency above is radio delay. Cursor lag while moving slowly is the filter, on purpose.

## Sensor rate

The watch asks `SensorManager` for 100 Hz (`samplingPeriodUs = 10_000`). The UI and the stats packet report how many gyroscope events arrived in the last window.

Target: 90 Hz or better sustained for 60 seconds with the screen on. Below 50 Hz, aim integration gets coarse and the 240 Hz desktop loop is interpolating guesses.

## Packet loss and notify throughput

`--debug` prints `rx` (packets the desktop accepted per second) and `loss_window` from sequence gaps. The same line prints the watch's `watch_samples`, `notify_ok`, and `notify_drop` when a stats packet has arrived.

Target over 60 seconds, watch still, then watch moving:

- sequence loss under 2% on one connection. A reconnect resets the sequence; that reset is not loss
- `notify_drop` (callback and API failures) near 0 while connected. Coalesced aim samples are logged separately and are not radio loss
- `rx` within about 10% of `aim_hz` (watch samples divided by the stats window, not a raw count treated as Hz)

If `watch_samples` is ~100 and `rx` is ~20 with a matching loss percentage, the radio or the connection interval is dropping notifications. If `watch_samples` is already ~20, the sensor path is the limit and BLE is not the first suspect.

## Long-run drift

Elevation comes from gravity, so pointing at a fixed spot on the wall should not walk up or down because of integration. Heading is integrated gyro, so it can drift.

Target, after Center, pointing at one spot for 10 minutes without touching the watch:

- vertical drift small enough that the cursor stays in the same button-sized region
- horizontal drift noted in pixels per minute, not required to be zero

A dead zone (`--dead-zone`) is off by default. WatchPointer removed one because it made the cursor walk. Only enable it if a Stage 0 log shows a reason, and re-check this drift test after.

## Click and drag

After Center, a tap lands on the point you were aiming at when the finger touched (touch-timestamp historical rewind), not on the point the tap's jolt reached. A hold of 350 ms locks drag (`DRAG LOCKED`); release drops. Verified live on physical Watch7 (`SM-L310`) across Omarchy, Windows 11, and macOS ([`docs/evidence/`](evidence/README.md)).

## Reconnect

Stop the watch service (or toggle Bluetooth), then start it again. The desktop logs `link down`, automatically rescans, reconnects (`link up epoch>=2`) without restarting the receiver, re-syncs watch DPI (`KIND_SENSITIVITY`), and snaps the cursor cleanly on the next aim. Verified live across all three desktop OSes ([`docs/evidence/`](evidence/README.md)).
