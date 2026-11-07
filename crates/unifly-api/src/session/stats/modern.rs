use std::collections::{BTreeMap, BTreeSet};

use futures_util::future::try_join_all;
use serde_json::{Map, Value, json};

use super::wire::{Series, Window, invalid};
use super::{Report, ReportQuery, wire};
use crate::model::MacAddress;
use crate::{Error, SessionClient};

#[derive(Clone, Copy, PartialEq, Eq)]
enum Schema {
    Wan,
    Counts,
    Client,
    SystemDevice,
    CountsDevice,
    PortDevice,
    WirelessDevice,
    WanDevice,
}

impl Schema {
    fn name(self) -> &'static str {
        match self {
            Self::Wan => "wanBySite",
            Self::Counts => "clientCountBySite",
            Self::Client => "activityBySiteByClient",
            Self::SystemDevice => "systemBySiteByDevice",
            Self::CountsDevice => "clientCountBySiteByDevice",
            Self::PortDevice => "portTrafficBySiteByDevice",
            Self::WirelessDevice => "wirelessBySiteByDevice",
            Self::WanDevice => "wanBySiteByDevice",
        }
    }

    fn mapping(self) -> &'static [(&'static str, &'static str)] {
        match self {
            Self::Wan | Self::WanDevice => &[
                ("rxBytes", "rx_bytes"),
                ("txBytes", "tx_bytes"),
                ("avgLatencyMs", "latency"),
                ("avgAvailabilityPct", "availability"),
            ],
            Self::Counts | Self::CountsDevice => &[
                ("distinctClientCount", "num_sta"),
                ("distinctUserClientCount", "num_user"),
                ("distinctGuestClientCount", "num_guest"),
            ],
            Self::Client => &[
                ("rxBytes", "tx_bytes"),
                ("txBytes", "rx_bytes"),
                ("rxPackets", "tx_packets"),
                ("txPackets", "rx_packets"),
                ("avgSignalDbm", "signal"),
                ("avgSatisfactionScore", "satisfaction"),
                ("wifiRxRetryPackets", "tx_retries"),
                ("wifiRxAttemptPackets", "wifi_tx_attempts"),
                ("maxRxRateTierKbps", "tx_rate"),
            ],
            Self::SystemDevice => &[
                ("avgCpuUtilizationPct", "cpu"),
                ("avgMemoryUtilizationPct", "mem"),
                ("uptimeS", "uptime"),
            ],
            Self::PortDevice => &[("rxBytes", "rx_bytes"), ("txBytes", "tx_bytes")],
            Self::WirelessDevice => &[
                ("rxBytes", "rx_bytes"),
                ("txBytes", "tx_bytes"),
                ("distinctClientCount", "num_sta"),
                ("avgClientSatisfactionScore", "satisfaction"),
                ("wifiTxRetryPackets", "tx_retries"),
                ("wifiTxAttemptPackets", "wifi_tx_attempts"),
                ("wifiTxDroppedPackets", "wifi_tx_dropped"),
                ("avgTotalCuPct", "cu_total"),
                ("avgRxSelfCuPct", "cu_self_rx"),
                ("avgTxSelfCuPct", "cu_self_tx"),
                ("avgInterfCuPct", "cu_interf"),
            ],
        }
    }
}

type Rows = BTreeMap<(i64, Option<String>, Option<String>), Map<String, Value>>;

pub(super) async fn report(
    client: &SessionClient,
    report: Report,
    query: &ReportQuery<'_>,
) -> Result<Vec<Value>, Error> {
    let window = Window::from_query(query)?;
    let macs = query
        .macs
        .map(|macs| {
            macs.iter()
                .map(|mac| normalized_mac(mac))
                .collect::<Result<BTreeSet<_>, _>>()
        })
        .transpose()?;
    if macs.as_ref().is_some_and(BTreeSet::is_empty) {
        return Ok(Vec::new());
    }
    let sites = client.list_sites().await?;
    let site = sites
        .iter()
        .find(|site| site.name == client.site() || site.id == client.site())
        .ok_or_else(|| invalid(format!("Statistics site '{}' was not found", client.site())))?;
    let site_id = uuid::Uuid::parse_str(&site.id)
        .map_err(|_| invalid("Modern statistics site ID is not a UUID"))?
        .to_string();
    let schemas: &[Schema] = match report {
        Report::Site => &[Schema::Counts, Schema::Wan],
        Report::Gateway => &[Schema::Wan],
        Report::Client => &[Schema::Client],
        Report::Device => &[
            Schema::SystemDevice,
            Schema::CountsDevice,
            Schema::PortDevice,
            Schema::WirelessDevice,
            Schema::WanDevice,
        ],
    };
    let responses = try_join_all(
        schemas
            .iter()
            .map(|schema| wire::fetch(client, schema.name(), window, &site_id)),
    )
    .await?;
    let families = traffic_families(schemas, &responses, &site_id)?;
    let mut rows = Rows::new();
    for (schema, series) in schemas.iter().zip(responses) {
        for series in series {
            if series.site_id != site_id {
                continue;
            }
            if report == Report::Device && !matches_family(*schema, &series, &families)? {
                continue;
            }
            let entity = entity(report, &series)?;
            if let Some(macs) = &macs
                && !entity.as_ref().is_some_and(|entity| macs.contains(entity))
            {
                continue;
            }
            merge(&mut rows, *schema, series, entity.as_deref(), window)?;
        }
    }
    rows.into_values()
        .map(|mut row| {
            finish(&mut row, report)?;
            row.retain(|key, _| {
                matches!(key.as_str(), "duration" | "mac" | "unit_id")
                    || query.attrs.map_or_else(
                        || report.attributes().contains(&key.as_str()),
                        |attrs| attrs.contains(key),
                    )
            });
            Ok(Value::Object(row))
        })
        .collect()
}

fn normalized_mac(mac: &str) -> Result<String, Error> {
    mac.parse::<MacAddress>()
        .map(|mac| mac.to_string())
        .map_err(|error| invalid(format!("Invalid statistics MAC address '{mac}': {error}")))
}

fn entity(report: Report, series: &Series) -> Result<Option<String>, Error> {
    let value = match report {
        Report::Device => series.device_mac.as_deref(),
        Report::Client => series.client_id.as_deref(),
        _ => return Ok(None),
    };
    value
        .ok_or_else(|| invalid("Statistics response is missing its device or client identity"))
        .and_then(normalized_mac)
        .map(Some)
}

/// A gateway's WAN and an AP's wireless traffic must not be added to port totals.
fn traffic_families(
    schemas: &[Schema],
    responses: &[Vec<Series>],
    site_id: &str,
) -> Result<BTreeMap<String, Schema>, Error> {
    let mut families = BTreeMap::new();
    for (schema, series) in schemas.iter().zip(responses) {
        if !matches!(
            schema,
            Schema::PortDevice | Schema::WirelessDevice | Schema::WanDevice
        ) {
            continue;
        }
        for series in series
            .iter()
            .filter(|series| series.site_id == site_id && !series.points.is_empty())
        {
            let mac = series
                .device_mac
                .as_deref()
                .ok_or_else(|| invalid("Device statistics response is missing its MAC"))?;
            families.insert(normalized_mac(mac)?, *schema);
        }
    }
    Ok(families)
}

fn matches_family(
    schema: Schema,
    series: &Series,
    families: &BTreeMap<String, Schema>,
) -> Result<bool, Error> {
    if !matches!(
        schema,
        Schema::PortDevice | Schema::WirelessDevice | Schema::WanDevice
    ) {
        return Ok(true);
    }
    let mac = series
        .device_mac
        .as_deref()
        .ok_or_else(|| invalid("Device statistics response is missing its MAC"))?;
    Ok(families.get(&normalized_mac(mac)?) == Some(&schema))
}

fn merge(
    rows: &mut Rows,
    schema: Schema,
    series: Series,
    entity: Option<&str>,
    window: Window,
) -> Result<(), Error> {
    let unit = series.unit_id.as_ref().map(Value::to_string);
    for point in series.points {
        if !window.overlaps(&point)? {
            continue;
        }
        let row = rows
            .entry((point.timestamp, entity.map(str::to_owned), unit.clone()))
            .or_default();
        row.insert("time".into(), json!(point.timestamp));
        row.insert("duration".into(), json!(point.duration));
        if let Some(entity) = entity {
            row.insert("mac".into(), json!(entity));
        }
        if let Some(unit) = &series.unit_id {
            row.insert("unit_id".into(), unit.clone());
        }
        copy_metrics(row, &point.values, schema.mapping(), "")?;
        if schema == Schema::WirelessDevice
            && let Some(bands) = point.values.get("byRadioBand")
        {
            let bands = bands
                .as_array()
                .ok_or_else(|| invalid("Statistics radio bands must be an array"))?;
            for band in bands {
                let name = band
                    .get("radioBand")
                    .and_then(Value::as_str)
                    .ok_or_else(|| invalid("Statistics radio band identity is missing"))?;
                let metrics = band
                    .as_object()
                    .ok_or_else(|| invalid("Statistics radio band must be an object"))?;
                copy_metrics(row, metrics, schema.mapping(), &format!("{name}-"))?;
            }
        }
    }
    Ok(())
}

fn copy_metrics(
    row: &mut Map<String, Value>,
    metrics: &Map<String, Value>,
    mapping: &[(&str, &str)],
    prefix: &str,
) -> Result<(), Error> {
    for (modern, legacy) in mapping {
        if let Some(value) = metrics.get(*modern).filter(|value| !value.is_null()) {
            if !value.is_number() {
                return Err(invalid(format!(
                    "Statistics metric '{modern}' is not numeric"
                )));
            }
            row.insert(format!("{prefix}{legacy}"), value.clone());
        }
    }
    Ok(())
}

fn finish(row: &mut Map<String, Value>, report: Report) -> Result<(), Error> {
    if let (Some(rx), Some(tx)) = (row.get("rx_bytes"), row.get("tx_bytes")) {
        let bytes = if let (Some(rx), Some(tx)) = (rx.as_u64(), tx.as_u64()) {
            json!(
                rx.checked_add(tx)
                    .ok_or_else(|| invalid("Statistics byte total overflow"))?
            )
        } else {
            let total = rx
                .as_f64()
                .zip(tx.as_f64())
                .map(|(rx, tx)| rx + tx)
                .filter(|total| total.is_finite())
                .ok_or_else(|| invalid("Statistics byte total is not finite"))?;
            json!(total)
        };
        row.insert("bytes".into(), bytes);
    }
    if matches!(report, Report::Site | Report::Gateway) {
        for (source, target) in [("rx_bytes", "wan-rx_bytes"), ("tx_bytes", "wan-tx_bytes")] {
            if let Some(value) = row.get(source).cloned() {
                row.insert(target.into(), value);
            }
        }
    }
    Ok(())
}
