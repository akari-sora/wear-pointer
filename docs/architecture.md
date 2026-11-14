# Architecture

wear-pointer turns a Galaxy Watch7 into an air mouse for a desktop. Stage 1 targets Omarchy (Hyprland). macOS and Windows have skeleton backends that are not built on Linux.

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
    log: print only (Stage 0)
    uinput: optional relative mouse
    macos / windows: skeletons, cfg-gated
```

## Why the split looks like this

The watch integrates heading. A dropped notification then only loses a sample the next packet already includes, instead of losing rotation. Elevation is recomputed from gravity on every sample, so pitch does not drift.

The desktop does not trust Bluetooth arrival time. Each packet carries the watch clock. The 1€ filter uses the gap between those timestamps. A separate 240 Hz loop eases the cursor toward the latest filtered point, so a burst of packets does not become a burst of jumps.

Clicks carry the finger-down timestamp. The desktop keeps one second of cursor history and presses where the pointer was then.

## Bluetooth roles

`PointerTransport` on the watch and `PointerLink` on the desktop are the swap point. The desktop link emits `Connected` / `Packet` / `Disconnected` with an epoch. A new epoch resets sequence tracking, the clock anchor, filters, and cursor history so a watch sequence restart is not thousands of lost packets. Packet bytes do not change. The desktop peripheral side is still a stub: [fallback-transport.md](fallback-transport.md).

Pointer notifications are notifications, not indications. Both watch roles share `OutboundQueue`: one GATT operation in flight, FIFO, sequence written at the hand-off to the stack. A newer aim replaces only the queued aim at the tail, so a click already behind it is not reordered, and a coalesced aim never consumes a sequence. Center and Pause drop the tail aim (old base, unsent); Stop clears the queue. Busy is retried with the same bytes after 5/10/20/40 ms; any other synchronous error, or busy past that budget, drops the link. A failed or missing callback is never resent: a failed aim is skipped, anything else drops the link so the desktop releases buttons. A lost drag-up while later aims keep arriving does not trip the 300 ms no-aim release; the link drop is what releases the button. The exact rules are in [protocol/SPEC.md](../protocol/SPEC.md).

The queue, the in-flight bookkeeping, retries, and the callback timeout all run on the runtime handler thread. GATT binder callbacks post there, so a completion cannot race the notify call it completes.

Pause means the watch tracks while awake: the wake lock is released, the sensor listener stays, and no aim is sent. `SensorGapMonitor` looks at gyro event timestamps. A hole over 500 ms while paused is remembered and causes Resume then Center on resume; a hole while active sends Center at once. A FIFO delivered late but complete is not a hole. Holes between 50 ms (the integrator's step clamp) and 500 ms lose a little heading without a recenter; logcat counts them as `gyroGapsOver50ms`.

## What is unit-tested

The Rust crate `wear-pointer-core` tests the integrator, the 1€ filter, projection, packet codec (including the vector in `protocol/SPEC.md`), sequence loss, relative latency, dead zone, click rewind, and the 240 Hz follower.

The Wear OS module has JVM unit tests for the same aim vector and the same aim packet bytes, for `OutboundQueue` (ordering, sequence, retries, Center boundary, plus a seeded random simulation that checks sequence continuity, the Center boundary, and in-order discrete events across 300 runs), and for `SensorGapMonitor`. Those tests do not talk to a radio or a sensor.

## What is not tested on hardware

Nothing in this tree has run on a Galaxy Watch7, and nothing has injected a pointer into a Hyprland session. [stage0-hardware-validation.md](stage0-hardware-validation.md) is the checklist for that. [acceptance-metrics.md](acceptance-metrics.md) is the bar those logs should be compared to. The numbers there are targets, not measurements.

## Pairing

There is no whitelist. The desktop connects to the first peripheral advertising the service (or, in the fallback role, the watch connects to the first desktop advertising it). Quit the receiver when you are not using it.

## Rates

| Loop | Rate | Where |
| --- | --- | --- |
| Sensor registration | 100 Hz requested (`10_000` µs) | watch |
| Cursor follow | 240 Hz | desktop |
| Stats packet | about 1 Hz | watch → desktop |
| Debug line | 1 Hz, only with `--debug` | desktop stderr |
