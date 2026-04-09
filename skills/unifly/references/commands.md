# unifly Command Reference

This file is a **gotchas-focused** reference. Every command accepts
`--help` at runtime with exhaustive flag listings; consult this file for
non-obvious flags, dual-API boundaries, correct argument forms, and the
cross-cutting patterns listed at the end.

**API legend:** **I** = Integration API required. **L** = Session API
required (username + password). **H** = Works in any mode, but enriched
by Hybrid. Consult `concepts.md` for the full gate matrix.

## Global Flags

```
-p, --profile <NAME>    Profile to use
-c, --controller <URL>  Override controller URL
-s, --site <SITE>       Target site (name or UUID)
-o, --output <FORMAT>   table | json | json-compact | yaml | plain
-k, --insecure          Accept self-signed TLS
-v, -vv, -vvv           Verbose logging
-q, --quiet             Suppress non-error output
-y, --yes               Skip confirmation prompts
    --timeout <SECS>    Request timeout (default 30)
    --color <MODE>      auto | always | never
    --no-cache          Force fresh login (bypass session cache)
    --api-key <KEY>     One-shot Integration API key override
```

All also accept the matching `UNIFI_*` environment variable (see
concepts.md).

## Devices `[H for list/get, L for commands]`

```bash
unifly devices list [--all] [-o json]
unifly devices get <id|mac> [-o json]
unifly devices adopt <mac> [--ignore-limit]
unifly devices remove <id|mac>
unifly devices restart <id|mac>
unifly devices locate <mac> [--on true|false]
unifly devices port-cycle <id|mac> <port_idx>
unifly devices stats <id|mac>
unifly devices pending
unifly devices upgrade <mac> [--url <firmware-url>]
unifly devices provision <mac>
unifly devices speedtest
unifly devices tags [subcommands]
```

**Gotchas:**

- `locate --on` is explicit boolean, not a toggle. `--on true` lights,
  `--on false` clears. Idempotent for automation.
- `upgrade --url` allows side-loading custom firmware URLs.
- `port-cycle` port index is zero-based.
- All device _commands_ (adopt, remove, restart, locate, port-cycle,
  upgrade, provision, speedtest) require Session API. Only `list`/`get` are
  Hybrid-safe.

## Clients `[H for list/find/get, L for roams/wifi + commands/reservations]`

```bash
unifly clients list [--all] [--type wireless|wired|guest]
unifly clients find <query>             # case-insensitive substring over IP, name, hostname, MAC
unifly clients get <mac|id>
unifly clients roams <client> [--limit N] # accepts name, hostname, IP, or MAC
unifly clients wifi <client>             # accepts name, hostname, IP, or MAC; aliases: wifi-experience, wifiman
unifly clients authorize <mac> [--minutes N] [--up-rate N] [--down-rate N]
unifly clients unauthorize <mac>
unifly clients block <mac>
unifly clients unblock <mac>
unifly clients kick <mac>                # force disconnect
unifly clients forget <mac>              # remove from controller memory
unifly clients reservations              # alias: res
unifly clients set-ip <mac> --ip <ipv4> [--network <name|id>]
unifly clients remove-ip <mac> [--network <name|id>]
```

**Gotchas:**

- `find` is the recommended search verb instead of `list | jq` pipelines.
  It matches substrings across IP, name, hostname, and MAC in a single
  pass, case-insensitive.
- `reservations` (alias `res`) lists **all** DHCP reservations including
  offline clients. It goes through Session API `/rest/user`.
- `roams` accepts any client identifier (name, hostname, IP, or MAC). It
  resolves to a MAC via the in-memory client snapshot before hitting
  `GET /v2/api/site/{site}/system-log/client-connection/{mac}`.
  Default limit is 50 events; override with `--limit`.
- `roams` output includes From/To AP names, SSID, signal strength, channel,
  and band for each event. Use it to diagnose sticky clients, ping-pong
  roaming, and bad handoffs.
- `set-ip` auto-detects the target network from the IP subnet unless
  `--network` is supplied explicitly.
- `remove-ip` defaults to removing from all networks. Scope it with
  `--network` if the MAC has reservations in multiple networks.
- `wifi` accepts any client identifier (name, hostname, IP, or MAC). It
  resolves to an IP via the in-memory snapshot before hitting
  `GET /v2/api/site/{site}/wifiman/{ip}/`. Shows wifi experience score,
  signal/noise/channel, band, link rates, nearest neighbor APs with signal
  strength, and the full uplink chain with per-hop experience scores.
- `wifi` only works for wireless clients. Wired clients return a 404.
- Band labels are normalized to human-readable: `2.4 GHz`, `5 GHz`, `6 GHz`.
- `list` wireless/bytes/hostname fields are only populated in Hybrid mode.

## Networks `[I for CRUD]`

```bash
unifly networks list
unifly networks get <id|name>
unifly networks create --name NAME --vlan N --management MODE \
  --ipv4-host <CIDR> [--dhcp --dhcp-start IP --dhcp-stop IP] \
  [--dns SERVER]... [-F payload.json]
unifly networks update <id> [flags...]
unifly networks delete <id>
unifly networks refs <id>                # reverse references
```

**Gotchas:**

- VLAN range is **1-4009** (enforced).
- `--management` accepts `gateway`, `switch`, or `vlan-only`.
- `--dns` is **repeatable** for multiple per-network DNS servers.
- `refs` is unique to networks: shows which WiFi SSIDs, firewall policies,
  and zones reference a given network. Use before deleting to understand
  blast radius.
- `--from-file` / `-F` accepts a full JSON payload (see examples/).

## WiFi `[I for CRUD, L for neighbors/channels]`

```bash
unifly wifi list
unifly wifi get <id|name>
unifly wifi neighbors [--within SECONDS] [--limit N] [--all] # alias: rogueap
unifly wifi channels
unifly wifi create --name SSID --security MODE --passphrase PASS --network ID \
  [--broadcast-type standard|iot-optimized] [--frequencies 2.4,5,6] [-F payload.json]
unifly wifi update <id> [flags...]
unifly wifi delete <id>
```

**Gotchas:**

- `--security` values: `open`, `wpa2-personal`, `wpa3-personal`,
  `wpa2-wpa3-personal`, `wpa2-enterprise`, `wpa3-enterprise`.
- `--broadcast-type iot-optimized` enables IoT optimizations (2.4 GHz-only
  limits, lower beacon power).
- `neighbors` uses `GET /api/s/{site}/stat/rogueap` and surfaces APs seen by
  your own radios. Signal is from your AP's perspective, not the neighbor's.
  Default display limit is 25; use `--all` or `--limit N` to see more.
- `neighbors --within` is in seconds, and the underlying `stat/rogueap`
  endpoint also uses seconds rather than the epoch milliseconds common in
  other UniFi stats routes.
- `channels` uses `GET /api/s/{site}/stat/current-channel` and shows
  regulatory channel availability per band (2.4 GHz, 5 GHz, 5 GHz DFS,
  6 GHz) for the controller's country. Useful for DFS planning and
  verifying which 6 GHz channels are available in your region.
- `--frequencies` is comma-separated: `2.4`, `5`, `6`. All three are valid
  on WiFi 6E and WiFi 7 APs.
- `--from-file` accepts full payloads for complex SSID configurations
  (enterprise RADIUS, MAC filters, VLAN tagging).

## Firewall `[I]`

### Policies

```bash
unifly firewall policies list
unifly firewall policies get <id>
unifly firewall policies create --name NAME --action allow|block|reject \
  --source-zone ZID --dest-zone ZID \
  [--src-ip IP,CIDR,RANGE] [--dst-ip ...] [--src-port N,N] [--dst-port ...] \
  [--src-network ID] [--dst-network ID] \
  [--states NEW,ESTABLISHED] [--ip-version IPV4_ONLY|IPV6_ONLY|BOTH] \
  [--description TEXT] [--logging] [-F payload.json]
unifly firewall policies update <id> [flags...]
unifly firewall policies patch <id> [--enabled true|false] [--logging true|false]
unifly firewall policies delete <id>
unifly firewall policies reorder --source-zone ZID --dest-zone ZID (--get | --set "id1,id2,id3") [--after-system]
```

**Gotchas:**

- `patch` is a fast partial-update for toggling `enabled`/`logging`. Use
  it instead of `update` when only changing those fields (cheaper, no
  round-trip fetch).
- `--src-ip`/`--dst-ip` accept a mix of IPs, CIDRs, and ranges
  (`10.0.0.1-10.0.0.100`), comma-separated.
- `reorder --get` prints the current order. `reorder --set` writes a new
  order. Round-trip pattern: get, edit, set.
- `reorder --after-system` places user policies after system-defined rules.
- `--logging` is a boolean; both bare form (`--logging`) and explicit
  (`--logging true`) work.
- `--description` exists on `create` and `update`.

### Zones

```bash
unifly firewall zones list
unifly firewall zones get <id>
unifly firewall zones create --name NAME [--networks ID,ID,...] [-F payload.json]
unifly firewall zones update <id> [flags...]
unifly firewall zones delete <id>
```

**Gotchas:**

- `--networks` accepts comma-separated network IDs or names.
- `--from-file` is now supported on zones (recent addition).

## NAT `[I]`

```bash
unifly nat policies list
unifly nat policies get <id>
unifly nat policies create --name NAME --nat-type masquerade|source|destination \
  [--src-address CIDR] [--dst-address CIDR] \
  [--src-port N] [--dst-port N] \
  [--translated-address IP] [--translated-port N] \
  [--protocol tcp|udp|all] [-F payload.json]
unifly nat policies update <id> [--name NAME | --description DESC] \
  [--type masquerade|source|destination] [--protocol tcp|udp|all] \
  [--enabled true|false] [--src-address CIDR] [--dst-address CIDR] \
  [--src-port N] [--dst-port N] \
  [--translated-address IP] [--translated-port N] [-F payload.json]
unifly nat policies delete <id>
```

**Gotchas:**

- **`nat policies update <id>`** fetches the existing rule and merges
  only the changed fields. Use `--name` or `--description` (mutually
  exclusive) for the display label, plus any combination of `--type`,
  `--protocol`, `--enabled`, address/port flags, or `--from-file`.
- `masquerade` is source NAT using the outgoing interface address (most
  common for Internet-bound traffic).
- `destination` is how port forwarding works on UniFi: specify
  `--dst-port` (the external port), `--translated-address` (internal IP),
  and `--translated-port` (internal port).
- `--from-file` accepts full payloads.

## ACL `[I]`

```bash
unifly acl list                    # alias: ls
unifly acl get <id>
unifly acl create [flags...] [-F payload.json]
unifly acl update <id> [flags...] [-F payload.json]
unifly acl delete <id>
unifly acl reorder [--get | --set "id1,id2,id3"]
```

Similar reorder semantics to firewall policies.

## DNS `[I]`

```bash
unifly dns list
unifly dns get <id>
unifly dns create --domain NAME --record-type A|AAAA|CNAME|MX|TXT|SRV|Forward \
  --value VALUE [--ttl SECS] [-F payload.json]
unifly dns update <id> [flags...]
unifly dns delete <id>
```

**Gotchas:**

- `--ttl` range is `0-86400` (enforced).
- `Forward` record type sets up DNS forwarding for a domain.

## Traffic Lists `[I]`

```bash
unifly traffic-lists list
unifly traffic-lists get <id>
unifly traffic-lists create --name NAME --list-type ports|ipv4|ipv6 --values "80,443" [-F payload.json]
unifly traffic-lists update <id> [flags...]
unifly traffic-lists delete <id>
```

**Gotchas:**

- `--list-type` is required. `ports`, `ipv4`, or `ipv6`.
- Referenced by firewall policies, NAT policies, and ACLs. Ideal for
  avoiding rule duplication.

## Hotspot `[I]`

```bash
unifly hotspot list
unifly hotspot get <id>
unifly hotspot create --name NAME --count N --minutes N [--quota MB] [--up-rate KBPS] [--down-rate KBPS]
unifly hotspot delete <id>
unifly hotspot purge --filter "EXPR"
```

**Gotchas:**

- `create --count N` generates N voucher codes in one call. Each code
  inherits the other flags (duration, quota, rate limits).
- `purge --filter` accepts the Integration filter DSL and is unifly's
  only bulk-delete-by-expression operation. Examples:
  `status.eq('UNUSED')`, `name.contains('Conference')`,
  `created_at.lt('2024-01-01')`. Use carefully; it deletes matching
  vouchers immediately.

## Events `[L]`

```bash
unifly events list [--within HOURS] [--all]
unifly events watch [--types CAT1,CAT2] [-o json]
```

**Gotchas:**

- `watch --types` filter values are **EventCategory** enum names,
  case-insensitive: `Device`, `Client`, `Network`, `System`, `Admin`,
  `Firewall`, `Vpn`, `Unknown`. Comma-separated. **`EVT_*` glob patterns
  do not work**, despite what older documentation may suggest.
- `watch` streams from WebSocket. It runs until Ctrl-C. Use `-o json` and
  pipe into `jq -c` for line-delimited JSON for downstream processing.
- `list --within HOURS` limits to the last N hours.

## Stats `[L]`

```bash
unifly stats site [--interval 5minute|hourly|daily|monthly] [--start ISO] [--end ISO]
unifly stats device <mac> [--interval ...] [--attrs attr1,attr2]
unifly stats client <mac> [--interval ...]
unifly stats gateway [--interval ...]
unifly stats dpi [--group-by by-app|by-cat] [--macs MAC1,MAC2]
```

**Gotchas:**

- `--start`/`--end` are ISO 8601 timestamps (`2024-01-01T00:00:00Z`).
- `--attrs` limits the metrics returned; smaller payloads, faster queries.
- `stats dpi` requires `--group-by`. `by-app` buckets by application,
  `by-cat` buckets by category.
- Session API only; all commands fail without credentials.

## DPI `[I for apps/categories, L for status/enable/disable]`

```bash
unifly dpi apps
unifly dpi categories
unifly dpi status
unifly dpi enable
unifly dpi disable
```

**Gotchas:**

- `apps` and `categories` are Integration API reference lookups.
- `status`, `enable`, `disable` are Session API lifecycle controls for the
  DPI subsystem itself. Use these to toggle DPI on/off without touching
  the web UI.

## Settings `[L]`

```bash
unifly settings list
unifly settings get <KEY>
unifly settings set <KEY> <FIELD> <VALUE>
unifly settings set <KEY> --data '{"field": "value"}'
unifly settings export
```

**Gotchas:**

- All subcommands use the Session API (`rest/setting` / `set/setting/{key}`).
- `list` shows a summary table of all ~44 setting sections with key, field
  count, enabled status, and notable values.
- `get` masks `x_`-prefixed fields (credentials, internal secrets) in table
  mode. Use `-o json` to see everything.
- `set` performs a read-modify-write: fetches the current section, patches
  the specified field, and PUTs the full section back. Values are parsed as
  bool (`true`/`false`), number, or string fallback.
- `set --data` merges a JSON object into the section. Mutually exclusive
  with the positional `<FIELD> <VALUE>` form.
- `export` always outputs full JSON regardless of `--output` flag.
- The PUT endpoint replaces the entire section; the handler strips `_id`,
  `site_id`, and `key` metadata before sending.

## System `[L]`

```bash
unifly system info
unifly system health
unifly system sysinfo
unifly system backup create
unifly system backup list
unifly system backup download <filename> [--path DIR]
unifly system backup delete <filename>
unifly system reboot
unifly system poweroff
```

**Gotchas:**

- `backup download --path DIR` writes to a specific directory instead of
  cwd.
- `backup delete` is scoped to a specific backup file.
- `reboot` and `poweroff` are destructive. Always summarize to the user
  before running even with `--yes`.

## Admin `[L]`

```bash
unifly admin list
unifly admin invite --email EMAIL --role ROLE
unifly admin revoke <admin_id>
unifly admin update <admin_id> [--role ROLE]
```

**Gotchas:**

- `revoke` and `update` take a **positional `<admin_id>`**, not
  `--email`. Pre-fetch the ID via `admin list -o json` before revoking.

## Sites `[L]`

```bash
unifly sites list
unifly sites create --name NAME --description TEXT
unifly sites delete <name>
```

Site `create --description` is **required**.

## VPN `[I for servers/tunnels, L for site-to-site/remote-access/clients/peers/settings, V2 for connections/magic-site-to-site]`

```bash
unifly vpn servers [--all] [-o json]
unifly vpn tunnels [--all] [-o json]
unifly vpn site-to-site list [--all] [-o json]
unifly vpn site-to-site get <id> [-o json]
unifly vpn site-to-site create -F payload.json
unifly vpn site-to-site update <id> -F payload.json
unifly vpn site-to-site delete <id>
unifly vpn remote-access list [--all] [-o json]
unifly vpn remote-access get <id> [-o json]
unifly vpn remote-access create -F payload.json
unifly vpn remote-access update <id> -F payload.json
unifly vpn remote-access suggest-port [-o json]
unifly vpn remote-access download-config <id> [--path PATH]
unifly vpn remote-access delete <id>
unifly vpn clients list [--all] [-o json]
unifly vpn clients get <id> [-o json]
unifly vpn clients create -F payload.json
unifly vpn clients update <id> -F payload.json
unifly vpn clients delete <id>
unifly vpn connections list [--all] [-o json]
unifly vpn connections get <id> [-o json]
unifly vpn connections restart <id>
unifly vpn peers list [server-id] [--all] [-o json]
unifly vpn peers get <server-id> <id> [-o json]
unifly vpn peers create <server-id> -F payload.json
unifly vpn peers update <server-id> <id> -F payload.json
unifly vpn peers delete <server-id> <id>
unifly vpn peers subnets [-o json]
unifly vpn magic-site-to-site list [--all] [-o json]
unifly vpn magic-site-to-site get <id> [-o json]
unifly vpn settings list [--all] [-o json]
unifly vpn settings get <teleport|magic-site-to-site-vpn|openvpn|peer-to-peer> [-o json]
unifly vpn settings set <key> --enabled true|false
unifly vpn settings patch <key> -F payload.json
```

**Gotchas:**

- `servers` and `tunnels` are Integration API inventory only.
- `site-to-site` uses Session API `rest/networkconf` records filtered to
  `purpose=site-vpn`.
- `remote-access` uses Session API `rest/networkconf` records filtered to
  `purpose=remote-user-vpn`.
- `remote-access suggest-port` uses the Session v2 API
  `network/port-suggest?service=openvpn` helper and returns
  `available_ports`.
- `remote-access download-config` fetches the Session v2 API
  `vpn/openvpn/<id>/configuration` export and writes `<id>.ovpn` by
  default.
- `clients` uses Session API `rest/networkconf` records filtered to
  `purpose=vpn-client`.
- `connections` uses the Session v2 API `vpn/connections` inventory and
  `vpn/<id>/restart` action.
- `peers` uses Session v2 API `wireguard/*/users` batch endpoints. `create`,
  `update`, and `delete` require the parent remote-access server ID.
- `subnets` lists already-consumed subnets from
  `v2/api/site/<site>/wireguard/users/existing-subnets`.
- `magic-site-to-site` uses the Session v2 API
  `magicsitetositevpn/configs` inventory endpoint and is read-only.
- `settings` uses Session API `rest/setting` records for VPN feature toggles.
- `patch` accepts either a raw session setting body or the wrapper emitted
  by `get` (`{ "key": ..., "enabled": ..., "fields": { ... } }`).
- Sensitive nested material such as private keys and PSKs is redacted from
  `get` output. Reconstruct those fields explicitly before updating if the
  controller requires them unchanged.

## Alarms `[L]`

```bash
unifly alarms list [--unarchived]
unifly alarms archive <id>
unifly alarms archive-all
```

## API (raw passthrough) `[any mode]`

```bash
unifly api <path> [-m get|post|put|patch|delete] [-d '<json-body>']
```

**Gotchas:**

- Routes through the Session client, so CSRF tokens and session caching
  are handled automatically.
- Paths are relative to the controller base URL. Examples:
  - Session v1: `api/s/default/stat/device`
  - Session v2: `v2/api/site/default/traffic-flow-latest-statistics`
  - Integration v1: `integration/v1/sites/default/clients`
  - Commands: `cmd/stamgr`, `cmd/devmgr`
- `-d '<json>'` is used for `post`, `put`, and `patch`.
- `delete` does not require a body.
- Essential when unifly does not wrap a specific endpoint yet.

## Topology, TUI, Completions, Config, Countries

- `unifly topology`: Pretty-print the gateway > switch > AP > client tree
  (Hybrid recommended for complete uplink data).
- `unifly tui [--theme NAME] [--log-file PATH]`: Launches the Ratatui
  dashboard. `UNIFLY_THEME` env var also sets the theme.
- `unifly completions bash|zsh|fish|powershell|elvish`: Emit completion
  script to stdout.
- `unifly config init | show | set | profiles | use | set-password`:
  Profile management. `set-password` stores in OS keyring.
- `unifly countries`: List country codes for WiFi regulatory settings.

## Cross-Cutting Patterns

### `--from-file` / `-F` (universal create/update)

Accepted by: `networks`, `wifi`, `firewall policies`, `firewall zones`,
`nat policies`, `acl`, `dns`, `traffic-lists`, `hotspot`. The flag
mutually excludes inline flags on the same field. Prefer `--from-file`
for anything beyond a handful of flags.

```bash
unifly networks create -F network.json
unifly firewall policies create -F policy.json
```

See `examples/` for payload templates.

### Integration Filter DSL

`--filter` on list commands and `hotspot purge --filter` accepts a small
expression language:

```
field.eq('value')
field.neq('value')
field.contains('substring')
field.startswith('prefix')
field.endswith('suffix')
field.gt(123), field.lt(123), field.gte, field.lte
field.in(['a', 'b', 'c'])
```

Combine with `&&` and `||`:

```bash
unifly devices list --filter "state.eq('ONLINE') && model.startswith('U6')"
```

Only Integration API commands respect `--filter`. Session commands filter
client-side via `jq` after fetching.

### Default List Limit Is 25

All `list` commands default to `--limit 25` and print a truncation hint
when results hit the ceiling. For enumeration use `--all` (auto-paginate)
or `--limit 200` (or higher) explicitly. **Agents running enumeration
queries should always pass one of these flags to avoid silent truncation.**

### Output Modes for Pipelines

- `-o json`: Structured output, the default for agent use
- `-o json-compact`: Single-line JSON per record, great for line-oriented
  processing
- `-o plain`: Emits IDs one per line, ideal for `xargs`:
  ```bash
  unifly clients list -o plain | xargs -n1 unifly clients block
  ```
- `-o table`: Human display only, not for parsing

### Dry-Run-Like Patterns

unifly does not have an explicit `--dry-run` flag. The idiomatic patterns
are:

1. **Read before write.** `get` the entity, show it to the user, then
   `update`.
2. **Use `reorder --get`** for firewall/ACL ordering changes before
   `reorder --set`.
3. **Use `networks refs <id>`** before deleting a network to see what
   depends on it.
4. **Hand off to the TUI** for visual verification on the `Firewall`,
   `Networks`, or `Devices` screens.
