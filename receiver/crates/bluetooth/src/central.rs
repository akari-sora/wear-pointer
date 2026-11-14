//! Desktop as BLE central. Scans for the pointer service, subscribes to
//! notifications, and scans again after the watch drops.

use crate::{LinkEvent, PointerLink};
use anyhow::{anyhow, Context};
use async_trait::async_trait;
use btleplug::api::{Central, CentralEvent, Manager as _, Peripheral as _, ScanFilter};
use btleplug::platform::{Adapter, Manager};
use futures::StreamExt;
use std::pin::Pin;
use std::sync::mpsc::Sender;
use std::time::{Duration, Instant};
use uuid::Uuid;
use wear_pointer_core::{POINTER_UUID, SERVICE_UUID};

const STEP_TIMEOUT: Duration = Duration::from_secs(15);

/// No notification at all for this long ends the session. The watch sends a
/// stats packet every second while it is connected, paused or not. Android
/// cannot reliably drop a link the desktop opened: when the watch closes its
/// GATT server (Stop, link drop after a failed notify, app restart) the link
/// can stay up with this subscription dead, and no disconnect event arrives.
/// Seen on two API 35/36 emulators; see docs/emulator-harness.md.
pub const SILENCE_TIMEOUT: Duration = Duration::from_secs(3);

/// Why `pump_link` returned. The notification stream staying open is not a
/// reason to keep the session; `DeviceDisconnected` is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PumpStop {
    DeviceDisconnected,
    NotificationsEnded,
    EventsEnded,
    /// Nothing arrived for the silence timeout.
    Silent,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PumpAdapterEvent {
    /// Already filtered to the peripheral this session owns.
    Disconnected,
    Other,
}

pub struct CentralLink {
    service: Uuid,
    pointer: Uuid,
    /// Substring of btleplug's adapter info (`hci1 (usb:...)`). `None` takes
    /// the first adapter.
    adapter: Option<String>,
}

impl CentralLink {
    pub fn new() -> anyhow::Result<Self> {
        Ok(Self {
            service: Uuid::parse_str(SERVICE_UUID)?,
            pointer: Uuid::parse_str(POINTER_UUID)?,
            adapter: None,
        })
    }

    /// Use the adapter whose info contains `name`, for example `hci1`.
    pub fn with_adapter(mut self, name: Option<String>) -> Self {
        self.adapter = name;
        self
    }
}

impl Default for CentralLink {
    fn default() -> Self {
        Self::new().expect("service UUIDs are static")
    }
}

#[async_trait]
impl PointerLink for CentralLink {
    async fn run(&mut self, incoming: Sender<LinkEvent>, clock: Instant) -> anyhow::Result<()> {
        let manager = Manager::new().await.context("bluetooth manager")?;
        let adapters = manager.adapters().await.context("listing bluetooth adapters")?;
        let mut chosen = None;
        for candidate in adapters {
            let info = candidate.adapter_info().await.unwrap_or_default();
            let wanted = match &self.adapter {
                Some(name) => info.contains(name.as_str()),
                None => true,
            };
            if wanted {
                chosen = Some((candidate, info));
                break;
            }
        }
        let (adapter, info) = chosen.ok_or_else(|| match &self.adapter {
            Some(name) => anyhow!("no bluetooth adapter matching {name:?}"),
            None => anyhow!("no bluetooth adapter. Is BlueZ running?"),
        })?;
        eprintln!("wear-pointer: bluetooth adapter ready ({info}), role=central");
        // TODO(pairing): pin the first watch, or load an address whitelist.
        // A service-UUID match is not authentication.
        let mut epoch = 0u64;
        loop {
            epoch = epoch.wrapping_add(1);
            match session(
                &adapter,
                self.service,
                self.pointer,
                epoch,
                clock,
                &incoming,
            )
            .await
            {
                Ok(reason) => eprintln!("wear-pointer: watch disconnected ({reason})"),
                Err(err) => eprintln!("wear-pointer: bluetooth session ended: {err:#}"),
            }
            tokio::time::sleep(Duration::from_secs(1)).await;
            eprintln!("wear-pointer: scanning again");
        }
    }
}

/// Read notifications until the peripheral disconnects, even if the
/// notification stream stays open across the drop (btleplug 0.9+), or until
/// nothing has arrived for `silence`.
pub async fn pump_link(
    mut notifications: Pin<Box<dyn futures::Stream<Item = Vec<u8>> + Send>>,
    mut events: Pin<Box<dyn futures::Stream<Item = PumpAdapterEvent> + Send>>,
    epoch: u64,
    clock: Instant,
    incoming: &Sender<LinkEvent>,
    silence: Duration,
) -> PumpStop {
    let quiet = tokio::time::sleep(silence);
    tokio::pin!(quiet);
    loop {
        tokio::select! {
            () = &mut quiet => return PumpStop::Silent,
            packet = notifications.next() => {
                match packet {
                    Some(bytes) => {
                        quiet.as_mut().reset(tokio::time::Instant::now() + silence);
                        let event = LinkEvent::Packet {
                            epoch,
                            received_at_s: clock.elapsed().as_secs_f64(),
                            bytes,
                        };
                        if incoming.send(event).is_err() {
                            return PumpStop::EventsEnded;
                        }
                    }
                    None => return PumpStop::NotificationsEnded,
                }
            }
            event = events.next() => {
                match event {
                    Some(PumpAdapterEvent::Disconnected) => return PumpStop::DeviceDisconnected,
                    Some(PumpAdapterEvent::Other) => {}
                    None => return PumpStop::EventsEnded,
                }
            }
        }
    }
}

async fn session(
    adapter: &Adapter,
    service: Uuid,
    pointer: Uuid,
    epoch: u64,
    clock: Instant,
    incoming: &Sender<LinkEvent>,
) -> anyhow::Result<&'static str> {
    adapter
        .start_scan(ScanFilter {
            services: vec![service],
        })
        .await
        .context("start scan")?;
    eprintln!("wear-pointer: scanning for the watch");

    let peripheral = wait_for_watch(adapter, service).await?;
    adapter.stop_scan().await.ok();

    let name = peripheral
        .properties()
        .await
        .ok()
        .flatten()
        .and_then(|props| props.local_name)
        .unwrap_or_else(|| "watch".to_string());
    let id = peripheral.id();
    eprintln!("wear-pointer: connecting to {name} ({id})");
    let id_text = id.to_string();
    let characteristic = match open_characteristic(&peripheral, pointer).await {
        Ok(c) => c,
        Err(err) => {
            peripheral.disconnect().await.ok();
            return Err(err);
        }
    };
    // Subscribe only after the notification stream exists, so the watch's
    // first Info (sent when the CCCD is written) has a consumer.
    let notifications = match peripheral.notifications().await.context("notifications") {
        Ok(n) => n,
        Err(err) => {
            peripheral.disconnect().await.ok();
            return Err(err);
        }
    };
    let adapter_events = match adapter.events().await.context("adapter events") {
        Ok(e) => e,
        Err(err) => {
            peripheral.disconnect().await.ok();
            return Err(err);
        }
    };
    incoming
        .send(LinkEvent::Connected {
            epoch,
            device: format!("{name} ({id_text})"),
        })
        .ok();
    if let Err(err) = tokio::time::timeout(STEP_TIMEOUT, peripheral.subscribe(&characteristic))
        .await
        .context("subscribe timed out")
        .and_then(|r| r.context("subscribe"))
    {
        incoming
            .send(LinkEvent::Disconnected {
                epoch,
                reason: "subscribe failed".to_string(),
            })
            .ok();
        peripheral.disconnect().await.ok();
        return Err(err);
    }
    eprintln!("wear-pointer: subscribed. --debug prints rate, loss, and latency once a second");

    let notify_bytes = Box::pin(notifications.filter_map(move |notification| {
        let value = (notification.uuid == pointer).then_some(notification.value);
        async move { value }
    }));
    let disconnects = Box::pin(adapter_events.map(move |event| match event {
        CentralEvent::DeviceDisconnected(disconnected) if disconnected.to_string() == id_text => {
            PumpAdapterEvent::Disconnected
        }
        _ => PumpAdapterEvent::Other,
    }));
    let stop = pump_link(notify_bytes, disconnects, epoch, clock, incoming, SILENCE_TIMEOUT).await;
    let reason = match stop {
        PumpStop::DeviceDisconnected => "device disconnected",
        PumpStop::NotificationsEnded => "notification stream ended",
        PumpStop::EventsEnded => "adapter events ended",
        PumpStop::Silent => "no packet for 3 s; watch stopped sending",
    };
    incoming
        .send(LinkEvent::Disconnected {
            epoch,
            reason: reason.to_string(),
        })
        .ok();
    peripheral.disconnect().await.ok();
    Ok(reason)
}

async fn open_characteristic(
    peripheral: &btleplug::platform::Peripheral,
    pointer: Uuid,
) -> anyhow::Result<btleplug::api::Characteristic> {
    tokio::time::timeout(STEP_TIMEOUT, peripheral.connect())
        .await
        .context("connect timed out")?
        .context("connect")?;
    tokio::time::timeout(STEP_TIMEOUT, peripheral.discover_services())
        .await
        .context("discover timed out")?
        .context("discover services")?;
    peripheral
        .characteristics()
        .into_iter()
        .find(|candidate| candidate.uuid == pointer)
        .ok_or_else(|| anyhow!("watch is missing the pointer characteristic"))
}

async fn wait_for_watch(
    adapter: &Adapter,
    service: Uuid,
) -> anyhow::Result<btleplug::platform::Peripheral> {
    let mut events = adapter.events().await.context("scan events")?;
    let mut announced = 0u32;
    loop {
        tokio::select! {
            event = events.next() => {
                let Some(event) = event else {
                    anyhow::bail!("scan event stream ended");
                };
                let id = match event {
                    CentralEvent::ServicesAdvertisement { id, services } if services.contains(&service) => Some(id),
                    CentralEvent::DeviceDiscovered(id) | CentralEvent::DeviceUpdated(id) => Some(id),
                    _ => None,
                };
                if let Some(id) = id {
                    let peripheral = adapter.peripheral(&id).await?;
                    if advertises_service(&peripheral, service).await {
                        return Ok(peripheral);
                    }
                }
            }
            _ = tokio::time::sleep(Duration::from_secs(5)) => {
                if let Some(found) = poll_peripherals(adapter, service).await? {
                    return Ok(found);
                }
                announced += 1;
                if announced == 1 || announced % 6 == 0 {
                    eprintln!("wear-pointer: still scanning. Open wear-pointer on the watch and grant Bluetooth.");
                }
            }
        }
    }
}

async fn poll_peripherals(
    adapter: &Adapter,
    service: Uuid,
) -> anyhow::Result<Option<btleplug::platform::Peripheral>> {
    for peripheral in adapter.peripherals().await.context("peripherals")? {
        if advertises_service(&peripheral, service).await {
            return Ok(Some(peripheral));
        }
    }
    Ok(None)
}

async fn advertises_service(peripheral: &btleplug::platform::Peripheral, service: Uuid) -> bool {
    match peripheral.properties().await {
        Ok(Some(props)) => {
            // On BlueZ, btleplug's `adapter.events()` synthesizes `DeviceDiscovered`
            // for every cached D-Bus device, including dead RPAs from an earlier
            // watch session whose `RSSI` was cleared on disconnect. Requiring a
            // live `RSSI` skips dead RPAs instead of stalling `connect()` on them.
            let live = if cfg!(target_os = "linux") {
                props.rssi.is_some()
            } else {
                true
            };
            live && props.services.iter().any(|uuid| *uuid == service)
        }
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use futures::channel::mpsc;

    #[tokio::test]
    async fn device_disconnected_ends_the_session_while_notifications_stay_open() {
        let (notify_tx, notify_rx) = mpsc::unbounded();
        let (event_tx, event_rx) = mpsc::unbounded();
        let (incoming_tx, incoming_rx) = std::sync::mpsc::channel();
        let clock = Instant::now();
        let pump = pump_link(
            Box::pin(notify_rx),
            Box::pin(event_rx),
            7,
            clock,
            &incoming_tx,
            Duration::from_secs(30),
        );
        event_tx
            .unbounded_send(PumpAdapterEvent::Disconnected)
            .expect("event stream open");
        // Leave notify_tx alive so the notification stream does not end.
        let stop = tokio::time::timeout(Duration::from_secs(2), pump)
            .await
            .expect("pump waited for the notification stream to end");
        assert_eq!(stop, PumpStop::DeviceDisconnected);
        let _keep_notifications_open = notify_tx;
        assert!(incoming_rx.try_recv().is_err());
    }

    /// The watch closed its GATT server but the link stayed up: the stream
    /// stays open, no disconnect event comes, and nothing arrives.
    #[tokio::test]
    async fn a_silent_link_ends_the_session() {
        let (notify_tx, notify_rx) = mpsc::unbounded::<Vec<u8>>();
        let (_event_tx, event_rx) = mpsc::unbounded();
        let (incoming_tx, incoming_rx) = std::sync::mpsc::channel();
        let clock = Instant::now();
        notify_tx.unbounded_send(vec![1, 8]).expect("open");
        let stop = tokio::time::timeout(
            Duration::from_secs(2),
            pump_link(
                Box::pin(notify_rx),
                Box::pin(event_rx),
                3,
                clock,
                &incoming_tx,
                Duration::from_millis(150),
            ),
        )
        .await
        .expect("silence watchdog fired");
        assert_eq!(stop, PumpStop::Silent);
        assert!(matches!(incoming_rx.try_recv(), Ok(LinkEvent::Packet { epoch: 3, .. })));
        let _keep_open = notify_tx;
    }

    /// Packets keep resetting the watchdog.
    #[tokio::test]
    async fn steady_packets_keep_the_session() {
        let (notify_tx, notify_rx) = mpsc::unbounded::<Vec<u8>>();
        let (_event_tx, event_rx) = mpsc::unbounded();
        let (incoming_tx, _incoming_rx) = std::sync::mpsc::channel();
        let clock = Instant::now();
        let feeder = tokio::spawn(async move {
            for _ in 0..10 {
                tokio::time::sleep(Duration::from_millis(50)).await;
                if notify_tx.unbounded_send(vec![1, 8]).is_err() {
                    break;
                }
            }
            notify_tx
        });
        let started = Instant::now();
        let stop = pump_link(
            Box::pin(notify_rx),
            Box::pin(event_rx),
            1,
            clock,
            &incoming_tx,
            Duration::from_millis(150),
        )
        .await;
        assert_eq!(stop, PumpStop::Silent);
        // 10 packets 50 ms apart, then 150 ms of quiet.
        assert!(started.elapsed() >= Duration::from_millis(600));
        drop(feeder);
    }
}
