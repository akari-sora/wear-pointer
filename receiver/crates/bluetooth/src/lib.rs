//! Bluetooth transport.
//!
//! [`PointerLink`] is the swap point between the two roles in
//! `docs/fallback-transport.md`. The packet bytes are identical either way.
//!
//! * [`CentralLink`] (Stage 1): this process scans, connects, and subscribes.
//!   The watch is the GATT peripheral.
//! * [`PeripheralLink`]: the desktop would advertise and accept writes, which
//!   is WatchPointer's direction. That side is not implemented yet. The watch
//!   app already speaks it. `--role peripheral` fails with an explanation
//!   instead of pretending the cursor is connected.

mod central;
mod peripheral;

pub use central::CentralLink;
pub use peripheral::PeripheralLink;

use async_trait::async_trait;
use std::sync::mpsc::Sender;
use std::time::Instant;

/// Lifecycle of one BLE session. `epoch` increments each time the central
/// connects, so a packet from a previous connection cannot be applied after
/// the watch resets its sequence number.
#[derive(Clone, Debug, PartialEq)]
pub enum LinkEvent {
    Connected {
        epoch: u64,
        device: String,
    },
    Packet {
        epoch: u64,
        /// Seconds since `clock` passed to the link. Captured when the
        /// notification is read, not when the cursor thread dequeues it.
        received_at_s: f64,
        bytes: Vec<u8>,
    },
    Disconnected {
        epoch: u64,
        reason: String,
    },
}

#[async_trait]
pub trait PointerLink: Send {
    /// Run until the link is shut down. Implementations that can reconnect
    /// loop internally. `clock` is the same `Instant` the cursor loop uses,
    /// so packet timestamps and ticks share one origin.
    async fn run(&mut self, incoming: Sender<LinkEvent>, clock: Instant) -> anyhow::Result<()>;
}
