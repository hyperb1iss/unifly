+++
title = "Crate Structure"
description = "Two-crate workspace: unifly-api library and unifly binary"
weight = 1
+++

## unifly-api

**Role:** Library: transport, business logic, and domain model.

Published on [crates.io](https://crates.io/crates/unifly-api). The engine powering everything:

### Transport Layer

- **Integration API client**: RESTful endpoints with API key authentication
- **Session API client**: Session-based with cookie and CSRF token handling
- **Site Manager client**: Ubiquiti cloud fleet API (`api.ui.com`) with API key authentication
- **WebSocket client**: Real-time event streaming
- **TLS**: Custom `rustls` configuration for self-signed certificates

Key design decisions:

- Uses `reqwest` with cookie jar for session management
- CSRF tokens captured from login response, rotated via `X-Updated-CSRF-Token` header
- WebSocket reconnection handled at the transport level

### Domain Layer

- **Controller**: Lifecycle management (connect, authenticate, fetch, disconnect)
- **DataStore**: `DashMap`-based entity storage with `tokio::watch` channels
- **Entity models**: Strongly-typed Rust structs for all 20+ UniFi resource types
- **Background tasks**: Periodic refresh (10s in TUI, configurable) and command processing
- **Data merging**: Integration API + Session API data combined per entity

Provides two connection modes:

- `Controller::connect()`: Full lifecycle with background refresh and WebSocket events
- `Controller::oneshot()`: Fire-and-forget for CLI commands (no background tasks)

## unifly

**Role:** Single binary and configuration.

Produces one binary with feature-gated capabilities (`cli`, `tui`, and
`tui-graphics` features). The default feature set includes `cli` and `tui`;
graphics-protocol charts are opt-in with `tui-graphics`.

### CLI (`unifly`)

- **clap-derived** command tree with 28 top-level commands
- **Output formatting**: Table, JSON, YAML, plain text via `tabled`
- **Shell completions**: Bash, Zsh, Fish via `clap_complete`
- **Man pages**: Generated at build time via `clap_mangen`

### TUI (`unifly tui`)

Real-time dashboard built with `ratatui`:

- **11 screens**: Dashboard, Devices, Clients, Networks, Firewall, Topology, Events, Stats, WiFi, Settings, Onboarding
- **Data bridge**: Translates `Controller` events into TUI actions
- **SilkCircuit theme**: Opaline-powered color palette with the project's visual identity
- **Octant charts**: High-density terminal-native charts with truecolor gradients
- **Optional pixel charts**: Graphics-protocol charts behind `tui-graphics`
- **Reactive rendering**: Only re-renders on data changes via `EntityStream` subscriptions

### Configuration

- **Profile system**: Named profiles for multiple controllers
- **Keyring integration**: OS-native credential storage via the `keyring` crate
- **TOML config**: File-based settings in the platform-standard config directory (`~/.config/unifly`, `~/Library/Application Support/unifly`, `%APPDATA%\unifly`)
- **Environment overlay**: Environment variables override file config
- **Setup wizard**: Interactive configuration with `dialoguer`
