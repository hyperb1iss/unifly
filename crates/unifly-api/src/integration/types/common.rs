use std::collections::HashMap;

use serde::{Deserialize, Serialize};
use serde_json::Value;
use uuid::Uuid;

/// Generic pagination wrapper returned by all list endpoints.
///
/// Items are decoded individually: a record the model cannot parse is
/// dropped with a warning instead of failing the whole page, so one
/// unexpected payload shape cannot blank an entire collection.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[serde(bound(deserialize = "T: serde::de::DeserializeOwned"))]
pub struct Page<T> {
    pub offset: i64,
    pub limit: i32,
    pub count: i32,
    pub total_count: i64,
    #[serde(deserialize_with = "lenient_items")]
    pub data: Vec<T>,
}

fn lenient_items<'de, D, T>(deserializer: D) -> Result<Vec<T>, D::Error>
where
    D: serde::Deserializer<'de>,
    T: serde::de::DeserializeOwned,
{
    let raw = Vec::<Value>::deserialize(deserializer)?;
    Ok(raw
        .into_iter()
        .filter_map(|item| match serde_json::from_value::<T>(item) {
            Ok(parsed) => Some(parsed),
            Err(error) => {
                tracing::warn!(
                    item_type = std::any::type_name::<T>(),
                    %error,
                    "skipping list item that failed to parse"
                );
                None
            }
        })
        .collect())
}

/// Site overview — from `GET /v1/sites`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SiteResponse {
    pub id: Uuid,
    pub name: String,
    /// Used as the Session API site name (`/api/s/{internalReference}/`).
    pub internal_reference: String,
}

/// Application info — from `GET /v1/info`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ApplicationInfoResponse {
    #[serde(flatten)]
    pub fields: HashMap<String, Value>,
}

/// Error response returned by the Integration API.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ErrorResponse {
    pub message: Option<String>,
    #[serde(flatten)]
    pub extra: HashMap<String, Value>,
}
