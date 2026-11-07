// Session API system log endpoints (v2)
//
// Covers paginated system history and client connection timeline queries.

use serde::ser::SerializeStruct;
use serde::{Deserialize, Serialize, Serializer};
use serde_json::{Map, Value};
use tracing::debug;

use crate::error::Error;
use crate::session::client::SessionClient;

/// A bounded system-log page query, always ordered newest first.
#[derive(Debug, Clone, Copy)]
pub struct SystemLogQuery {
    /// Lower timestamp bound in Unix milliseconds.
    pub timestamp_from: i64,
    /// Upper timestamp bound in Unix milliseconds, fixed across pages.
    pub timestamp_to: i64,
    /// Zero-based page number.
    pub page_number: u32,
    pub page_size: u32,
}

impl Serialize for SystemLogQuery {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut query = serializer.serialize_struct("SystemLogQuery", 6)?;
        query.serialize_field("timestampFrom", &self.timestamp_from)?;
        query.serialize_field("timestampTo", &self.timestamp_to)?;
        query.serialize_field("pageNumber", &self.page_number)?;
        query.serialize_field("pageSize", &self.page_size)?;
        query.serialize_field("sortByField", "TIMESTAMP")?;
        query.serialize_field("sortDirection", "DESCENDING")?;
        query.end()
    }
}

/// System-log response with pagination metadata preserved for completeness checks.
#[derive(Debug, Clone, Deserialize)]
pub struct SystemLogPage {
    pub data: Vec<SystemLogEntry>,
    pub page_number: u32,
    pub total_element_count: u64,
    pub total_page_count: u32,
    /// Some controllers report only a lower bound for the total number of pages.
    #[serde(default)]
    pub or_more: bool,
    #[serde(default)]
    pub has_next: bool,
}

/// A raw system-log record. Categories and templates evolve across controller versions.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SystemLogEntry {
    pub id: Option<String>,
    /// Unix timestamp in milliseconds.
    pub timestamp: i64,
    pub category: Option<String>,
    pub subcategory: Option<String>,
    pub severity: Option<String>,
    pub status: Option<String>,
    pub key: Option<String>,
    pub event: Option<String>,
    pub message: Option<String>,
    pub message_raw: Option<String>,
    pub title_raw: Option<String>,
    #[serde(default)]
    pub parameters: Map<String, Value>,
    /// Preserve unknown fields when identifying records that have no ID.
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}

impl SessionClient {
    /// Read a page of system logs without restricting categories or dropping audit records.
    ///
    /// `POST /v2/api/site/{site}/system-log/all`
    pub async fn list_system_log(&self, query: &SystemLogQuery) -> Result<SystemLogPage, Error> {
        self.post_read_raw(self.site_url_v2("system-log/all"), query)
            .await
    }

    /// Get a client's connection timeline: connects, disconnects, and roams.
    ///
    /// `GET /v2/api/site/{site}/system-log/client-connection/{mac}`
    ///
    /// **Quirk:** the MAC must appear in both the URL path and the
    /// `?mac=` query parameter.
    pub async fn get_client_roams(
        &self,
        mac: &str,
        limit: Option<u32>,
    ) -> Result<Vec<Value>, Error> {
        let limit = limit.unwrap_or(200);
        let path = format!(
            "system-log/client-connection/{mac}?mac={mac}&separateConnectionSignalParam=false&limit={limit}"
        );
        let url = self.site_url_v2(&path);
        debug!(mac, limit, "fetching client roam timeline");
        let value = self.get_raw(url).await?;
        Ok(match value {
            Value::Array(items) => items,
            other => vec![other],
        })
    }
}
