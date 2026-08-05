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

Connection defaults are overridable via `UNIFLY_E2E_URL`, `UNIFLY_E2E_USERNAME`, `UNIFLY_E2E_PASSWORD`, `UNIFLY_E2E_SITE`, and `UNIFLY_E2E_TIMEOUT_SECS`. CI runs the same lifecycle in `.github/workflows/e2e.yml`.

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
