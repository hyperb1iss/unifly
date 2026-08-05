+++
title = "Library (unifly-api)"
description = "Rust library for custom UniFi integrations"
weight = 3
+++

[![Crates.io](https://img.shields.io/crates/v/unifly-api.svg)](https://crates.io/crates/unifly-api)
[![docs.rs](https://docs.rs/unifly-api/badge.svg)](https://docs.rs/unifly-api)

The engine behind unifly is published independently on [crates.io](https://crates.io/crates/unifly-api). Use it to build your own UniFi tools, integrations, or automations in Rust.

## Quick Start

Add to your `Cargo.toml`:

```toml
[dependencies]
unifly-api = "0.10"
secrecy = "0.10"
tokio = { version = "1", features = ["full"] }
```

## Low-Level API Access

Talk directly to the controller with the `IntegrationClient`:

```rust
use unifly_api::{IntegrationClient, TransportConfig, TlsMode, ControllerPlatform};
use secrecy::SecretString;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let transport = TransportConfig {
        tls: TlsMode::DangerAcceptInvalid,
        ..Default::default()
    };
    let client = IntegrationClient::from_api_key(
        "https://192.168.1.1",
        &SecretString::from("your-api-key"),
        &transport,
        ControllerPlatform::UnifiOs,
    )?;

    // list_devices takes (site_uuid, offset, limit)
    let site_id = uuid::Uuid::parse_str("your-site-uuid")?;
    let page = client.list_devices(&site_id, 0, 50).await?;
    println!("Found {} devices", page.data.len());
    Ok(())
}
```

The `IntegrationClient` gives you direct control over individual API calls. Use it when you need to target specific endpoints or build custom query patterns.

For Session API access (events, stats, device commands), use `SessionClient` with cookie/CSRF auth instead.

## High-Level Controller

The `Controller` manages both APIs, background refresh, WebSocket events, and data merging:

```rust
use unifly_api::{Controller, ControllerConfig, AuthCredentials, TlsVerification};
use secrecy::SecretString;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let config = ControllerConfig {
        url: "https://192.168.1.1".parse()?,
        auth: AuthCredentials::ApiKey(SecretString::from("your-api-key")),
        tls: TlsVerification::DangerAcceptInvalid,
        ..Default::default()
    };
    let controller = Controller::new(config);
    controller.connect().await?;

    // Snapshot: get current data immediately
    let devices = controller.devices_snapshot();
    println!("Found {} devices", devices.len());

    // Reactive subscription: notified when data changes
    let mut stream = controller.devices();
    while let Some(updated) = stream.changed().await {
        println!("Device count updated: {}", updated.len());
    }

    Ok(())
}
```

### When to Use Which

| Approach                | Use Case                                                                 |
| ----------------------- | ------------------------------------------------------------------------ |
| `IntegrationClient`     | Direct REST calls, custom query patterns, Integration API only           |
| `SessionClient`         | Events, stats, device commands, Session API only                         |
| `Controller`            | Full lifecycle with both APIs, automatic refresh, reactive subscriptions |
| `Controller::oneshot()` | Single CLI-style fetch with no background tasks                          |

## Architecture

{% mermaid() %}
graph TD
subgraph "unifly-api"
IC["IntegrationClient<br/><i>REST + API Key</i>"]
SC["SessionClient<br/><i>Cookie + CSRF</i>"]
WS["WebSocket<br/><i>Live Events</i>"]
CTRL["Controller<br/><i>Lifecycle + Routing</i>"]
DS["DataStore<br/><i>DashMap + watch</i>"]
end

    CTRL --> IC
    CTRL --> SC
    CTRL --> WS
    IC --> DS
    SC --> DS
    WS --> DS
    DS --> ES["EntityStream&lt;T&gt;<br/><i>Reactive subscriptions</i>"]

{% end %}

| Type              | Purpose                                                                                                                                  |
| ----------------- | ---------------------------------------------------------------------------------------------------------------------------------------- |
| `Controller`      | Main entry point. Wraps `Arc<ControllerInner>` for cheap cloning across async tasks                                                      |
| `DataStore`       | Entity storage. `DashMap` + `watch` channels for lock-free reactive updates                                                              |
| `EntityStream<T>` | Reactive subscription. `current()` for a snapshot, `changed()` to await the next update (returns `None` when the controller disconnects) |
| `EntityId`        | Dual-identity enum: `Uuid(Uuid)` for Integration API or `Legacy(String)` for Session API                                                 |
| `AuthCredentials` | Auth mode: `ApiKey`, `Credentials`, `Hybrid`, or `Cloud` variants                                                                        |

## Connection Modes

| Mode                    | Use Case                       | Background Tasks                                 |
| ----------------------- | ------------------------------ | ------------------------------------------------ |
| `Controller::connect()` | Long-lived apps (TUI, daemons) | Refresh loop (10s), WebSocket, command processor |
| `Controller::oneshot()` | Fire-and-forget (CLI commands) | None. Single fetch, then done                    |

## Fallible DNS Conversion

Converting a raw Integration DNS response into a `DnsPolicy` is fallible. The `TryFrom<DnsPolicyResponse>` impl returns an `UnrecognizedDnsRecordType` error carrying the offending type token when the controller reports a record type the library doesn't model, instead of silently mislabeling it:

```rust
use unifly_api::model::DnsPolicy;

let policy = DnsPolicy::try_from(response)?; // Err(UnrecognizedDnsRecordType) on unknown types
```

{% warning(title="Upgrading from 0.9") %}
The infallible `From<DnsPolicyResponse> for DnsPolicy` impl is replaced by this `TryFrom`. It is the only breaking change in the 0.10 API for library consumers: switch call sites to `try_from` / `try_into` and handle (or propagate) the error.
{% end %}

## Full Documentation

See [docs.rs/unifly-api](https://docs.rs/unifly-api) for the complete API reference with all types, methods, and examples.

## Next Steps

- [Architecture Overview](/architecture/): how the crates fit together
- [Data Flow](/architecture/data-flow): connection lifecycle and DataStore design
- [API Surface](/architecture/api-surface): Integration API vs Session API endpoints
