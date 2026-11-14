//! Desktop-as-peripheral fallback.
//!
//! btleplug is a central stack. Advertising a GATT server needs a BlueZ
//! peripheral implementation (for example `bluer`'s local GATT API) behind
//! the same [`crate::PointerLink`] trait. The watch already connects and
//! writes packets when its role is "desktop advertises". Swapping this stub
//! for that implementation does not change the packet codec or the cursor
//! session.

use crate::PointerLink;
use async_trait::async_trait;
use std::sync::mpsc::Sender;
use std::time::Instant;

pub struct PeripheralLink;

#[async_trait]
impl PointerLink for PeripheralLink {
    async fn run(
        &mut self,
        _incoming: Sender<crate::LinkEvent>,
        _clock: Instant,
    ) -> anyhow::Result<()> {
        anyhow::bail!(
            "role=peripheral is not implemented on the desktop yet. \
             That is the fallback where this process advertises and the watch connects \
             (WatchPointer's direction). The watch app can already do that side. \
             Stage 1 uses --role central, which is the default: the watch advertises and \
             this process connects. See docs/fallback-transport.md."
        )
    }
}
