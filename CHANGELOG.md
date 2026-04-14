# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/),
and this project adheres to [Semantic Versioning](https://semver.org/).

## [Unreleased]

### Added

- **`--after-system` on `firewall policies create`** to place a newly
  created policy after system-defined rules in one step (previously only
  available on `reorder`).
- **`--from-file` shorthand fields** for firewall policies: `dst_ip`,
  `dst_port`, `src_ip`, `src_port`, `dst_network`, `src_network` in
  policy JSON files are now resolved into the canonical
  `source_filter`/`destination_filter` before submission. Combined IP +
  port filters correctly nest `portFilter` inside `ipAddressFilter`.
- **`firewall groups`** CRUD commands for managing port groups, address
  groups, and IPv6 address groups via the Session API (`rest/firewallgroup`).
  Supports `list` (with `--type` filter), `get`, `create`, `update`, and
  `delete`.
- **Port/address group references in firewall policies**: `--dst-port-group`,
  `--src-port-group`, `--dst-address-group`, `--src-address-group` CLI flags
  and corresponding `dst_port_group` / `dst_address_group` shorthand fields
  in `--from-file` JSON. Group names are resolved to `external_id` UUIDs at
  create/update time.
- **`config cloud-setup`** guided onboarding for Site Manager profiles with
  API key validation, console selection, site discovery, and profile writing
- **`cloud` command group** for Site Manager fleet visibility: `hosts`,
  `sites`, `devices`, `isp`, and `sdwan`
- **`cloud switch <site>`** to retarget the active cloud profile to a different
  controller site using connector-resolved site names, internal references, or
  UUIDs
- **Site Manager fleet client** in `unifly-api` with `nextToken` pagination
  and cloud rate-limit handling
- **Cloud host auto-resolution** for controller-bound Integration commands
  when a profile omits `host_id` but the API key can unambiguously identify
  a console
- **`vpn servers get <id>`** and **`vpn tunnels get <id>`** for full VPN
  detail views with subnet, port, peer address, IKE version, and raw fields
- **`vpn status`** for live IPsec security association status from the
  Session API `stat/ipsec-sa` endpoint
- **`vpn health`** for the VPN subsystem slice of `stat/health`
- **`wifi neighbors`** to list neighboring and rogue APs seen by your access
  points, including signal, channel, SSID, and observer AP MAC
- **`wifi channels`** to show per-radio regulatory channel availability from
  the Session API
- **`clients roams <mac>`** to surface a client's connection timeline from the
  v2 system-log endpoint
- **`clients wifi <ip>`** to show per-client Wi-Fi experience metrics such as
  signal/noise, `wifi_experience`, link rates, nearest neighbors, and uplink
  chain data

### Changed

- Cloud auth now works end-to-end in CLI, TUI settings, and onboarding:
  `auth_mode = "cloud"` preserves `host_id`, `host_id_env`, `api_key_env`,
  defaults the controller URL to `https://api.ui.com`, and forces strict TLS
- Config and auth error guidance now points to both `config init` for local
  controllers and `config cloud-setup` for Site Manager onboarding
- Cloud authentication failures now mention Site Manager RBAC and console
  access, instead of only local controller password guidance
- Enriched `vpn servers` and `vpn tunnels` list output with subnet, port,
  protocol, peer, and IKE visibility where the controller returns those
  fields
- **Renamed "legacy" nomenclature to "session" throughout.** The UniFi
  `/api/*` and `/v2/api/*` HTTP surface is not deprecated — Ubiquiti ships new
  functionality there regularly, and with the API-key discovery it is no
  longer even tied to cookie session auth. It is now consistently called the
  **Session API** in code, types, docs, and user-facing messages.
  - Rust: `legacy/` module → `session/`, `LegacyClient` → `SessionClient`,
    `LegacyClientEntry` / `LegacyDevice` / `LegacyEvent` / `LegacySite` /
    `LegacyAlarm` / `LegacyUserEntry` → `Session*`, `Error::LegacyApi` →
    `Error::SessionApi`, `require_legacy` → `require_session`,
    `has_legacy_access` → `has_session_access`, `legacy_prefix` →
    `session_prefix`, `ensure_legacy_access` → `ensure_session_access`.
  - Config: `auth_mode = "legacy"` → `auth_mode = "session"`. This is a
    breaking change to config files; update existing profiles by hand or run
    `unifly config init` to regenerate.
  - TUI: `AuthMode::Legacy` → `AuthMode::Session`; the onboarding wizard's
    auth mode picker now shows "Username / Password (Session API)".
  - CLI errors: "session expired or invalid credentials" is now
    "API key rejected..." or "session expired..." depending on the client's
    auth kind.
  - Docs: AGENTS.md, README.md, CHANGELOG.md, `skills/unifly/**`, and
    `docs/guide/**` updated to reflect the new nomenclature.
  - `EntityId::Legacy(String)` is **kept** — that variant names an ID
    format (MongoDB ObjectId string vs UUID), not the API surface.

### Fixed

- Port range items in firewall policy payloads now serialize as
  `PORT_NUMBER_RANGE` instead of `PORT_RANGE`, which the UDM API rejects.
  `PORT_RANGE` is still accepted on read for backward compatibility.

## [0.8.0] - 2026-04-05

### Added

- **`api` command** for raw API passthrough (`unifly api <path>` with GET/POST)
- **NAT policy management** with `nat policies` CRUD for masquerade, source NAT, and destination NAT rules (CLI + TUI)
- **v2 API support** with `site_url_v2` and `get_raw` helpers for Network App 9+ endpoints
- **DPI multi-endpoint cascade** that tries v2 traffic-flow, then `stat/sitedpi`, then `stat/dpi` fallback
- **`dpi status`, `dpi enable`, `dpi disable`** subcommands for toggling Deep Packet Inspection
- **`clients reservations`** subcommand for DHCP reservation listing
- **`--network` flag** for scoped `clients remove-ip`
- **`--from-file` support** for firewall zones create and update
- **`dns_servers` support** in network create and update payloads
- **`--after-system` flag** for firewall policy ordering
- **Windows installer** (`install.ps1`) and platform-native config paths
- **Animated TUI tour GIF** in docs
- **Cursor plugin** manifest (`.cursor-plugin/plugin.json`)

### Fixed

- Client MAC addresses now read from top-level Integration API field instead of `access` object (was showing UUID as MAC)
- Replaced archived `serde_yml` with maintained `serde_yaml_ng` fork (unsoundness issues in serde_yml)
- NAT policy CRUD uses v2 API (`/v2/api/site/{site}/nat`) instead of non-existent Integration API endpoint
- WiFi detail endpoint fetched correctly for `wifi get`
- WiFi `--from-file` JSON field aliases work properly
- `bssTransitionEnabled` only sent for STANDARD wifi broadcasts
- `fastRoamingEnabled` added to wifi security config
- Correct network reference structure for wifi create
- Correct security configuration field name for wifi
- Avoided f32 precision artifacts in wifi frequency values
- Ordering envelope deserialized correctly for firewall policies
- `allowReturnTraffic` included in firewall policy action payloads
- Port filter values serialized as integers
- Required defaults added for gateway-managed network creation
- Correct DHCP range property name in network create
- Network management type serialized correctly in create request
- Truncation hint shown when list results exceed default limit
- Legacy API access enabled in API-key auth mode
- Legacy 401 responses distinguish rejected API keys from expired sessions (`Error::InvalidApiKey` vs `Error::SessionExpired`); controller runtime tests verify API-key auth reaches legacy HTTP routes and raw POST/PUT/DELETE paths stay CSRF-free
- Clippy path resolution idioms satisfied

### Changed

- Plugin manifests synced to v0.8.0 and Cursor plugin added
- Skill documentation streamlined with example payloads
- README intro streamlined and skill install section collapsed

## [0.7.0] - 2026-04-01

### Added

- **MFA/TOTP support** with persistent session caching
- **About overlay** and section headers in TUI settings form
- **Donate button** in TUI status bar with settings toggle

### Fixed

- Firewall policy and zone endpoints treated as non-fatal during refresh
- Event message placeholder templates resolved correctly
- Firewall ordering and ACL update alignment
- Refresh event deduplication and bounded controller fanout
- Firewall traffic filter and device MAC handling hardened
- Config profile flows work as documented
- Blank events fixed and event display cleaned up
- XDG config path used consistently across platforms

### Changed

- Major refactoring wave across the entire codebase:
  - API layer: split integration client domains, controller modules (lifecycle, runtime, query, refresh, commands, payloads, subscriptions), websocket runtime, typed command request payloads, response types
  - CLI layer: split args into submodules, split command handlers (devices, clients, firewall, acl, config), extracted shared command helpers
  - TUI layer: split all screen modules (dashboard, devices, clients, networks, firewall, topology, stats, events, onboarding, settings), extracted app navigation/lifecycle/action/render helpers, shared controller widgets
- Added funding config and upgraded release notes model

## [0.6.0] - 2026-03-27

### Added

- **`topology` command** for network tree visualization with device hierarchy
- **`clients find`** for quick client lookup by name, IP, or MAC
- **SilkCircuit themed CLI output** powered by opaline across all commands
- **Firewall traffic filter support** with read (Phase 1) and write (Phase 2)
- **DHCP reservation management** for create, list, and delete static mappings
- **Legacy site listing** so `sites list` works with legacy API auth
- **One-line installer** (`install.sh`) for Linux and macOS

### Fixed

- WebSocket TLS fixed for rustls 0.23 and tungstenite 0.29
- Integration API paths aligned with OpenAPI spec
- Store batch refresh snapshot rebuilds
- CLI rejects empty update requests and unavailable integration surfaces
- Legacy event behavior restored
- TUI traffic chart stability improvements
- Local list filter parsing

### Changed

- Merged `unifly-tui` into single binary as `unifly tui` subcommand
- Bumped MSRV to 1.94 and fixed new clippy lints
- Upgraded all dependencies to latest
- Updated justfile for single-binary layout
- Switched opaline to crates.io 0.4.0
- Overhauled agent skill with missing commands and flag fixes

## [0.2.0] - 2026-03-03

### Added

- **Interactive theme selector** in TUI settings screen via Opaline engine
- **Opaline token-based theme engine** integrated into TUI

### Changed

- **Consolidated from 5 crates to 2** by merging `unifly-core` into `unifly-api` and absorbing `unifly-config` and `unifly-tui` into `unifly`
- Switched opaline dependency from local path to crates.io
- Migrated CI workflows to shared-workflows
- Added justfile for workspace dev recipes

### Fixed

- Default impls added for all TUI screen structs
- Bumped Node to 24 in docs workflow
- Bumped action versions and switched to trusted publishing
- Release note generation for first release

## [0.1.1] - 2026-02-25

### Fixed

- README pointer fixed for workspace root

## [0.1.0] - 2026-02-23

### Added

- **CLI** with 22 resource commands: `devices`, `clients`, `networks`, `wifi`, `firewall`, `dns`, `vpn`, `acl`, `admin`, `alarms`, `dpi`, `events`, `hotspot`, `radius`, `sites`, `stats`, `system`, `traffic-lists`, `wans`, `countries`, `config`, `completions`
- **TUI** with 8 real-time screens: Dashboard, Devices, Clients, Networks, Firewall, Topology, Events, Stats
- **Dual-API engine** with Integration API (REST, API key auth) and Legacy API (session-based, cookie/CSRF) with automatic Hybrid negotiation
- **WebSocket event streaming** with 10K rolling buffer, severity filtering, pause/scroll-back
- **Area-fill traffic charts** with Braille line overlay, auto-scaling axes, and period selection (1h/24h/7d/30d)
- **Dashboard** with btop-style overview: WAN traffic chart, gateway info, connectivity health, CPU/MEM gauges, network/WiFi panels, top clients, recent events
- **Device management** with list, get, restart, locate (LED flash), adopt, forget; 5-tab detail panel in TUI (Overview, Performance, Radios, Clients, Ports)
- **Client management** with list with type filtering (All/Wireless/Wired/VPN/Guest), block/unblock, kick
- **Network management** with list, get, create, update, delete VLANs; inline edit overlay in TUI
- **Firewall management** with policies, zones, ACL rules across three sub-tabs with visual rule reordering
- **Zoomable topology view** with gateway-to-AP tree, pan, zoom, fit-to-view, color-coded by device type and state
- **Historical statistics** with WAN bandwidth, client counts, DPI application/category breakdown
- **Hotspot voucher management** with create, list, delete, revoke guest vouchers
- **DNS policy management** with local DNS record CRUD
- **VPN, RADIUS, WAN interface inspection** for read-only views of VPN servers/tunnels, RADIUS profiles, WAN interfaces
- **Multi-profile configuration** with named controller profiles and interactive setup wizard (`config init`)
- **5 output formats**: table, JSON, compact JSON, YAML, plain text (`-o` flag)
- **OS keyring credential storage** for API keys and passwords with plaintext fallback
- **Environment variable support**: `UNIFI_API_KEY`, `UNIFI_URL`, `UNIFI_PROFILE`, `UNIFI_SITE`, `UNIFI_OUTPUT`, `UNIFI_INSECURE`, `UNIFI_TIMEOUT`
- **Shell completions** for Bash, Zsh, Fish via `completions` command
- **SilkCircuit theme** with neon-on-dark color palette, semantic highlighting, and ANSI fallback
- **Published library crate** (`unifly-api` on crates.io) for building custom UniFi tools
- **AI agent skill** that teaches coding assistants UniFi infrastructure management via the CLI
- **Cross-platform distribution** via Homebrew tap, shell/PowerShell installers, cargo install, GitHub releases for Linux (x86_64, aarch64), macOS (x86_64, aarch64), Windows (x86_64)

### Security

- TLS verification defaults to system CA store (self-signed certs require explicit `--insecure` flag)
- Config file permissions restricted to owner (0600) on Unix
- Credential storage via OS keyring with plaintext fallback only when keyring is unavailable

[Unreleased]: https://github.com/hyperb1iss/unifly/compare/v0.8.0...HEAD
[0.8.0]: https://github.com/hyperb1iss/unifly/compare/v0.7.0...v0.8.0
[0.7.0]: https://github.com/hyperb1iss/unifly/compare/v0.6.0...v0.7.0
[0.6.0]: https://github.com/hyperb1iss/unifly/compare/v0.2.0...v0.6.0
[0.2.0]: https://github.com/hyperb1iss/unifly/compare/v0.1.1...v0.2.0
[0.1.1]: https://github.com/hyperb1iss/unifly/compare/v0.1.0...v0.1.1
[0.1.0]: https://github.com/hyperb1iss/unifly/releases/tag/v0.1.0
