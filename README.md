# wear-pointer

**Turn a round Wear OS smartwatch into an eyes-free spatial air mouse — 100 Hz wrist orientation over 20-byte BLE GATT notifications to a 240 Hz 1€-filtered desktop cursor on Linux (Hyprland / uinput), macOS, and Windows.**

[![Releases](https://img.shields.io/badge/releases-GitLab-fc6d26)](https://gitlab.com/aa22396584/wear-pointer/-/releases) [![GitLab CI](https://gitlab.com/aa22396584/wear-pointer/badges/main/pipeline.svg)](https://gitlab.com/aa22396584/wear-pointer/-/pipelines) [![License: MIT](https://img.shields.io/badge/license-MIT-blue.svg)](LICENSE)

> Canonical repository and issues: https://gitlab.com/aa22396584/wear-pointer  
> Releases and downloads: https://gitlab.com/aa22396584/wear-pointer/-/releases

[![wear-pointer synchronized Wear OS + Hyprland 0.57 live demo](docs/assets/demo-preview.gif)](https://gitlab.com/aa22396584/wear-pointer/-/raw/main/docs/assets/demo-16x9-en.mp4)

> **Full HD Demo Videos (1080p 30fps):** [16:9 繁體中文](https://gitlab.com/aa22396584/wear-pointer/-/raw/main/docs/assets/demo-16x9-zh.mp4) · [16:9 English](https://gitlab.com/aa22396584/wear-pointer/-/raw/main/docs/assets/demo-16x9-en.mp4) · [9:16 繁體中文 (Reels/Threads)](https://gitlab.com/aa22396584/wear-pointer/-/raw/main/docs/assets/demo-9x16-zh.mp4) · [9:16 English (Shorts/Reels)](https://gitlab.com/aa22396584/wear-pointer/-/raw/main/docs/assets/demo-9x16-en.mp4)

![wear-pointer circular Wear OS spatial pointer showcase](docs/assets/showcase.png)

Validated on a physical **Samsung Galaxy Watch7 (`SM-L310`, Wear OS 6 / Android 16)** with a cross-platform Rust desktop receiver for **Omarchy / Hyprland** (`zwlr_virtual_pointer_v1` + optional Linux `uinput`), **macOS** (`CGEventPost` + CoreBluetooth), and **Windows 11** (`SendInput` + WinRT BLE).

The aiming model is ported from [WatchPointer](https://github.com/umarz317/WatchPointer) by Umar Afzal (MIT): gravity for up/down, integrated gyro for left/right, a [1€ filter](https://gery.casiez.net/1euro/) driven by the watch clock, and a desktop cursor loop at 240 Hz. The watch samples at 100 Hz and requests `CONNECTION_PRIORITY_HIGH` (`7.5–11.25 ms` BLE interval) on subscribe. Copyright for WatchPointer is in [NOTICE](NOTICE). The packet layout and default Bluetooth direction are wear-pointer's: the watch advertises, the desktop connects.

Physical hardware measurements across all three desktop OSes (`100.4–102.6 pkt/s`, `0.0%` sequence loss, `10.6–13.2 ms` mean relative latency on Omarchy BlueZ, `6.6–10.6 ms` on Windows 11 WinRT, and `7.1–11.9 ms` on macOS CoreBluetooth) are documented in [docs/acceptance-metrics.md](docs/acceptance-metrics.md), [docs/evidence/README.md](docs/evidence/README.md), and [docs/stage0-hardware-validation.md](docs/stage0-hardware-validation.md), alongside automated CI harnesses in [docs/desktop-harness.md](docs/desktop-harness.md). See [Current status](#current-status).

## Layout

| Path | What |
| --- | --- |
| `wearos/` | Kotlin / Jetpack Compose for Wear OS. Sensors, GATT server, tap / hold / right-click / pause / center, rotary scroll. |
| `protocol/` | Protocol v1: UUIDs, packet bytes, the aim test vector. |
| `receiver/` | Rust workspace. `core` (tested motion), `bluetooth` (btleplug central), `platform` (Hyprland, log, optional uinput, macOS/Windows skeletons). |
| `tools/` | End-to-end Linux/Hyprland BLE harness (`tools/harness/`) and Remotion promo video project (`tools/promo/`). |
| `docs/` | Architecture, Stage 0 checklist, acceptance targets, BLE fallback, showcase media (`docs/assets/`). |

## Current status

| Piece | Checked here | Remaining / Notes |
| --- | --- | --- |
| Rust `cargo test` (motion, ergonomic `1600 DPI` & live `KIND_SENSITIVITY`, off-screen clamp, packets, loss, latency, 240 Hz follow, BLE shutdown & device filter) | yes, 45 tests on macOS / 43 on Linux | — |
| Rust release builds (`x86_64-unknown-linux-gnu`, `aarch64-apple-darwin` `WearPointer.app`, `x86_64-pc-windows-msvc`) | yes, all 3 desktop platforms built & shipped with native app icons | — |
| Wear OS `assembleRelease` / `bundleRelease` and JVM unit tests | yes, JDK 21, `compileSdk = 35`, `targetSdk = 35`, `v0.2.2` (`versionCode = 4`) installed & verified on physical `SM-L310` | — |
| Physical Watch7 (`SM-L310`) BLE peripheral, 100 Hz sensor rate, live DPI sync (`400–3200 DPI`), mid-drag drop & reconnect | yes: `~102 pkt/s`, `0.0%` loss across Omarchy BlueZ, Windows 11 WinRT, and macOS CoreBluetooth ([metrics](docs/acceptance-metrics.md), [evidence](docs/evidence/README.md)) | 10-minute stationary yaw drift characterization optional |
| Hyprland cursor movement | yes, physical `SM-L310` → Omarchy Hyprland 0.56.2 (`zwlr_virtual_pointer_v1`, `hyprctl cursorpos`) + QEMU harness | multi-monitor output selection uses first `wl_output` |
| macOS `CGEventPost`, Windows `SendInput` | yes, live physical `SM-L310` verified end-to-end on macOS (`WearPointer.app` CoreBluetooth + `CGEventPost`) and Windows 11 (`WinRT` + `SendInput`) ([evidence](docs/evidence/README.md)) | macOS prompts once for Bluetooth & Accessibility on `WearPointer.app` |
| Desktop-as-peripheral fallback | watch side is written; desktop `--role peripheral` exits with an error | [fallback](docs/fallback-transport.md) |
| Device selection / pairing | `--device <MATCH>` filters by ID, BD_ADDR, or local name (e.g. `--device SM-L310`) | Cryptographic whitelist out of protocol v1; quit receiver (`Ctrl-C`) when idle |

`--debug` latency is relative to the fastest packet, stamped when the notification is read. It is not a pure radio delay. `aim_hz` uses the watch stats window; `rx` counts every packet kind. Numbers in [docs/acceptance-metrics.md](docs/acceptance-metrics.md) are targets, not measurements.

CI jobs require both tags `r9700x` and `docker` (runner 57287642). `r9700x-docker` is that runner's description, not a tag. Shared runners do not have this pair. The `yes | sdkmanager` pipe is not used; it exits 141 under `pipefail` even when sdkmanager succeeds. CI covers unit tests and builds only, not Watch7 or Hyprland behaviour. [docs/emulator-harness.md](docs/emulator-harness.md) runs the watch app against a second emulator on macOS; [docs/desktop-harness.md](docs/desktop-harness.md) runs the watch emulator over `/dev/vhci` into real BlueZ, `wear-pointer`, Hyprland, and `wev`. Neither replaces Stage 0 on a physical Watch7.

Stop on the watch is the Stop button. Pause tracks while awake (wake lock off, sensors still registered). A gyro timestamp hole over 500 ms, while paused or active, recenters. If Stop did not run: `adb shell am force-stop dev.wearpointer` (debug) or `adb shell am force-stop dev.wearpointer.app` (release; add `-s <serial>` when several devices are attached).

## Install from release

Signed Wear OS APKs, Linux `x86_64`, macOS `aarch64`, and Windows `x86_64` receiver archives, plus `SHA256SUMS.txt` are published on [GitLab Releases](https://gitlab.com/aa22396584/wear-pointer/-/releases):

```bash
# Verify checksums after downloading from https://gitlab.com/aa22396584/wear-pointer/-/releases
sha256sum -c SHA256SUMS.txt

# 1. Install the signed Wear OS APK onto your watch over ADB
adb install -r wear-pointer-v0.2.2-wearos.apk

# 2. Extract and run the desktop receiver (Linux / macOS / Windows)
tar -xzf wear-pointer-v0.2.2-x86_64-linux.tar.gz
./wear-pointer-v0.2.2-x86_64-linux/wear-pointer --debug --backend log
./wear-pointer-v0.2.2-x86_64-linux/wear-pointer --debug

# On macOS (launches signed WearPointer.app bundle for CoreBluetooth + Accessibility TCC):
tar -xzf wear-pointer-v0.2.2-aarch64-macos.tar.gz
./wear-pointer-v0.2.2-aarch64-macos/wear-pointer --debug
```

## Build the receiver

Rust 1.89 or newer (this checkout was tested with 1.100). BlueZ headers are a build dependency on Linux (`libdbus-1-dev`, `pkg-config`).

```bash
cd receiver
cargo test
cargo build -p wear-pointer --release
```

Optional relative uinput backend (also needs permission on `/dev/uinput` at run time):

```bash
cargo build -p wear-pointer --features uinput
```

## Run the receiver

Stage 0, no cursor movement, one debug line per second:

```bash
cd receiver
cargo run -p wear-pointer -- --debug --backend log
```

On a Hyprland session (`WAYLAND_DISPLAY` set), the default backend is Hyprland. Pass `--backend log` until Stage 0 looks sane.

```bash
cargo run -p wear-pointer --release -- --debug
```

```text
wear-pointer [--debug] [--role central|peripheral]
             [--backend hyprland|macos|windows|log|uinput]
             [--dpi 1600] [--speed 1.6] [--steady 0.8] [--quick 0.01]
             [--dead-zone 0.0] [--screen 1920x1080]
             [--adapter hci1] [--device SM-L310] [--trace FILE] [--replay FILE]
```

`--dpi` sets sensitivity in DPI (default **`1600 DPI`**, ergonomic `±15.6°` horizontal / `±9.2°` vertical forearm pivot; `1000 DPI` = `±25°`). Sensitivity can also be adjusted live from the watch (`400–3200 DPI` in `200 DPI` steps via `−`/`+` buttons, rotary bezel tuning, or tap-to-reset `1600 DPI`), synced immediately over BLE (`KIND_SENSITIVITY`). `--speed` (`dpi / 1000`) matches WatchPointer: higher reaches the edge of the screen with less turn. `--dead-zone` is degrees of aim to ignore. Leave it at 0 unless a log shows a reason; WatchPointer removed a dead zone because the cursor walked. `--steady` and `--quick` are the 1€ filter's still cutoff (Hz) and speed coefficient.

`--role peripheral` is the documented fallback (desktop advertises). It is not implemented and exits with an explanation.

## Build the watch app

Wear OS 4 / 5, min SDK 33. Open `wearos/` in Android Studio, or:

```bash
cd wearos
export ANDROID_HOME="$HOME/android-sdk"
./gradlew :app:assembleDebug :app:testDebugUnitTest
adb install -r app/build/outputs/apk/debug/app-debug.apk
```

No signing keys are in the repo. Release APKs on GitLab Releases are signed with the maintainer upload keystore kept outside the tree (`wearos/key.properties`, gitignored); source debug builds use the local debug keystore that Android Gradle Plugin creates automatically.

On the watch: open the app, grant Bluetooth, leave it in the foreground. Point the 12 o'clock chevron (`▲`) at the screen and tap **Center** so both the desktop cursor and the spatial reticle puck sit in the middle. Tap the central pad to left-click, hold about 350 ms until the emerald ring locks to drag, use **Right** / **Center** / **Pause** in the chord-safe dock, **Stop** / **Start** at 6 o'clock, and rotate the bezel or crown to scroll. Tap the top status capsule (e.g. `LINKED · 100 HZ`) to open live sensor/GATT telemetry and the BLE role switch.

If the watch reports that it cannot advertise, read [docs/fallback-transport.md](docs/fallback-transport.md). The watch can switch to "desktop advertises". The desktop half of that role is still a stub.

Full hardware steps: [docs/stage0-hardware-validation.md](docs/stage0-hardware-validation.md).

## Protocol

[protocol/SPEC.md](protocol/SPEC.md). Service UUID `7a1e0001-4c3b-4e2a-9f10-6d5c4b3a2910`. Packets are at most 20 bytes and carry a sequence number and a microsecond watch timestamp so the receiver can count loss and relative latency.

## License

MIT, [LICENSE](LICENSE). WatchPointer's MIT notice is in [NOTICE](NOTICE).
