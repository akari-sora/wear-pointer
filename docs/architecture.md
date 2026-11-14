# Architecture

wear-pointer turns a Galaxy Watch7 into an air mouse for a desktop across Linux (Omarchy / Hyprland `zwlr_virtual_pointer_v1` and optional `uinput`), macOS (`CGEventPost`), and Windows (`SendInput`).

The aim math is a port of [WatchPointer](https://github.com/umarz317/WatchPointer) (Copyright (c) 2026 Umar Afzal, MIT). See `NOTICE`. The Bluetooth direction and the packet framing are wear-pointer's.

## Pieces

```
watch sensors (100 Hz)
    gyro + gravity
        |
        v
AimTracker on the watch
    elevation from gravity (no drift)
    heading = integral of rotation about vertical
        |
        v
protocol v1 packet  (sequence + timestamp_us + aim)
        |
        v
BLE  ---- default: watch advertises, desktop subscribes
        \- fallback: desktop advertises, watch writes
        |
        v
receiver session
    1€ filter using watch timestamps
    laser projection onto the screen
    240 Hz ease toward the filtered target
        |
        v
PointerBackend
    hyprland: zwlr_virtual_pointer_v1
    macos: CGEventPost (auto-detects CGDisplay::main)
    windows: SendInput (auto-detects GetSystemMetrics)
    log: print only (Stage 0 / headless diagnostics)
    uinput: optional Linux relative mouse
```

## Why the split looks like this

The watch integrates heading. A dropped notification then only loses a sample the next packet already includes, instead of losing rotation. Elevation is recomputed from gravity on every sample, so pitch does not drift.

The desktop does not trust Bluetooth arrival time. Each packet carries the watch clock. The 1€ filter uses the gap between those timestamps. A separate 240 Hz loop eases the cursor toward the latest filtered point, so a burst of packets does not become a burst of jumps.

Clicks carry the finger-down timestamp. The desktop keeps one second of cursor history and presses where the pointer was then.

## Bluetooth roles

`PointerTransport` on the watch and `PointerLink` on the desktop are the swap point. The desktop link emits `Connected` / `Packet` / `Disconnected` with an epoch. A new epoch resets sequence tracking, the clock anchor, filters, and cursor history so a watch sequence restart is not thousands of lost packets. Packet bytes do not change. The desktop peripheral side is still a stub: [fallback-transport.md](fallback-transport.md).

Pointer notifications are notifications, not indications. When a central subscribes to CCCD, `WatchPeripheral` also requests `BluetoothGatt.CONNECTION_PRIORITY_HIGH` (`7.5–11.25 ms` LE connection interval) without mutating the watch's system Bluetooth adapter name. Both watch roles share `OutboundQueue`: one GATT operation in flight, FIFO, sequence written at the hand-off to the stack. A newer aim replaces only the queued aim at the tail, so a click already behind it is not reordered, and a coalesced aim never consumes a sequence. Center and Pause drop the tail aim (old base, unsent); Stop clears the queue. Busy is retried with the same bytes after 5/10/20/40 ms; any other synchronous error, or busy past that budget, drops the link. A failed or missing callback is never resent: a failed aim or stats notification is skipped, whereas a failed discrete input event drops the link. Dropping the link makes the watch silent and closes and rebuilds its GATT server; Android cannot force a central-initiated link down, so the desktop ends the session itself after 3 s without any packet (`SILENCE_TIMEOUT`), and its 300 ms no-aim rule releases a held drag first. A lost drag-up while later aims keep arriving does not trip the 300 ms rule; the watch going silent is what does. The exact rules are in [protocol/SPEC.md](../protocol/SPEC.md).

The queue, the in-flight bookkeeping, retries, and the callback timeout all run on the runtime handler thread. GATT binder callbacks post there, so a completion cannot race the notify call it completes.

Pause means the watch tracks while awake: the wake lock is released, the sensor listener stays, and no aim is sent. `SensorGapMonitor` looks at gyro event timestamps. A hole over 500 ms while paused is remembered and causes Resume then Center on resume; a hole while active sends Center at once. A FIFO delivered late but complete is not a hole. Holes between 50 ms (the integrator's step clamp) and 500 ms lose a little heading without a recenter; logcat counts them as `gyroGapsOver50ms`.

## What is unit-tested

The Rust crate `wear-pointer-core` tests the integrator, the 1€ filter, projection, packet codec (including the vector in `protocol/SPEC.md`), sequence loss, relative latency, dead zone, click rewind, and the 240 Hz follower.

The Wear OS module has JVM unit tests for the same aim vector and the same aim packet bytes, for `OutboundQueue` (ordering, sequence, retries, Center boundary, plus a seeded random simulation that checks sequence continuity, the Center boundary, and in-order discrete events across 300 runs), and for `SensorGapMonitor`. Those tests do not talk to a radio or a sensor.

## Hardware validation

Physical validation was completed on a **Samsung Galaxy Watch7 (`SM-L310`, Wear OS 6 / Android 16 API 36, `BP2A.250325.020.L310XXS2BZF4`)** paired over real 2.4 GHz BLE against:

- **Omarchy / Linux (`x86_64`):** BlueZ 5.87 + Hyprland 0.56.2 (`zwlr_virtual_pointer_v1`), sustaining `100.44 pkt/s` (`0.0%` sequence loss across 58 s, `13.19 ms` mean relative latency) and real Wayland cursor centering (`hyprctl cursorpos`).
- **Windows 11 (`x86_64`):** WinRT BLE central sustaining `98.6–101.0 pkt/s` (`0.0%` sequence loss, `8.9–11.9 ms` mean relative latency, `< 30 ms` max).
- **macOS (`aarch64`):** Built and unit-tested natively with `CGEventPost` and embedded `NSBluetoothAlwaysUsageDescription` `Info.plist`.

See [stage0-hardware-validation.md](stage0-hardware-validation.md) and [acceptance-metrics.md](acceptance-metrics.md) for measured numbers and procedures.

## Device selection and pairing

Pass `--device <SUBSTRING>` to restrict connection to a peripheral whose ID, BD_ADDR, or advertised local name matches case-insensitively (e.g., `--device SM-L310`). Without `--device`, the desktop connects to the first peripheral advertising the pointer service UUID. Cryptographic pairing whitelist is still out of protocol v1; quit the receiver (`Ctrl-C`) when not in use.

## Rates

| Loop | Rate | Where |
| --- | --- | --- |
| Sensor registration | 100 Hz requested (`10_000` µs) | watch |
| Cursor follow | 240 Hz | desktop |
| Stats packet | about 1 Hz | watch → desktop |
| Debug line | 1 Hz, only with `--debug` | desktop stderr |
