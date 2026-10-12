# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/),
and this project adheres to [Semantic Versioning](https://semver.org/).

## [Unreleased]

### Added

- **Synthetic demo fleet for screenshots and recordings.**
  `unifly tui --demo-fleet` runs the real TUI against an offline, fictional
  network with no controller, credentials, or network access: "Neon Harbor
  Studios", with a UDM Pro Max gateway, three switches, nine access points,
  about 140 clients on six VLANs and four SSIDs, a zone-based firewall, NAT,
  ACLs, and a week of events. A once-a-second simulation keeps WAN traffic
  (with render-upload and asset-sync bursts), device telemetry, client roams
  and arrivals, and security events moving, and the on-demand queries behind
  the Stats and WiFi screens answer from the same fleet. The same
  `--demo-seed` replays the same fleet and the same animation. Device
  actions (restart, locate, upgrade, PoE power cycle) and client actions
  (block, unblock, kick) play out in the simulation; configuration changes
  are refused with a clear error. Plain `--demo` still sanitizes a real
  controller, and now falls back to the demo fleet when no controller is
  configured instead of opening onboarding.
  `--demo-fleet` and `--demo-seed` are global flags (`UNIFI_DEMO_FLEET`,
  `UNIFI_DEMO_SEED`), and a curated set of read-only CLI commands answers
  from the same fleet in every output format: devices, clients, networks,
  WiFi, firewall, NAT, ACL, events, stats, topology, and sites listings,
  plus the first reads of a diagnosis. `system health` and `system info`,
  `wans list` (WAN up on 203.0.113.42, WAN2 without link), and `alarms
  list` all answer from the fleet's standing problems (the offline AP, the
  AP with firmware waiting, the dark camera port, the unplugged WAN2), so
  they agree with `events list` and `devices get`. `wifi get` and
  `devices stats` answer too.
  Any other command fails with a "not available in demo mode" error before
  it reads config or touches the network. Library users get the same fleet
  from `Controller::demo` behind the new `demo` feature of `unifly-api`.
- **`totp_command` profile setting: MFA sessions that renew themselves.** A
  profile can name a command that prints the current TOTP code, as an argv
  array run without a shell, e.g. `totp_command = ["op", "item", "get",
  "UniFi", "--otp"]`. unifly runs it (no stdin, 10 s limit, trimmed stdout)
  for every login that needs a code, so a long-running TUI on an MFA account
  keeps its Session-backed features past the first cookie expiry. A non-zero
  exit, a timeout, or empty output counts as no code and is logged without
  the output. On Unix the command runs in its own process group, which is
  stopped on a timeout or when something the command started still holds its
  output open after it exits; a background process with its output
  redirected (a cache daemon) is left running. A locked password manager may
  prompt on each attempt, about every 30 s while the session is expired. An
  explicit one-off code (`--totp`, `UNIFI_TOTP`, or `totp_env`) still serves
  the first login; the command feeds every re-login, and the first login too
  when there is no explicit code. The CLI and the TUI launch paths resolve
  it through the same helper, so the TUI's direct `--controller` /
  `--api-key` path now also honours a hybrid profile's `totp_env`.
- **MFA sessions can renew themselves when a fresh code is available.**
  `unifly-api` gains `TotpProvider`, a closure that returns the current TOTP
  code, set as `ControllerConfig::totp_provider` (or
  `ReauthCredentials::totp_provider` on a bare `SessionClient`). The
  automatic re-login after a cookie expiry asks it for a code on every
  attempt. When it has none, returns something other than six digits, or
  repeats the code the last login sent, no login is sent and the 30 s
  cooldown applies, so a provider that cannot really produce fresh codes
  never presents the password with a spent one. At connect it supplies the
  code when `totp_token` is unset, and only if the controller actually
  issues the MFA challenge, so a restored cached session never runs it. The
  CLI and TUI build one from a profile's `totp_command`; `--totp`,
  `UNIFI_TOTP`, and `totp_env` are each read once at startup, so they never
  become a provider.
- **Version-matched agent guidance** via `unifly skill`, available offline
  without controller configuration. The installed skill and network manager
  agent now load the binary's guide, with on-demand concepts, workflows,
  payload examples, and a command inventory generated from Clap. Corrected
  authentication boundaries, raw API paths, identifiers, and output recipes.
  Older binaries need an update before they can serve the new discovery skill.
- **`unifly skill --install` puts the discovery skill where agents look.**
  It writes `~/.agents/skills/unifly/SKILL.md` for the current user, or
  targets Claude Code's skills directory with `--claude` or any skills
  directory with `--dir`, without reading configuration or touching a
  controller. A rerun is a no-op, and a skill from `npx skills add` or an
  earlier release is upgraded in place; any other `SKILL.md` stays untouched
  unless `--force` is given. A symlinked skill directory or file is managed
  elsewhere, so it is reported and never written, even with `--force`. The
  installed path is printed on stdout.
  Homebrew and AUR install system-wide and cannot write into a home
  directory, so the formula's caveats and the AUR package's install message
  point users at the command instead. The `install.sh` and `install.ps1`
  installers run as the user, so they install the skill themselves once the
  binary supports it (skip with `UNIFLY_NO_SKILL=1`, or `-NoSkill` in
  PowerShell); `install.sh` skips it when running as root.
- **Device commands accept names, IPs, and any MAC spelling.** `devices
  get`, `stats`, `restart`, `locate`, `upgrade`, `provision`, `remove`,
  `port-cycle`, `ports`, `ports-export`, and `port-set` used to need the
  exact device ID or a lowercase, colon-separated MAC, so `unifly devices
  get ap-roof` failed with "not found". They now take the ID, the device
  name (case-insensitive), its IP, or its MAC with colons, dashes, dots
  (`aabb.ccdd.eeff`), or no separators, in any case. An identifier spelled
  like a MAC or IP only ever matches a MAC or IP, never a device name, so a
  device named after another address cannot capture a command aimed at
  it. A name two devices share is an error that lists both (exit 2), never
  a guess, and an identifier that matches nothing exits 4 and points at
  `devices list`.
  `devices adopt` stays MAC-only, since a pending device is not in the
  adopted inventory yet, and now rejects a malformed MAC before sending it.
  The client actions that take a MAC (`block`, `unblock`, `kick`, `forget`,
  `set-ip`, `remove-ip`) reject a malformed one the same way, so an IPv6
  address can no longer be read as a MAC.
  `MacAddress` in `unifly-api` parses the dotted form too.
- **Get commands accept display names.** `networks get Studio`, `firewall
  policies get "drop inbound ssh"`, and the same for WiFi, firewall zones
  and groups, NAT, ACL, traffic lists, DNS policies (by domain), and
  hotspot vouchers (by code) now resolve the name case-insensitively
  instead of demanding the UUID. `clients get` takes a name, hostname, IP,
  or MAC in any spelling, matching what `clients roams` and `clients wifi`
  already accepted, and all three now report a name two clients share
  instead of silently picking the first. Every lookup goes through one
  shared resolver, so ambiguity and not-found errors read the same
  everywhere.
- **The agent guide lists JSON field names.** `unifly skill concepts`
  now spells out the keys of devices, clients, events, alarms, site health,
  WANs, and stats rows, and names the controller wire fields (`uplink`,
  `radio_table`, `system_stats`, `datetime`, `msg`) that typed output does
  not use, so agents stop probing with `jq` fallbacks such as
  `.uplink // .uplink_info`. A test checks every listed key against real
  output.

### Security

- **Settings output no longer leaks credentials.** `settings export`, and
  JSON/YAML output from `settings list` and `settings get`, used to include
  every `x_`-prefixed credential field (only the table view masked them).
  They now leave those fields out at any depth, and a new `--with-secrets`
  flag includes them when you need the real values. Table output from
  `settings get` now masks nested credentials too, not just top-level ones.
  `settings set` carries over credentials that an update leaves out of an
  object (at any depth), so an edited export can be applied back safely.
  An array that holds credentials is replaced only when the update's
  version carries at least one of them, and the update is then taken as
  written; otherwise `set` refuses and names the fields.
  `--allow-secret-removal` overrides the check. Scripts that read `x_`
  fields from these commands need `--with-secrets`.

### Fixed

- **List output and paging are deterministic.** Lists read from the
  controller store came out in a different order on every run, because the
  store is a concurrent map, so `--offset`/`--limit` pages could overlap or
  skip records and `--limit` on `events list` kept arbitrary events instead
  of the newest. Every store-backed list now sorts before filtering and
  paging, and every output format gets the same order. Devices, clients,
  networks, WiFi, sites, zones, firewall groups, DNS policies, and traffic
  lists sort case-insensitively by name with a stable tie-breaker (clients
  by alias, else hostname, then IP and MAC). `firewall policies list` shows
  policies in evaluation order: grouped by source then destination zone, by
  ascending `index` within each pair (user rules, then system rules, then
  the pair's default). ACL rules follow their evaluation `index` and NAT
  rules their `rule_index`, both now carried in JSON output. Events and
  hotspot vouchers list newest first.
- **Session expiry no longer kills a long-running TUI.** The session cookie
  lapses roughly an hour in; previously every Session-backed fetch (client
  details, health, events, NAT, firewall groups, Wi-Fi neighbours, switch
  ports) failed with "Session expired" from then on and the WebSocket never
  came back, because login only ever ran at connect. `SessionClient` now
  keeps the login credentials (`enable_reauth`) and on `SessionExpired`
  re-logs in once and refreshes the session cache. A **read** is then
  retried transparently; a **state-changing request is not replayed** --
  the client re-authenticates so the next call succeeds and surfaces the
  error, because a 401 is normally raised before the controller dispatches
  the write but nothing in the API contract guarantees it, and a replayed
  `POST` would create a second record or repeat a device command. Failed
  re-logins are rate-limited to one per 30 s, and a successful one clears
  that cooldown so a later expiry is not blocked by it. Each request records
  the session generation it was sent under, so a late 401 for a cookie that
  another request already replaced retries on the new session instead of
  clearing it. Stale cookies are cleared before the re-login and a login that
  returns no new cookie is reported as a failure rather than silently
  retried. A session opened with a fixed TOTP code does not renew itself,
  because replaying a spent code would fail every attempt while still
  presenting the correct password; MFA sessions renew only through a
  `TotpProvider`, which the CLI and TUI build from `totp_command` (see
  Added). A session restored from cache for an MFA
  account with no TOTP source stops re-logging in after the first re-login
  meets the MFA challenge, instead of repeating it every 30 s for as long as
  the process runs. The WebSocket reads its cookie from the session client
  on every reconnect, and a server-side clean close now waits the initial
  backoff instead of reconnecting immediately.
- **A failed fetch during refresh keeps the last known data.** Firewall
  policies, firewall zones, ACL rules, DNS policies, vouchers, NAT policies,
  firewall groups, and events come from optional endpoints. A timeout, 5xx,
  or 404 on any of them used to count as an empty list, so one transient
  error wiped that collection from the store while the refresh still
  reported success, and the TUI and CLI showed nothing until the next good
  refresh. A failed optional fetch now leaves its collection untouched, and
  only a successful response (empty or not) replaces it. Site health follows
  the same rule, so a successful empty health response now clears it.
- **Networks and NAT rules survive partial failures during refresh.** A
  network whose detail fetch failed used to drop out of the refresh and
  vanish from the store; it now keeps its last known record, or its summary
  if it was never loaded. A NAT listing that answers HTTP 200 with something
  other than a rule array (UniFi OS can wrap errors this way) is now an error
  instead of an empty list, so the refresh keeps the current NAT rules
  rather than clearing them.
- **Clients and devices keep their session details through a failed session
  fetch.** When a session lookup that enriches Integration data failed,
  clients lost their hostname, traffic counters, Wi-Fi details, uplink, and
  DHCP reservation for a refresh cycle, and devices lost their client count,
  WAN IPv6, uplink, ports, and radios, so the TUI flickered to blanks. Those
  fields now carry over from the last good refresh, and anything the
  Integration API reports itself still wins.
- **Saving in the TUI settings screen no longer erases profile settings.**
  Test and Apply rebuilt the profile from the form alone, so `totp_env`,
  `totp_command`, `ca_cert`, and `timeout` were deleted from `config.toml`,
  and an unset `insecure` came back as an explicit `false` that overrode
  `[defaults].insecure`. Fields the form does not show are now written back
  as loaded. `insecure` keeps its stored value, unset included, until you
  touch the toggle, which then saves exactly what it shows; an unset value
  displays what it inherits (verification when the profile has a
  `ca_cert`, otherwise `[defaults].insecure`).
- **The TUI no longer writes keyring credentials to `config.toml`.** A
  profile whose API key or password lives in the OS keyring loaded into the
  settings form as an empty field, failed validation until the secret was
  retyped, and then saved it to `config.toml` in plaintext. The form now
  shows such a credential as "stored in keyring", accepts it without
  retyping, connects with the keyring value, and leaves the entry alone. A
  newly typed credential (in settings or the first-run setup) goes to the
  keyring, as `config init` stores it, with plaintext only as a reported
  fallback when no keyring is usable (none running, locked, or failing). A
  keyring that refuses the secret stops the save with a message naming the
  field, leaves `config.toml` unchanged, and puts back any keyring entry
  the same save had already written. An existing entry that cannot be read
  back first is never overwritten; that stops the save the same way. If
  writing `config.toml` fails after the keyring step, those keyring changes
  are put back too, so a failed save never pairs the old profile with a
  new credential.
  `config init` and
  `config set-password` classify keyring errors the same way. Credentials
  already kept in `config.toml` stay there.

- **The Firewall screen's NAT tab was always empty.** NAT policy updates
  reached the TUI but were never handed to the screens, so the tab stayed
  blank on every controller.

- **Stats time axis.** `stat/report` rows carry `time` in milliseconds and
  the Stats screen plotted them as seconds, so the axis showed unrelated
  clock times. Timestamps are converted before plotting.

- **Event backlog order.** Events loaded at connect now reach the Events
  screen and the dashboard oldest first; the store's unordered snapshot used
  to arrive in arbitrary order.

- **Topology draws the real uplink tree.** The Topology screen used to
  connect every switch to every access point, whatever the cabling. It now
  lays devices out under the device they report as their uplink, gateway at
  the root, with the parent's port number on each wired link, uplinks the
  controller reports as wireless (Session `uplink.type`, now carried as
  `Device::uplink_medium`) dashed, and devices with no reported uplink
  attached to the gateway by a dim dotted line. Boxes are sized to their
  names and wide sibling sets wrap onto extra rows, so at 100% zoom every
  name is drawn whole even at 80 columns.

### Performance

- **TUI memory and idle CPU.** The dashboard used ~100 MB RSS at start and
  grew without bound overnight (250 MB+). Root cause was glibc per-thread
  malloc arena fragmentation fed by a full controller refresh every 10 s
  that parsed the entire `stat/device` / `stat/sta` / `rest/user` payloads
  into flattened `serde_json` maps on a one-worker-per-core tokio runtime.
  - The binary now builds its own tokio runtime: one worker for `tui`, two
    for CLI commands, instead of tokio's one-per-core default. Refresh
    cadence is unchanged.
  - `polling_interval_secs` is now honoured: when no WebSocket is available
    (API-key or cloud auth) and it is shorter than the full refresh, the
    controller polls only per-device statistics from the Integration API in
    between. Results are applied onto the current store records, so a full
    refresh that lands mid-poll keeps its names, ports, and removals.
    Previously the setting was unused.
  - WebSocket `device:sync` decode failures are logged (once at warn) instead
    of silently disabling live stats; all numeric fields accept number or
    string encodings.
  - Session models (`SessionDevice`, `SessionClientEntry`, `SessionUserEntry`)
    carry typed fields instead of a `#[serde(flatten)]` catch-all map; the
    switch-port commands fetch the raw record on demand.
  - WebSocket `device:sync` frames are parsed into a typed `DeviceSync`
    instead of retaining the raw JSON and coalesced into one store snapshot
    per second, merging partial frames field by field. `WebSocketHandle` no
    longer holds an idle broadcast receiver, which had kept every one of the
    ring's 1024 slots (each a full device record) alive indefinitely.
  - Session list responses decode per record: a record with an unexpected
    shape is skipped with a warning instead of failing the whole collection.
  - Session bodies decode from bytes without an intermediate UTF-8 `String`.
  - Dashboard traffic chart records one real sample per second (was four,
    with exponential smoothing interpolating between controller updates that
    arrive ~1/s). The chart still redraws on every 250 ms tick while focused,
    scrolling continuously by the fraction of the interval elapsed and
    holding the last real value out to "now", so motion is smooth without
    plotting invented points. Window is 60 samples (one minute).
- **Binary size.** Release builds use fat LTO, reqwest is built with
  `rustls-no-provider` so only `ring` is linked (aws-lc-rs removed), and 17
  unused dependencies were dropped (six ratatui widget crates, two panic
  handlers, `strum`, `arc-swap`, `async-stream`, `bytes`, `indicatif`,
  `bytesize`, `humantime`, `indexmap`, `flate2`, plus the unused
  `tracing-subscriber/json` feature). Stripped x86_64 binary: 22.0 MB → 18.2 MB.
  A new `release-small` cargo profile (`opt-level = "s"`) builds a 13.0 MB
  binary for low-power hosts at ~5% more TUI CPU.

### Changed

- **`unifly-api` breaking changes.** `SessionDevice`, `SessionClientEntry`,
  and `SessionUserEntry` no longer carry a flattened `extra` map; use
  `SessionClient::get_device_raw` when the full device record is needed.
  `WebSocketHandle::new` takes a `CookieProvider` closure instead of a fixed
  cookie string, and `SessionClient::enable_reauth` with `ReauthCredentials`
  is new. `ControllerConfig` has a new `totp_provider` field, so struct
  literals need `totp_provider: None` or `..Default::default()`.

## [0.10.0] - 2026-08-07

This section also covers 0.9.0, 0.9.1, and 0.9.2, which shipped from the
same running notes. Each of those releases has its own notes on the
[GitHub releases page](https://github.com/hyperb1iss/unifly/releases).

### Added

- **Create commands print the created entity on stdout** in the requested
  `--output` format (`-o plain` emits the new ID), so pipelines can
  capture IDs directly: `unifly networks create ... -o json | jq -r .id`.
  Covers networks, wifi, firewall policies/zones/groups, NAT, ACL, DNS,
  traffic-lists, hotspot vouchers (codes included), and the VPN create
  surface (session records, with private keys and PSKs redacted). The
  human confirmation stays on stderr. `sites create` is unchanged: the
  controller returns no record.

- **TUI auto-reconnect**: a failed controller connection no longer leaves
  the dashboard dead. The data bridge retries with exponential backoff
  (2s doubling to a 30s cap, indefinitely), the status bar shows the
  attempt number while reconnecting, and `Ctrl+r` triggers an immediate
  manual retry when disconnected.
- **Switch port management as code**: `devices ports`, `devices ports-export`,
  and `devices port-set` cover the full per-port surface (mode, native VLAN,
  tagged VLANs, PoE, speed). `port-set --from-file` accepts a JSONC payload
  matching `ApplyPortsRequest` with splice semantics (ports omitted from the
  file keep their existing override; per-port `"reset": true` clears one).
  `ports-export` emits sparse JSONC by default and `--all` for first-time
  bootstrapping. `--with-clients` annotates `ports` output with connected
  clients, and `ports-export --with-clients` injects
  `// last-seen <ISO8601>: <mac>` comments before each port for git-diff
  drift detection on config-as-code repos.
- **`Client::switch_port`** populated from the session-side `sw_port` and
  merged onto integration-only clients via the hybrid join. `clients get`
  detail view shows the wired port when present.
- **`settings` top-level command**: `list / get <KEY> / set <KEY> / export`
  for site-level Session API settings.
- **Full VPN management surface**: `vpn site-to-site` CRUD; `vpn
  remote-access` CRUD plus `suggest-port` and `download-config <id>`; `vpn
  clients` CRUD; `vpn connections` list/get/restart for the legacy v2
  connection inventory; `vpn peers` list/get/create/update/delete plus
  `subnets`; `vpn magic-site-to-site` list/get; `vpn settings`
  list/get/set/patch with `teleport`, `magic-site-to-site`, `openvpn`, and
  `peer-to-peer` flag groups. Most write paths accept `--from-file` for
  declarative config.
- **TUI HyperChart widget** with tachyonfx-driven effects, an Octant Canvas
  bandwidth chart with live legend on the Stats screen, a rank column on
  ranked-bar lists, and a 160ms `fade_from_fg` transition between screens.
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

- **Breaking for `unifly-api` consumers:** the lossy
  `From<DnsPolicyResponse> for DnsPolicy` impl is replaced by `TryFrom`,
  returning a typed `UnrecognizedDnsRecordType` error instead of
  mislabeling unknown record types as `ForwardDomain`.
- **Credential precedence reversed**: when both an explicit `api_key` (or
  `password`) is set in a profile's TOML config and a corresponding keyring
  entry exists, the explicit config value now wins. This is the opposite of
  the older behavior and makes headless and shared-machine workflows
  predictable. See PR #22.
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

- The `[defaults]` config fields `insecure` and `timeout` are now honored
  during profile resolution instead of being parsed and ignored. Precedence
  is CLI flags > env vars > profile > `[defaults]` > builtin. A profile's
  `ca_cert` beats an inherited `[defaults]` `insecure = true`, and
  `--insecure=false` / `UNIFI_INSECURE=false` explicitly re-enables
  verification over any profile or defaults setting (bare `--insecure`
  still means true).
- `config init` now asks whether the controller uses a self-signed
  certificate (defaulting yes for IPs, `.local`/`.lan`-style hosts, and
  single-label hostnames) and writes `insecure = true` into the profile,
  so the canonical UDM/UCG setup no longer produces a profile that fails
  TLS everywhere `-k` isn't passed.
- **NAT policies appear in `nat policies list` under session-only auth.**
  The session-only refresh snapshot omitted NAT entirely, so rules
  created and persisted on the controller were invisible to the list
  command while update and delete still worked against them.
- **DNS policies parse modern `*_RECORD` type tokens** (#27). Controllers
  send `A_RECORD`-style identifiers; the old short-name tables mislabeled
  every non-forward record as `ForwardDomain` with an empty value, and
  `dns create` / `dns update` emitted tokens the controller rejects. One
  canonical mapping now covers read and write, unknown record types are
  skipped with a warning instead of mislabeled, and the CLI shows short
  DNS names (`A`, `CNAME`, `Forward`).
- **Firewall policy lists survive real controller payloads** (#26). Port
  ranges arrive as `start`/`stop` per Ubiquiti's OpenAPI spec; the model
  demanded invented `startPort`/`endPort` names, and one failing policy
  blanked the entire list. Integration list pages now also decode
  per-item, so a single unparseable record can never empty a collection,
  and pagination advances by the server-reported wire count so dropped
  items cannot truncate a scan.
- **The TUI honors CLI flag and env overrides** (#25). The profile path
  dropped `--insecure`, `--site`, `--api-key`, `--totp`, `--no-cache`,
  and `--controller`, so `UNIFI_INSECURE=true unifly tui -k` failed TLS
  against self-signed certs while the CLI worked; connect failures now
  show the reason in the status bar instead of a silent "disconnected".
- **Profile `timeout` takes effect.** The `--timeout` flag's baked-in
  clap default made every profile timeout dead weight; resolution now
  follows flag/env, then profile, then the 30-second default, including
  cloud transports.
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

[Unreleased]: https://github.com/hyperb1iss/unifly/compare/v0.10.0...HEAD
[0.10.0]: https://github.com/hyperb1iss/unifly/compare/v0.8.0...v0.10.0
[0.8.0]: https://github.com/hyperb1iss/unifly/compare/v0.7.0...v0.8.0
[0.7.0]: https://github.com/hyperb1iss/unifly/compare/v0.6.0...v0.7.0
[0.6.0]: https://github.com/hyperb1iss/unifly/compare/v0.2.0...v0.6.0
[0.2.0]: https://github.com/hyperb1iss/unifly/compare/v0.1.1...v0.2.0
[0.1.1]: https://github.com/hyperb1iss/unifly/compare/v0.1.0...v0.1.1
[0.1.0]: https://github.com/hyperb1iss/unifly/releases/tag/v0.1.0
