# Roadmap

What's planned, what's known to be limited, and what's on the wish list.

## Known Gaps

Issues that exist in the current release and are documented so nobody wastes time rediscovering them.

- **Backup restore is missing.** `unifly system backup` covers `create`, `list`, `download`, and `delete`. Restoring a downloaded backup file still has to go through the controller UI.
- **`refs` only exists for networks.** `networks refs <id>` answers "what depends on this entity before I delete it?" — no equivalent yet for WiFi broadcasts, firewall zones, firewall groups, traffic-lists, or other delete-blocking entities.
- **CLI test coverage is growing but not comprehensive.** `cli_test.rs` and the dockerized `e2e_test.rs` suite cover a lot of the surface, including firewall groups CRUD, the NAT policy lifecycle, and switch port reads under session auth; the full VPN write surface and switch-port writes have lighter coverage than older entities.
- **TUI snapshot test harness is missing.** Unit tests for effects, forms, and screen state exist (~20 files), but there is no `ratatui::TestBackend`-driven render harness that locks down visual output across changes.

## Next Up

Near-term priorities for the next 1-2 releases.

- **Cloud-native TUI dashboard.** The connector and `unifly cloud` fleet CLI are in place; the next major step is a dedicated Site Manager dashboard with fleet rollups, host selection, and ISP / SD-WAN health surfaced as TUI charts. Subsumes the older "multi-site dashboard" idea.
- **`refs` for non-network entities.** Extend the `networks refs` pattern to WiFi broadcasts, firewall zones, firewall groups, and traffic-lists so safe deletion is a one-command check across the dependency graph.
- **`system backup restore <filename>`.** Round-trip the existing backup surface so a downloaded `.unf` can be re-applied without leaving the CLI.

## Recently Completed

Features that landed since the last roadmap update.

- **TUI auto-reconnect.** The data bridge retries failed connects with exponential backoff (2 seconds doubling to a 30-second cap) and reports each attempt in the status bar; `Ctrl+r` forces an immediate retry while disconnected. Distinct from the reconnect lifecycle fix below, which made a second `connect()` possible; this makes the TUI drive it automatically.
- **Create commands print the created entity.** Every create renders the created record on stdout in the chosen `--output` format (`-o plain` emits the bare ID) with the confirmation on stderr, so scripts capture IDs directly instead of re-listing by name. VPN create responses are redacted like `get`; `sites create` prints nothing because the controller returns no record.
- **`[defaults]` config section honored.** `defaults.insecure` and `defaults.timeout` apply wherever a profile leaves the value unset, with flag/env > profile > `[defaults]` > builtin precedence and a tri-state `--insecure` that can force verification back on. `config init` asks about self-signed certificates up front instead of writing profiles that fail TLS on UDM-style controllers.
- **Issue fix wave: #25, #26, #27.** The TUI honors CLI flag and env overrides (`--insecure`, `--site`, `--api-key`, and friends) and shows connect failures in the status bar; firewall policy lists survive real controller payloads (`start`/`stop` port ranges, per-item decode, pagination by the server-reported count); DNS policies parse modern `*_RECORD` type tokens on both read and write.
- **Profile `timeout` takes effect.** The `--timeout` flag's baked-in clap default made every profile timeout dead weight; resolution follows flag/env, then profile, then `[defaults]`, then the 30-second builtin, including cloud transports.
- **Fallible DNS conversion (semver break).** The lossy `From<DnsPolicyResponse>` impl is replaced by `TryFrom` returning a typed `UnrecognizedDnsRecordType` error, so `unifly-api` consumers must handle the `Result`.
- **Session-auth e2e coverage.** The dockerized suite covers firewall groups CRUD, a NAT policy create/update/delete lifecycle, and switch port reads under session-only auth. The probing surfaced and fixed the session refresh snapshot leaving `nat policies list` permanently empty.
- **Switch port management as code.** `devices ports`, `devices ports-export`, and `devices port-set` cover the full per-port surface (mode, native VLAN, tagged VLANs, PoE, speed) with JSONC `--from-file` payloads, splice semantics (omitted ports keep their override; `"reset": true` removes one), `--with-clients` enrichment, and `// last-seen <ISO8601>: <mac>` markers in `ports-export` for git-diff drift detection on config-as-code repos.
- **Full VPN management surface.** `vpn site-to-site`, `vpn remote-access` (with `suggest-port` and `download-config`), `vpn clients`, `vpn peers` (with `subnets`), `vpn magic-site-to-site`, `vpn settings` (with `teleport`, `magic-site-to-site`, `openvpn`, and `peer-to-peer` flag groups), and `vpn connections` list/get/restart for the legacy v2 inventory. Most write paths accept `--from-file`.
- **Firewall groups CRUD.** `firewall groups list/get/create/update/delete` for port, address, and IPv6 address groups. Firewall policies reference groups by name via `--dst-port-group` / `--dst-address-group` (and the matching `--from-file` shorthands), resolved to `external_id` UUIDs automatically.
- **Site settings command.** `unifly settings list/get/set/export` for Session API site-level settings.
- **Wi-Fi observability TUI screen.** The TUI Wifi screen now has four sub-tabs — Overview, Clients (with `wifi_experience` scores, link rates, signal/noise), Neighbors, and Roaming — surfacing the same data that `wifi neighbors / channels`, `clients wifi`, and `clients roams` produce on the CLI.
- **Wi-Fi observability commands.** `wifi neighbors`, `wifi channels`, `clients roams`, `clients wifi` for neighboring AP scans, regulatory channel data, per-client roam timelines, and Wi-Fi experience metrics.
- **VPN detail views.** `vpn servers get`, `vpn tunnels get`, `vpn status` (IPsec SA), and `vpn health`.
- **Site Manager support.** Cloud connector auth, `unifly cloud` fleet commands, host auto-resolution, and `config cloud-setup` work end-to-end for connector-backed CLI/TUI use.
- **API-key-on-Session-API discovery.** The Integration API key authenticates against Session API HTTP endpoints on UniFi OS, covering nearly every CLI command without a password. Hybrid is now only needed for live WebSocket features.
- **Legacy → Session rename.** The codebase renamed "Legacy API" to "Session API" everywhere to reflect that the surface is not deprecated.
- **Credential precedence reversal.** Explicit `api_key` / `password` in a profile config now overrides any keyring entry (PR #22), making headless and shared-machine workflows predictable.
- **Controller reconnect lifecycle fix.** Replaced the one-shot top-level `CancellationToken` with a parent token plus per-connect child tokens. `disconnect()` cancels the current child only, and the next `connect()` mints a fresh child, so reconnect now works on the second and subsequent attempts.
- **Device radio parsing.** `parse_session_radios` populates the `radios` field from the session-side `radio_table` and `radio_table_stats`, so `devices get` and the Devices TUI detail panel now show per-radio band, channel, width, TX power, and stats instead of an empty array.
- **HyperChart + Octant Canvas TUI.** New chart widget with tachyonfx-driven effects, an Octant Canvas bandwidth chart with live legend on Stats, a rank column on ranked-bar lists, and a 160ms `fade_from_fg` transition between screens.
- **Plugin manifest sync automated.** The shared release workflow's `version-files` input patches `.claude-plugin/plugin.json`, `.claude-plugin/marketplace.json`, `.cursor-plugin/plugin.json`, and `skills/unifly/SKILL.md` automatically on every bump.
- **ClawHub skill publish automated.** `cicd.yml` runs `npx clawhub sync --root ./skills --all --changelog "Release v$VERSION"` on tag push.
- **AUR package update automated.** The `update-aur` job in `cicd.yml` bumps `pkgver` and pushes the updated `PKGBUILD` via `github-actions-deploy-aur` on tag push.
- **E2E test suite.** A feature-gated suite (`--features e2e`) drives the built binary against a dockerized UniFi controller running in simulation mode (`just e2e` for the full lifecycle), plus a Site Manager client test suite (`site_manager_client_test.rs`).
- **Donate → Sponsor.** TUI status bar links to GitHub Sponsors.

## Wish List

Longer-term ideas, not yet committed.

- **WireGuard server creation.** VPN CRUD now covers peers, clients, site-to-site, remote-access, magic-site-to-site, and settings; outright `vpn servers create` is the remaining gap.
- **Firmware management beyond `devices upgrade`.** The single-device `devices upgrade <mac> [--url <firmware-url>]` surface ships today. The fleet-level layer — version inventory, "is an update available?" checks, and scheduled upgrade waves — is the gap.
- **TUI snapshot harness.** Headless rendering tests using `ratatui`'s `TestBackend` to lock down visual changes in widgets and screens, complementing the existing unit tests for effects, forms, and state.
- **Config file schema migration.** Auto-upgrade config files when fields change between versions. The macOS directory migration shipped; structural field migration (e.g. renaming or restructuring profile fields) has not.
- **Batch operations.** Apply changes across multiple devices or networks in a single command (restart all APs of a model, update all guest SSIDs, etc.) without `xargs` choreography.
