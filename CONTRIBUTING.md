# Contributing to unifly

Thanks for your interest in contributing! Bug reports, feature ideas, and code contributions all help make unifly better.

## Reporting Bugs

Open a [GitHub Issue](https://github.com/hyperb1iss/unifly/issues) with:

- unifly version (`unifly --version`)
- Controller model and firmware version
- Steps to reproduce
- Expected vs. actual behavior
- Relevant error output (use `-vvv` for full debug logging)

## Requesting Features

Open a [GitHub Issue](https://github.com/hyperb1iss/unifly/issues) describing:

- The use case: what problem does this solve?
- How you'd expect it to work (CLI syntax, TUI behavior, etc.)
- Whether it relates to the Integration API, Session API, or both

Check the [ROADMAP.md](ROADMAP.md) first; it might already be planned.

## Development Setup

### Prerequisites

- **Rust 1.94+** (edition 2024) via [rustup](https://rustup.rs/)
- **Nightly rustfmt**: `rustup component add rustfmt --toolchain nightly`
- **just** task runner: `cargo install just`
- A UniFi Network controller for integration testing (Cloud Key, Dream Machine, or self-hosted)

### Build & Test

```bash
git clone https://github.com/hyperb1iss/unifly.git
cd unifly
cargo build --workspace
cargo test --workspace
```

### Run from Source

```bash
cargo run -p unifly -- devices list
cargo run -p unifly -- tui
```

Or use the just recipes:

```bash
just cli devices list
just tui
```

### Workspace Structure

```
crates/
  unifly-api/      # Library: HTTP/WS transport, Controller, DataStore, domain models
  unifly/          # Single binary: CLI commands + TUI dashboard (feature-gated via `tui` feature)
```

Dependency chain: `unifly` depends on `unifly-api`. The TUI is optional and can be excluded with `--no-default-features --features cli`.

For deeper architectural context and code policies, see [AGENTS.md](AGENTS.md).

## Code Style

### Formatting

```bash
just fmt                  # runs cargo +nightly fmt --all
just fmt-check            # read-only check (same as CI)
```

The project uses nightly rustfmt with a custom `rustfmt.toml` (100-char max width, field init shorthand, try shorthand).

### Linting

```bash
just clippy               # runs cargo clippy --workspace --all-targets
```

The workspace has opinionated clippy configuration. Key rules:

- `unsafe_code = "forbid"`: no unsafe code, period
- `unwrap_used = "deny"`: use `?`, `.ok()`, `.unwrap_or()`, or proper error handling
- `pedantic = "deny"`: clippy pedantic lints are enforced (with a few pragmatic exceptions)
- `all = "deny"` and `perf = "deny"`: no warnings slide

Numeric cast lints (`cast_precision_loss`, `cast_possible_truncation`, `cast_sign_loss`) are set to warn, not deny.

See `Cargo.toml` `[workspace.lints]` for the full configuration.

### Conventions

- Error types use `thiserror` with `miette` for rich diagnostics
- Async runtime is `tokio`; all public async APIs are `Send + Sync`
- Entity IDs use the `EntityId` enum (`Uuid` | `Legacy`) for dual-API compatibility
- Secrets are wrapped in `secrecy::SecretString`. Never log or display credentials

## Testing

### Test Layout

```
crates/unifly-api/tests/
  integration_client_test.rs     # wiremock-based Integration API tests
  session_client_test.rs         # wiremock-based Session API tests
  site_manager_client_test.rs    # wiremock-based Site Manager cloud tests
  controller_runtime_test.rs     # Controller lifecycle + refresh loop

crates/unifly/tests/
  cli_test.rs                    # assert_cmd-based CLI tests
  e2e_test.rs                    # feature-gated e2e suite, dockerized controller
```

Unit tests are inline in source files under `#[cfg(test)] mod tests`.

### Test Libraries

| Library                         | Purpose                                                              |
| ------------------------------- | -------------------------------------------------------------------- |
| **wiremock**                    | Mock HTTP servers for Integration/Session/Site Manager API tests     |
| **insta**                       | Snapshot tests for output formatting (`just snap-review` to approve) |
| **assert_cmd** + **predicates** | End-to-end CLI tests that spawn the built binary                     |
| **tempfile**                    | Per-test config dir isolation                                        |
| **tokio-test**                  | Poll-based async unit tests                                          |
| **pretty_assertions**           | Better diffs on assertion failures                                   |

### End-to-End Tests

The e2e suite (`crates/unifly/tests/e2e_test.rs`) drives the built binary against a real UniFi Network controller running in simulation mode inside Docker (`tests/e2e/docker-compose.yml`). It is feature-gated behind `--features e2e` and never runs in a default `cargo test`.

```bash
just e2e            # full lifecycle: start controller, wait, test, tear down
just e2e-up         # start the controller container
just e2e-wait       # poll until the controller is ready
just e2e-build      # compile the gated test binary without running it
just e2e-test       # run the suite against a running controller
just e2e-down       # stop and remove the container
```

`just e2e` with `UNIFLY_E2E_URL` unset or empty and the Docker CI job set
`UNIFLY_E2E_SIMULATION=1` to identify the seeded simulation fixture. Its
pending-device test requires a nonempty result. External-controller runs leave this unset and compare the pending
list against inventory without assuming any device needs adoption.

Connection defaults are overridable via `UNIFLY_E2E_URL`, `UNIFLY_E2E_USERNAME`, `UNIFLY_E2E_PASSWORD`, `UNIFLY_E2E_SITE`, and `UNIFLY_E2E_TIMEOUT_SECS`. CI runs the same lifecycle in `.github/workflows/e2e.yml`.

The suite has two lanes. The session lane authenticates with username and password and runs on every e2e invocation against the simulation controller. The Integration lane (tests named `integration_profile_*`) exercises the modern Integration API (REST, `X-API-KEY`). It lists DNS policies, firewall policies and zones, networks (plus one `networks get`), Wi-Fi broadcasts, ACL rules, and traffic lists, and runs create/delete round trips for a DNS A record and a firewall policy with a destination port range. It runs only when `UNIFLY_E2E_API_KEY` carries a key and skips otherwise (the key reaches the CLI through the profile's `api_key_env`, so it never lands on disk).

The simulation image cannot provision an Integration key by script. The controller ships the full Integration surface (`/integration/v1/`), but key authentication resolves through the UniFi OS account subsystem rather than the Network application's local store, and the bare `jacobalberty/unifi` image has no UniFi OS layer. A session login exposes no key-creation endpoint, and seeding the controller's Mongo `api_key` collection directly does not yield a key the Integration filter accepts. To run the Integration lane, mint a key in a real UniFi OS console (Control Plane, Integrations) and run only that lane, since the session tests assert simulation-mode state and toggle site settings:

```bash
UNIFLY_E2E_URL=https://<console> UNIFLY_E2E_API_KEY=<key> \
  cargo test -p unifly --features e2e --test e2e_test -- --test-threads=1 integration_profile
```

The Integration lane can target a different site with
`UNIFLY_E2E_INTEGRATION_SITE`; otherwise it uses `UNIFLY_E2E_SITE`.

The round trips create and then delete their own records (DNS domains `e2e-*.unifly.test`, firewall policies `e2e-fwp-*`), and each one sweeps leftovers matching its prefix before creating, so an aborted run is reclaimed on the next one.

### Network 11 Validation Lab

The Docker simulation controller remains the legacy CI lane. Network 11
validation uses UniFi OS Server, because the standalone Network image
cannot provide its account subsystem or mint Integration API keys.

The 0.13 validation environment uses UniFi OS Server 6.0.21 with Network
11.0.99 on a Debian 13.7 VM (2 vCPUs, 4 GiB RAM, 50 GiB sparse disk).
Local-only onboarding avoids a UI.com account. Host forwarding exposes the
console on localhost; the guest has no bridge into the managed LAN.
Install the official UniFi OS Server package, complete local onboarding,
and mint an Integration key through the console's Integrations page.

Synthetic devices can populate real controller telemetry through
[jamesbraid/unifi-emu](https://github.com/jamesbraid/unifi-emu/tree/56542317eeb8c49868624d489d90db5dcd20f6af).
The pinned emulator exchanges actual inform/adoption messages. A U7 Pro,
USM8P switch, and UXG Enterprise gateway exercise
wireless, switching, and WAN statistics with a client. The gateway inform
must advertise its Ethernet ports and supported firewall API versions for
WAN statistics and zone-based firewall provisioning. Do not seed the
controller database to fabricate successful API responses.

Critical-alert validation requires a real transition. A simulated gateway
can report elapsed WAN downtime in `uptime_stats.WAN` after a healthy
baseline. Acknowledge the resulting alert while the link is still down,
verify its system-log record is read, then restore the original inform
driver and confirm connected devices and WAN statistics. Physical port
flags alone do not exercise this monitoring path.

Use a telemetry site for Session tests and a separate site with firewall
zones for Integration write round trips. The Session lane changes test-site
DPI and Teleport settings and restores them; run it only on a disposable
controller. Tests share one authenticated session seed in memory and copy
it into isolated per-test caches. The wrong-password test uses a fresh
login. Both lanes run sequentially because settings tests share site state.

Record the controller version, authentication mode, exact command, exit
status, nonempty data assertions, and cleanup readback. A green process
with a skipped test is not a completed round trip. The fresh lab validates
current monthly buckets; old populated monthly history and populated
LAG/MC-LAG/stack detail remain fixture coverage unless suitable hardware
and retained history are available.

### Test Policy

- Unit tests should be pure and deterministic. No real network calls.
- Integration tests use wiremock or assert_cmd, never a real controller. The feature-gated e2e suite is the one exception: it targets the dockerized controller and only runs through the `just e2e*` recipes or an explicit `--features e2e`.
- Tests must not require a specific UniFi hardware or firmware version.

## Pull Request Workflow

1. **Fork** the repository
2. **Branch** from `main` (`feature/my-feature` or `fix/my-fix`)
3. **Implement** your changes with tests where applicable
4. **Run the CI gate locally** before pushing:
   ```bash
   just check               # fmt-check + maintainability + clippy + test
   ```
5. **Open a PR** targeting `main`
6. Describe what changed and why. Link the relevant issue if one exists.

Feedback is collaborative, not adversarial. We'll work through any issues together.

## License

By contributing, you agree that your contributions will be licensed under the [Apache License 2.0](LICENSE), the same license covering the project.
