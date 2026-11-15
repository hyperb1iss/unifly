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

- **CLI commands** (`unifly devices list`, etc.): 29 top-level commands
  (28 without the `tui` feature) covering devices and switch port management
  (`ports / ports-export / port-set`), clients, networks, WiFi, firewall
  (policies, zones, and groups), NAT, DNS, ACL, traffic-lists, hotspot,
  events, stats, DPI, VPN (servers, tunnels, site-to-site, remote-access,
  clients, connections, peers, magic-site-to-site, settings), Wi-Fi
  observability (neighbors, channels, roams, experience), site settings
  (`settings`), cloud fleet queries, and a raw `api` escape hatch.
- **TUI dashboard** (`unifly tui`) -- 11-screen Ratatui interface for real-time
  monitoring and interactive management.
- **Agent skill** at `skills/unifly/SKILL.md`: a discovery stub that loads
  version-matched guidance through `unifly skill`.

The binary is powered by the `unifly-api` library crate, which is also
published independently on crates.io for Rust developers building custom
integrations.

The moat is **triple-path coverage**: unifly speaks the modern Integration
API (REST, API key), the Session API (cookie + CSRF), and Site Manager cloud
fleet/connector APIs. Most competing tools are one path or two; unifly can
mix local and cloud workflows in one binary.

---

## Quick Command Reference

The project uses **just** for task orchestration. All recipes live in the
`justfile` at the repo root.

```bash
# The canonical "before commit" gate
just check              # fmt-check + maintainability + clippy + test

# Individual gates
just fmt-check          # nightly rustfmt + prettier, read-only
just maintainability    # scripts/check-maintainability.sh line-count gate
just clippy             # cargo clippy --workspace --all-targets
just test               # cargo test --workspace

# Fix-it automation
just fix                # clippy --fix + cargo fmt + prettier --write
just fmt                # cargo fmt --all + prettier --write

# Running
just cli <args>         # cargo run -p unifly -- <args>
just tui <args>         # cargo run -p unifly -- tui <args>

# Targeted tests
just test-crate unifly-api
just test-verbose       # cargo test --workspace -- --nocapture
just snap-review        # cargo insta review for snapshot tests

# End-to-end (dockerized controller, feature-gated)
just e2e                # full lifecycle: up, wait, test, tear down
just e2e-up             # start the controller container
just e2e-wait           # poll until the controller is ready
just e2e-build          # compile the gated e2e binary without running it
just e2e-test           # run the suite against a running controller
just e2e-down           # stop and remove the container

# Build
just build              # debug build (workspace)
just build-release      # release build (workspace)

# Install
just install            # cargo install --path crates/unifly (CLI + TUI)
just install-cli        # CLI without TUI dependencies

# Docs and cleanup
just doc                # cargo doc --workspace --no-deps --open
just docs-build         # Zola docs site build + llms.txt generation
just docs-serve         # Zola docs site with live reload
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
- `DataStore` -- aggregate of 14 `EntityCollection`s (devices, clients,
  networks, wifi_broadcasts, firewall_policies, firewall_zones, firewall_groups,
  nat_policies, acl_rules, dns_policies, traffic_matching_lists, vouchers,
  sites, events)
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
    Cloud { api_key: SecretString, host_id: String },
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

**Cloud mode now works for both fleet and connector paths.** `unifly cloud ...`
talks directly to the Site Manager fleet API at `api.ui.com/v1/`, while
regular Integration-backed commands can tunnel through the cloud connector
when `auth_mode = "cloud"` or `--host-id` is set. Session-only commands still
require direct controller/session access.

---

## CLI Architecture

### Module Layout

```
crates/unifly/src/cli/
  args.rs                    # Top-level Cli struct, Command enum, GlobalOpts
  args/
    common.rs                # GlobalOpts, ListArgs, OutputFormat
    <entity>.rs              # 24 entity-specific files (25 total with common.rs)
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
5. Verify `unifly skill commands` includes the new command automatically
6. Update `crates/unifly/skill/commands.md` for behavior help cannot convey

### Access Gate Gotcha

`crates/unifly/src/cli/commands/util/access.rs` defines
**`ensure_integration_access`** and **`ensure_session_access`**. The
Integration gate is called by 8 handler files (acl, dns, hotspot,
networks, traffic_lists, wifi, vpn for servers/tunnels, and firewall
for policies and zones). The Session gate is called by `events`,
`nat`, `firewall` (groups subgroup and group-name resolution inside
policies), `devices` (ports / ports-export / port-set), and `vpn`
(status, health, site-to-site, remote-access, clients, connections,
peers, magic-site-to-site, settings). The `firewall` and `vpn`
handlers dispatch the gate per-subcommand rather than at the top
level. The top-level `settings` command has no gate call; insufficient
auth surfaces as a raw session-client error. New commands should add
the appropriate gate call for clean error messages when the auth mode
is insufficient.

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
`totp_env` in config. `totp_env` names an env var read once at startup, so
both give a single-use code that serves only the first login. The
profile's `totp_command` (argv array, no shell, 10 s timeout, see
`config/totp_command.rs`) becomes a `TotpProvider` that runs for every
re-login, which is what lets an MFA session renew when its cookie expires.
CLI and TUI launch and `profile_to_controller_config` resolve TOTP through
the shared controller configuration builder and `resolve_totp_provider`.
Never wrap a value captured once as
a provider; the session client refuses to resend a spent code anyway.

`--no-cache` forces a fresh Session login, bypassing the session cookie
cache stored under the system config dir.

Value resolution follows CLI flag/env > profile > `[defaults]` >
builtin (30-second timeout, system TLS verification). A profile's
explicit `ca_cert` beats an inherited `defaults.insecure = true`,
since a configured CA still means verification. `--insecure` /
`UNIFI_INSECURE` is tri-state (`Option<bool>` with `require_equals`):
bare `-k` means true, `--insecure=false` forces verification over any
profile or defaults value. `--timeout` has no clap default; the
30-second fallback is applied during resolution so profile and
`[defaults]` timeouts take effect.

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
    navigation.rs            #  key, paste, and mouse routing
    render.rs                #  chrome, overlays, GLOBAL_KEYS / FORM_KEYS
    notify.rs                #  toasts and the notification history
    commands.rs              #  confirm, run_command, spawn_op
    dispatch.rs              #  App-level action handling
    <area>.rs                #  per-area command and fetch handling
  screens/                   # 11 screens (Dashboard, Devices, Clients,
                             #  Networks, Firewall, Topology, Events, Stats,
                             #  Wifi, Onboarding, Settings)
  widgets/                   # shared widgets: click (hit map, key hints),
                             #  line_input, wrap, detail_grid, sub_tabs,
                             #  toolbar
  forms/                     # profile draft, connection test, and field
                             #  widgets shared by Onboarding and Settings
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

**Key routing** lives in `app/navigation.rs::handle_key_event`, in this
order: `Ctrl+c` (quit) and `Ctrl+l` (repaint) from every state; `F1`
help; an open confirm dialog; an open prompt; the help and notification
overlays; Setup and Settings, which take every remaining key; the
search bar; the About overlay; `Ctrl+r` (not while the screen captures
input, since a refresh could swap the record a form is editing); the
active screen; and last the global keys (`handle_global_key_event`). A
screen sees a key before the globals do, so a key it returns `None` for
falls through to them.

Each screen implements `Component` (`component.rs`):

- `help()` returns `(keys, description)` for the mode on screen and
  feeds the help overlay, beside `GLOBAL_KEYS` (or `FORM_KEYS` while a
  form has the keyboard: Setup, Settings, the Networks form). The `/`
  row is dropped where `search_query()` is `None`. List screens start
  from `LIST_HELP`.
- `captures_input()` is true while a field has the keyboard. Keys then
  reach the screen raw and never go on to the global shortcuts, and
  `handle_paste()` receives pasted text.
- `search_query()` returns `Some` where `/` filters something; search
  only opens there.
- `hints()` returns ranked `click::Hint`s, `(rank, hint)`, for what the
  selected row or the screen can do right now. Rank 0 matters most, and
  the highest ranks are dropped first on a narrow terminal.

**One footer.** A screen does not draw a hint row. The status bar
(`render_status_bar` in `app/render.rs`) draws the active screen's
`hints()` after the connection state, as many as fit, and keeps the
app-wide buttons from `status_hints` (`/ search`, `^r retry`, `? help`)
against the right edge. A hint with a key is a button that sends
`Action::PressKey`, so a tap goes through the same key path. Navigation
keys every list shares (`j/k`, `h/l`, `g/G`, paging) belong in `help()`
only, never in `hints()`. Keep `help()`, `hints()`, and the key handler
in step: all three cover only what works for the row, sub-tab, and mode
on screen. While the screen captures input the status bar shows only
`F1 help` and `^c quit`, and the form draws its own key line (Setup,
Settings, the Networks form).

**Toolbar row.** Filter and sub-tab strips go through
`widgets/sub_tabs.rs` (`sub_tab_strip`: every tab keeps its width, and
the active one is bold and underlined, never bracketed). State that
describes the view (the sort, a band, an AP focus, the feed state, a
move in progress) goes through `widgets/toolbar.rs`: `toolbar::render`
puts the strip on the left and a `toolbar::chip` against the right
edge, and drops the right side when the row has no room. A chip shows
the key that changes it. Row counts go in the screen title.

**Writes** never touch the Controller from a screen. A screen returns
an action and the App runs it:

- `Action::Run { command, notice }` runs a `Command` through
  `run_command`: a progress toast shows at once, a success calls
  `full_refresh()` so the store has the change before the toast turns
  into `notice.done`, and a failure becomes an error toast.
- `Action::ShowConfirm(ConfirmRequest)` asks first. The request carries
  the prompt, a `detail` line saying what yes costs, and the action to
  dispatch on yes. `open_confirm` never replaces a pending confirm and
  never opens over a prompt or the search bar: a confirm that arrives
  late, from background work, is dropped with a toast to press the key
  again.
- `Action::OpenPrompt(PromptRequest)` asks for one value. The answer
  comes back to the asking screen as `Action::PromptSubmitted`; the
  screen remembers what it asked for, and reopens the prompt with a
  hint when the value cannot be used.
- The `Request*` actions (restart, block, fixed IP, port update, network
  delete) are resolved App-side, where the store supplies names and
  MACs. That is what keeps them correct under `--demo` redaction.
- A write whose result the store can show is checked after the refresh,
  not assumed: `app/rule_switch.rs` (firewall `E` and `L`),
  `app/wifi/ssids.rs` (SSID writes, against the record read back), and
  the port and fixed IP paths in `app/devices_clients.rs` report "the
  controller accepted the change but still shows the old value" as an
  error or warning.

**Access** arrives once per connect as `Action::AccessUpdated(Access)`
with three flags: `integration`, `session`, and `live_events`. Screens
use them to explain an empty panel (waiting, nothing there, or not
readable with this profile) and to skip fetches the controller would
refuse. A demo fleet session reports full access.

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
  site_manager_client_test.rs    # wiremock-based Site Manager cloud tests
  controller_runtime_test.rs     # Controller lifecycle + refresh loop

crates/unifly/tests/
  cli_test.rs                    # assert_cmd-based CLI tests (fast, no controller)
  e2e_test.rs                    # gated e2e suite against a dockerized controller

tests/e2e/                       # docker-compose.yml + wait-for-controller.sh
```

Unit tests are inline in source files under `#[cfg(test)] mod tests`.
The e2e suite (`e2e_test.rs`) is feature-gated behind `--features e2e`
and drives the built binary against a real UniFi Network controller
running in simulation mode via `tests/e2e/docker-compose.yml`. `just
e2e` runs the full lifecycle (up, wait, test, tear down); `UNIFLY_E2E_URL`,
`UNIFLY_E2E_USERNAME`, `UNIFLY_E2E_PASSWORD`, `UNIFLY_E2E_SITE`, and
`UNIFLY_E2E_TIMEOUT_SECS` override the connection defaults. CI runs the
same lifecycle in `.github/workflows/e2e.yml`. The `integration_profile_*`
tests form a second lane for the Integration API (REST, `X-API-KEY`)
that runs only when `UNIFLY_E2E_API_KEY` is set and skips otherwise; the
simulation image cannot mint a key (Integration auth is backed by the
UniFi OS account subsystem the bare image lacks), so this lane needs a
real UniFi OS controller plus a console-minted key. See `CONTRIBUTING.md`
for the full provisioning findings.

### Libraries

- **wiremock**: mock HTTP servers for the Integration, Session, and
  Site Manager client test suites.
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
  The one carve-out is the feature-gated e2e suite, which targets the
  dockerized controller and never runs in a default `cargo test`.
- Tests must not require a specific UniFi hardware or firmware version.
- The TUI has inline unit tests (effects, forms, screen state) but no
  `ratatui::TestBackend` render harness yet. Adding one is welcomed.

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

**Credential precedence**: when both an explicit `api_key` (or
`password`) is set in a profile's TOML config and a corresponding
keyring entry exists, the explicit config value wins. This is the
opposite of the older behavior; see PR #22 (`config/mod.rs` resolve
paths). Treat this as the contract for any code that reaches into
config when materializing `AuthCredentials`.

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

## Pull Request Reviews

**Always handle and respond to CodeRabbit reviews.** Read the review summary
and inline threads after each push, including findings on an earlier head
that remain relevant. A green check alone does not establish that a review
ran; inspect its result for skipped or rate-limited reviews.

Every actionable thread needs a direct reply with its disposition and
evidence: the fixing commit and validation, a concrete reason to decline,
or a tracked follow-up with scope and rationale. A top-level summary does
not replace inline replies. Treat findings as claims to verify, and fix
confirmed issues without weakening tests, lints, or product behavior.

Before merging or reporting completion, check the latest head's CI and
review state, and ensure no actionable finding is unanswered. Respond to
follow-up findings too. Final PR replies end with `~ via nova ⚡` on its own
line.

---

## Release Workflow

Releases use the shared workflow at
`hyperb1iss/shared-workflows/.github/workflows/rust-release.yml`.

### Before triggering

Run `just changelog-cut X.Y.Z` and commit the result. It moves the
`[Unreleased]` notes under `## [X.Y.Z] - <date>`, opens a fresh empty
`[Unreleased]`, and updates the compare links. Skipping it leaves the
shipped notes under `[Unreleased]`, so the changelog credits them to the
next release.

### Steps (handled automatically once triggered)

1. `.github/workflows/release.yml` is triggered manually via
   `workflow_dispatch` with a `version` or `bump` input.
2. The shared workflow bumps the workspace version in `Cargo.toml` and
   the plugin manifests (its `version-files` list), runs the
   full build + test + clippy gate, generates release notes with git-iris
   and uploads them as an artifact, commits the bump, tags `vX.Y.Z`,
   pushes the commit and tag, and dispatches `.github/workflows/cicd.yml`
   on the tag with its run ID.
3. That dispatched cicd run (the only one that releases; a tag push does
   not trigger cicd):
   - Rebuilds and tests via the shared rust-ci workflow
   - Builds release artifacts for 4 targets (linux amd64+arm64, macOS
     arm64, Windows x86_64 MSVC)
   - Publishes `unifly-api` and `unifly` to crates.io (shared rust-publish)
   - Creates a GitHub Release with all artifacts and the notes artifact
     from the release run (git-iris regenerates them only as a fallback)
   - Updates the Homebrew formula, the AUR package, and the ClawHub skill

Recovering a failed release depends on how far it got:

- **Nothing published yet** (the dispatch never ran, or the cicd run failed
  before `publish`): rerun `release.yml` with the same `version` and
  `resume_release` set. It regenerates the notes and dispatches a fresh
  cicd run on the existing tag without moving it.
- **Crates already on crates.io** (a later job such as the GitHub Release,
  Homebrew, or AUR failed): rerun just the failed jobs of the existing
  cicd run (`gh run rerun <run-id> --failed`). A fresh dispatch would try
  to publish again and fail, because `cargo publish` rejects a version
  that already exists.

Pushing a tag by hand does not release anything.

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

The release workflow patches all three through its `version-files` input,
so a normal release keeps them in sync. The discovery skill is versionless;
ClawHub publication receives the release version explicitly.
Check them by hand only when bumping the version outside `release.yml`.

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
  `controller::refresh::full_refresh`. Guest, authorized, and blocked
  state is merged by MAC from `stat/sta` and `rest/user`, and block,
  unblock, and kick go to `cmd/stamgr` whenever a session exists: a
  blocked client that left the connected list has no Integration UUID
  to act on.
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
- **Device `radios` come from the session `radio_table`**, merged with
  `radio_table_stats` by `convert.rs::parse_session_radios`. The
  Integration `interfaces` JSON is not the source.
- **An Integration PUT replaces the whole record.** An update must send
  back every field it fetched, including ones unifly does not model
  (the flattened `extra` on the ACL types). An ACL rule written without
  its `protocolFilter` matches every protocol. Unset optional fields are
  left out, not sent as null.
- **The Integration WiFi list is a summary** without `hidden`,
  `client_isolation`, `band_steering`, `mlo_enabled`, or `fast_roaming`.
  Those `WifiBroadcast` fields are `Option<bool>` and stay `None` until
  the broadcast's detail is read. An SSID with per-network preshared
  keys (`preshared_key_count > 0`) has no single passphrase to write.
- **A fixed IP is keyed by the session network `_id`**, not the
  Integration UUID. `controller/commands/fixed_ip.rs` maps a UUID
  through `rest/networkconf`'s `external_id`, accepts an `_id` or a
  name, and with no network picks the LAN whose subnet holds the
  address.
- **Pending devices come from `/v1/pending-devices`** on Integration
  profiles, as an optional fetch, keyed `pending:<mac>` in the device
  snapshot.
- **Network writes are checked by kind.** Isolation, internet access,
  mDNS, and IPv6 exist only on some network kinds and are refused on
  the others. IPv6 can be turned off but not on (the API needs an
  address assignment and a prefix source), an emptied DNS list removes
  the override key, and delete sends `force` as a query parameter only
  when set.

### CLI Quirks

- **Create commands print the created entity on stdout** in the selected
  `--output` format: `-o plain` emits the bare ID, `-o json | jq -r .id`
  is the capture idiom. The confirmation message goes to stderr.
  `sites create` prints nothing (the controller returns no record). VPN
  create responses pass through session-query redaction, so generated
  private keys and PSKs stay off stdout. A response that cannot be
  rendered warns on stderr and still exits zero; the create succeeded,
  and failing would invite duplicate-create retries.
- **`nat policies list` works in session-only auth.** The session
  refresh snapshot fetches the v2 NAT inventory alongside the other
  session collections.
- **Port range writes use integer `start`/`stop` fields** from the OpenAPI
  spec. Reads also accept legacy `startPort`/`endPort` names and string
  values. The Integration E2E firewall round trip verifies writes and
  readback. DNS record types are `*_RECORD` tokens (`A_RECORD`, not `A`).
  Integration list pages decode per-item so one unparseable record cannot
  blank a collection, and pagination advances by the server-reported count.
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
- **`firewall policies patch`** is the partial-update path for `logging`
  and `reportFlowEnabled`, the only fields the controller's PATCH takes.
  `patch --enabled` is sent as a full update (GET, then PUT of the whole
  record), and `FirewallPolicyPatch` has no `enabled` field.
- **`firewall groups`** uses the Session API (`rest/firewallgroup`), not
  Integration. The `external_id` (UUID) in group responses is what
  firewall policies reference. Policy create/update supports
  `--dst-port-group` / `--dst-address-group` flags (and `dst_port_group`
  / `dst_address_group` shorthand fields in `--from-file` JSON) to
  resolve group names to `external_id` UUIDs automatically.
- **`networks refs <id>`** is the only command that answers "what depends
  on this entity before I delete it." No equivalent exists for other
  entities yet.
- **`unifly api <path>`** sends `integration/` paths through the
  Integration client, preserving API-key auth and cloud connector routing.
  Other paths use the Session client with automatic CSRF handling. Failed
  mutations are not replayed through a different authentication path.
- **Network 11 stats use explicit time-series queries.** Probe
  `v2/api/stats/source` once per Session client; a strict 404 selects legacy
  reports. Modern requests use `v2/api/stats/series` with a quoted
  `siteId.eq('UUID')` filter. The UUID is the Session site's `_id`, not its
  Integration ID. Preserve native bucket durations in milliseconds and
  fractional metrics. Modern `daily` means native 8-hour buckets; monthly
  uses native monthly retention. Do not substitute 8-hour data for yearly
  history. Monthly wire bounds expand to January anchors to avoid the
  controller's timezone alignment bug; returned buckets still overlap the
  original requested window.
- **Site creation sends the display name as `desc`.** The controller
  generates the internal site reference. Supplying `name` in the wire
  request breaks creation on Network 11. The CLI accepts `--name` or its
  `--description` alias, not both.
- **`clients roams` and `clients wifi`** accept any client identifier
  (name, hostname, IP, or MAC). Resolution uses the in-memory snapshot,
  so the client must appear in `clients list`. `roams` resolves to MAC;
  `wifi` resolves to IP.
- **`wifi neighbors`** defaults to 25 results. Use `--all` or `--limit N`
  to see more.
- **`devices ports / ports-export / port-set`** are the switch port
  config-as-code surface. Port indices are **1-based** to match the
  controller's wire format. `port-set --from-file` accepts a JSONC
  payload matching `ApplyPortsRequest` (a `ports` array with per-port
  `index`, `name`, `mode`, `native_network_id`/`native_vlan`,
  `tagged_network_ids`/`tagged_vlans`, `tagged_all`, `poe`, `speed`,
  `reset`). Splice semantics: ports not listed keep their existing
  override; `"reset": true` removes that port's entry. The CLI `--poe`
  flag accepts only `off`, `auto`, `pasv24`, `passthrough` (no `on`,
  because `auto` IS the on/negotiate mode). Setting speed to "auto"
  on the wire means `autoneg: true` with no `speed` field, not
  `"speed": "auto"` (which the controller rejects). `ports
--with-clients` and `ports-export --with-clients` annotate output
  with connected clients (and `// last-seen <ts>: <mac>` markers in
  the export, for git-diff drift detection).
- **`settings`** (top-level) reads and writes site-level Session API
  settings. Subcommands: `list`, `get <KEY>`, `set <KEY>`, `export`.
  Session-only — needs `auth_mode = "hybrid"` or `"session"`.
- **`vpn` is broad now**. `vpn servers / tunnels` are read-only views
  on the modern Integration API; `vpn site-to-site / remote-access /
clients / peers / magic-site-to-site / settings` are Session-backed
  and most use the Session gate. `vpn connections list/get/restart`
  is the legacy v2 connection inventory and restart path. `vpn
remote-access suggest-port` and `vpn remote-access download-config
<id>` are convenience helpers; `vpn peers subnets` enumerates
  configured peer subnets.
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
  handles this. `event.rs::stop_signal` turns `SIGTERM`, `SIGHUP`,
  `SIGINT`, and `SIGQUIT` into a normal quit, which restores it too.
- **The data bridge auto-reconnects.** Failed connects retry with
  exponential backoff (2s doubling to a 30s cap, indefinitely), emitting
  `Reconnecting`/`Disconnected` actions so the status bar stays
  truthful. `Ctrl+r` forces an immediate retry while disconnected by
  cancelling the old bridge and spawning a fresh one. Bridge lifetimes
  on one controller are serialized: each new bridge task first awaits
  its predecessor's `JoinHandle`, so teardown always completes before
  the next connect starts. A switch to another controller
  (`apply_connection`) does not wait on the one being left, and sets
  `Connecting` at once. While connected, `Ctrl+r` runs `full_refresh()`
  instead; the `refreshing` flag makes a held key one refresh.
- **`Reconnecting` has two meanings.** `unifly-api` publishes
  `ConnectionState::Reconnecting` from the second failed periodic
  refresh in a row (`refresh/periodic.rs`), with the clients still
  installed, and any successful `full_refresh()` publishes `Connected`
  again. So after a connect it means "the link is failing", and data,
  fetches, and writes must carry on; before one it means "still
  connecting". Screens tell the two apart with `screens/link.rs`
  (`Link::Unsteady`), and the App with `controller_reachable()`. Only
  `Disconnected`, `Connecting`, and `Failed` may refuse a write up
  front. `full_refresh()` is single-flight (`refresh_gate`).
- **`Tab` belongs to the App.** It always switches screens. Sub-tabs,
  filters, and periods step with `h`/`l` and the left and right arrows
  on every screen; never bind `Tab` in a list screen. A form may use it
  for the next field, since it captures input.
- **A screen that uses `Esc` must return an action** (`Action::Invalidate`
  will do). `None` means the key was not used, and the App goes on to
  its own `Esc`: dismiss a warning or error toast, then clear a kept
  filter, then go back a screen.
- **Paging keys reach list screens as vim keys.** `list_alias` turns
  `PgDn`/`PgUp` into `Ctrl+d`/`Ctrl+u` and `Home`/`End` into `g`/`G`
  unless the screen captures input, so bind the vim keys and the rest
  follows.
- **Chords are not letters, and pastes are not keys.** A `Ctrl` or `Alt`
  chord is never forwarded to a list screen as its plain letter (only
  `Ctrl+d` and `Ctrl+u` are). Global keys match the key code alone,
  because terminals differ on reporting Shift with `?`. Bracketed paste
  is on: text arrives as one event for the search bar, a prompt, or a
  screen that captures input, and is dropped anywhere else.
- **Help descriptions have about 24 columns** in the overlay, and key
  labels have 12. A longer description wraps onto the next row and a
  wider key takes a row of its own, so nothing is cut, but both cost
  rows on an 80x24 terminal. Word them to fit; several screens have a
  test that holds their keys and descriptions to the column.
- **Do not close a form before its save lands.** The Networks save is
  App-side: the form locks, closes once the refreshed data has arrived,
  and stays open with the draft and the reason on a failure.
- **Never write a prefilled value back unchanged.** Under redaction a
  prompt's starting value is a stand-in, so an answer equal to it is a
  no-op (the port rename does this).
- **Firewall reordering is a draft.** `K`/`J` move the policy on screen
  only; `Enter` sends one `Action::MovePolicy` with the net steps and
  `Esc` drops the draft. Nothing is written per keypress.
- **Background fetch failures belong in their panel**, not in a toast on
  a timer. Stats, WiFi, and Alarms carry the error in the screen state.
- **A late answer must not land on the wrong controller or the wrong
  moment.** History, alarm, and stats results travel in
  `Action::MonitorFetched { generation, .. }` and WiFi answers carry
  the WiFi generation; a result from before a controller switch is
  dropped where it is handled. A background check that ends in a
  question (`Action::NetworkDeleteChecked`, the SSID visibility read)
  asks only if the user is still on that row with nothing open.
- **Refuse locally, with the reason.** A key the row or the profile's
  access cannot serve returns an info toast from the screen (`refusal`,
  `not_ours`, `NEEDS_SESSION`, `NEEDS_INTEGRATION`) and is left out of
  `hints()` and `help()`. Do not confirm first and fail afterwards.
- **An unknown setting is not `false`.** The WiFi list is a summary, so
  the SSIDs tab reads each record (`screens/wifi/ssid_detail.rs`) and
  draws `Setting::Reading`, `Unknown`, or `Absent` until it knows. A
  toggle such as `H` reads the record before it asks which way to go.
- **Locate state follows the controller's answer.** `locating` is
  updated when the command succeeds, never before it runs, and is
  cleared on a controller switch.
- **`Action::ControllerChanged` clears everything a screen holds**: open
  details, forms, drafts, and fetched data. Applying a connection from
  Settings sends it.
- **Onboarding and Settings save only what a test connected with.**
  Both run `forms/probe.rs`: the test resolves the profile through
  `resolve_session_connection` (a typed password ahead of
  `UNIFI_PASSWORD`; Settings puts the launch flags on top, the wizard
  prefills its steps from them instead), and fails a hybrid connect that
  only warned. A launch flag stands down once the form changes a value
  it covers, and `ApplyConnection` carries the `ConnectionOverrides`
  the test ran under. `--timeout` and `--no-cache` stay session flags
  and are never written. A save writes back what the form did not
  change (`api_key_env`, `host_id_env`, unused-mode credentials) as
  loaded. Each test has a generation; a cancelled or replaced one
  reports nothing. The save runs after that check, off the UI loop, and
  `ApplyConnection` carries the tested `ControllerConfig`, never one
  rebuilt from the form. A sign-in test never reads the profile's
  session cookie cache: it parks its cookie in a private cache
  (`Handover`), and only a successful save moves it into the profile's,
  which is how one MFA code serves both the test and the dashboard. A
  discarded test logs its session out through
  `Controller::logout_session()`, and `forms/parked.rs` sweeps parked
  cookies whose owner process is gone at launch (not under
  `--demo-fleet`). In a redacting session the wizard masks every value
  (`OnboardingScreen::hide_values`). In a test build `ProbeSlot` starts
  nothing, so unit tests hand the
  screens results directly and never reach the network, the keyring, or
  the real config.

---

## File Organization Conventions

### Source Files

- One entity per file under `cli/args/` and `cli/commands/`.
- When a command has many subhandlers (e.g. `devices`, `clients`,
  `firewall`, `config_cmd`, `acl`, `cloud`, `vpn`, `settings`), split
  into a subdirectory with `mod.rs` and per-subcommand files.
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
- `docs/content/`: Zola docs site content (guide, reference, architecture)
- `docs/config.toml`: Zola site configuration
- `docs/images/`: committed screenshots and GIFs
- `docs/plans/` --**scratch area, do not commit contents** (not currently
  gitignored but treat as ephemeral)

The docs site deploys via `.github/workflows/docs.yml`, which triggers
only on `docs/**` changes (and edits to the workflow itself). Root
markdown changes do not redeploy the site.

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
6. Verify the generated `unifly skill commands` inventory includes the command
7. Update `crates/unifly/skill/commands.md` with operational gotchas
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

The installed `skills/unifly/SKILL.md` is a discovery stub. Keep operational
instructions in `crates/unifly/skill/`, embedded by `cli/skill.rs` so Cargo
packages and standalone binaries carry the matching guide. The command
inventory is generated from Clap; do not maintain another command table.

`unifly skill --install` writes the stub into `~/.agents/skills/unifly/`
(or `--dir`, or `--claude`) from an embedded copy at
`crates/unifly/skill/discovery-skill.md`, because Cargo cannot package
files outside the crate. Edit both copies together; a unit test fails when
they differ. The installer replaces an existing `SKILL.md` without `--force`
only when its digest is in `PUBLISHED_DIGESTS` (`cli/skill/install.rs`), so
whenever the stub changes, append its new digest (the failing test prints
it) and never remove old ones.

When behavior changes, update the relevant core, commands, concepts, or
workflows topic. Update the payload templates under `skill/examples/` when
request shapes change, and keep the example registry and deserialization
tests in sync. Runtime bundling prevents version skew; behavioral prose
still needs source review when auth, output, or mutation semantics change.

The skill command must remain offline and independent of configuration,
credentials, and controller access. Run the skill tests in both the default
and CLI-only builds, and check Cargo packaging includes every guide asset.

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
- **`docs/images/`**: shipped screenshots. The README's promo video is a GitHub
  `user-attachments` upload (a repo file cannot play inline in a README).
  The source cuts are attached to the v0.11.0 release only; later releases
  do not carry them
- **`aur/update-aur.sh`**: AUR package refresh script used by
  `just aur-update`
