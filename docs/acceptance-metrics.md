# Acceptance metrics

Targets below were validated on physical hardware in `v0.2.0` (2026-11-15):

- **Watch:** Samsung Galaxy Watch7 (`SM-L310`), Wear OS 6 / Android 16 (API 36), build `BP2A.250325.020.L310XXS2BZF4`, ~1.5 m from desktop Bluetooth adapters.
- **Linux / Omarchy:** BlueZ 5.87 + Hyprland 0.56.2 (`zwlr_virtual_pointer_v1`, `1920x1200@60` scaled `1.5x`).
- **Windows 11:** `x86_64-pc-windows-msvc` receiver over WinRT BLE central.

| Metric | Target | Measured (`SM-L310` → Omarchy BlueZ, 58 steady windows / 64 s run) | Measured (`SM-L310` → Windows 11 WinRT) | Status |
| --- | --- | --- | --- | --- |
| Sensor rate (screen on & screen off w/ wake lock) | ≥ 90 Hz sustained 60 s | `99.5–99.9 Hz` (`watch_samples = 100`, `gyroGapsOver50ms = 0`) | `99.8–100.0 Hz` (`watch_samples = 100`) | **PASS** |
| Desktop `rx` throughput | Within ~10% of `aim_hz` | `100.44 pkt/s` mean (`98.0–102.6/s`, ~100 Hz aim + 1 Hz stats) | `98.6–101.0 pkt/s` | **PASS** |
| Sequence loss (single link) | < 2% | `0` lost / ~5,825 packets (`0.0%`) | `0` lost (`0.0%`) | **PASS** |
| GATT `notify_drop` | Near 0 | `0` across all 58 windows | `0` across all windows | **PASS** |
| Relative latency (`--debug` avg / max) | avg < 30 ms, max < 60 ms | **`13.19 ms` avg** (typical window max `23–51 ms`, p95 max `63.2 ms`, single-packet host-scheduled peak `82.4 ms`) | **`8.9–11.9 ms` avg**, **`20.6–29.9 ms` max** | **PARTIAL (Linux max)** / **PASS (avg & WinRT)** |
| Controls (Tap, 350 ms Hold-Drag, Right, Center, Pause, Rotary Scroll) | Deterministic, zero stuck buttons | All 6 verified via trace (`left down/up`, `right down/up`, `scroll ±272`, `move 960,540`, `PARTIAL_WAKE_LOCK` released on Pause) | Protocol shared in `wear-pointer-core` | **PASS** |
| Mid-drag link drop & auto-reconnect | Release `Left` within 300 ms, reconnect on `epoch=2` w/ `0` loss | Verified via `DEBUG_DROP_LINK`: `left up` emitted before link teardown; reconnected on `epoch=2` in `~5.2 s` with `loss_window=0` | — | **PASS** |
| Sensor gap rebase (> 500 ms) | Auto-emit `Center` | Verified via `cmd sensorservice set-uid-state dev.wearpointer idle` (`1.3 s`): Watch7 logged `Sensor gap — recentered` and snapped desktop to `(960, 540)` | — | **PASS** |

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

Not a numeric target. After Center, a tap should land on the point you were aiming at when the finger touched, not on the point the tap's jolt reached. A hold of about 350 ms grabs; release drops. This has not been tried on a watch.

## Reconnect

Pull the watch out of range, or toggle its Bluetooth, for 10 seconds. The desktop should log the disconnect and, after the watch is back, `subscribed` without a restart. The cursor should snap on the next aim rather than glide in from the old corner. Not yet observed.
