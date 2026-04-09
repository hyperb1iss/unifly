---
name: unifly
version: "0.8.2"
description: >-
  This skill should be used when the user asks to "manage UniFi devices",
  "configure UniFi networks", "create a VLAN", "provision an SSID",
  "create firewall rules", "reorder firewall policies", "create a NAT rule",
  "set up port forwarding", "configure masquerade NAT", "add DNS records",
  "manage traffic matching lists", "create DHCP reservations", "list DHCP reservations",
  "block a client", "kick a client", "find a client by IP or name",
  "adopt a device", "restart a UniFi device", "cycle a PoE port",
  "upgrade device firmware", "run a speed test", "stream UniFi events",
  "watch real-time events", "query UniFi stats", "analyze DPI traffic",
  "enable DPI", "generate hotspot vouchers", "show network topology",
  "audit firewall policies", "create a backup", "call the raw UniFi API",
  "check network health", or any task involving UniFi network infrastructure
  management via the unifly CLI. Also triggers on mentions of unifly, UniFi,
  Ubiquiti, UDM, UCG, USG, USW, UAP, UXG, UNVR, U6, U7, or UniFi controller
  operations.
---

# unifly: UniFi Network Management

unifly is a Rust CLI for managing Ubiquiti UniFi network infrastructure. It
unifies the modern Integration API (REST, API key) and the Session API (cookie
plus CSRF) behind a single coherent interface, plus real-time WebSocket event
streaming. 27 top-level commands cover devices, clients, networks, WiFi,
firewall policies and zones, NAT policies, ACLs, DNS, traffic matching lists,
hotspot vouchers, DPI, stats, backups, and a raw API escape hatch.

Unique capabilities worth leading with when the user's task suits them:

- **Dual-API enrichment** merges Integration and Session data (e.g. client
  bytes, hostnames, uplink MACs only exist in the Session API; configuration CRUD only
  exists in Integration). On UniFi OS, API key mode can already reach session
  HTTP; Hybrid adds the WebSocket session for live monitoring.
- **Real-time event streaming** via `unifly events watch` over WebSocket.
- **Firewall policy reordering** via `reorder --get` / `reorder --set` for
  deterministic, round-trippable ordering edits.
- **`unifly api` raw passthrough** for endpoints unifly does not wrap.
- **Multi-profile** (`-p home`, `-p office`) for managing multiple controllers
  from one command line.

## Prerequisites

Verify availability before running any command:

```bash
command -v unifly >/dev/null 2>&1 && unifly --version || echo "unifly not installed"
```

If unifly is not installed, prefer `brew install hyperb1iss/tap/unifly` on
macOS or `cargo install --git https://github.com/hyperb1iss/unifly.git unifly`
elsewhere. After install, run `unifly config init` for the interactive
wizard. See `examples/config.toml` for manual configuration.

## Authentication Modes

unifly supports three modes. **API key mode is enough for most HTTP
automation on UniFi OS controllers.** Choose **Hybrid** when the task needs
live WebSocket features (`events watch`) or you want maximum compatibility.

| Mode          | Credentials             | What It Unlocks                                                                                               |
| ------------- | ----------------------- | ------------------------------------------------------------------------------------------------------------- |
| `integration` | API key                 | Integration API plus session HTTP on UniFi OS: CRUD, device commands, stats, reservations, admin, event list |
| `session`      | Username + password     | Session HTTP + WebSocket only: events watch, stats, device commands, DPI control, admin, backups              |
| `hybrid`      | API key + username/pass | Everything above, including session WebSocket plus enriched records with maximum controller compatibility       |

Session WebSocket still rejects API keys, so `events watch` needs `session` or
`hybrid`.

For the complete command-to-API gate matrix (which commands require which
auth mode), consult `references/concepts.md`.

## Command Inventory

All commands follow `unifly [global-flags] <command> <action> [args]`.

| Command         | Aliases    | Actions                                                                                                        |
| --------------- | ---------- | -------------------------------------------------------------------------------------------------------------- |
| `devices`       | `dev`, `d` | list, get, adopt, remove, restart, locate, port-cycle, stats, pending, upgrade, provision, speedtest, tags     |
| `clients`       | `cl`       | list, find, get, roams, wifi, authorize, unauthorize, block, unblock, kick, forget, reservations (`res`), set-ip, remove-ip |
| `networks`      | `net`, `n` | list, get, create, update, delete, refs                                                                        |
| `wifi`          | `w`        | list, get, neighbors, channels, create, update, delete                                                        |
| `firewall`      | `fw`       | policies {list, get, create, update, patch, delete, reorder}, zones {list, get, create, update, delete}        |
| `nat`           |            | policies {list, get, create, update, delete}                                                                   |
| `acl`           |            | list, get, create, update, delete, reorder                                                                     |
| `dns`           |            | list, get, create, update, delete                                                                              |
| `traffic-lists` |            | list, get, create, update, delete                                                                              |
| `hotspot`       |            | list, get, create, delete, purge                                                                               |
| `events`        |            | list, watch                                                                                                    |
| `alarms`        |            | list, archive, archive-all                                                                                     |
| `stats`         |            | site, device, client, gateway, dpi                                                                             |
| `dpi`           |            | apps, categories, status, enable, disable                                                                      |
| `topology`      | `topo`     | _(no subcommands)_                                                                                             |
| `system`        | `sys`      | info, health, sysinfo, backup {create, list, download, delete}, reboot, poweroff                               |
| `settings`      |            | list, get, set, export                                                                                         |
| `sites`         |            | list, create, delete                                                                                           |
| `admin`         |            | list, invite, revoke, update                                                                                   |
| `wans`          |            | list                                                                                                           |
| `vpn`           |            | servers {list, get}, tunnels {list, get}, status, health, site-to-site {list, get, create, update, delete}, remote-access {list, get, create, update, suggest-port, download-config, delete}, clients {list, get, create, update, delete}, connections {list, get, restart}, peers {list, get, create, update, delete, subnets}, magic-site-to-site {list, get}, settings {list, get, set, patch} |
| `radius`        |            | profiles                                                                                                       |
| `countries`     |            | _(no subcommands)_                                                                                             |
| `api`           |            | Raw API passthrough (GET/POST/PUT/PATCH/DELETE any path)                                                       |
| `config`        |            | init, show, set, profiles, use, set-password                                                                   |
| `tui`           |            | _(no subcommands)_                                                                                             |
| `completions`   |            | bash, zsh, fish, powershell, elvish                                                                            |

For flag details and gotchas, consult `references/commands.md`. Every entity
command accepts `--help` at runtime as the authoritative reference.

## Output Formats

All list and get commands accept `--output` / `-o`:

| Format         | Flag              | Use Case                              |
| -------------- | ----------------- | ------------------------------------- |
| `table`        | `-o table`        | Human display (default)               |
| `json`         | `-o json`         | Agent processing, pipe to `jq`        |
| `json-compact` | `-o json-compact` | Single-line JSON for scripting        |
| `yaml`         | `-o yaml`         | Config file output                    |
| `plain`        | `-o plain`        | One ID per line for `xargs` pipelines |

**Default for agent use: `-o json`.** Emit structured output, pipe through
`jq`, and only fall back to `table` when the result is being shown to a human.

## Power Patterns

These patterns unlock unifly's most distinctive capabilities. For full
recipes with runnable shell scripts, consult `references/workflows.md`.

### `--from-file` for complex create/update

Most entities accept `--from-file <path.json>` (or `-F`) instead of flag
salad: `networks`, `wifi`, `firewall policies`, `firewall zones`, `nat policies`,
`acl`, `dns`, `traffic-lists`, `hotspot`, `vpn site-to-site`, `vpn remote-access`,
`vpn clients`, and `vpn peers`. Construct the JSON payload, validate it, then
apply. See `examples/` for payload templates.

```bash
unifly networks create -F examples/network-iot-vlan.json
unifly firewall policies create -F examples/firewall-block-iot.json
```

### Real-time event streaming

```bash
# All events
unifly events watch

# Filter by EventCategory (case-insensitive): Device, Client, Network,
# System, Admin, Firewall, Vpn, Unknown
unifly events watch --types "Firewall,Admin"

# JSON stream for piping into alerting
unifly events watch --types Client -o json | jq -c 'select(.severity == "warning")'
```

### Firewall policy reorder (round-trippable)

```bash
# Read current order for a zone pair
unifly firewall policies reorder --source-zone <zid> --dest-zone <zid> --get

# Write back an explicit order
unifly firewall policies reorder --source-zone <zid> --dest-zone <zid> \
  --set "<id1>,<id2>,<id3>"
```

### Raw API escape hatch

For endpoints unifly does not wrap (including UniFi v2 routes and Integration
paths), use `unifly api`. It routes through the Session client, so CSRF token
management and session caching are automatic.

```bash
unifly api "v2/api/site/default/traffic-flow-latest-statistics"
unifly api "cmd/stamgr" -m post -d '{"cmd":"kick-sta","mac":"aa:bb:cc:dd:ee:ff"}'
unifly api "api/s/default/set/setting/teleport" -m put -d '{"enabled":true}'
```

### Session API VPN payloads and settings

`unifly vpn site-to-site` wraps Session API `rest/networkconf` records whose
`purpose` is `site-vpn`. This is the current CRUD path for manual IPsec and
OpenVPN site-to-site records exposed by the controller.

```bash
unifly vpn site-to-site list -o json
unifly vpn site-to-site get <id> -o json
unifly vpn site-to-site create -F site-to-site.json
unifly vpn site-to-site update <id> -F site-to-site.json
unifly vpn site-to-site delete <id>
```

`unifly vpn remote-access` wraps Session API `rest/networkconf` records whose
`purpose` is `remote-user-vpn`. This is the current CRUD path for L2TP,
OpenVPN, and WireGuard remote-access servers exposed by the controller.

```bash
unifly vpn remote-access list -o json
unifly vpn remote-access get <id> -o json
unifly vpn remote-access create -F remote-access.json
unifly vpn remote-access update <id> -F remote-access.json
unifly vpn remote-access suggest-port -o json
unifly vpn remote-access download-config <id> --path .
unifly vpn remote-access delete <id>
```

`unifly vpn clients` wraps Session API `rest/networkconf` records whose
`purpose` is `vpn-client`. This is the current CRUD path for configured
OpenVPN and WireGuard client profiles exposed by the controller.

```bash
unifly vpn clients list -o json
unifly vpn clients get <id> -o json
unifly vpn clients create -F vpn-client.json
unifly vpn clients update <id> -F vpn-client.json
unifly vpn clients delete <id>
```

`unifly vpn peers` wraps the Session v2 API WireGuard peer endpoints for
remote-access VPN servers. `list` can enumerate all peers or scope to a
single server ID; `create`, `update`, and `delete` require the parent
remote-access server ID.

```bash
unifly vpn peers list -o json
unifly vpn peers list <server-id> -o json
unifly vpn peers get <server-id> <peer-id> -o json
unifly vpn peers create <server-id> -F peer.json
unifly vpn peers update <server-id> <peer-id> -F peer.json
unifly vpn peers delete <server-id> <peer-id>
unifly vpn peers subnets -o json
```

`unifly vpn connections` wraps the Session v2 API VPN client connection
inventory exposed at `v2/api/site/<site>/vpn/connections`. `restart`
issues the same controller action the web UI uses for a single connection.

```bash
unifly vpn connections list -o json
unifly vpn connections get <id> -o json
unifly vpn connections restart <id>
```

`unifly vpn magic-site-to-site` wraps the Session v2 API
`magicsitetositevpn/configs` inventory endpoint. It is currently
read-only.

```bash
unifly vpn magic-site-to-site list -o json
unifly vpn magic-site-to-site get <id> -o json
```

`unifly vpn settings` wraps the Session API `rest/setting` records for the VPN
feature toggles the controller exposes today: `teleport`,
`magic-site-to-site-vpn`, `openvpn`, and `peer-to-peer`.

```bash
unifly vpn settings list -o json
unifly vpn settings get peer-to-peer -o json
unifly vpn settings set teleport --enabled true
unifly vpn settings patch peer-to-peer -F peer-to-peer.json
```

`site-to-site get`, `remote-access get`, `clients get`, `connections get`,
`peers get`, and `magic-site-to-site get` return redacted records with
summary fields and the sanitized controller payload under `fields`.

`settings get` returns a redacted wrapper with `key`, `enabled`, and `fields`.
`patch` accepts either the raw session setting body or that wrapper shape and
will send the inner `fields` object back to the controller.

### Bulk operations via filter DSL

`hotspot purge --filter` accepts the Integration filter DSL for bulk deletion
without ID iteration:

```bash
unifly hotspot purge --filter "status.eq('UNUSED')"
unifly hotspot purge --filter "name.contains('Conference')"
```

### TUI handoff for human verification

Propose a change, let a human visually confirm in the TUI before committing:

```bash
# Agent inspects, proposes. Human runs unifly tui and verifies on
# screen 4 (Networks) or 5 (Firewall) before the agent applies the change.
unifly tui
```

### Multi-profile targeting

```bash
unifly -p home devices list
unifly -p office firewall policies list
UNIFI_PROFILE=warehouse unifly system health
```

## Essential Gotchas

1. **Default list limit is 25.** The CLI prints a truncation hint when
   results hit the default. For enumeration, always pass `--all` or
   `--limit 200` (or higher).
2. **Environment variables use the `UNIFI_` prefix, not `UNIFLY_`.** Relevant
   vars: `UNIFI_URL`, `UNIFI_API_KEY`, `UNIFI_USERNAME`, `UNIFI_PASSWORD`,
   `UNIFI_SITE`, `UNIFI_PROFILE`, `UNIFI_OUTPUT`, `UNIFI_INSECURE`,
   `UNIFI_TIMEOUT`, `UNIFI_TOTP`. The only `UNIFLY_*` var is `UNIFLY_THEME`
   for the TUI.
3. **`--yes` / `-y`** skips confirmation prompts for mutations. Required for
   non-interactive use.
4. **API key mode covers most commands** on UniFi OS, including Session API
   endpoints (stats, device commands, Wi-Fi observability, client enrichment).
   Use **Hybrid only when live WebSocket streaming is needed** (`events watch`,
   TUI live refresh). Client and device enrichment fields work in API key mode.
5. **Local controllers only.** unifly targets on-prem controllers. The Site
   Manager cloud API (`api.ui.com`) is not yet implemented.
6. **Exit codes are meaningful.** `0` on success, non-zero on error. Capture
   stderr for diagnostics.

## Agent Workflow

1. Verify the tool exists with `command -v unifly`.
2. Check auth mode with `unifly config show` before running commands that
   require Session or Integration specifically.
3. Run `unifly system health -o json` as the first touch to confirm
   connectivity.
4. Inspect before mutating: `list` / `get` the entity first, capture IDs.
5. For complex creates, write a JSON payload and use `--from-file`.
6. After mutations, re-fetch the entity with `get` to confirm state.
7. For irreversible operations (delete, reboot, poweroff), surface a
   summary to the user before running even with `--yes`.

## Additional Resources

### Reference Files

- **`references/commands.md`**: Per-command flag reference with gotchas
  (non-obvious flags, dual-API boundaries, correct argument forms)
- **`references/concepts.md`**: UniFi networking concepts, dual-API gate
  matrix, auth decision tree, environment variables, platform config paths,
  MFA/TOTP, error taxonomy
- **`references/workflows.md`**: Runnable automation recipes (event
  streaming, safe firewall reorder, bulk DHCP reservations, ad-blocking via
  DNS policies, cafe voucher flow, incident response)

### Example Files

- **`examples/config.toml`**: Multi-profile config template
- **`examples/network-iot-vlan.json`**: VLAN creation payload for `--from-file`
- **`examples/firewall-block-iot.json`**: Firewall policy payload
- **`examples/nat-masquerade.json`**: NAT masquerade policy payload
- **`examples/nat-port-forward.json`**: Destination NAT (port forward) payload
- **`examples/wifi-iot.json`**: WiFi SSID payload
- **`examples/vpn-remote-access-wireguard.json`**: WireGuard remote-access VPN payload
- **`examples/vpn-site-to-site-ipsec.json`**: IPsec site-to-site tunnel payload
- **`examples/vpn-client-openvpn.json`**: OpenVPN client payload
- **`examples/vpn-wireguard-peer.json`**: WireGuard peer configuration payload
