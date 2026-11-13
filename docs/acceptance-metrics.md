# Acceptance metrics

These are the targets for a later pass. None of them have been measured. Stage 0 produces the logs. Do not treat a green `cargo test` as evidence that the cursor feels right.

Record the setup next to the numbers: watch model and Wear OS build, Omarchy/Hyprland version, distance from the adapter, and whether the watch was on the wrist.

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

- sequence loss under 2%
- `notify_drop` near 0 while connected
- `rx` within about 10% of `watch_samples`

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
