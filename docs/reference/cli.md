# 💜 CLI Reference

Every command supports `--help` for exhaustive flag listings. This page documents subcommands, key flags, and gotchas you won't find in `--help`.

**API legend:** **I** = Integration API (API key). **L** = Session API (username/password). **H** = Works in any mode, enriched by Hybrid.

## Commands

| Command         | Alias      | API   | Description                                    |
| --------------- | ---------- | ----- | ---------------------------------------------- |
| `devices`       | `dev`, `d` | H     | Manage adopted and pending devices             |
| `clients`       | `cl`       | H     | Manage connected clients and DHCP reservations |
| `networks`      | `net`, `n` | I     | Manage networks and VLANs                      |
| `wifi`          | `w`        | I     | Manage WiFi broadcasts (SSIDs)                 |
| `firewall`      | `fw`       | I     | Manage firewall policies and zones             |
| `nat`           |            | L     | Manage NAT policies (masquerade, SNAT, DNAT)   |
| `acl`           |            | I     | Manage ACL rules                               |
| `dns`           |            | I     | Manage DNS policies (local records)            |
| `traffic-lists` |            | I     | Manage traffic matching lists                  |
| `hotspot`       |            | I     | Manage hotspot vouchers                        |
| `vpn`           |            | Mixed | View VPN inventory, session site-to-site, remote-access, and client VPN records, OpenVPN helpers, connections, WireGuard peers, magic site-to-site configs, and VPN settings |
| `events`        |            | L     | View and stream events                         |
| `alarms`        |            | L     | Manage alarms                                  |
| `stats`         |            | L     | Query statistics and reports                   |
| `sites`         |            | L     | Manage sites                                   |
| `admin`         |            | L     | Administrator management                       |
| `system`        | `sys`      | Mixed | System operations and info                     |
| `topology`      | `topo`     | H     | Show network topology tree                     |
| `dpi`           |            | Mixed | DPI reference data and control                 |
| `radius`        |            | I     | View RADIUS profiles                           |
| `wans`          |            | I     | View WAN interfaces                            |
| `countries`     |            | I     | List available country codes                   |
| `config`        |            | Local | Manage CLI configuration                       |
| `completions`   |            | Local | Generate shell completions                     |
| `api`           |            | L     | Raw API passthrough (GET/POST/PUT/PATCH/DELETE any endpoint) |
| `tui`           |            | H     | Launch the real-time terminal dashboard        |

::: tip
List commands default to 25 rows. Pass `--all` or `--limit 200` for complete results.
:::

## Devices

```bash
unifly devices list                             # All adopted devices
unifly devices list --filter "state.eq('ONLINE')"  # Filter by status
unifly devices get <ID|MAC>                     # Device details
unifly devices pending                          # Devices awaiting adoption
unifly devices adopt <MAC>                      # Adopt a pending device
unifly devices remove <ID|MAC>                  # Unadopt a device
unifly devices restart <ID|MAC>                 # Restart
unifly devices locate <MAC> --on true           # Flash LED (--on false to stop)
unifly devices port-cycle <ID|MAC> <PORT>       # Power-cycle a PoE port
unifly devices stats <ID|MAC>                   # Real-time device stats
unifly devices upgrade <MAC>                    # Trigger firmware upgrade
unifly devices provision <MAC>                  # Force re-provision config
unifly devices speedtest                        # WAN speed test (gateway only)
unifly devices tags                             # List device tags
```

Gotchas: `restart`, `locate`, `upgrade`, `provision`, `speedtest`, and `port-cycle` require Session API access. `list` and `get` work with any auth mode but return richer data in Hybrid (client count, uplink MAC).

## Clients

```bash
unifly clients list                             # Connected clients
unifly clients list --type wireless             # Filter by type
unifly clients find "ring-doorbell"             # Search by name, IP, or MAC
unifly clients get <MAC>                        # Client details
unifly clients reservations                     # All DHCP reservations
unifly clients set-ip <MAC> --ip 10.0.10.50    # Create DHCP reservation
unifly clients set-ip <MAC> --ip 10.0.10.50 --network <ID>  # Scoped to network
unifly clients remove-ip <MAC>                  # Remove DHCP reservation
unifly clients block <MAC>                      # Block from connecting
unifly clients unblock <MAC>                    # Unblock
unifly clients kick <MAC>                       # Force reconnection
unifly clients authorize <MAC> --minutes 60     # Authorize guest access
unifly clients unauthorize <MAC>                # Revoke guest access
unifly clients forget <MAC>                     # Remove from client history
unifly clients roams <MAC>                      # Roaming history for a client
unifly clients wifi <MAC>                       # WiFi experience details for a client
```

Gotchas: `list` returns enriched data in Hybrid mode (traffic bytes, hostname, wireless, VLAN). `block`/`unblock`/`kick`/`forget` and DHCP reservation commands require Session API.

## Networks

```bash
unifly networks list                            # All networks/VLANs
unifly networks get <ID>                        # Full network details
unifly networks create --name "IoT" --management gateway --vlan 20 --ipv4-host "10.0.20.1/24"
unifly networks create -F network.json          # Create from JSON file
unifly networks update <ID> --enabled false
unifly networks delete <ID>
unifly networks refs <ID>                       # What depends on this network?
```

Gotchas: `list` returns summary data without `ipv4Configuration`. Use `get` for full config. `refs` is the only pre-delete dependency check; use it before deleting.

## WiFi

```bash
unifly wifi list                                # All SSIDs
unifly wifi get <ID>                            # SSID details
unifly wifi create --name "Guest" --network <ID> --security wpa2-personal --passphrase "..."
unifly wifi create -F wifi.json                 # Create from JSON file
unifly wifi update <ID> --enabled false
unifly wifi delete <ID>
unifly wifi neighbors                           # Scan nearby APs (RF environment)
unifly wifi channels                            # Channel utilization analysis
```

Gotchas: Serde defaults to PascalCase for enums in `--from-file` JSON. Use `"Wpa2Personal"` not `"wpa2_personal"`. The `--security` flag on the CLI accepts kebab-case (`wpa2-personal`). `neighbors` and `channels` are read-only observability commands that query Session API data.

## Firewall

### Policies

```bash
unifly firewall policies list
unifly firewall policies get <ID>
unifly firewall policies create --name "Block IoT" --action block \
  --source-zone <ID> --dest-zone <ID>
unifly firewall policies create -F policy.json
unifly firewall policies update <ID> -F policy.json
unifly firewall policies patch <ID> --enabled false    # Quick toggle
unifly firewall policies patch <ID> --logging true     # Toggle logging
unifly firewall policies delete <ID>
unifly firewall policies reorder --source-zone <ID> --dest-zone <ID> --get
unifly firewall policies reorder --source-zone <ID> --dest-zone <ID> --set <ID1,ID2,...>
```

Gotchas: `patch` is the fast path for toggling `enabled`/`logging` only. Use `update` for other fields. `reorder` with `--get` shows current order; `--set` applies a new order. First-match wins, so ordering matters.

### Zones

```bash
unifly firewall zones list
unifly firewall zones get <ID>
unifly firewall zones create --name "IoT Zone" --networks <ID1,ID2>
unifly firewall zones create -F zone.json
unifly firewall zones update <ID> --networks <ID1,ID2>
unifly firewall zones delete <ID>
```

## NAT

```bash
unifly nat policies list
unifly nat policies get <ID>
unifly nat policies create --name "Masquerade" --type masquerade --interface-id <ID>
unifly nat policies create -F nat.json
unifly nat policies update <ID> --name "New Name" --enabled true
unifly nat policies delete <ID>
```

Use `nat policies update <ID>` to modify an existing rule. Pass any
combination of `--name`, `--type`, `--enabled`, address/port flags, or
`--from-file`. Only the specified fields are changed.

NAT types: `masquerade` (outgoing interface address), `source` (explicit rewrite), `destination` (port forwarding/DNAT). NAT routes through the Session v2 API, so credentials are required even in Hybrid mode.

## ACL

```bash
unifly acl list
unifly acl get <ID>
unifly acl create --name "Block printer" --rule-type ipv4 --action block \
  --source-zone <ID> --dest-zone <ID>
unifly acl create -F acl.json
unifly acl update <ID> -F acl.json
unifly acl delete <ID>
unifly acl reorder --get                        # Current order
unifly acl reorder --set <ID1,ID2,...>          # Apply new order
```

## DNS

```bash
unifly dns list
unifly dns get <ID>
unifly dns create --record-type A --domain "nas.home" --value "10.0.10.5"
unifly dns create -F dns.json
unifly dns update <ID> -F dns.json
unifly dns delete <ID>
```

Supported record types: `A`, `AAAA`, `CNAME`, `MX`, `TXT`, `SRV`, `Forward`.

## Traffic Lists

```bash
unifly traffic-lists list
unifly traffic-lists get <ID>
unifly traffic-lists create --name "Ad servers" --list-type ipv4 --items "1.2.3.4,5.6.7.8"
unifly traffic-lists create -F list.json
unifly traffic-lists update <ID> -F list.json
unifly traffic-lists delete <ID>
```

List types: `ports`, `ipv4`, `ipv6`. Referenced by firewall policies, NAT, and ACLs.

## Events

```bash
unifly events list                              # Recent events (last 24h)
unifly events list --within 4                   # Events from last 4 hours
unifly events list --limit 100                  # More results
unifly events watch                             # Live event feed (WebSocket)
unifly events watch --types Device              # Filter by category
unifly events watch --types Device,Client       # Multiple categories
```

The `--types` flag accepts `EventCategory` values (case-insensitive): `Device`, `Client`, `Network`, `System`, `Admin`, `Firewall`, `Vpn`, `Unknown`.

::: warning
`--types` takes category names, not `EVT_*` glob patterns. Use `Device` not `EVT_SW_*`.
:::

## Statistics

```bash
unifly stats site                               # Site-level stats
unifly stats device                             # Per-device bandwidth
unifly stats client                             # Per-client stats
unifly stats gateway                            # Gateway stats (WAN, uptime)
unifly stats dpi                                # DPI application breakdown
unifly stats dpi --group-by by-cat              # Group by category
unifly stats gateway --interval 5m              # High-resolution data
unifly stats site --interval daily              # Long-term trends
```

Supported intervals: `5m` (high resolution), `hourly` (default), `daily`, `monthly`.

## Hotspot

```bash
unifly hotspot list
unifly hotspot get <ID>
unifly hotspot create --name "Day Pass" --count 10 --minutes 1440
unifly hotspot delete <ID>
unifly hotspot purge --filter "status.eq('EXPIRED')"  # Bulk delete
```

## Admin

```bash
unifly admin list                               # List site administrators
unifly admin invite --name "Alex" --email "alex@example.com" --role admin
unifly admin revoke <ADMIN_ID>                  # Positional, not --email
unifly admin update <ADMIN_ID> --role readonly
```

Gotcha: `revoke` takes a positional admin ID, not an email flag.

## Alarms

```bash
unifly alarms list                              # All alarms
unifly alarms list --unarchived                 # Active alarms only
unifly alarms archive <ID>                      # Archive one alarm
unifly alarms archive-all                       # Archive everything
```

## DPI

```bash
unifly dpi apps                                 # List known applications (Integration)
unifly dpi categories                           # List known categories (Integration)
unifly dpi status                               # Current DPI state (Session)
unifly dpi enable                               # Turn on DPI (Session)
unifly dpi disable                              # Turn off DPI (Session)
```

## System

```bash
unifly system info                              # Application version (Integration)
unifly system health                            # Site health summary (Session)
unifly system sysinfo                           # Controller system info (Session)
unifly system backup create                     # Create backup
unifly system backup list                       # List backups
unifly system backup download <FILENAME>        # Download backup
unifly system backup delete <FILENAME>          # Delete backup
unifly system reboot                            # Reboot hardware (UDM only)
unifly system poweroff                          # Power off hardware (UDM only)
```

::: warning
`reboot` and `poweroff` are destructive and require confirmation (`-y` to skip).
:::

## Other Commands

```bash
unifly topology                                 # Network tree visualization
unifly vpn servers                              # List VPN servers
unifly vpn tunnels                              # List site-to-site tunnels
unifly vpn site-to-site list                    # List session site-to-site VPN records
unifly vpn site-to-site get <ID>                # Inspect one site-to-site VPN record
unifly vpn site-to-site create -F vpn.json      # Create a session site-to-site VPN
unifly vpn site-to-site update <ID> -F vpn.json # Update a session site-to-site VPN
unifly vpn site-to-site delete <ID>             # Delete a session site-to-site VPN
unifly vpn remote-access list                   # List session remote-access VPN servers
unifly vpn remote-access get <ID>               # Inspect one remote-access VPN server
unifly vpn remote-access create -F vpn.json     # Create a session remote-access VPN server
unifly vpn remote-access update <ID> -F vpn.json # Update a session remote-access VPN server
unifly vpn remote-access suggest-port           # Suggest available OpenVPN ports
unifly vpn remote-access download-config <ID>   # Download an OpenVPN client config
unifly vpn remote-access delete <ID>            # Delete a session remote-access VPN server
unifly vpn clients list                         # List configured session VPN clients
unifly vpn clients get <ID>                     # Inspect one configured VPN client
unifly vpn clients create -F vpn.json           # Create a configured VPN client
unifly vpn clients update <ID> -F vpn.json      # Update a configured VPN client
unifly vpn clients delete <ID>                  # Delete a configured VPN client
unifly vpn connections list                     # List session VPN client connections
unifly vpn connections get <ID>                 # Inspect one VPN client connection
unifly vpn connections restart <ID>             # Restart one VPN client connection
unifly vpn peers list [SERVER_ID]               # List WireGuard peers, optionally by server
unifly vpn peers get <SERVER_ID> <ID>           # Inspect one WireGuard peer
unifly vpn peers create <SERVER_ID> -F peer.json # Create a WireGuard peer
unifly vpn peers update <SERVER_ID> <ID> -F peer.json # Update a WireGuard peer
unifly vpn peers delete <SERVER_ID> <ID>        # Delete a WireGuard peer
unifly vpn peers subnets                        # List subnets already used by peers
unifly vpn magic-site-to-site list              # List magic site-to-site VPN configs
unifly vpn magic-site-to-site get <ID>          # Inspect one magic site-to-site VPN config
unifly vpn settings list                        # List session VPN-related site settings
unifly vpn settings get teleport                # Inspect one VPN setting
unifly vpn settings set teleport --enabled true # Toggle a VPN setting
unifly vpn settings patch peer-to-peer -F peer.json   # Apply a JSON payload
unifly radius profiles                          # List RADIUS profiles
unifly wans list                                # List WAN interfaces
unifly sites list                               # List sites
unifly sites create --name "Branch Office"      # Create site
unifly sites delete <NAME>                      # Delete site
unifly countries                                # Country codes for WiFi regulatory
```

## Raw API

Escape hatch for any controller endpoint. Routes through the Session client with automatic CSRF handling.

```bash
unifly api api/s/default/stat/sitedpi                    # GET a session endpoint
unifly api v2/api/site/default/nat                       # GET a v2 endpoint
unifly api api/s/default/stat/stadpi -m POST -d '{"type":"by_app"}'  # POST
unifly api api/s/default/set/setting/teleport -m PUT -d '{"enabled":true}'      # PUT
unifly api integration/v1/sites/<site-id>/hotspot/vouchers/<id> -m DELETE       # DELETE
```

## Configuration

```bash
unifly config init                              # Interactive setup wizard
unifly config show                              # Show resolved config
unifly config set auth_mode hybrid              # Set a config value
unifly config set-password                      # Store password in keyring
unifly config set-password --profile office     # For a specific profile
unifly config profiles                          # List profiles (* = active)
unifly config use <PROFILE>                     # Switch default profile
```

Valid `config set` keys: `controller`, `site`, `auth_mode`, `api_key`, `api_key_env`, `username`, `insecure`, `timeout`, `ca_cert`.

## `--from-file` / `-F`

Most create/update commands accept `-F <file.json>` to read the request body from a JSON file. This is the preferred approach for complex payloads.

Accepted by: `networks`, `wifi`, `firewall policies`, `firewall zones`, `nat policies`, `acl`, `dns`, `traffic-lists`, `hotspot`, `vpn site-to-site`, `vpn remote-access`, `vpn clients`, `vpn peers`.

```bash
unifly networks create -F network.json
unifly firewall policies create -F policy.json
```

See [examples/](https://github.com/hyperb1iss/unifly/tree/main/skills/unifly/examples) for payload templates.

## Integration Filter DSL

`--filter` on list commands accepts a small expression language:

```bash
unifly devices list --filter "state.eq('ONLINE') && model.startswith('U6')"
```

Operators: `eq`, `neq`, `contains`, `startswith`, `endswith`, `gt`, `lt`, `gte`, `lte`, `in`. Combine with `&&` and `||`.

Only Integration API commands respect `--filter`.

## Global Flags

```
-p, --profile <NAME>     Controller profile to use
-c, --controller <URL>   Controller URL (overrides profile)
-s, --site <SITE>        Site name or UUID
-o, --output <FORMAT>    Output: table, json, json-compact, yaml, plain
-k, --insecure           Accept self-signed TLS certificates
-v, --verbose            Increase verbosity (-v, -vv, -vvv)
-q, --quiet              Suppress non-error output
-y, --yes                Skip confirmation prompts
    --timeout <SECS>     Request timeout (default: 30)
    --color <MODE>       Color: auto, always, never
    --no-cache           Force fresh login (bypass session cache)
    --api-key <KEY>      Integration API key (one-shot override)
```

## Output Formats

| Format       | Flag              | Best For                        |
| ------------ | ----------------- | ------------------------------- |
| Table        | `-o table`        | Human reading (default)         |
| JSON         | `-o json`         | Scripting, agent use            |
| Compact JSON | `-o json-compact` | Line-oriented processing, pipes |
| YAML         | `-o yaml`         | Config-style output             |
| Plain        | `-o plain`        | IDs for `xargs` pipelines       |

```bash
# Pipe plain IDs into another command
unifly clients list -o plain | xargs -n1 unifly clients get
```

## 🎯 Next Steps

- [TUI Dashboard](/reference/tui): real-time monitoring with keybindings
- [Authentication](/guide/authentication): which auth mode enables which commands
- [Configuration](/guide/configuration): profiles, environment variables, precedence
- [Troubleshooting](/troubleshooting): common errors and fixes
