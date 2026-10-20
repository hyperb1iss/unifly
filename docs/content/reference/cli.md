+++
title = "CLI Reference"
description = "All 29 commands, flags, output formats, and gotchas"
weight = 1
+++

Every command supports `--help` for exhaustive flag listings. This page documents subcommands, key flags, and gotchas you won't find in `--help`.

**API legend:** **I** = Integration API (API key). **L** = Session API (API key on UniFi OS, or username/password). **H** = Works in any mode, enriched when session HTTP access exists.

## Commands

| Command         | Alias      | API   | Description                                                                                                                                                                  |
| --------------- | ---------- | ----- | ---------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `devices`       | `dev`, `d` | H     | Manage adopted and pending devices                                                                                                                                           |
| `clients`       | `cl`       | H     | Manage connected clients and DHCP reservations                                                                                                                               |
| `networks`      | `net`, `n` | I     | Manage networks and VLANs                                                                                                                                                    |
| `wifi`          | `w`        | I     | Manage WiFi broadcasts (SSIDs)                                                                                                                                               |
| `firewall`      | `fw`       | Mixed | Manage firewall policies, zones, and groups                                                                                                                                  |
| `nat`           |            | L     | Manage NAT policies (masquerade, SNAT, DNAT)                                                                                                                                 |
| `acl`           |            | I     | Manage ACL rules                                                                                                                                                             |
| `dns`           |            | I     | Manage DNS policies (local records)                                                                                                                                          |
| `traffic-lists` |            | I     | Manage traffic matching lists                                                                                                                                                |
| `hotspot`       |            | I     | Manage hotspot vouchers                                                                                                                                                      |
| `vpn`           |            | Mixed | View VPN inventory, session site-to-site, remote-access, and client VPN records, OpenVPN helpers, connections, WireGuard peers, magic site-to-site configs, and VPN settings |
| `events`        |            | L     | View and stream events                                                                                                                                                       |
| `alarms`        |            | L     | Manage alarms                                                                                                                                                                |
| `stats`         |            | L     | Query statistics and reports                                                                                                                                                 |
| `sites`         |            | L     | Manage sites                                                                                                                                                                 |
| `admin`         |            | L     | Administrator management                                                                                                                                                     |
| `system`        | `sys`      | Mixed | System operations and info                                                                                                                                                   |
| `settings`      |            | L     | Read and write site-level settings sections                                                                                                                                  |
| `topology`      | `topo`     | H     | Show network topology tree                                                                                                                                                   |
| `dpi`           |            | Mixed | DPI reference data and control                                                                                                                                               |
| `radius`        |            | I     | View RADIUS profiles                                                                                                                                                         |
| `wans`          |            | I     | View WAN interfaces                                                                                                                                                          |
| `countries`     |            | I     | List available country codes                                                                                                                                                 |
| `cloud`         |            | Cloud | Query the Site Manager fleet (consoles, sites, devices, ISP, SD-WAN)                                                                                                         |
| `config`        |            | Local | Manage CLI configuration                                                                                                                                                     |
| `completions`   |            | Local | Generate shell completions                                                                                                                                                   |
| `api`           |            | L     | Raw API passthrough (GET/POST/PUT/PATCH/DELETE any endpoint)                                                                                                                 |
| `tui`           |            | H     | Launch the real-time terminal dashboard                                                                                                                                      |

{% tip() %}
List commands default to 25 rows. Pass `--all` or `--limit 200` for complete results.
{% end %}

## Devices

```bash
unifly devices list                             # All adopted devices
unifly devices list --filter "state.eq('Online')"  # Filter by status
unifly devices get <DEVICE>                     # Device details
unifly devices pending                          # Devices awaiting adoption
unifly devices adopt <MAC>                      # Adopt a pending device
unifly devices remove <DEVICE>                  # Unadopt a device
unifly devices restart <DEVICE>                 # Restart
unifly devices locate <DEVICE> --on true        # Flash LED (--on false to stop)
unifly devices port-cycle <DEVICE> <PORT>       # Power-cycle a PoE port
unifly devices stats <DEVICE>                   # Real-time device stats
unifly devices upgrade <DEVICE>                 # Trigger firmware upgrade
unifly devices provision <DEVICE>               # Force re-provision config
unifly devices speedtest                        # WAN speed test (gateway only)
unifly devices tags                             # List device tags
```

`<DEVICE>` is the device name (case-insensitive), ID, IP, or MAC in any common spelling (`aa:bb:cc:dd:ee:ff`, `AA-BB-CC-DD-EE-FF`, `aabb.ccdd.eeff`). A name two devices share is an error that lists both; pass an ID then. `adopt` takes the MAC, since a pending device is not in the adopted inventory yet.

Gotchas: `restart`, `locate`, `upgrade`, `provision`, `speedtest`, and `port-cycle` require Session API access. `list` and `get` work with any auth mode but return richer data when session HTTP access exists (client count, uplink MAC).

### Switch Ports (config-as-code)

```bash
unifly devices ports <DEVICE>                   # Current port config
unifly devices ports <DEVICE> --with-clients    # Annotate with connected clients
unifly devices ports-export <DEVICE>            # Export overrides as JSONC
unifly devices ports-export <DEVICE> --all      # Include every port
unifly devices port-set <DEVICE> 5 --mode access --native-vlan IoT
unifly devices port-set <DEVICE> -F ports.jsonc # Apply a JSONC file
unifly devices port-set <DEVICE> 5 --reset      # Back to controller defaults
```

Port indices are **1-based**, matching the controller's wire format. `port-set -F` accepts a JSONC payload (comments and trailing commas allowed) with splice semantics: ports not listed keep their existing override, and a per-port `"reset": true` removes that port's override entirely. `--with-clients` adds connection annotations, and on `ports-export` it prepends `// last-seen <timestamp>: <mac>` comment lines for git-diff drift detection. `ports-export` always writes JSONC, whatever `-o` says. See [Switch Ports](/guide/switch-ports) for the full workflow and payload schema.

## Clients

```bash
unifly clients list                             # Connected clients
unifly clients list --type wireless             # Filter by type
unifly clients find "ring-doorbell"             # Search by name, IP, or MAC
unifly clients get <CLIENT>                     # Client details
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
unifly clients roams <CLIENT>                   # Roaming history for a client
unifly clients wifi <CLIENT>                    # WiFi experience details for a client
```

`<CLIENT>` is a name, hostname, ID, IP, or MAC in any common spelling. Other `get` commands take an ID or the record's name, compared case-insensitively (DNS policies by domain, vouchers by code).

Gotchas: `list` returns enriched data in Hybrid mode (traffic bytes, hostname, wireless, VLAN). `block`/`unblock`/`kick`/`forget` and DHCP reservation commands require Session API.

## Networks

```bash
unifly networks list                            # All networks/VLANs
unifly networks get <ID|NAME>                   # Full network details
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
unifly wifi get <ID|NAME>                       # SSID details
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
unifly firewall policies get <ID|NAME>
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
unifly firewall zones get <ID|NAME>
unifly firewall zones create --name "IoT Zone" --networks <ID1,ID2>
unifly firewall zones create -F zone.json
unifly firewall zones update <ID> --networks <ID1,ID2>
unifly firewall zones delete <ID>
```

### Groups

```bash
unifly firewall groups list
unifly firewall groups list --type port-group
unifly firewall groups get <ID|NAME>
unifly firewall groups create --name "Web Ports" --type port-group --members "80,443,8000-8002"
unifly firewall groups create --name "IoT Nets" --type address-group --members "10.0.30.0/24"
unifly firewall groups update <ID> --members "80,443"
unifly firewall groups delete <ID>
```

Gotchas: groups use the **Session API** (`rest/firewallgroup`), not Integration; an API key on UniFi OS is sufficient. Types: `port-group` (default), `address-group`, `ipv6-address-group`. The `external_id` UUID in group responses is what firewall policies reference. Policy `create`/`update` resolve group **names** for you via `--src-port-group` / `--dst-port-group` / `--src-address-group` / `--dst-address-group` flags (or `dst_port_group` / `dst_address_group` shorthand fields in `--from-file` JSON).

## NAT

```bash
unifly nat policies list
unifly nat policies get <ID|NAME>
unifly nat policies create --name "Masquerade" --type masquerade --interface-id <ID>
unifly nat policies create -F nat.json
unifly nat policies update <ID> --name "New Name" --enabled true
unifly nat policies delete <ID>
```

Use `nat policies update <ID>` to modify an existing rule. Pass any
combination of `--name`, `--type`, `--enabled`, address/port flags, or
`--from-file`. Only the specified fields are changed.

NAT types: `masquerade` (outgoing interface address), `source` (explicit rewrite), `destination` (port forwarding/DNAT). NAT routes through the Session v2 API: an API key on UniFi OS is sufficient, and session-only profiles work too (`nat policies list` populates from the session refresh snapshot). Only pure-Integration setups without session HTTP access cannot manage NAT.

## ACL

```bash
unifly acl list
unifly acl get <ID|NAME>
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
unifly dns get <ID|NAME>
unifly dns create --record-type a --domain "nas.home" --value "10.0.10.5"
unifly dns create -F dns.json
unifly dns update <ID> -F dns.json
unifly dns delete <ID>
```

Supported record types: `a`, `aaaa`, `cname`, `mx`, `txt`, `srv`, `forward`. The `--record-type` values are **lowercase**; uppercase `A` is rejected by argument parsing. Table output displays the familiar short uppercase names (`A`, `AAAA`, ...).

## Traffic Lists

```bash
unifly traffic-lists list
unifly traffic-lists get <ID|NAME>
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

{% warning() %}
`--types` takes category names, not `EVT_*` glob patterns. Use `Device` not `EVT_SW_*`.
{% end %}

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
unifly hotspot get <ID|NAME>
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

{% warning() %}
`reboot` and `poweroff` are destructive and require confirmation (`-y` to skip).
{% end %}

## VPN

```bash
unifly vpn servers                              # List VPN servers (Integration)
unifly vpn tunnels                              # List site-to-site tunnels (Integration)
unifly vpn status                               # Live IPsec tunnel status
unifly vpn health                               # VPN subsystem health
unifly vpn site-to-site list                    # List site-to-site VPN records
unifly vpn site-to-site get <ID>                # Inspect one site-to-site VPN record
unifly vpn site-to-site create -F vpn.json      # Create a site-to-site VPN
unifly vpn site-to-site update <ID> -F vpn.json # Update a site-to-site VPN
unifly vpn site-to-site delete <ID>             # Delete a site-to-site VPN
unifly vpn remote-access list                   # List remote-access VPN servers
unifly vpn remote-access get <ID>               # Inspect one remote-access VPN server
unifly vpn remote-access create -F vpn.json     # Create a remote-access VPN server
unifly vpn remote-access update <ID> -F vpn.json # Update a remote-access VPN server
unifly vpn remote-access suggest-port           # Suggest available OpenVPN ports
unifly vpn remote-access download-config <ID>   # Download an OpenVPN client config
unifly vpn remote-access delete <ID>            # Delete a remote-access VPN server
unifly vpn clients list                         # List configured VPN clients
unifly vpn clients get <ID>                     # Inspect one configured VPN client
unifly vpn clients create -F vpn.json           # Create a configured VPN client
unifly vpn clients update <ID> -F vpn.json      # Update a configured VPN client
unifly vpn clients delete <ID>                  # Delete a configured VPN client
unifly vpn connections list                     # List VPN client connections
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
unifly vpn settings list                        # List VPN-related site settings
unifly vpn settings get teleport                # Inspect one VPN setting
unifly vpn settings set teleport --enabled true # Toggle a VPN setting
unifly vpn settings patch peer-to-peer -F peer.json   # Apply a JSON payload
```

Gotchas: `servers` and `tunnels` are read-only Integration API inventory; everything else is Session-backed and works with an API key on UniFi OS. Create and update for `site-to-site`, `remote-access`, `clients`, and `peers` are payload-driven: build the JSON with `--from-file` (see the [examples/](https://github.com/hyperb1iss/unifly/tree/main/crates/unifly/skill/examples) templates). Sensitive material such as WireGuard private keys and IPsec pre-shared keys is **redacted** in both `get` and `create` output; reconstruct those fields explicitly in update payloads if the controller requires them unchanged. The full walkthrough lives on the [VPN page](/guide/vpn).

## Settings

```bash
unifly settings list                            # All site setting sections
unifly settings get dpi                         # One section
unifly settings set dpi enabled true            # Patch one field
unifly settings set usg --data '{"mss_clamp": "auto"}'  # Merge a JSON object
unifly settings export                          # JSON dump of every section, secrets left out
unifly settings export --with-secrets           # Include x_ credential fields
```

Site-level settings live in Session API setting sections. `set` performs a read-modify-write of the whole section. `x_`-prefixed secret fields show as `***` in table mode and are left out of JSON/YAML output and `export` unless you pass `--with-secrets`. `set` never erases a credential the update left out. Session-gated: needs an API key on UniFi OS or session credentials. Details on the [Site Settings page](/guide/settings).

## Cloud (Site Manager)

```bash
unifly cloud hosts                              # Consoles visible to the API key
unifly cloud sites                              # Sites across the fleet
unifly cloud devices                            # Devices across all consoles
unifly cloud devices --host <ID>                # Scope to one console
unifly cloud isp                                # ISP metrics
unifly cloud sdwan                              # SD-WAN configs
unifly cloud switch <SITE>                      # Point the cloud profile at a site
```

`cloud` talks directly to the Site Manager fleet API at `api.ui.com` with a Site Manager API key; no controller connection is involved. With `auth_mode = "cloud"`, regular Integration-backed commands (`networks list`, `firewall policies list`, ...) tunnel through the cloud connector to one console. Set up with `unifly config cloud-setup`; the full story is on the [Cloud & Site Manager page](/guide/cloud).

## Other Commands

```bash
unifly topology                                 # Network tree visualization
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
unifly config cloud-setup                       # Guided Site Manager (cloud) profile
unifly config show -o table                     # Show saved config with secrets masked
unifly config set auth_mode hybrid              # Set a config value
unifly config set-password                      # Store password in keyring
unifly config set-password --profile office     # For a specific profile
unifly config profiles                          # List profiles (* = active)
unifly config use <PROFILE>                     # Switch default profile
```

Valid `config set` keys: `controller`, `site`, `auth_mode`, `api_key`, `api_key_env`, `host_id`, `host_id_env`, `username`, `insecure`, `timeout`, `ca_cert`. `auth_mode` accepts `integration`, `session`, `hybrid`, or `cloud` (`legacy` is normalized to `session`).

## `--from-file` / `-F`

Most create/update commands accept `-F <file.json>` to read the request body from a JSON file. This is the preferred approach for complex payloads.

Accepted by: `networks`, `wifi`, `firewall policies`, `firewall zones`, `firewall groups`, `nat policies`, `acl`, `dns`, `traffic-lists`, `vpn site-to-site`, `vpn remote-access`, `vpn clients`, `vpn peers`, `vpn settings patch`, and `devices port-set` (JSONC for switch port config-as-code).

JSONC is accepted everywhere: comments and trailing commas are stripped before parsing.

```bash
unifly networks create -F network.json
unifly firewall policies create -F policy.json
```

See [examples/](https://github.com/hyperb1iss/unifly/tree/main/crates/unifly/skill/examples) for payload templates.

## List Filtering (`--filter`)

`--filter` on list commands is evaluated **client-side** and supports exactly one expression, with three operators:

```bash
unifly devices list --filter "state.eq('Online')"
unifly clients list --filter "name.contains('ring')"
unifly networks list --filter "vlan.in('20','30','40')"
```

Field paths address the JSON output shape (dot notation for nesting). An unparseable expression fails with exit code 2 before the command fetches anything. A well-formed expression that names a field the output lacks still matches nothing and exits 0, so a silent empty result usually means a misspelled field, not an empty collection. An empty expression applies no filter.

The full server-side Integration filter DSL (`neq`, `gt`/`lt`, `startswith`, `&&`/`||`) applies only to `hotspot purge --filter`, which passes the expression to the controller.

## Capturing Created IDs

Create commands print the created entity on stdout in the requested output format, with the human confirmation line on stderr. That makes ID capture a one-liner:

```bash
NET_ID=$(unifly networks create --name "IoT" --vlan 30 \
  --management gateway --ipv4-host "10.0.30.1/24" -o json | jq -r '.id')

# -o plain emits just the new ID
ZONE_ID=$(unifly firewall zones create --name "IoT Zone" -o plain)
```

`sites create` is the exception: the controller returns no record to print. VPN create output has sensitive fields (private keys, PSKs) redacted, same as `get`.

## Offline Agent Guide

The installed binary serves its matching agent guidance without connecting
to a controller or reading credentials:

```bash
unifly skill
unifly skill commands
unifly skill concepts
unifly skill workflows
unifly skill examples
unifly skill examples network-iot-vlan.json > network.json
```

The command inventory is generated from the CLI parser. Guide topics print
plain Markdown; named examples print the raw file, regardless of `--output`.
The installable agent skill is a small stub that loads this guide. Binaries
that predate the command need an update.

```bash
unifly skill --install                 # ~/.agents/skills/unifly/SKILL.md
unifly skill --install --claude        # Claude Code's skills directory
unifly skill --install --dir <DIR>     # DIR/unifly/SKILL.md
```

`--install` prints the installed path on stdout. It leaves an identical skill
alone, upgrades one from `npx skills add` or an earlier release, and refuses
(exit code 6) to replace any other `SKILL.md` without `--force`. It never
writes through a symlinked skill directory or file.

## Global Flags

```text
-p, --profile <NAME>     Controller profile to use
-c, --controller <URL>   Controller URL (overrides profile)
-s, --site <SITE>        Site name or UUID
-o, --output <FORMAT>    Output: table, json, json-compact, yaml, plain
-k, --insecure           Accept self-signed TLS certs (--insecure=false forces verification)
-v, --verbose            Increase verbosity (-v, -vv, -vvv)
-q, --quiet              Suppress non-error output
-y, --yes                Skip confirmation prompts
    --timeout <SECS>     Request timeout (default 30, profiles may override)
    --color <MODE>       Color: auto, always, never
    --no-cache           Force fresh login (bypass session cache)
    --api-key <KEY>      Integration API key (one-shot override)
    --demo               Sanitize PII in the TUI (env UNIFI_DEMO)
    --demo-fleet         Use the synthetic offline demo fleet (env UNIFI_DEMO_FLEET)
    --demo-seed <SEED>   Demo fleet seed (env UNIFI_DEMO_SEED)
```

`--insecure` is tri-state: absent defers to the profile and `[defaults]`, `-k` / `--insecure` accepts self-signed certificates, and `--insecure=false` forces verification no matter what the config says. The explicit value requires the equals form (`--insecure false` does not parse). `--timeout` has no baked-in flag default; when the flag and `UNIFI_TIMEOUT` are absent, the profile's `timeout` wins, then `[defaults]`, then the built-in 30 seconds.

## Demo Fleet

`--demo-fleet` (or `UNIFI_DEMO_FLEET=1`) answers a curated set of read-only commands from the synthetic, offline fleet the [TUI demo mode](/reference/tui#demo-mode) uses, so CLI screenshots need no controller, credentials, or config file. Every output format works, except that `devices ports-export` always writes JSONC as it does against a real controller, and `--demo-seed <N>` picks a different fleet that is just as reproducible.

| Area          | Commands                                                                                         |
| ------------- | ------------------------------------------------------------------------------------------------ |
| Diagnosis     | `system health`, `system info`, `alarms list`, `wans list`                                       |
| Devices       | `devices list`, `devices get`, `devices stats`, `devices ports`, `devices ports-export`          |
| Clients       | `clients list`, `clients get`, `clients find`, `clients roams`, `clients wifi`                   |
| Networks      | `networks list`, `networks get`                                                                  |
| WiFi          | `wifi list`, `wifi get`, `wifi neighbors`, `wifi channels`                                       |
| Firewall      | `firewall policies list/get`, `firewall zones list/get`, `nat policies list/get`, `acl list/get` |
| Events, stats | `events list`, `stats site`, `stats gateway`, `stats dpi`                                        |
| Other         | `topology`, `sites list`                                                                         |

Any other command exits with code 5 and a "not available in demo mode" error before it reads config or touches the network. Each run generates the fleet fresh from the seed, so names, MACs, IPs, and IDs match from one run to the next. Timestamps follow the launch time, except the `// last-seen` markers of `devices ports-export --with-clients`, which carry a fixed `2026-03-18T15:30Z` so a seed exports the same bytes on every run.

The fleet comes with problems to diagnose: an offline access point, one with a firmware update waiting, a camera whose switch port went dark, and an unplugged second WAN. Alarms, the WAN list, events, device details, and switch ports all tell the same story, and site health flags the offline access point as a `wlan` warning. `devices ports` and `devices ports-export` (with or without `--with-clients`) show each port's VLANs, PoE, and link state in the same shape a real controller produces, while `devices port-set` is refused like every other write.

```bash
unifly --demo-fleet devices list
unifly --demo-fleet alarms list
unifly --demo-fleet devices get ap-roof -o json
unifly --demo-fleet devices ports sw-core --with-clients
unifly --demo-fleet clients list --all -o json | jq length
unifly --demo-fleet --demo-seed 7 stats gateway -o json-compact
```

## Output Formats

| Format       | Flag              | Best For                        |
| ------------ | ----------------- | ------------------------------- |
| Table        | `-o table`        | Human reading (default)         |
| JSON         | `-o json`         | Scripting, agent use            |
| Compact JSON | `-o json-compact` | Line-oriented processing, pipes |
| YAML         | `-o yaml`         | Config-style output             |
| Plain        | `-o plain`        | IDs for `xargs` pipelines       |

Exporters write a fixed format and ignore `-o`: `devices ports-export` writes JSONC for `port-set -F`, and `settings export` writes JSON.

```bash
# Pipe plain IDs into another command
unifly clients list -o plain | xargs -n1 unifly clients get
```

## Next Steps

- [TUI Dashboard](/reference/tui): real-time monitoring with keybindings
- [Authentication](/guide/authentication): which auth mode enables which commands
- [Configuration](/guide/configuration): profiles, environment variables, precedence
- [VPN](/guide/vpn), [Switch Ports](/guide/switch-ports), [Site Settings](/guide/settings), [Cloud & Site Manager](/guide/cloud): feature deep-dives
- [Troubleshooting](/troubleshooting): common errors and fixes
