# wear-pointer BLE protocol v1

One notification or write is one packet. Packets are at most **20 bytes** so they fit in the default ATT payload (MTU 23 minus the 3-byte ATT header). Multi-byte integers are little-endian.

The byte layout is the same in both transport roles. Who advertises is not part of the packet. See [fallback-transport.md](../docs/fallback-transport.md).

Machine-readable constants: [`v1.json`](v1.json). The Rust decoder and the Kotlin encoder must match this file. A unit test on each side checks the aim vector below.

## Identifiers

| Name | UUID |
| --- | --- |
| Service | `7a1e0001-4c3b-4e2a-9f10-6d5c4b3a2910` |
| Pointer characteristic | `7a1e0002-4c3b-4e2a-9f10-6d5c4b3a2910` |
| Control characteristic | `7a1e0003-4c3b-4e2a-9f10-6d5c4b3a2910` |

These UUIDs are wear-pointer's. They are not WatchPointer's service ids.

### Role: watch is peripheral (Stage 1 default)

The watch advertises the service (low latency, connectable). The 128-bit service UUID is in the advertising data. The local name `wear-pointer` is in the scan response, because both do not reliably fit in a 31-byte legacy advert.

| Characteristic | Properties | Data |
| --- | --- | --- |
| Pointer | Read, Notify | Watch sends packets |
| Control | Write, Write Without Response | Desktop sends a command byte |

The desktop subscribes to Pointer by writing the Client Characteristic Configuration descriptor (`00002902-0000-1000-8000-00805f9b34fb`) to `0x0001` (notifications). The watch does not set `PROPERTY_INDICATE` and always passes `confirm = false` to `notifyCharacteristicChanged`. A `true` confirm bit would be an indication, which this subscription does not enable.

The watch sends at most one notification per connected central until `onNotificationSent`. A newer aim sample replaces an aim that has not started transmitting. Clicks and drags stay queued and are not replaced by aim traffic. `notifies_ok` in a stats packet is the number of notifications whose callback reported success in the window. `notifies_drop` is API rejections plus callback failures. Aims coalesced while one was in flight are counted in logcat (`coalesced=`), not as radio loss.

### Role: desktop is peripheral (fallback)

| Characteristic | Properties | Data |
| --- | --- | --- |
| Pointer | Write, Write Without Response | Watch sends the same packets |
| Control | Notify | Presence only. Payload is unused. The watch subscribes so both sides see the link drop. |

## Header

Every packet starts with 8 bytes.

| Offset | Type | Field |
| --- | --- | --- |
| 0 | u8 | `version`. Must be `1`. Receivers drop other versions. |
| 1 | u8 | `kind` |
| 2 | u16 | `sequence`. Increments by 1 for every packet the watch attempts to send on the link, including ones the stack then drops. Wraps at 65535. The receiver counts `wrapping_sub(previous) - 1` as lost packets. A repeated sequence is a duplicate, not a loss of 65535. |
| 4 | u32 | `timestamp_us`. Watch monotonic clock in microseconds, wrapping at 2^32 (about 71.6 minutes). Same time base as `SystemClock.elapsedRealtimeNanos()` divided by 1000. Aim packets use the **sensor** timestamp. Clicks and drags use the moment the finger went down (or, for a drag release, just before it lifted). |

Receivers unwrap `timestamp_us` with `wrapping_sub` and estimate one-way delay from the sample that arrived quickest (minimum of `local_time - unwrapped_watch_time`). Reported latency is relative to that fastest packet, not an absolute clock sync. The first packet of a stretch therefore shows about 0 ms.

## Kinds

Unknown kinds are ignored. The header is still enough to count the sequence, and the rest of that notification is discarded. A newer watch can add a kind without breaking an older receiver, as long as each notification stays one packet.

### 0 Aim (12 bytes)

| Offset | Type | Field |
| --- | --- | --- |
| 8 | i16 | `left_cdeg`. Hundredths of a degree the 12 o'clock edge has turned **left** of the last Center. Wrapped to about ±18000. |
| 10 | i16 | `up_cdeg`. Hundredths of a degree the 12 o'clock edge has tipped **up** from the elevation at Center. |

Sent on each gyroscope sample while the link is up and the pointer is not paused. Target rate is 100 Hz. The watch keeps integrating while paused or disconnected, and it does not send aim samples in those states.

### 1 Left click (8 bytes)

Header only. `timestamp_us` is finger-down time. The desktop clicks where the cursor was at that time, so the tap's jolt does not move the click.

### 2 Right click (8 bytes)

Same as left click, right button.

### 3 Scroll (10 bytes)

| Offset | Type | Field |
| --- | --- | --- |
| 8 | i16 | `delta_px`. Positive scrolls up (away from the user). Fractional rotary steps accumulate on the watch until they reach a whole pixel. |

### 4 Center (8 bytes)

The watch has zeroed heading and set the elevation reference to the current gravity pitch. The next aim samples are relative to that pose. The desktop recenters filters and the cursor target.

### 5 Pause (9 bytes)

| Offset | Type | Field |
| --- | --- | --- |
| 8 | u8 | `1` paused, `0` resumed. |

While paused the watch does not send aim. The desktop stops moving the cursor and releases a held drag. Aim integration continues, so resume lines up.

### 6 Info (9 to 20 bytes)

| Offset | Type | Field |
| --- | --- | --- |
| 8 | u8 | Battery percent. `255` means unknown. |
| 9 | u8 | Flags. Bit 0 = charging. |
| 10 | u16 | `sensor_hz_x10`. Measured gyroscope rate times 10. `0` if unknown. |
| 12 | utf-8 | Device name, truncated to at most 8 bytes on a character boundary. No NUL terminator. |

Sent when a central subscribes, then about once a minute.

### 7 Drag (9 bytes)

| Offset | Type | Field |
| --- | --- | --- |
| 8 | u8 | `1` button down (grab), `0` button up (drop). |

`timestamp_us` is finger-down for a grab, and slightly before lift for a drop. The watch sends a grab after the finger has been down for 350 ms. A shorter press is a left click instead.

### 8 Stats (16 bytes)

Sent about once a second while a central is subscribed. This is the Stage 0 counters from the watch's point of view.

| Offset | Type | Field |
| --- | --- | --- |
| 8 | u16 | `samples`. Gyroscope events in the window. |
| 10 | u16 | `notifies_ok`. Pointer notifications the stack accepted. |
| 12 | u16 | `notifies_drop`. Packets skipped because nobody was subscribed, or the stack rejected the notify. |
| 14 | u16 | `window_ms`. Length of the window. |

## Control commands (desktop to watch)

One byte written to the control characteristic:

| Value | Meaning |
| --- | --- |
| `0x01` | Send a stats packet as soon as practical. |

Other values are ignored.

## Worked aim packet

Sequence 1, timestamp 1000 µs, left = 12.34°, up = −0.50°.

```
01 00 01 00 e8 03 00 00 d2 04 ce ff
```

`v1.json` field `aim_vector_hex` is this packet without spaces.

## Rates

The watch reads sensors at **100 Hz** (`samplingPeriodUs = 10_000`). The desktop cursor loop runs at **240 Hz** and eases toward the latest filtered target with a 5 ms time constant. Those rates match WatchPointer. Bluetooth will not deliver a perfect 100 Hz; sequence gaps and the stats packet are how Stage 0 measures the real rate and the loss.

## Pairing whitelist

Not in v1. The desktop connects to the first peripheral that advertises the service. A whitelist is a TODO. Quit the receiver when you are not using it.
