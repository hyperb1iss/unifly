## Start here

Use unifly to inspect and manage UniFi Network infrastructure. This guide is
embedded in the installed binary. Command discovery comes from the same Clap
schema that parses commands; operational guidance is maintained with the code.

Load only the detail needed for the task:

| Command                                    | When to use it                                                  |
| ------------------------------------------ | --------------------------------------------------------------- |
| `unifly skill commands`                    | Discover all commands and read operational gotchas              |
| `unifly skill concepts`                    | Choose auth, target a site, understand IDs and JSON field names |
| `unifly skill workflows`                   | Diagnose connectivity or plan configuration changes             |
| `unifly skill examples`                    | List configuration and payload templates                        |
| `unifly skill examples switch-ports.jsonc` | Print one template verbatim for saving to a file                |
| `unifly <command> --help`                  | Check exact flags, defaults, accepted values, and subcommands   |

All skill topics work offline without credentials or a configured controller.
Output is plain Markdown (or the raw example), regardless of `--output`.

## Work against the right controller

Confirm the intended profile and site from the user's request. Select them with
`-p <profile>` and `-s <site>`. The environment prefix is `UNIFI_`, not `UNIFLY_`;
`UNIFLY_THEME` is a separate theme setting. Use `config profiles` and `config
show -o table` for stored settings, but account for CLI and environment overrides yourself.
The explicit table format matters: JSON/YAML config output exposes stored secrets,
and `UNIFI_OUTPUT` can change the default format.
Never print secret environment variables or credential files into the conversation.

| Profile auth mode | Available access                                                |
| ----------------- | --------------------------------------------------------------- |
| `integration`     | Integration API and, on UniFi OS, Session HTTP using an API key |
| `session`         | Session HTTP and WebSocket using username/password              |
| `hybrid`          | Both APIs plus the session cookie needed for WebSocket          |
| `cloud`           | Site Manager fleet queries and connector-routed Integration API |

The API path matters more than the command family: NAT and switch port
configuration use Session HTTP, while device removal and PoE cycling use the
Integration API. Cloud access cannot run Session-only operations. Streaming
`events watch` needs session or hybrid authentication; an API key alone is
insufficient. See `unifly skill concepts` for the detailed boundaries.

For first-time setup, use `unifly config init` for local access or `unifly config
cloud-setup` for Site Manager. Preserve configured TLS verification; use a custom
CA where appropriate rather than automatically adding `--insecure`.

## Inspect, act, verify

1. Read the relevant command help before constructing a request. For an audit of
   the CLI itself, use help and source inspection only; a trial mutation can
   reach a real controller.
2. Start with a read appropriate to the target: `unifly -p home system health
-o json` for local Session HTTP, or `unifly -p cloud cloud hosts -o json` for
   cloud fleet access. A cloud connector read can use `networks list`.
3. Inspect existing objects and capture their IDs. Prefer `-o json` for analysis.
   Use `--all` when the command supports it to avoid truncated inventory; other
   commands have their own limits (check help).
4. Apply the user's authorized change. Complex writes often accept `--from-file`;
   use `unifly skill examples` for starting payloads, replacing all placeholders.
   A JSON export is not automatically a valid write payload.
5. Re-fetch the affected entity and verify the intended fields. After an
   ambiguous failure, inspect state before retrying a create or destructive
   operation. An accepted asynchronous action is not evidence of completion.

Carry existing user authorization forward. For changes whose target or impact
is still unclear, resolve that uncertainty before executing. The `--yes` flag
skips CLI confirmation; it does not supply authorization. Preview bulk deletes,
firewall order changes, and connectivity-breaking edits before applying them.

## Output and automation

Most list/get commands support JSON, compact JSON, YAML, table, and plain output.
Check command-specific exceptions in `unifly skill commands`. Compact JSON emits
one JSON value (a list is an array), not one line per entity. Use `jq -c '.[]'`
when you need one entity per line. Events watch emits individual event values.

Create commands generally emit the created entity on stdout and confirmation on
stderr. With `-o plain`, supported entity creates emit a bare ID. Site creation
and system backup creation return no record. A successful create whose response
cannot be rendered warns on stderr but still exits zero; inspect state before
repeating it. VPN output redacts secrets and cannot be blindly round-tripped.

Keep diagnostics from stderr when a command fails. Exit codes distinguish usage,
authentication, missing resources, unsupported operations, connection failures,
and timeouts. Use `unifly skill concepts` for the mapping and recovery guidance.
