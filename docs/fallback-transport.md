# Fallback transport

Stage 1 assumes the Galaxy Watch7 can be a BLE peripheral: it advertises, it runs a GATT server, and the desktop subscribes to notifications.

That is the opposite of WatchPointer. WatchPointer's Mac advertises and the Apple Watch connects and writes. Some Wear OS radios, or some firmware builds, reject peripheral advertising (`ADVERTISE_FAILED_FEATURE_UNSUPPORTED`, or `bluetoothLeAdvertiser == null`). Stage 0 exists to find out which one the Watch7 is.

## The swap

Both sides talk in protocol v1 bytes (`protocol/SPEC.md`). The role only changes who advertises and which GATT operation carries the bytes.

| | Watch advertises (default) | Desktop advertises (fallback) |
| --- | --- | --- |
| Advertiser | watch `BluetoothLeAdvertiser` | desktop (not implemented) |
| Watch code | `WatchPeripheral` | `WatchCentral` |
| Desktop code | `CentralLink` (`btleplug`) | `PeripheralLink` (stub) |
| Pointer characteristic | notify, watch → desktop | write without response, watch → desktop |
| Control characteristic | write, desktop → watch | notify, presence only |

On the watch, Settings (tap the top status pill) has a **Link** section with `BLE app` (watch advertises), `BT HID` and `Alt` (desktop advertises); choosing one switches this. The choice is stored in shared preferences. If advertising fails as unsupported, the screen says so and offers "Use BT HID".

On the desktop:

```bash
cargo run -p wear-pointer -- --role central      # default
cargo run -p wear-pointer -- --role peripheral   # exits with an explanation
```

`PointerLink` in `receiver/crates/bluetooth` is the trait a real peripheral implementation should implement. `btleplug` is a central stack, so the desktop advertiser needs something that can register a BlueZ GATT application (the `bluer` local GATT API is the obvious candidate). Dropping that in behind `PeripheralLink` does not change the codec, the session, or the Hyprland backend.

The watch central path is implemented now: scan for the service UUID, connect, request a high connection priority, and write packets. It has not been exercised against a desktop peripheral, because that side does not exist yet.

## What to do in Stage 0

1. Run the default role first (watch advertises, desktop `--role central`).
2. If the watch UI reports that it cannot advertise, switch the watch role and stop. The desktop stub will not complete the link. That result is still a Stage 0 answer: the Watch7 cannot be the peripheral, and the next chunk of work is the BlueZ advertiser.
3. Do not block the rest of Stage 0 on the fallback. Sensor rate can still be read on the watch UI (`samples` per second) with no desktop at all.
