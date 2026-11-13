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

#[async_trait]
pub trait PointerLink: Send {
    /// Run until the link is shut down. Implementations that can reconnect
    /// loop internally. Packets are the raw protocol-v1 notifications or
    /// writes, one datagram each.
    async fn run(&mut self, incoming: Sender<Vec<u8>>) -> anyhow::Result<()>;
}
