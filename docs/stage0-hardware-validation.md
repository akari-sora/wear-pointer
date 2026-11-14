# Stage 0 — Galaxy Watch7 and Omarchy

No step below has been run by the author of this tree. `cargo test` and the Wear OS unit tests do not open a radio and do not move a cursor. This checklist is what to run on the watch and on the Hyprland session, and what to write down.

Do the log backend before the Hyprland backend. A bad packet stream should not be the first thing that moves the pointer.

## Watch

1. Install Android Studio (or use the SDK) and open the `wearos/` directory.
2. On the Watch7: Settings → About → tap the software version until developer options turn on. Enable ADB debugging and wireless debugging. Confirm the watch is on Wear OS 5 (or note the actual build).
3. Pair `adb` (the phone is not required; the app is standalone):

   ```bash
   adb pair <watch-ip>:<pairing-port>
   adb connect <watch-ip>:<debug-port>
   ```

4. Build and install:

   ```bash
   cd wearos
   export ANDROID_HOME="$HOME/android-sdk"   # wherever the SDK lives
   ./gradlew :app:assembleDebug
   adb install -r app/build/outputs/apk/debug/app-debug.apk
   ```

5. Open **wear-pointer** on the watch. Grant Bluetooth (advertise, connect, scan) and notifications.
6. Leave the app on screen. For the rate test, set the screen timeout as long as it will go. Wrist detection can suspend sensors when the watch is in a hand like a remote; if the rate collapses the moment it leaves the wrist, note that and repeat with it on the wrist.
7. Read the top line. It shows link state, gyroscope samples in the last second, packets the stack accepted, and packets dropped. `adb logcat -s wear-pointer` prints the same once a second: `samples=… sent=… drop=…`.

### BLE peripheral

- Status should become **Advertising** without "cannot advertise".
- If it says the watch cannot advertise, or logcat shows `ADVERTISE_FAILED_FEATURE_UNSUPPORTED` or `no BLE advertiser`, stop. That is a Stage 0 result. Switch role only to confirm the UI path; the desktop peripheral is not implemented, so the link will not come up. See [fallback-transport.md](fallback-transport.md).
- From the Omarchy machine, while the watch says Advertising:

  ```bash
  bluetoothctl
  power on
  scan le
  ```

  Look for a device whose advertising data includes the service UUID `7a1e0001-4c3b-4e2a-9f10-6d5c4b3a2910`. The local name in the scan response is whatever the watch adapter is willing to set (`wear-pointer` if `setName` worked).

### Sensor rate

With the app in the foreground, watch the Hz figure for a minute.

- Write down the steady number.
- Target and how to read a miss: [acceptance-metrics.md](acceptance-metrics.md).
- Cover the watch or let the screen sleep and write down the new number. That is the suspend behavior, separate from BLE.

## Desktop (Omarchy / Hyprland)

Rust 1.89 or newer. BlueZ running. The user in the group that can open the adapter (`bluetooth` on most distros).

From a terminal on the Hyprland session (so `WAYLAND_DISPLAY` is set) the default backend is Hyprland. For Stage 0, force the log backend:

```bash
cd receiver
cargo run -p wear-pointer -- --debug --backend log
```

Expect:

```
wear-pointer: bluetooth adapter ready, role=central
wear-pointer: scanning for the watch
wear-pointer: connecting to …
wear-pointer: subscribed. --debug prints rate, loss, and latency once a second
wear-pointer debug: rx=…/s loss_window=… (…%) latency_ms avg=… min=… max=… cursor=… watch_samples=… notify_ok=… notify_drop=…
```

Move the watch. The log backend prints a cursor position about 10 times a second and does not call the compositor. Clicks print `left down` / `left up`. The bezel prints `scroll`.

### Notify throughput and loss

Leave it running for 60 seconds still, then 60 seconds moving. Copy two debug lines from each. Compare `rx` to `watch_samples`. Sequence gaps show up as `loss_window`. The watch's `notify_drop` is the stack rejecting a notification, or a packet built while nobody was subscribed.

### Latency

`latency_ms` is relative to the fastest packet so far (WatchPointer's estimate). Min near 0 is normal at the start. Record avg and max after a minute of motion. This is not a synchronized clock.

### Reconnect

1. Confirm the debug line is moving.
2. Toggle Bluetooth off on the watch, or walk out of range, for about 10 seconds.
3. The desktop should print `watch disconnected` or `bluetooth session ended`, then `scanning again`, then `subscribed` without a restart.
4. Toggle the watch app closed and open. Same expectation.
5. Write down whether it came back, and how long it sat on "still scanning".

### Only then, Hyprland

```bash
cargo run -p wear-pointer --release -- --debug
```

This uses `zwlr_virtual_pointer_v1`. Hyprland advertises that global by default. If the process says the compositor is missing it, stay on `--backend log` and record the error.

Point the 12 o'clock edge at the screen, tap **Center**, and see whether the cursor sits near the middle. This is the first moment the pointer should actually move. It has not been done in this environment.

Optional, if the virtual pointer global is missing and `/dev/uinput` is writable:

```bash
cargo build -p wear-pointer --features uinput
cargo run -p wear-pointer --features uinput -- --backend uinput --debug
```

That backend is relative. The first sample sets the origin and does not jump. It is a worse fit for absolute aim than Hyprland.

## Stop the watch

The on-watch **Stop** button, and the notification Stop action, unregister sensors, stop advertising, close the GATT server, release the partial wake lock, and stop the service. Pause means **tracks while awake**: the wake lock is released, sensors stay registered, and aim packets stop. If the gyro timestamps show a hole longer than half a second while paused, the watch recenters on resume, because non-wakeup sensors can drop samples while the CPU is suspended. Press Center if the cursor is still off. Closing the screen is not Stop. Record in the Stage 0 log whether pausing with the screen off produced a recenter, and the `gyroGapsOver50ms=` counts.

A drag-up that the stack refuses as busy is retried with the same bytes. Any other refusal, a failed callback, or no callback within 1 s drops the Bluetooth link instead of sending the drag again. The desktop releases the mouse button on that disconnect. The 300 ms no-aim rule does not cover a lost drag-up while aim packets keep arriving.

If Stop did not run, from a host that already has the watch on adb:

```bash
adb shell am force-stop dev.wearpointer
```

With more than one device, `adb -s <watch-serial> shell am force-stop dev.wearpointer`.

## Quit the receiver

There is no pairing whitelist (TODO). The desktop connects only when the advertisement contains the service UUID. That is not authentication. Quit the receiver when you are not using it.

`--debug` latency is the excess over the fastest packet, stamped when the notification is read. It includes host scheduling after BlueZ and is not a synchronized end-to-end delay. `aim_hz` is the watch gyroscope count divided by the stats window. `rx` counts every packet kind per second.

## What to bring back

- Watch model, Wear OS version, whether peripheral advertising worked.
- Sensor Hz with the screen on, and again with the screen off.
- One minute of `--debug` lines (still and moving): `rx`, loss, latency, `watch_samples`, `notify_ok`, `notify_drop`.
- Whether reconnect recovered without a restart.
- If you tried Hyprland: whether Center put the cursor in the middle, and any compositor error.
