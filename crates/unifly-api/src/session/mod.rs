// Session API client modules
//
// Hand-written client for the UniFi controller's session (non-OpenAPI) endpoints.
// Covers stat/, cmd/, rest/, and system-level operations wrapped in the
// standard `{ meta: { rc, msg }, data: [...] }` envelope.

pub mod auth;
pub mod client;
pub mod clients;
pub mod devices;
pub mod events;
pub mod firewallgroup;
pub mod models;
pub mod nat;
pub mod networkconf;
pub mod session_cache;
pub mod sites;
pub mod stats;
pub mod system;
pub mod system_log;
pub mod vpn;
pub mod wifi;
pub mod wireguard;

pub use client::{SessionAuth, SessionClient};
