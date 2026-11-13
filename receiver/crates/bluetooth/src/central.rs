//! Desktop as BLE central. Scans for the pointer service, subscribes to
//! notifications, and scans again after the watch drops.

use crate::PointerLink;
use anyhow::{anyhow, Context};
use async_trait::async_trait;
use btleplug::api::{Central, CentralEvent, Manager as _, Peripheral as _, ScanFilter};
use btleplug::platform::{Adapter, Manager};
use futures::StreamExt;
use std::sync::mpsc::Sender;
use std::time::Duration;
use uuid::Uuid;
use wear_pointer_core::{POINTER_UUID, SERVICE_UUID};

pub struct CentralLink {
    service: Uuid,
    pointer: Uuid,
}

impl CentralLink {
    pub fn new() -> anyhow::Result<Self> {
        Ok(Self {
            service: Uuid::parse_str(SERVICE_UUID)?,
            pointer: Uuid::parse_str(POINTER_UUID)?,
        })
    }
}

impl Default for CentralLink {
    fn default() -> Self {
        Self::new().expect("service UUIDs are static")
    }
}

#[async_trait]
impl PointerLink for CentralLink {
    async fn run(&mut self, incoming: Sender<Vec<u8>>) -> anyhow::Result<()> {
        let manager = Manager::new().await.context("bluetooth manager")?;
        let adapter = manager
            .adapters()
            .await
            .context("listing bluetooth adapters")?
            .into_iter()
            .next()
            .ok_or_else(|| anyhow!("no bluetooth adapter. Is BlueZ running?"))?;
        eprintln!("wear-pointer: bluetooth adapter ready, role=central");
        // TODO(pairing): pin the first watch, or load an address whitelist.
        // Stage 1 connects to any peripheral advertising the pointer service.
        loop {
            match session(&adapter, self.service, self.pointer, &incoming).await {
                Ok(()) => eprintln!("wear-pointer: watch disconnected"),
                Err(err) => eprintln!("wear-pointer: bluetooth session ended: {err:#}"),
            }
            tokio::time::sleep(Duration::from_secs(1)).await;
            eprintln!("wear-pointer: scanning again");
        }
    }
}

async fn session(
    adapter: &Adapter,
    service: Uuid,
    pointer: Uuid,
    incoming: &Sender<Vec<u8>>,
) -> anyhow::Result<()> {
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
    peripheral.connect().await.context("connect")?;
    peripheral
        .discover_services()
        .await
        .context("discover services")?;
    let characteristic = peripheral
        .characteristics()
        .into_iter()
        .find(|candidate| candidate.uuid == pointer)
        .ok_or_else(|| anyhow!("watch is missing the pointer characteristic"))?;
    peripheral
        .subscribe(&characteristic)
        .await
        .context("subscribe")?;
    eprintln!("wear-pointer: subscribed. --debug prints rate, loss, and latency once a second");

    let mut notifications = peripheral.notifications().await.context("notifications")?;
    while let Some(notification) = notifications.next().await {
        if notification.uuid != pointer {
            continue;
        }
        if incoming.send(notification.value).is_err() {
            break;
        }
    }
    peripheral.disconnect().await.ok();
    Ok(())
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
            props.services.iter().any(|uuid| *uuid == service)
                || props.local_name.as_deref() == Some("wear-pointer")
        }
        _ => false,
    }
}
