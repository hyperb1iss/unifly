# AGENTS.md

Instructions for AI agents working on the **unifly** codebase. This file is the
single source of truth for repository conventions. `CLAUDE.md` is a symlink to
this file. Cursor, Codex, Aider, Cline, and Claude Code all read from here.

For end-user documentation see `README.md`. For contributor workflow see
`CONTRIBUTING.md`. For how agents should _use_ the unifly CLI at runtime see
`skills/unifly/SKILL.md` (a separate audience from this file).

---

## What This Repository Is

unifly is a Rust CLI and TUI for managing Ubiquiti UniFi network
infrastructure. A single `unifly` binary ships three user-facing surfaces:

- **CLI commands** (`unifly devices list`, etc.): 27 top-level commands
  covering devices, clients, networks, WiFi, firewall, NAT, DNS, ACL,
  traffic-lists, hotspot, events, stats, DPI, VPN, Wi-Fi observability
  (neighbors, channels, roams, experience), and a raw `api` escape hatch.
- **TUI dashboard** (`unifly tui`) -- 10-screen Ratatui interface for real-time
  monitoring and interactive management.
- **Agent skill** at `skills/unifly/SKILL.md`: bundled documentation that
  teaches AI agents to drive the CLI.

The binary is powered by the `unifly-api` library crate, which is also
published independently on crates.io for Rust developers building custom
integrations.

The moat is **dual-API coverage**: unifly speaks both the modern Integration
API (REST, API key) and the Session API (cookie + CSRF) and reconciles
data between them via Hybrid auth mode. Most competing tools are one or the
other.

---

## Quick Command Reference

The project uses **just** for task orchestration. All recipes live in the
`justfile` at the repo root.

```bash
# The canonical "before commit" gate
just check              # fmt-check + clippy + test

# Individual gates
just fmt-check          # nightly rustfmt, read-only
just clippy             # cargo clippy --workspace --all-targets
just test               # cargo test --workspace

# Fix-it automation
just fix                # clippy --fix + cargo fmt --all
just fmt                # cargo fmt --all

# Running
just cli <args>         # cargo run -p unifly -- <args>
just tui <args>         # cargo run -p unifly -- tui <args>

# Targeted tests
just test-crate unifly-api
just test-verbose       # cargo test --workspace -- --nocapture
just snap-review        # cargo insta review for snapshot tests

# Build
just build              # debug build (workspace)
just build-release      # release build (workspace)

# Install
just install            # cargo install --path crates/unifly (CLI + TUI)
just install-cli        # CLI without TUI dependencies

# Docs and cleanup
just doc                # cargo doc --workspace --open
just clean              # cargo clean
```

**Always run `just check` before committing.** It is the same gate CI runs.

---

## Workspace Layout

This is a **2-crate Cargo workspace** (not 5, despite older notes):

```
crates/
  unifly-api/         # Library: HTTP/WS transport, Controller, DataStore, model
  unifly/             # Single binary: CLI commands + TUI dashboard (feature-gated)
```

Dependency chain: `unifly` depends on `unifly-api`.

**Edition 2024. MSRV 1.94. Resolver 3. Workspace version is pinned in
`[workspace.package]` at `Cargo.toml`.** All member crates inherit the
version via `version.workspace = true`.

Published to crates.io: `unifly-api`, `unifly`. Private/internal artifacts
live under `aur/`, `homebrew-tap` (separate repo), and `skills/unifly/` (for
ClawHub and Claude Code plugin).

---

## Architecture Essentials

### Dual-API Transport

`crates/unifly-api/src/` splits transport into two clients:

- **`integration/`**: `IntegrationClient`, `X-API-KEY` auth, modern REST
  endpoints at `/proxy/network/integration/v1/`. Returns clean JSON with
  UUIDs. Covers configuration CRUD (networks, wifi, firewall, nat, dns,
  acl, hotspot, traffic-lists, wans).
- **`session/`**: `SessionClient`, session cookie + CSRF for session auth,
  plus `X-API-KEY` on UniFi OS session HTTP endpoints. Uses envelope-wrapped
  responses at `/proxy/network/api/` and `/proxy/network/v2/api/`. Covers
  events, stats, Wi-Fi/client observability, device commands, admin,
  backups, DPI control. Session WebSocket still requires a session cookie.

Both clients share a common `TransportConfig` and `TlsMode`. TLS modes:
`SystemDefaults`, `AcceptInvalid`, `CustomPem`. Credentials are wrapped in
`secrecy::SecretString` throughout. **Never log or display them.**

### Controller Facade

`crates/unifly-api/src/controller/` wraps everything behind a single
`Controller` type:

```rust
pub struct Controller(Arc<ControllerInner>);
```

The `Arc` makes `Controller` cheaply cloneable across async tasks. Key
operations live in submodules:

- `lifecycle.rs`: connect, disconnect, reconnect
- `runtime.rs`: background refresh loop, command processor
- `query.rs` / `query/`: read-side operations
- `commands/`: write-side operations (`execute(CoreCommand)`)
- `payloads.rs` / `payloads/`: request body construction
- `refresh.rs`: periodic data sync
- `subscriptions.rs`: WebSocket event fan-out
- `session_queries.rs`: Session-specific read paths, including `raw_get`/
  `raw_post` used by the `unifly api` command

### Reactive DataStore

`crates/unifly-api/src/store/` implements lock-free reactive storage:

- `EntityCollection<T>`: `DashMap` for lookup, `tokio::watch::Sender<Vec<T>>`
  for broadcast
- `DataStore` -- aggregate of 13 `EntityCollection`s (devices, clients,
  networks, wifi_broadcasts, firewall_policies, firewall_zones, nat_policies,
  acl_rules, dns_policies, traffic_matching_lists, vouchers, sites, events)
  plus `watch::Sender` fields for site_health, last_full_refresh, and
  last_ws_event
- `EntityStream<T>`: subscriber handle wrapping `watch::Receiver`; provides
  `current()`, `latest()`, `changed().await`

**Serde `rc` feature is required** because `Arc<T>` is serialized for JSON
output. That is set via `serde = { features = ["derive", "rc"] }` in the
workspace dependencies.

### Entity Identifiers

```rust
pub enum EntityId {
    Uuid(Uuid),      // Integration API records
    Legacy(String),  // Session API records (string _id fields)
}
```

Synthetic keys used for non-MAC entities: `net:{id}`, `wifi:{id}`,
`fwp:{id}`, `fwz:{id}`, `acl:{id}`, etc. Some domain types (`Client`,
`Device`) use MAC as the natural key.

### Auth Modes

```rust
pub enum AuthCredentials {
    ApiKey(SecretString),
    Credentials { username: String, password: SecretString },
    Hybrid { api_key: SecretString, username: String, password: SecretString },
    Cloud { api_key: SecretString },  // Site Manager API, not yet implemented
}
```

**ApiKey mode is enough for most HTTP work on UniFi OS controllers.**
`connect()` builds a `SessionClient` with `X-API-KEY`, so `clients list`,
`devices list`, `topology`, device commands, stats, Wi-Fi observability
commands (`wifi neighbors`, `wifi channels`, `clients roams`, `clients wifi`),
admin operations, DHCP reservations, and `events list` can all use session
HTTP without a password. Use **Hybrid** when you need session WebSocket
features such as `events watch`, or when you want maximum compatibility
across controller variants. Merge still happens inline in `full_refresh()`
in `controller/refresh.rs`.

---

## CLI Architecture

### Module Layout

```
crates/unifly/src/cli/
  args.rs                    # Top-level Cli struct, Command enum, GlobalOpts
  args/
    common.rs                # GlobalOpts, ListArgs, OutputFormat
    <entity>.rs              # 22 entity-specific files (23 total with common.rs)
  commands/
    mod.rs                   # dispatch() router
    util/
      access.rs              # ensure_integration_access / ensure_session_access
    <entity>.rs              # handler per entity
    <entity>/                # for entities with multiple subhandlers
  error.rs                   # CliError (thiserror + miette)
  output.rs                  # Table/JSON/YAML/plain format dispatch
```

The pattern for every entity: `args/<entity>.rs` defines the clap structs,
`commands/<entity>.rs` (or `<entity>/handler.rs`) implements `async fn
handle(controller, args, global) -> Result<(), CliError>`. Dispatch happens
in `commands/mod.rs::dispatch()` via match on `Command`.

**To add a new top-level command:**

1. Add a new variant to `Command` in `cli/args.rs`
2. Create `cli/args/<new>.rs` with the clap subcommand definitions
3. Create `cli/commands/<new>.rs` with `handle(controller, args, global)`
4. Add the dispatch arm in `cli/commands/mod.rs::dispatch()`
5. Update `skills/unifly/SKILL.md` command inventory (do not skip this)
6. Update `skills/unifly/references/commands.md` gotchas section

### Access Gate Gotcha

`crates/unifly/src/cli/commands/util/access.rs` defines
**`ensure_integration_access`** and **`ensure_session_access`**. The
Integration gate is called by 7 handlers (acl, dns, firewall, hotspot,
networks, traffic_lists, wifi). The Session gate is called by `nat` and
`events`. New commands should add the appropriate gate call for clean
error messages when the auth mode is insufficient.

### Output Formats

`cli/output.rs` dispatches on `OutputFormat`:

```rust
pub enum OutputFormat {
    Table,         // default, human-facing, uses tabled crate
    Json,          // pretty JSON
    JsonCompact,   // single-line JSON per record
    Yaml,          // serde_yaml_ng
    Plain,         // one ID per line for xargs pipelines
}
```

Every list/get handler uses `output::render(format, data)` to emit results.
Table rendering requires implementing `tabled::Tabled` on a row struct
(usually a local struct in the handler that borrows from the domain model).

### Global Flags and Environment

`cli/args/common.rs` defines `GlobalOpts` with clap `env` attributes. The
env var prefix is **`UNIFI_`**, not `UNIFLY_`. Exception: `UNIFLY_THEME`
(TUI only).

Hidden/undocumented flag: `--totp` (with `UNIFI_TOTP` env). Not in `--help`
but functional. Used for MFA controllers and 1Password CLI integration via
`totp_env` in config.

`--no-cache` forces a fresh Session login, bypassing the session cookie
cache stored under the system config dir.

---

## TUI Architecture

```
crates/unifly/src/tui/
  mod.rs                     # launch() entry point
  terminal.rs                # alternate screen, raw mode, cleanup guard
  theme.rs                   # semantic opaline adapter
  event.rs                   # crossterm -> Action translation
  action.rs                  # Action enum
  component.rs / screen.rs   # traits
  data_bridge.rs             # Controller streams -> App state
  app.rs / app/              # top-level App state + run loop
  screens/                   # 10 screens (Dashboard, Devices, Clients,
                             #  Networks, Firewall, Topology, Events, Stats,
                             #  Onboarding, Settings)
  widgets/                   # shared custom widgets
  forms/                     # editable form overlays (Networks, Settings)
```

`mod.rs::launch()` sets up file-based tracing (stderr is unavailable in alt
screen mode), installs panic hooks, initializes the opaline theme, builds a
Controller, and runs `App::run().await`.

Theme access is a global singleton: `opaline::current()` returns
`Arc<Theme>`, cheap to clone per-frame. `theme.rs` is a semantic adapter
that maps opaline tokens to unifly-specific color accessors (e.g.
`unifly.tx_fill`, `unifly.chart.0-5`).

`data_bridge.rs` subscribes to `EntityStream<T>` channels from the
Controller's DataStore and pushes updates into App state. Screens read
from App state during render. No direct Controller access from screens.

Key integration: the `ThemeSelector` widget from `opaline::widgets` is
embedded as a settings overlay. It holds an `Arc<Theme>` snapshot at open
time for exact rollback on Esc (the pattern was contributed back from the
git-iris bug fix).

---

## Error Handling

Two error types, composed via `From` impls:

- **`CoreError`** (`unifly-api/src/core_error.rs`): library-level, 14+
  variants, one `From<unifly_api::Error>` impl mapping transport errors
- **`CliError`** (`unifly/src/cli/error.rs`): CLI-level, `thiserror` +
  `miette` for rich terminal diagnostics, wraps `CoreError`

**Conventions:**

- Use `thiserror` for library errors, never `anyhow` in `unifly-api`
- CLI error messages are formatted by `miette` with color and snippets
- **Never `.unwrap()`** on user-provided data. Workspace lint
  `clippy::unwrap_used = "deny"` enforces this
- `?` is the standard propagation. Use `.map_err(CliError::from)` or
  `wrap_err` when crossing layer boundaries

---

## Testing

### Layout

```
crates/unifly-api/tests/
  integration_client_test.rs     # wiremock-based Integration API tests
  session_client_test.rs         # wiremock-based Session API tests
  controller_runtime_test.rs     # Controller lifecycle + refresh loop

crates/unifly/tests/
  cli_test.rs                    # assert_cmd-based CLI tests (fast, no controller)
  e2e_test.rs                    # end-to-end tests against a simulation controller

tests/fixtures/                  # repo-root shared test data
```

Unit tests are inline in source files under `#[cfg(test)] mod tests`.
The e2e suite (`e2e_test.rs`) spins up a wiremock-backed simulation
controller and runs full command flows including auth, refresh, and
output validation.

### Libraries

- **wiremock**: mock HTTP servers for Integration/Session tests and the
  e2e simulation controller. Serves static JSON fixtures from
  `tests/fixtures/`.
- **insta**: snapshot tests for output formatting. `just snap-review`
  opens the interactive UI to approve changes. Snapshot files live next to
  the test as `.snap` or `.snap.new`.
- **assert_cmd** + **predicates**: CLI tests that spawn the built binary.
- **tempfile**: per-test config dir isolation so tests don't touch
  `~/.config/unifly/`.
- **tokio-test**: poll-based async unit tests.
- **pretty_assertions**: better diffs on assertion failures.

`insta` is built with `opt-level = 3` in `[profile.dev.package.insta]` so
snapshot diffing is fast even in debug builds.

### Policy

- Unit tests should be pure and deterministic. No real network calls.
- Integration tests use wiremock or assert_cmd, never a real controller.
- Tests must not require a specific UniFi hardware or firmware version.
- The TUI has no automated test coverage yet. Adding tests here is
  welcomed.

---

## Code Policies

### Lint Configuration

`Cargo.toml` enforces a strict workspace-wide lint profile:

```toml
[workspace.lints.rust]
unsafe_code = "forbid"              # no unsafe anywhere, period

[workspace.lints.clippy]
all      = { level = "deny" }
perf     = { level = "deny" }
pedantic = { level = "deny" }

unwrap_used              = "deny"   # use ?, .ok(), .unwrap_or()
enum_glob_use            = "deny"
out_of_bounds_indexing   = "deny"
undocumented_unsafe_blocks = "deny"

# numeric casts produce warnings (not denials, to allow pragmatism)
cast_precision_loss      = "warn"
cast_possible_truncation = "warn"
cast_sign_loss           = "warn"

# complexity and modernization warnings
too_many_lines           = "warn"
cognitive_complexity     = "warn"
future_not_send          = "warn"
manual_let_else          = "warn"
semicolon_if_nothing_returned = "warn"
```

A few pedantic rules are explicitly allowed for pragmatic reasons:
`module_name_repetitions`, `significant_drop_tightening`,
`must_use_candidate`, `return_self_not_must_use`, `doc_markdown`,
`missing_errors_doc`, `missing_panics_doc`. Do not disable other pedantic
lints without a strong reason.

### Format

`rustfmt.toml` pins: edition 2024, 100-char max width, field init
shorthand, try shorthand. **Nightly rustfmt is required** for stable
output (`rustup component add rustfmt --toolchain nightly`). `just fmt`
runs the nightly formatter.

### Secrets

All credentials flow through `secrecy::SecretString`. Do not:

- Log secret values via `tracing` or `println!`
- Deserialize them into plain `String`
- Embed them in error messages
- Store them in plain files outside the OS keyring unless the user
  explicitly opts in via `auth_mode` config

### Dependencies

`deny.toml` enforces:

- Vulnerability advisories fail the build
- License allowlist: MIT, Apache-2.0, BSD-2/3-Clause, ISC, Unicode-3.0,
  Unicode-DFS-2016, Zlib, OpenSSL, MPL-2.0
- Wildcards are denied
- Git sources are denied by default (all deps must come from crates.io)

Run `cargo deny check` before adding or upgrading dependencies. New
licenses require a deliberate policy update.

---

## Release Workflow

Releases use the shared workflow at
`hyperb1iss/shared-workflows/.github/workflows/rust-release.yml`.

### Steps (handled automatically once triggered)

1. `.github/workflows/release.yml` is triggered manually via
   `workflow_dispatch` with a `version` or `bump` input.
2. The shared workflow bumps the workspace version in `Cargo.toml`, runs
   the full build + test + clippy gate, commits the bump, tags `vX.Y.Z`,
   pushes the tag.
3. The tag push triggers `.github/workflows/cicd.yml` which:
   - Rebuilds and tests via the shared rust-ci workflow
   - Builds release artifacts for 4 targets (linux amd64+arm64, macOS
     arm64, Windows gnu)
   - Publishes `unifly-api` and `unifly` to crates.io (shared rust-publish)
   - Creates a GitHub Release with all artifacts and git-iris-generated
     notes
   - Updates the Homebrew formula in `hyperb1iss/homebrew-tap`

### What Is NOT Automated Yet

- ClawHub skill publish (`npx clawhub publish ./skills/unifly`). This is
  on the CI roadmap. For now, republish manually after each release.
- Plugin manifest version sync. **`.claude-plugin/plugin.json`,
  `.claude-plugin/marketplace.json`, and `.cursor-plugin/plugin.json` must
  be updated by hand** to match the new workspace version. They have
  drifted before and caused a ClawHub publish at the wrong version.
- AUR package update (`just aur-update <version>` must be run by hand).

When bumping the version, always update the plugin manifests in the same
commit.

---

## Sacred Rules for Agents

1. **Never `git push` without explicit approval.** Bliss handles all
   pushes. Commit locally, show the result, wait.
2. **Never force-push to main without a `--force-with-lease` and an
   explicit confirmation.** The main branch is shared. Force pushes have
   gone wrong before.
3. **Never bypass hooks** with `--no-verify`, `--no-gpg-sign`, or similar.
   If a hook fails, investigate. Do not route around it.
4. **Never commit secrets, API keys, or the `token/` directory.** That
   directory is `.gitignore`d but guard against accidental `git add -A`.
   Prefer adding files by name.
5. **Never commit `docs/plans/`**. It is a scratch area for design notes
   and tracking docs that should not ship. Treat it like a gitignored
   notebook even though it is not currently gitignored.
6. **Never delete or modify files you did not touch.** Other agents may be
   working in this repo simultaneously. Respect their in-progress work.
7. **Never run destructive operations** (`cargo clean` in CI paths,
   `git reset --hard`, `git clean -fd`, etc.) without explicit approval.
8. **Never `.unwrap()` in production code paths.** The `unwrap_used` lint
   will fail the build. Use `?`, `.ok()`, `.unwrap_or_default()`, or
   proper error propagation.
9. **Never introduce `unsafe` code.** `unsafe_code = "forbid"` at the
   workspace level. There is no justification in this codebase.
10. **Never remove or weaken an existing clippy lint** to work around a
    warning. Fix the code instead.

---

## Plugin Manifests

Three plugin manifests must stay in sync with `workspace.package.version`:

```
.claude-plugin/plugin.json          # Claude Code plugin
.claude-plugin/marketplace.json     # Claude Code marketplace listing
.cursor-plugin/plugin.json          # Cursor marketplace plugin
```

The `version` field in each must match the workspace version exactly.
These have drifted before. If the workspace is at `0.8.0`, all three must
say `0.8.0`.

When releasing, patch them manually before the release commit, or add a CI
step (not yet implemented) to patch them automatically in the shared
workflow.

---

## Non-Obvious Gotchas

### UniFi API Quirks

- **CSRF tokens rotate.** The Session client captures `X-CSRF-Token` on
  login and updates it from `X-Updated-CSRF-Token` on every response. Do
  not cache the token across requests; trust the rotating value.
- **UniFi OS wraps some errors as HTTP 200.** Response bodies can look
  like `{"error": {"code": N, "message": "..."}}` with a 200 status. The
  envelope decoder must detect this before parsing the `data` field.
- **`stat/admin` is controller-level, not site-scoped.** Use `api_url`
  not `site_url` for admin operations.
- **Integration field names differ from expectations.** `networks get`
  uses `hostIpAddress` (not `host`), `prefixLength` (not `prefix`),
  `dhcpConfiguration.mode/leaseTimeSeconds/ipAddressRange` (not
  `dhcp.server.*`). The `networks list` endpoint returns SUMMARY data
  without `ipv4Configuration`. Must fetch each network individually to
  get full config. See `convert.rs::parse_network_fields` which handles
  both old and new styles.
- **Integration clients lack fields** the UI shows: `wireless`,
  `uplink_device_mac`, `vlan`, `tx_bytes`, `rx_bytes`, `hostname`. These
  must come from Session API. Hybrid merge is by IP-address match in
  `controller::refresh::full_refresh`.
- **`stat/rogueap` uses epoch seconds**, not milliseconds. Passing
  millisecond-style values or assuming stats-report semantics returns empty
  data silently.
- **`wifiman/{ip}/` band field is `wlan_band`**, not `band`. Values are
  `2.4g` / `5g` / `6g` (station data uses `ng` / `na` / `6e`). Uplink
  devices use `display_name` (not `device_name`/`name`) and `experience`
  (not `wifi_experience`). Neighbor signal is nested:
  `signal: [{"signal": -67, "signal_type": "AP_AP"}]`.
- **`stat/report/*.ap` and `*.site` use different attribute prefixes**.
  `.ap` expects `ng-cu_total`; `.site` expects `ap-ng-cu_total`.
- **`system-log/client-connection/{mac}`** requires MAC duplication in both
  the URL path and the `?mac=` query parameter. The response uses a deeply
  nested `parameters` structure: event details live under
  `parameters.DEVICE_FROM.name`, `parameters.WLAN.name`,
  `parameters.SIGNAL_STRENGTH.name`, `parameters.RADIO_BAND.name`,
  `parameters.CHANNEL.name`, etc. — **not** flat top-level fields.
- **`stat/current-channel`** returns country-level regulatory data, not
  per-radio rows. Channel lists are keyed by band: `channels_ng` (2.4 GHz),
  `channels_na` (5 GHz), `channels_na_dfs`, `channels_6e` (6 GHz), plus
  width-specific variants (`channels_na_40`, `channels_6e_80`, etc.) and
  AFC data. Country is identified by `code` (numeric, e.g. `"840"`),
  `key` (e.g. `"US"`), and `name` (e.g. `"United States"`).
- **Session v2 observability routes return raw JSON**, not the classic
  `{meta, data}` envelope. Use `get_raw()` / `raw_get()` patterns.
- **Device `radios` is always empty**. Parsing from the `interfaces`
  JSON is not yet implemented. Known gap.

### CLI Quirks

- **Default list limit is 25** with a silent truncation hint. Agents and
  scripts should pass `--all` or `--limit 200+` for enumeration.
- **`events watch --types` takes `EventCategory` enum values** (Device,
  Client, Network, System, Admin, Firewall, Vpn, Unknown) case-insensitive,
  **not `EVT_*` glob patterns**. The matching logic is in
  `cli/commands/events.rs::watch_events`.
- **`admin revoke <ADMIN>` is positional**, not a `--email` flag.
- **`nat policies update`** uses `--name` or `--description` (mutually
  exclusive) for the display label. Both map to the v2 API `description`
  field. The `--name` flag is the user-friendly alias.
- **`firewall policies patch`** is a fast partial-update path for toggling
  `enabled`/`logging`; use it instead of `update` when only those fields
  change.
- **`networks refs <id>`** is the only command that answers "what depends
  on this entity before I delete it." No equivalent exists for other
  entities yet.
- **`unifly api <path>`** routes through the Session client and handles
  CSRF automatically, so it can reach Session v1, v2, and Integration
  endpoints without caring about auth mode.
- **`clients roams` and `clients wifi`** accept any client identifier
  (name, hostname, IP, or MAC). Resolution uses the in-memory snapshot,
  so the client must appear in `clients list`. `roams` resolves to MAC;
  `wifi` resolves to IP.
- **`wifi neighbors`** defaults to 25 results. Use `--all` or `--limit N`
  to see more.
- **Serde defaults to PascalCase** for enums without `#[serde(rename_all
= "...")]`. When writing JSON payload files for `--from-file`, use
  `"Gateway"` not `"gateway"`, `"Wpa2Personal"` not `"wpa2_personal"`.
  Exception: `FirewallAction` has a custom deserializer that accepts
  lowercase.

### TUI Quirks

- **Do not `println!` or `eprintln!` in the TUI.** Stderr and stdout are
  captured by the alternate screen. All TUI logging goes to a file via
  `tracing_appender` with a `WorkerGuard` held by `launch()`.
- **Panic hooks must be installed before terminal setup** so that panics
  restore the terminal state before crashing. `terminal::install_hooks`
  handles this.
- **Controller reconnect lifecycle is broken.** The internal
  `CancellationToken` becomes permanent after the first disconnect.
  Reconnect does not work correctly yet. Known gap, documented so
  agents do not waste time chasing a fix unless it is the explicit task.

---

## File Organization Conventions

### Source Files

- One entity per file under `cli/args/` and `cli/commands/`.
- When a command has many subhandlers (e.g. `devices`, `clients`,
  `firewall`, `config_cmd`, `acl`), split into a subdirectory with
  `mod.rs` and per-subcommand files.
- Domain types live in `unifly-api/src/model/<entity>.rs` and are
  re-exported from `lib.rs`.
- Request structs for mutations live in
  `unifly-api/src/command/requests/<group>.rs`.

### Documentation Files

- `README.md`: end-user install + usage
- `CONTRIBUTING.md`: contributor onboarding
- `CHANGELOG.md`: version history
- `ROADMAP.md`: forward-looking plans
- `AGENTS.md` / `CLAUDE.md`: this file (CLAUDE.md is a symlink)
- `skills/unifly/SKILL.md`: agent skill for USING the CLI
- `docs/images/`: committed screenshots and GIFs
- `docs/plans/` --**scratch area, do not commit contents** (not currently
  gitignored but treat as ephemeral)

### Scratch Areas

- `docs/plans/`: design docs and session tracking
- `research/`: external research, API audit notes
- `specs/`: early project specs, some outdated
- `target/`: cargo build artifacts (gitignored)

---

## When Making Changes

### Adding a New Command

1. Define clap args in `cli/args/<cmd>.rs`
2. Add `Command::<Cmd>(args)` variant in `cli/args.rs`
3. Implement `handle()` in `cli/commands/<cmd>.rs`
4. Add dispatch arm in `cli/commands/mod.rs::dispatch`
5. Call `util::access::ensure_integration_access` if Integration-only
6. Update `skills/unifly/SKILL.md` command inventory (count + table)
7. Update `skills/unifly/references/commands.md` with gotchas
8. Add a happy-path test in `crates/unifly/tests/cli_test.rs`

### Adding a New API Endpoint

1. Add the request type to `unifly-api/src/command/requests/<group>.rs`
2. Add the implementation to `unifly-api/src/controller/commands/<group>.rs`
3. Add wiremock-based tests in
   `crates/unifly-api/tests/integration_client_test.rs` or
   `session_client_test.rs`
4. Re-export from `unifly-api/src/lib.rs` if it is part of the public API
5. Update the CLI surface if the endpoint needs a command

### Adding a New Domain Type

1. Define in `unifly-api/src/model/<entity>.rs`
2. Add conversion logic in `unifly-api/src/convert.rs` (handles both
   Integration and Session field shapes)
3. Add an `EntityCollection<T>` to `unifly-api/src/store/data_store.rs`
   if the type is reactive
4. Re-export from `unifly-api/src/lib.rs`

### Updating the Agent Skill

The skill at `skills/unifly/SKILL.md` has its own word budget and
structure (see `skills/unifly/references/` for detailed content). When
adding or renaming CLI commands, update:

- `skills/unifly/SKILL.md` command inventory table
- `skills/unifly/references/commands.md` per-command section
- `skills/unifly/references/concepts.md` dual-API gate matrix if the
  command affects auth mode requirements
- `skills/unifly/references/workflows.md` if the change enables a new
  differentiator pattern worth highlighting

Update `skills/unifly/examples/*.json` if the payload shape for
`--from-file` changes.

---

## Pointers

- **`README.md`**: end-user documentation, install, features, TUI
  screenshots
- **`CONTRIBUTING.md`**: PR workflow, code style overview
- **`CHANGELOG.md`**: version history
- **`ROADMAP.md`**: planned features and known gaps
- **`skills/unifly/SKILL.md`**: agent skill for _using_ the CLI (not for
  developing it)
- **`Cargo.toml`**: workspace version, lints, dependencies, profiles
- **`justfile`**: task recipes
- **`.github/workflows/`**: CI + release + docs workflows
- **`docs/images/`**: shipped screenshots and the animated TUI tour GIF
- **`aur/update-aur.sh`**: AUR package refresh script used by
  `just aur-update`
