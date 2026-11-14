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

The watch sends at most one notification per connected central until `onNotificationSent`. Packets leave in the order they were queued (FIFO); nothing overtakes a queued packet. A newer aim sample replaces only an aim still at the tail of the queue. Clicks and drags are never replaced by aim traffic.

Delivery rules on the watch:

- `notifyCharacteristicChanged` returns busy (`ERROR_GATT_WRITE_REQUEST_BUSY`): the stack did not take the bytes. The same bytes, same `sequence`, are handed over again after 5, 10, 20, 40 ms (five attempts in total). Then the link is dropped.
- Any other synchronous error: the link is dropped at once.
- `onNotificationSent` with a failure status, or no callback within 1 s: delivery is unknown, so nothing is resent (a click could be applied twice). A failed aim is counted and the queue moves on. A failed click, drag, Center, or Pause drops the link. This matters for drag-up: while aims keep arriving, the desktop's 300 ms no-aim release never fires.
- Dropping the link means: the watch stops sending, discards the queue, calls `cancelConnection`, and closes and rebuilds its GATT server. Android cannot reliably end a link the central opened (on the API 35/36 emulators the central stays connected and only gets Service Changed; see [docs/emulator-harness.md](../docs/emulator-harness.md)). The desktop therefore does not rely on a disconnect event: the watch going silent releases a held drag after 300 ms (no aim), and **no packet of any kind for 3 s ends the desktop session**, which then scans and reconnects. While connected and awake the watch sends a stats packet every second, paused or not. If the watch CPU suspends during Pause, the desktop ends the session after 3 s and reconnects once the watch advertises again.
- The next link starts at `sequence` 0. `notifies_ok` in a stats packet is the number of notifications whose callback reported success in the window. `notifies_drop` is API rejections plus callback failures. Aims coalesced while one was in flight are counted in logcat (`coalesced=`), not as radio loss.

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
| 2 | u16 | `sequence`. Assigned when the watch first hands the packet to the stack, in that send order. Aims replaced before they are sent do not get a number. Wraps at 65535. The receiver counts `wrapping_sub(previous) - 1` as lost packets. A repeated sequence is a duplicate, not a loss of 65535. |
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

Center is a calibration boundary on the wire: no aim built before it is sent after it, and no aim built after it is sent before it. When Center is queued, the aim waiting at the tail (old base, not yet sent) is dropped. An aim already ahead of a queued click stays and goes out before Center, so the desktop still reads it against the old base. The watch also sends Center by itself when the gyro timestamps show a hole longer than 500 ms (see Pause).

### 5 Pause (9 bytes)

| Offset | Type | Field |
| --- | --- | --- |
| 8 | u8 | `1` paused, `0` resumed. |

While paused the watch does not send aim. The desktop stops moving the cursor and releases a held drag. Queuing Pause drops the aim waiting at the tail, as Center does.

Pause keeps the sensor listener and releases the wake lock, so the heading integrates **while the watch is awake**. Non-wakeup sensors can lose events while the CPU is suspended (hardware FIFO full, or no FIFO). The watch checks gyro event timestamps: a hole longer than 500 ms seen while paused makes the watch send Resume then Center on resume; a hole seen while active sends Center at once. A late but complete FIFO has no hole and does not recenter. Whether a Watch7 opens such holes is a Stage 0 measurement.

Stop on the watch unregisters sensors and closes the GATT server. Queued packets are discarded. The desktop releases a held button after 300 ms without aim and ends the session after 3 s of silence, whether or not a disconnect event arrives.

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
