//! Devices waiting for adoption.
//!
//! The Integration device list holds adopted devices only; the ones a site
//! can adopt come from a separate endpoint. They join the device snapshot
//! so `devices list` and the TUI show them, as a Session-only refresh
//! already does from `stat/device`.

use crate::integration_types::PendingDeviceResponse;
use crate::model::Device;
use crate::store::DataStore;

/// Whether `device` is a row this module added, not an adopted device.
fn is_pending_row(device: &Device) -> bool {
    device
        .id
        .as_legacy()
        .is_some_and(|id| id.starts_with("pending:"))
}

/// The pending devices to add to `adopted`. `fetched` is `None` when the
/// fetch failed, which keeps the rows the store already has so one error
/// does not make a waiting device vanish. A device that shows up in both
/// lists while it is being adopted is listed once, as adopted.
pub(super) fn devices(
    fetched: Option<Vec<PendingDeviceResponse>>,
    site_id: &uuid::Uuid,
    adopted: &[Device],
    store: &DataStore,
) -> Vec<Device> {
    let rows: Vec<Device> = match fetched {
        Some(rows) => rows
            .iter()
            .filter_map(|row| crate::convert::pending_device_from_integration(row, site_id))
            .collect(),
        None => store
            .devices_snapshot()
            .iter()
            .filter(|device| is_pending_row(device))
            .map(|device| Device::clone(device))
            .collect(),
    };
    rows.into_iter()
        .filter(|row| !adopted.iter().any(|device| device.mac == row.mac))
        .collect()
}
