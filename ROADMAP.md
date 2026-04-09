# Roadmap

What's planned, what's known to be broken, and what's on the wish list.

## Known Gaps

Issues that exist in the current release and are documented so nobody wastes time rediscovering them.

- **Controller reconnect is broken.** The internal `CancellationToken` becomes permanent after the first disconnect. Reconnect does not work correctly yet.
- **Device `radios` field is always empty.** Parsing radio data from the `interfaces` JSON is not implemented.
- **TUI has no test coverage.** The 10 screens and widgets are untested. Adding tests here is welcomed.
- **CLI test coverage is growing.** `cli_test.rs` and the new `e2e_test.rs` suite cover many commands, but coverage is not yet comprehensive.
- **~~NAT policies have no `update` subcommand.~~** Resolved: `nat policies update` is now available.
- **~~`nat` and `events` skip access gates.~~** Resolved: both now use `ensure_session_access`.
- **Plugin manifest version sync is manual.** `.claude-plugin/` and `.cursor-plugin/` manifests must be bumped by hand during releases.
- **ClawHub skill publish is manual.** Not yet wired into the release workflow.
- **AUR package update is manual.** Requires `just aur-update <version>` after each release.

## Next Up

Near-term priorities for the next 1-2 releases.

- **Cloud/Site Manager API support.** The `AuthCredentials::Cloud` variant exists but the transport is not yet implemented. This would enable managing controllers through Ubiquiti's cloud dashboard without direct network access.
- **`networks refs` for all entities.** Currently only networks have a "what depends on this?" command. Extend to WiFi, firewall zones, and other entities to make safe deletion easier.
- **Automate plugin manifest sync.** Add a CI step to the shared release workflow that patches version fields in plugin manifests before tagging.
- **Automate ClawHub publish.** Wire `npx clawhub publish ./skills/unifly` into the release workflow.
- **Fix controller reconnect lifecycle.** Replace the one-shot `CancellationToken` with proper reconnect state management.

## Recently Completed

Features that landed since the last roadmap update.

- **Wi-Fi observability commands.** `wifi neighbors`, `wifi channels`, `clients roams`, `clients wifi` — neighboring AP scans, regulatory channel data, per-client roam timelines, and Wi-Fi experience metrics.
- **API-key-on-Session-API discovery.** The Integration API key authenticates against Session API HTTP endpoints on UniFi OS, covering nearly every CLI command without a password. Hybrid is now only needed for WebSocket.
- **Legacy → Session rename.** The entire codebase renamed "Legacy API" to "Session API" to reflect that the surface is not deprecated.
- **VPN detail views.** `vpn servers get`, `vpn tunnels get`, `vpn status` (IPsec SA), `vpn health`.
- **E2E test suite.** Full simulation controller with wiremock-backed end-to-end tests.
- **Donate → Sponsor.** TUI status bar links to GitHub Sponsors.

## Wish List

Longer-term ideas, not yet committed.

- **Wi-Fi observability TUI screen.** Surface neighbor APs, regulatory channels, and per-client Wi-Fi experience in the TUI dashboard — potentially as a sub-tab on Devices (RF Environment) and Clients (Roaming/RF).
- **Full VPN CRUD.** Create/delete WireGuard servers, manage peers, download `.conf` files. Requires Session API endpoint discovery for peer management.
- **Cloud/Site Manager TUI.** Aggregate fleet view using the `unifly cloud` command surface — hosts, sites, ISP metrics rendered as TUI charts.
- **Backup management.** Create, download, and restore controller backups via the Session API.
- **Firmware management.** Check for updates, schedule upgrades, track firmware versions across the fleet.
- **Multi-site dashboard.** Aggregate view across multiple sites in the TUI.
- **Device radio parsing.** Fill in the empty `radios` field from `radio_table_stats` so the TUI Devices detail panel shows radio info.
- **TUI test harness.** Headless rendering tests for screen components using `ratatui`'s test backend.
- **Config file migration.** Auto-upgrade config files when the schema changes between versions.
- **Batch operations.** Apply changes across multiple devices/networks in a single command (restart all APs, update all SSIDs, etc.).
