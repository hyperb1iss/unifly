use tokio::sync::mpsc;

use crate::command::CommandEnvelope;
use crate::model::MacAddress;
use crate::store::DataStore;
use crate::websocket::{DeviceSync, NumOrStr};

use super::Controller;
use super::support::parse_session_device_wan_ipv6;

/// Apply a `device:sync` WebSocket message to the DataStore.
///
/// Merges CPU, memory, load averages, uptime, client count, and uplink
/// bandwidth into the existing device (looked up by MAC) without clobbering
/// Integration API fields. Callers batch several of these under one
/// [`EntityCollection::begin_batch`](crate::store::EntityCollection::begin_batch)
/// so a burst of frames publishes a single snapshot.
#[allow(clippy::cast_precision_loss)]
pub(super) fn apply_device_sync(store: &DataStore, data: &DeviceSync) {
    let Some(mac_str) = data.mac.as_deref() else {
        return;
    };
    let mac = MacAddress::new(mac_str);

    let sys = data.sys_stats.as_ref();
    let cpu = sys.and_then(|s| s.cpu.as_ref()).and_then(NumOrStr::as_f64);
    #[allow(clippy::as_conversions, clippy::cast_precision_loss)]
    let mem_pct = match (
        sys.and_then(|s| s.mem_used.as_ref())
            .and_then(NumOrStr::as_i64),
        sys.and_then(|s| s.mem_total.as_ref())
            .and_then(NumOrStr::as_i64),
    ) {
        (Some(used), Some(total)) if total > 0 => Some((used as f64 / total as f64) * 100.0),
        _ => None,
    };
    let load_averages: [Option<f64>; 3] = [
        sys.and_then(|s| s.loadavg_1.as_ref())
            .and_then(NumOrStr::as_f64),
        sys.and_then(|s| s.loadavg_5.as_ref())
            .and_then(NumOrStr::as_f64),
        sys.and_then(|s| s.loadavg_15.as_ref())
            .and_then(NumOrStr::as_f64),
    ];

    let uplink = data.uplink.as_ref();
    let tx_bps = uplink
        .and_then(|u| u.tx_bytes_r.as_ref())
        .and_then(NumOrStr::as_u64)
        .or_else(|| data.tx_bytes_r.as_ref().and_then(NumOrStr::as_u64));
    let rx_bps = uplink
        .and_then(|u| u.rx_bytes_r.as_ref())
        .and_then(NumOrStr::as_u64)
        .or_else(|| data.rx_bytes_r.as_ref().and_then(NumOrStr::as_u64));

    let bandwidth = match (tx_bps, rx_bps) {
        (Some(tx), Some(rx)) if tx > 0 || rx > 0 => Some(crate::model::common::Bandwidth {
            tx_bytes_per_sec: tx,
            rx_bytes_per_sec: rx,
        }),
        _ => None,
    };

    let uptime: Option<u64> = data
        .underscore_uptime
        .as_ref()
        .or(data.uptime.as_ref())
        .and_then(NumOrStr::as_i64)
        .and_then(|value| value.try_into().ok());
    let client_count = data
        .num_sta
        .as_ref()
        .and_then(NumOrStr::as_u64)
        .map(|num_sta| u32::try_from(num_sta).unwrap_or(u32::MAX));
    let wan_ipv6 = parse_session_device_wan_ipv6(
        data.wan1.as_ref().and_then(|wan| wan.ipv6.as_ref()),
        data.ipv6.as_ref(),
    );

    // Merge under the key's lock: a full refresh replacing or removing this
    // device on another worker must not be undone by a stale copy.
    store.devices.update_existing(mac.as_str(), |existing| {
        let mut device = existing.clone();
        if bandwidth.is_some() {
            device.stats.uplink_bandwidth = bandwidth;
        }
        if let Some(cpu) = cpu {
            device.stats.cpu_utilization_pct = Some(cpu);
        }
        if let Some(mem_pct) = mem_pct {
            device.stats.memory_utilization_pct = Some(mem_pct);
        }
        if let Some(load) = load_averages[0] {
            device.stats.load_average_1m = Some(load);
        }
        if let Some(load) = load_averages[1] {
            device.stats.load_average_5m = Some(load);
        }
        if let Some(load) = load_averages[2] {
            device.stats.load_average_15m = Some(load);
        }
        if uptime.is_some() {
            device.stats.uptime_secs = uptime;
        }
        if client_count.is_some() {
            device.client_count = client_count;
        }
        if wan_ipv6.is_some() {
            device.wan_ipv6.clone_from(&wan_ipv6);
        }
        device
    });
}

/// Process commands from the mpsc channel, routing each to the
/// appropriate Session API call.
pub(super) async fn command_processor_task(
    controller: Controller,
    mut rx: mpsc::Receiver<CommandEnvelope>,
) {
    let cancel = controller.inner.cancel_child.lock().await.clone();

    loop {
        tokio::select! {
            biased;
            () = cancel.cancelled() => break,
            envelope = rx.recv() => {
                let Some(envelope) = envelope else { break };
                let result = super::commands::route_command(&controller, envelope.command).await;
                let _ = envelope.response_tx.send(result);
            }
        }
    }
}
