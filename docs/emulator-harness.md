# Emulator harness

Two Android emulators on one Mac share the emulator's virtual Bluetooth
controller (netsim), so the watch app can run its real GATT peripheral and a
second emulator can connect to it as the desktop would. This checks the watch
app's Bluetooth path, queue, and packet order end to end without a Watch7.

It does **not** replace Stage 0. The radio, timing, and sensor are simulated;
a Linux desktop with BlueZ and Hyprland is not involved here (for the full
 `/dev/vhci` + BlueZ + Hyprland + `wev` harness on Linux, see
[desktop-harness.md](desktop-harness.md)). Rates and latency on an emulator mean
nothing.

## Pieces

| Piece | Where | Role |
| --- | --- | --- |
| Watch app | `wearos/app` on a Wear OS API 35 AVD | Peripheral, unchanged debug build |
| Probe | `wearos/probe` on a phone API 36 AVD | Central: scans, subscribes, logs `rx <elapsedRealtimeNanos> <hex>` under tag `wp-probe` |
| Replay | `receiver/crates/core/examples/replay.rs` | Feeds the probe log through the real desktop `PointerSession` with 240 Hz ticks and checks sequence, Center, and buttons |
| Scenario | `tools/emulator-scenario.sh` | Drives gyro (`adb emu sensor set gyroscope`) and the watch UI (`adb shell input`) |

The probe mirrors the receiver's 3 s silence rule (`SILENCE_TIMEOUT` in
`receiver/crates/bluetooth/src/central.rs`).

Debug builds of the watch app accept
`adb shell am broadcast -a dev.wearpointer.DEBUG_DROP_LINK`, which runs the
same link-drop path as a failed notify.

## Run

```sh
SDK=$HOME/Library/Android/sdk
$SDK/cmdline-tools/latest/bin/avdmanager create avd -n wearpointer_wear35 \
  -k "system-images;android-35-ext15;android-wear;arm64-v8a" -d wearos_large_round
$SDK/cmdline-tools/latest/bin/avdmanager create avd -n wearpointer_probe36 \
  -k "system-images;android-36;google_apis;arm64-v8a" -d pixel_6
$SDK/emulator/emulator -avd wearpointer_wear35 -port 5560 -no-snapshot &
$SDK/emulator/emulator -avd wearpointer_probe36 -port 5562 -no-snapshot &

cd wearos && ./gradlew :app:assembleDebug :probe:assembleDebug
adb -s emulator-5560 install -r -g app/build/outputs/apk/debug/app-debug.apk
adb -s emulator-5562 install -r -g probe/build/outputs/apk/debug/probe-debug.apk
adb -s emulator-5562 shell am start -n dev.wearpointer.probe/.ProbeActivity

cd .. && tools/emulator-scenario.sh /tmp/wp-run
cd receiver && cargo run -p wear-pointer-core --example replay < /tmp/wp-run/probe.log
```

The emulator's default pose is upright, so `up` is close to device +Y and
yaw is driven with gyro Y (`0:0.5:0`). Gyro Z barely turns the heading.

## What a run showed (2026-11-14, commit after 475d190)

- Sequence: contiguous from 0 on every link, including after a reconnect.
- Center pressed mid-turn while aims were queued: the next aim was within
  0.05° of zero. No aim from the old base after Center.
- Tap, right tap, hold-to-drag down/up, Pause/Resume all arrived once, in order.
- Turning while paused was integrated: the first aim after Resume carried it.
- A gyro hole over 500 ms (host overload) produced an automatic Center.
- **The watch cannot end a link the central opened.** `cancelConnection` fails
  (`bta_gatts_cancel_open: failed`), and closing and rebuilding the GATT server
  only triggers Service Changed; the central stays connected and subscribed to
  a dead characteristic. Killing the watch app does the same. Only the
  central's silence rule ended those links. The receiver now has that rule.
- Drag down followed by a forced link drop: the replayed desktop released the
  button. On a real receiver the 300 ms no-aim rule releases it first.

The host was heavily loaded during these runs (load average 16-19), which
starved the Wear emulator for seconds at a time. One notify callback timeout
in an earlier run came from that, not from the app.
