//! Async Rust client and reactive data layer for UniFi controller APIs.
//!
//! This crate provides both the HTTP transport layer and the domain model
//! for communicating with UniFi Network controllers.
//!
//! ## Transport layer
//!
//! - **Integration API** ([`IntegrationClient`]) — RESTful OpenAPI-based interface
//!   authenticated via `X-API-KEY` header. Primary surface for CRUD operations on
//!   devices, clients, networks, firewall rules, and other managed entities.
//!
//! - **Session API** ([`SessionClient`]) — UniFi's internal `/api/` and `/v2/api/`
//!   surface, authenticated by session cookie + CSRF (username/password login) or
//!   by Integration `X-API-KEY` on UniFi OS. Covers data not yet exposed by the
//!   Integration API: events, traffic stats, admin users, DPI data, system info,
//!   and real-time WebSocket events (WebSocket still requires the cookie session).
//!
//! Both clients share a common [`TransportConfig`] for reqwest-based HTTP transport
//! with configurable TLS ([`TlsMode`]: system CA, custom PEM, or danger-accept for
//! self-signed controllers) and timeout settings.
//!
//! ## Domain layer
//!
//! - **[`Controller`]** — Central facade managing the full lifecycle: authentication,
//!   background refresh, and command routing.
//!
//! - **[`DataStore`]** — Lock-free reactive storage built on `DashMap` + `watch` channels.
//!
//! - **[`EntityStream<T>`]** — Subscription handle for TUI reactive rendering.
//!
//! - **Domain model** ([`model`]) — Canonical types (`Device`, `Client`, `Network`,
//!   `FirewallPolicy`, `Event`, etc.) with [`EntityId`] supporting both UUID and
//!   string-based identifiers.

// ── Transport layer ──────────────────────────────────────────────
pub mod auth;
pub mod error;
pub mod integration;
pub mod session;
pub mod site_manager;
pub mod transport;
pub mod websocket;

// ── Domain layer (merged from unifly-core) ───────────────────────
pub mod command;
pub mod config;
pub mod controller;
pub mod convert;
pub mod core_error;
pub mod model;
pub mod store;
pub mod stream;

// ── Transport re-exports ─────────────────────────────────────────
pub use auth::{AuthStrategy, ControllerPlatform, Credentials};
pub use error::Error;
pub use integration::IntegrationClient;
pub use integration::types as integration_types;
pub use session::SessionClient;
pub use session::models as session_models;
pub use site_manager::SiteManagerClient;
pub use site_manager::types as site_manager_types;
pub use transport::{TlsMode, TransportConfig};

// ── Domain re-exports ────────────────────────────────────────────
pub use command::requests::*;
pub use command::{Command, CommandResult};
pub use config::{AuthCredentials, ControllerConfig, TlsVerification};
pub use controller::{ConnectionState, Controller, PortProfileUpdate};
pub use core_error::CoreError;
pub use store::DataStore;
pub use stream::EntityStream;

pub use model::{
    AclRule, Admin, Alarm, Client, ClientType, Country, Device, DeviceState, DeviceType,
    DpiApplication, DpiCategory, EntityId, Event, EventCategory, EventSeverity, FirewallGroup,
    FirewallGroupType, FirewallPolicy, FirewallZone, HealthSummary, IpsecSa, MacAddress,
    MagicSiteToSiteVpnConfig, NatPolicy, NatType, Network, PoeMode, PortMode, PortProfile,
    PortSpeedSetting, PortState, RadiusProfile, RemoteAccessVpnServer, Site, SiteToSiteVpn,
    StpState, SysInfo, SystemInfo, TrafficMatchingList, VpnClientConnection, VpnClientProfile,
    VpnServer, VpnSetting, VpnTunnel, WanInterface, WireGuardPeer,
};
