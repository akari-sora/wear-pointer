//! Desktop as BLE central. Scans for the pointer service, subscribes to
//! notifications, and scans again after the watch drops.

use crate::{LinkEvent, PointerLink};
use anyhow::{anyhow, Context};
use async_trait::async_trait;
use btleplug::api::{Central, CentralEvent, Manager as _, Peripheral as _, ScanFilter};
use btleplug::platform::{Adapter, Manager};
use futures::StreamExt;
use std::collections::HashMap;
use std::pin::Pin;
use std::sync::mpsc::Sender;
use std::time::{Duration, Instant};
use uuid::Uuid;
use wear_pointer_core::{POINTER_UUID, SERVICE_UUID};

const STEP_TIMEOUT: Duration = Duration::from_secs(15);
const FAILED_DEVICE_COOLDOWN: Duration = Duration::from_secs(8);

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
    /// Local process requested graceful shutdown.
    Shutdown,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PumpAdapterEvent {
    /// Already filtered to the peripheral this session owns.
    Disconnected,
    Other,
}

enum SessionOutcome {
    Disconnected(&'static str),
    Shutdown,
}

pub struct CentralLink {
    service: Uuid,
    pointer: Uuid,
    /// Substring of btleplug's adapter info (`hci1 (usb:...)`). `None` takes
    /// the first adapter.
    adapter: Option<String>,
    /// Case-insensitive substring filter against peripheral ID, BD_ADDR, or
    /// advertised local name (`--device`). `None` accepts the first matching
    /// service peripheral.
    device: Option<String>,
    /// Optional graceful-shutdown channel triggered on SIGINT / SIGTERM.
    shutdown: Option<tokio::sync::watch::Receiver<bool>>,
}

impl CentralLink {
    pub fn new() -> anyhow::Result<Self> {
        Ok(Self {
            service: Uuid::parse_str(SERVICE_UUID)?,
            pointer: Uuid::parse_str(POINTER_UUID)?,
            adapter: None,
            device: None,
            shutdown: None,
        })
    }

    /// Use the adapter whose info contains `name`, for example `hci1`.
    pub fn with_adapter(mut self, name: Option<String>) -> Self {
        self.adapter = name;
        self
    }

    /// Only connect to a peripheral whose ID, address, or local name contains
    /// `filter` (case-insensitive).
    pub fn with_device(mut self, filter: Option<String>) -> Self {
        self.device = filter.filter(|s| !s.trim().is_empty());
        self
    }

    /// Disconnect cleanly and return when `shutdown` flips to `true`.
    pub fn with_shutdown(mut self, shutdown: tokio::sync::watch::Receiver<bool>) -> Self {
        self.shutdown = Some(shutdown);
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
            None => anyhow!("no bluetooth adapter. Is Bluetooth / BlueZ running?"),
        })?;
        eprintln!("wear-pointer: bluetooth adapter ready ({info}), role=central");
        if let Some(filter) = &self.device {
            eprintln!("wear-pointer: device filter active: {filter:?}");
        }
        let mut epoch = 0u64;
        let mut failed_until: HashMap<String, Instant> = HashMap::new();
        let mut shutdown = self.shutdown.clone();
        loop {
            if is_shutdown(&shutdown) {
                return Ok(());
            }
            epoch = epoch.wrapping_add(1);
            failed_until.retain(|_, until| *until > Instant::now());
            match session(
                &adapter,
                self.service,
                self.pointer,
                self.device.as_deref(),
                &mut failed_until,
                &mut shutdown,
                epoch,
                clock,
                &incoming,
            )
            .await
            {
                Ok(SessionOutcome::Shutdown) => return Ok(()),
                Ok(SessionOutcome::Disconnected(reason)) => {
                    eprintln!("wear-pointer: watch disconnected ({reason})")
                }
                Err(err) => eprintln!("wear-pointer: bluetooth session ended: {err:#}"),
            }
            tokio::select! {
                () = wait_shutdown(&mut shutdown) => return Ok(()),
                () = tokio::time::sleep(Duration::from_secs(1)) => {}
            }
            eprintln!("wear-pointer: scanning again");
        }
    }
}

/// Read notifications until the peripheral disconnects, even if the
/// notification stream stays open across the drop (btleplug 0.9+), or until
/// nothing has arrived for `silence`.
#[cfg(test)]
pub async fn pump_link(
    notifications: Pin<Box<dyn futures::Stream<Item = Vec<u8>> + Send>>,
    events: Pin<Box<dyn futures::Stream<Item = PumpAdapterEvent> + Send>>,
    epoch: u64,
    clock: Instant,
    incoming: &Sender<LinkEvent>,
    silence: Duration,
) -> PumpStop {
    let mut shutdown = None;
    pump_link_until(
        notifications,
        events,
        epoch,
        clock,
        incoming,
        silence,
        &mut shutdown,
    )
    .await
}

pub async fn pump_link_until(
    mut notifications: Pin<Box<dyn futures::Stream<Item = Vec<u8>> + Send>>,
    mut events: Pin<Box<dyn futures::Stream<Item = PumpAdapterEvent> + Send>>,
    epoch: u64,
    clock: Instant,
    incoming: &Sender<LinkEvent>,
    silence: Duration,
    shutdown: &mut Option<tokio::sync::watch::Receiver<bool>>,
) -> PumpStop {
    let quiet = tokio::time::sleep(silence);
    tokio::pin!(quiet);
    loop {
        tokio::select! {
            () = wait_shutdown(shutdown) => return PumpStop::Shutdown,
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

fn is_shutdown(shutdown: &Option<tokio::sync::watch::Receiver<bool>>) -> bool {
    shutdown.as_ref().is_some_and(|rx| *rx.borrow())
}

async fn wait_shutdown(shutdown: &mut Option<tokio::sync::watch::Receiver<bool>>) {
    match shutdown {
        Some(rx) => {
            if *rx.borrow() {
                return;
            }
            while rx.changed().await.is_ok() {
                if *rx.borrow() {
                    return;
                }
            }
            std::future::pending::<()>().await
        }
        None => std::future::pending::<()>().await,
    }
}

async fn session(
    adapter: &Adapter,
    service: Uuid,
    pointer: Uuid,
    device_filter: Option<&str>,
    failed_until: &mut HashMap<String, Instant>,
    shutdown: &mut Option<tokio::sync::watch::Receiver<bool>>,
    epoch: u64,
    clock: Instant,
    incoming: &Sender<LinkEvent>,
) -> anyhow::Result<SessionOutcome> {
    // On Linux/BlueZ, `SetDiscoveryFilter(UUIDs=[...])` (used by btleplug when
    // `ScanFilter::services` is non-empty) drops Wear OS BLE advertisements whose
    // AD Flags byte is 0x00 on real Intel/MediaTek controllers. Unfiltered
    // discovery (`StartDiscovery`) sees the watch immediately, and
    // `advertises_service` still enforces the 128-bit service UUID before connect.
    let scan_filter = if cfg!(target_os = "macos") {
        ScanFilter {
            services: vec![service],
        }
    } else {
        ScanFilter::default()
    };
    adapter
        .start_scan(scan_filter)
        .await
        .context("start scan")?;
    eprintln!("wear-pointer: scanning for the watch");

    let found = wait_for_watch(adapter, service, device_filter, failed_until, shutdown).await;
    adapter.stop_scan().await.ok();
    let peripheral = match found? {
        Some(p) => p,
        None => return Ok(SessionOutcome::Shutdown),
    };
    if cfg!(target_os = "linux") {
        // Give BlueZ / controller HCI scan teardown a brief settle window so
        // `LE Create Connection` does not race `StopDiscovery` (`le-connection-abort-by-local`).
        tokio::time::sleep(Duration::from_millis(150)).await;
    }
    if is_shutdown(shutdown) {
        return Ok(SessionOutcome::Shutdown);
    }

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
    let characteristic = match open_characteristic(&peripheral, pointer, shutdown).await {
        Ok(Some(c)) => c,
        Ok(None) => {
            let _ = tokio::time::timeout(Duration::from_secs(2), peripheral.disconnect()).await;
            return Ok(SessionOutcome::Shutdown);
        }
        Err(err) => {
            failed_until.insert(id_text, Instant::now() + FAILED_DEVICE_COOLDOWN);
            let _ = tokio::time::timeout(Duration::from_secs(2), peripheral.disconnect()).await;
            return Err(err);
        }
    };
    // Subscribe only after the notification stream exists, so the watch's
    // first Info (sent when the CCCD is written) has a consumer.
    let notifications = match peripheral.notifications().await.context("notifications") {
        Ok(n) => n,
        Err(err) => {
            let _ = tokio::time::timeout(Duration::from_secs(2), peripheral.disconnect()).await;
            return Err(err);
        }
    };
    let adapter_events = match adapter.events().await.context("adapter events") {
        Ok(e) => e,
        Err(err) => {
            let _ = tokio::time::timeout(Duration::from_secs(2), peripheral.disconnect()).await;
            return Err(err);
        }
    };
    incoming
        .send(LinkEvent::Connected {
            epoch,
            device: format!("{name} ({id_text})"),
        })
        .ok();
    let sub_outcome = tokio::select! {
        () = wait_shutdown(shutdown) => None,
        res = tokio::time::timeout(STEP_TIMEOUT, peripheral.subscribe(&characteristic)) => Some(res),
    };
    match sub_outcome {
        None => {
            incoming
                .send(LinkEvent::Disconnected {
                    epoch,
                    reason: "shutdown".to_string(),
                })
                .ok();
            let _ = tokio::time::timeout(Duration::from_secs(2), peripheral.disconnect()).await;
            return Ok(SessionOutcome::Shutdown);
        }
        Some(Err(_)) | Some(Ok(Err(_))) => {
            let err = sub_outcome
                .unwrap()
                .context("subscribe timed out")
                .and_then(|r| r.context("subscribe"))
                .unwrap_err();
            incoming
                .send(LinkEvent::Disconnected {
                    epoch,
                    reason: "subscribe failed".to_string(),
                })
                .ok();
            let _ = tokio::time::timeout(Duration::from_secs(2), peripheral.disconnect()).await;
            return Err(err);
        }
        Some(Ok(Ok(()))) => {}
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
    let stop = pump_link_until(
        notify_bytes,
        disconnects,
        epoch,
        clock,
        incoming,
        SILENCE_TIMEOUT,
        shutdown,
    )
    .await;
    let reason = match stop {
        PumpStop::DeviceDisconnected => "device disconnected",
        PumpStop::NotificationsEnded => "notification stream ended",
        PumpStop::EventsEnded => "adapter events ended",
        PumpStop::Silent => "no packet for 3 s; watch stopped sending",
        PumpStop::Shutdown => "shutdown",
    };
    incoming
        .send(LinkEvent::Disconnected {
            epoch,
            reason: reason.to_string(),
        })
        .ok();
    let _ = tokio::time::timeout(Duration::from_secs(2), peripheral.disconnect()).await;
    if stop == PumpStop::Shutdown {
        Ok(SessionOutcome::Shutdown)
    } else {
        Ok(SessionOutcome::Disconnected(reason))
    }
}

async fn open_characteristic(
    peripheral: &btleplug::platform::Peripheral,
    pointer: Uuid,
    shutdown: &mut Option<tokio::sync::watch::Receiver<bool>>,
) -> anyhow::Result<Option<btleplug::api::Characteristic>> {
    let connect_res = tokio::select! {
        () = wait_shutdown(shutdown) => return Ok(None),
        res = tokio::time::timeout(STEP_TIMEOUT, peripheral.connect()) => res,
    };
    connect_res
        .context("connect timed out")?
        .context("connect")?;
    let discover_res = tokio::select! {
        () = wait_shutdown(shutdown) => return Ok(None),
        res = tokio::time::timeout(STEP_TIMEOUT, peripheral.discover_services()) => res,
    };
    discover_res
        .context("discover timed out")?
        .context("discover services")?;
    peripheral
        .characteristics()
        .into_iter()
        .find(|candidate| candidate.uuid == pointer)
        .map(Some)
        .ok_or_else(|| anyhow!("watch is missing the pointer characteristic"))
}

async fn wait_for_watch(
    adapter: &Adapter,
    service: Uuid,
    device_filter: Option<&str>,
    failed_until: &mut HashMap<String, Instant>,
    shutdown: &mut Option<tokio::sync::watch::Receiver<bool>>,
) -> anyhow::Result<Option<btleplug::platform::Peripheral>> {
    let mut events = adapter.events().await.context("scan events")?;
    let mut announced = 0u32;
    loop {
        tokio::select! {
            () = wait_shutdown(shutdown) => return Ok(None),
            event = events.next() => {
                let Some(event) = event else {
                    anyhow::bail!("scan event stream ended");
                };
                // A fresh `DeviceUpdated` or `ServicesAdvertisement` means a live
                // advertisement arrived over the air, clearing any cooldown.
                // `DeviceDiscovered` on BlueZ can be synthesized from D-Bus cache
                // for a rotated-away RPA whose RSSI was not cleared on connect abort.
                let (id, is_live_rx) = match event {
                    CentralEvent::ServicesAdvertisement { id, services } if services.contains(&service) => (Some(id), true),
                    CentralEvent::DeviceUpdated(id) => (Some(id), true),
                    CentralEvent::DeviceDiscovered(id) => (Some(id), false),
                    _ => (None, false),
                };
                if let Some(id) = id {
                    let id_str = id.to_string();
                    if is_live_rx {
                        failed_until.remove(&id_str);
                    } else if failed_until.get(&id_str).is_some_and(|until| *until > Instant::now()) {
                        continue;
                    }
                    let Ok(peripheral) = adapter.peripheral(&id).await else {
                        continue;
                    };
                    if advertises_service(&peripheral, service, device_filter).await {
                        return Ok(Some(peripheral));
                    }
                }
            }
            _ = tokio::time::sleep(Duration::from_secs(5)) => {
                failed_until.retain(|_, until| *until > Instant::now());
                if let Some(found) = poll_peripherals(adapter, service, device_filter, failed_until).await? {
                    return Ok(Some(found));
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
    device_filter: Option<&str>,
    failed_until: &HashMap<String, Instant>,
) -> anyhow::Result<Option<btleplug::platform::Peripheral>> {
    let now = Instant::now();
    for peripheral in adapter.peripherals().await.context("peripherals")? {
        let id_str = peripheral.id().to_string();
        if failed_until.get(&id_str).is_some_and(|until| *until > now) {
            continue;
        }
        if advertises_service(&peripheral, service, device_filter).await {
            return Ok(Some(peripheral));
        }
    }
    Ok(None)
}

pub fn matches_device_filter(
    filter: Option<&str>,
    id: &str,
    address: &str,
    local_name: Option<&str>,
) -> bool {
    let Some(needle) = filter.map(str::trim).filter(|s| !s.is_empty()) else {
        return true;
    };
    let needle = needle.to_ascii_lowercase();
    id.to_ascii_lowercase().contains(&needle)
        || (address != "00:00:00:00:00:00" && address.to_ascii_lowercase().contains(&needle))
        || local_name
            .map(|name| name.to_ascii_lowercase().contains(&needle))
            .unwrap_or(false)
}

async fn advertises_service(
    peripheral: &btleplug::platform::Peripheral,
    service: Uuid,
    device_filter: Option<&str>,
) -> bool {
    let id_str = peripheral.id().to_string();
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
                && matches_device_filter(
                    device_filter,
                    &id_str,
                    &props.address.to_string(),
                    props.local_name.as_deref(),
                )
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

    #[test]
    fn device_filter_matches_id_address_or_local_name_case_insensitively() {
        assert!(matches_device_filter(
            None,
            "hci0/dev_AA_BB",
            "AA:BB:CC:DD:EE:FF",
            None
        ));
        assert!(matches_device_filter(
            Some("aa:bb:cc"),
            "hci0/dev_AA_BB",
            "AA:BB:CC:DD:EE:FF",
            None
        ));
        assert!(matches_device_filter(
            Some("watch7"),
            "9f1c2e3a-0000",
            "00:00:00:00:00:00",
            Some("Galaxy Watch7 (L310)")
        ));
        assert!(!matches_device_filter(
            Some("00:00"),
            "9f1c2e3a-0000",
            "00:00:00:00:00:00",
            Some("wear-pointer")
        ));
        assert!(!matches_device_filter(
            Some("other-watch"),
            "hci0/dev_11_22",
            "11:22:33:44:55:66",
            Some("wear-pointer")
        ));
    }

    #[tokio::test]
    async fn shutdown_signal_ends_the_session_immediately() {
        let (_notify_tx, notify_rx) = mpsc::unbounded::<Vec<u8>>();
        let (_event_tx, event_rx) = mpsc::unbounded();
        let (incoming_tx, _incoming_rx) = std::sync::mpsc::channel();
        let (shutdown_tx, shutdown_rx) = tokio::sync::watch::channel(false);
        let mut shutdown = Some(shutdown_rx);
        let clock = Instant::now();
        tokio::spawn(async move {
            tokio::time::sleep(Duration::from_millis(40)).await;
            let _ = shutdown_tx.send(true);
        });
        let stop = tokio::time::timeout(
            Duration::from_secs(1),
            pump_link_until(
                Box::pin(notify_rx),
                Box::pin(event_rx),
                9,
                clock,
                &incoming_tx,
                Duration::from_secs(30),
                &mut shutdown,
            ),
        )
        .await
        .expect("shutdown stopped pump_link_until");
        assert_eq!(stop, PumpStop::Shutdown);
    }
}
