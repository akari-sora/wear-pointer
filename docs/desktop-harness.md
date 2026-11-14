# Desktop harness

Runs the unchanged Wear OS debug APK against the real Linux receiver, BlueZ,
and Hyprland without a physical Watch7:

```text
Wear OS API 35 AVD (emulator-5560)
  └─ netsimd virtual controller (hci.port on host)
       └─ tools/harness/bridge.py (Bumble, controller-first)
            └─ /dev/vhci (hci0 inside Arch QEMU/KVM VM)
                 └─ BlueZ bluetoothd -> btleplug CentralLink
                      └─ wear-pointer --backend hyprland --trace
                           └─ Hyprland (zwlr_virtual_pointer_v1 on virtio-vga + seatd)
                                ├─ cursor-sampler.py (Hyprland IPC cursorpos @ 100 Hz)
                                └─ fullscreen wev (Wayland client wl_pointer log)
```

Unlike [emulator-harness.md](emulator-harness.md), the entire Linux side is the
real stack: kernel HCI (`hci_vhci`), `bluetoothd`, `btleplug`, the `wear-pointer`
binary, Hyprland's virtual pointer protocol, Hyprland's screen clamping, and a
Wayland client window receiving `wl_pointer` motion and button events.

It still does **not** replace [Stage 0](stage0-hardware-validation.md) for
Watch7 hardware questions: whether Samsung's firmware lets the Watch7 advertise
as a peripheral, real 2.4 GHz RF packet loss and latency, off-wrist sensor
throttling, and the Watch7's physical sensor FIFO size during CPU suspend.

## Why a QEMU/KVM VM is used for Hyprland

Hyprland 0.57 / Aquamarine 0.16 has no headless-only environment switch: its
DRM backend requires a KMS card and `libseat` (`seatd`), and nested Wayland
inside headless `sway`/`weston` fails (`no allocator available` on
`zwp_linux_dmabuf_v1`). A small Arch Linux KVM guest with `-device virtio-vga`,
`seatd`, `cursor.no_hardware_cursors = 1`, and `hci_vhci` runs unmodified
Hyprland and BlueZ cleanly on any headless x86_64 Linux host.

## Why `bridge.py` replaces `bumble-hci-bridge`

`bumble-hci-bridge` opens `/dev/vhci` before connecting to `netsimd`'s TCP
socket, so the Linux kernel's initial `HCI Reset` (`0x0c03`) arrives before the
controller side is wired and times out (`-110`). `tools/harness/bridge.py`
opens the `netsimd` controller transport first and attaches both packet sinks
before yielding to the event loop.

## Pieces

| File | Role |
| --- | --- |
| `tools/harness/run.sh` | Builds/stages the receiver and APK, boots the Wear AVD and Arch VM, runs the scenario, and invokes `check.py` |
| `tools/harness/make-avd.sh`, `start-emulator.sh` | Creates and boots the headless `wp_wear35` (`wearos_large_round`, API 35 `x86_64`) emulator |
| `tools/harness/bridge.py` | Bridges `netsimd` `hci.port` (`10.0.2.2:<port>`) to `/dev/vhci` inside the VM |
| `tools/harness/hyprland.lua` | Minimal Hyprland 0.57 Lua config (`1920x1080@60`, zero gaps/borders, software cursor) |
| `tools/harness/vm/` | Cloud-init provisioning (`make-vm.sh`, `user-data`), VM lifecycle (`start-vm.sh`, `vm-ssh.sh`), per-run desktop setup/teardown (`desktop-up.sh`, `desktop-down.sh`, `hypr-start.sh`), and `cursor-sampler.py` |
| `tools/harness/scenario.sh` | Drives gyro turns, Center (still and mid-turn), tap, hold-to-drag, right-click, Pause/Resume, sensor-timestamp hole injection, forced link drop mid-drag, and Stop/Start |
| `tools/harness/check.py` | Parses `trace.log`, `marks.log`, `cursor.log`, `wev.log`, `monitors.txt`, and `watch.log`; exits non-zero if any of the 10 invariant groups fail |

## Sensor hole vs. late FIFO delivery

Both cases from `SensorGapMonitor` were verified on the emulator:

1. **Late but complete FIFO (not a hole):** Stopping `android.hardware.sensors-service.multihal`
   with `SIGSTOP` for 2 s queues 200 gyro samples with contiguous 10 ms
   timestamps inside `/dev/goldfish_pipe`; `SIGCONT` delivers all 200 in ~1 ms.
   `SensorGapMonitor` checks sensor event timestamps rather than wall-clock
   arrival, sees `gyroGapsOver50ms=0`, and does **not** falsely recenter.
2. **Dropped samples (real timestamp hole):** Running
   `adb shell cmd sensorservice set-uid-state dev.wearpointer idle` for 2 s
   (and `reset-uid-state` afterwards — works without root) makes Android's
   `SensorService` drop gyro events while sensor timestamps keep advancing.
   When resumed from Pause, `PointerRuntime` logs the gap and emits `Pause(false)`
   followed immediately by an automatic `Center` packet, returning Hyprland's
   cursor to `(960, 540)`.

## Run

On an x86_64 Linux host with `/dev/kvm`, `qemu-system-x86_64`, Docker (used
once by `make-vm.sh` to build `seed.iso`), Rust 1.89+, and the Android SDK:

```bash
tools/harness/run.sh
```

Artifacts are written under `~/wp-harness/vm/share/out/<RUN_NAME>/`
(`trace.log`, `receiver.log`, `watch.log`, `cursor.log`, `wev.log`,
`monitors.txt`, `marks.log`).

## What a run verified (2026-11-14)

- **3 BLE link epochs** (`epoch=1` initial connect, `epoch=2` reconnect after
  forced mid-drag link drop, `epoch=3` reconnect after Stop -> Start),
  **2,709 notifications**, `0` decode errors, and contiguous sequence numbers
  starting at `0` on every link (`0` gaps, `0` regressions across interleaved
  `Aim`, `LeftClick`, `RightClick`, `Drag`, `Center`, `Pause`, `Stats`, and
  `Info`).
- **Center state boundary:** Pressing Center mid-turn at `43.40°` produced a
  first post-Center `Aim` at `0.29°` (no pre-Center `Aim` leaked after Center).
- **Pause & sensor-gap recenter:** Turning `0.5 rad/s` for 1 s while paused
  emitted `0` `Aim` packets and `0` cursor moves while paused, then carried
  `28.55°` into the first `Aim` after Resume; injecting a 2 s `sensorservice`
  UID-idle hole while paused emitted an automatic `Center` (`seq 1567`) on
  Resume and moved Hyprland's cursor back to `(960, 540)`.
- **Forced link drop mid-drag & BlueZ RPA rotation:** Forcing `DEBUG_DROP_LINK`
  while holding a drag released the left button on both the receiver and the
  `wev` client, ended the silent session after `3.03 s`, and reconnected
  immediately on `epoch=2` at `seq == 0` (~1.2 s after scan restart) even after
  Android rotated its BLE resolvable private address (catching and fixing a
  30 s stall where `btleplug` synthesized `DeviceDiscovered` for BlueZ's cached
  dead RPA with `RSSI == None` and left a timed-out `Connect()` in progress).
- **Hyprland compositor & Wayland client (`wev`):** `hyprctl cursorpos` matched
  `(960, 540)` after Center, clamped negative projected X (`-160.1`) to
  `(0.0, 540.0)` at the left edge, matched interior cursor coordinates within
  `0.1 px` (`522.9` -> `523.0`), and delivered all 3 left press/release pairs
  (`272`) and 1 right press/release pair (`273`) to `wev`.
