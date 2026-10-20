# Operational command notes

Use `unifly <command> --help` for exact arguments and accepted values. The
command inventory below these notes comes from the installed CLI's parser.
Load `unifly skill concepts` for authentication and configuration, or
`unifly skill workflows` for recipes.

## Output, limits, and identifiers

Choose `-o json` for automation. List output is normally an array; get output
is normally an object. `unifly skill concepts` lists the JSON field names for
devices, clients, events, alarms, health, WANs, and stats. Compact JSON
(`-o json-compact`) serializes the same value on one line, including the
whole array for lists. Convert list output to one object per line with
`jq -c '.[]'` when needed. Event watching emits
one compact JSON event per line with either JSON format.

Config inspection is an exception to the JSON preference: use `config show
-o table`. JSON/YAML config output includes stored credentials; the explicit
format also overrides `UNIFI_OUTPUT`. Stats and raw API commands use pretty
JSON for table/plain output. Port export always emits JSONC.

Plain output (`-o plain`) usually emits record IDs. IDs may be Integration
UUIDs or Session IDs; a command accepting a MAC needs the `mac` field instead.
Get commands accept the record ID or its display name, compared exactly but
case-insensitively: networks, WiFi, firewall policies, zones, and groups,
NAT, ACL, traffic lists, DNS (by domain), and hotspot vouchers (by code).
Device commands and client get, roams, and WiFi experience also take an IP
or a MAC in any spelling (colons, dashes, dots, or none; any case); clients
answer to their hostname too. A MAC- or IP-shaped identifier only ever
matches MACs or IPs, never a name. A name two records share fails with exit
2 and lists the candidates; pass an ID then. Update, delete, and VPN commands
still take IDs. Client find searches IP, name, hostname, and MAC by
substring.

Most lists default to 25 entries. Pass `--all` for complete enumeration when
available. The shared list handler applies filtering, offset, and limit to
its available collection; `--all` removes the limit but preserves the offset.
A truncation hint appears on stderr only when more matching entries exist
than the limit, and `-q` silences it. Hotspot, events, and alarms lists
default to 100 and lack `--all`; client roams defaults to 50. Check each
command's help for pagination.

Lists built from the controller snapshot sort before filtering and paging,
so every run and every output format agrees and consecutive `--offset`
pages never overlap. Named entities sort case-insensitively by name
(clients by alias, else hostname, then IP and MAC), with unnamed entries
last. Firewall policies group by source then destination zone name and
follow the evaluation `index` within each pair. ACL rules follow `index`
and NAT rules `rule_index`. Events and hotspot vouchers list newest first.
Lists fetched live from the controller keep its order.

Local list filters accept one expression, with case-insensitive comparisons:

```text
name.eq('Office')
name.contains('guest')
state.in('Online','Offline')
```

Field paths follow the JSON shape and support nested fields. Boolean
combinations and comparison operators are unsupported in local filters.
A malformed expression fails with exit 2 before anything is fetched, but a
well-formed one naming a missing field matches nothing and exits 0. Hotspot
purge passes its filter to the controller and uses the server's filter
language instead.

Create commands generally print the created record on stdout and a
confirmation on stderr. Sites create and system backup create have no
created-record output. A create may also succeed without a renderable record
and warn on stderr; inspect existing state before retrying a successful
create. Quiet mode suppresses normal output and is unsuitable for capturing
created IDs.

## Files and mutations

Load `unifly skill examples` for supported payload templates. File payloads
use JSON or JSONC (comments and trailing commas are accepted). Do not mix
`--from-file` with inline payload flags: conflict checks vary by command,
and accepted inline flags can be ignored when a file is supplied.

Typed payloads often ignore unknown keys. Switch port payloads reject them;
VPN payloads with extra-field maps preserve controller-specific keys. Check
field names against the relevant example before writing. Traffic-list and DNS
updates require `--from-file`; they have no inline update fields.

The CLI has no general dry-run mode. Read current state before changing it,
then re-fetch to verify. Confirmation prompts exist for selected operations,
not every mutation. Use `--yes` for an already-authorized operation when its
prompt would block automation. Raw API mutations have no confirmation guard.

## Devices and switch ports

Device operations use different transports:

- Adoption, restart, and locate use Integration when its client and site ID
  are available, otherwise Session. Integration errors are not retried via Session.
- Removal and PoE port cycling require Integration.
- Firmware upgrade, provision, and speedtest require Session HTTP.
- Port inspection, export, and configuration require Session HTTP.

A UniFi OS API key can access Session HTTP without a username/password.
Cloud connector auth supports Integration operations, not Session operations.
Locate takes an explicit boolean (`--on true` or `--on false`). Firmware
upgrade accepts an optional custom firmware URL.

Port indices are 1-based. PoE's negotiating mode is `auto`, not `on`.
Port VLAN flags accept network names or Session IDs; duplicate names fail
resolution. Trunk mode without tagged VLANs allows all VLANs. Supplying a
list chooses custom tagging. Use speed `auto` for autonegotiation.

Port export always emits JSONC for `port-set --from-file`, regardless of
the global output format. Export is sparse by default; `--all` includes
ports without an override. A file applies only its listed ports and
preserves other overrides. Each entry needs an `index`.
Use `reset: true` to remove an override and restore controller defaults.
A single-port `--reset` does the same and conflicts with other setting flags.
File application and reset prompt unless `--yes` is supplied.

Port payload fields include `name`, `mode`, `native_network_id` (alias
`native_vlan`), `tagged_network_ids` (alias `tagged_vlans`), `tagged_all`,
`poe`, `speed`, and `reset`. An empty tagged array clears the list; an omitted
field preserves it. The single-port positional index conflicts with a file.

Port inspection and export support `--with-clients` for connected clients
and adopted devices. JSON entries carry a `kind` discriminator. Exported
`// last-seen ` comments record the observation time, MAC, name, and kind;
re-exporting supports configuration and connection comparisons.

## Clients and wireless diagnostics

Client block, unblock, and kick use Integration when its client and site ID
are available, otherwise Session. Integration errors are not retried via Session.
Forget, guest authorization, reservations, roams, and WiFi experience require
Session HTTP. Reservations include offline clients. Setting a reserved IP
can infer its network from the subnet; an explicit network overrides that
choice. Removing a reservation defaults to all networks unless scoped.

Client roams and WiFi experience accept a name, hostname, ID, IP, or MAC and
resolve against the current client snapshot. A MAC for roams or an IPv4
address for WiFi experience that the snapshot lacks goes to the controller
as is; other identifiers need a matching client in the snapshot.
WiFi experience targets wireless clients and reports signal, rates, nearby
APs, and uplink information when the controller provides them.

WiFi neighbors uses Session observations from your APs. The time window is
in seconds, not milliseconds. Results default to 25; use `--all` or an
explicit limit. WiFi channels reports country-level regulatory availability,
not the current channel of each AP; its JSON and YAML rows carry `country` and
`country_code`. Device radios describe individual APs. Topology draws a tree in
table and plain output; its JSON and YAML hold `gateway`, `devices` (each with
its uplinked `clients`), and `unlinked_clients`.

## Networks, WiFi, firewall, NAT, ACL, and DNS

Network, WiFi configuration, firewall policy/zone, ACL, DNS, and traffic-list
commands require Integration. WiFi neighbors/channels are Session exceptions.
Network VLAN values are 1 through 4009. Network DNS flags are repeatable.
Use network refs before deleting to inspect dependencies. Network and WiFi
delete expose `--force`; inspect its help before bypassing reference checks.

WiFi security and broadcast type are separate choices. Frequencies are
comma-separated GHz values (`2.4,5,6`). Use file payloads for enterprise
RADIUS settings and other fields absent from inline flags. RADIUS profiles
and WAN inventory are available through their Integration reference commands.

Firewall policy patch changes enabled/logging without the full update path.
Reorder reads two lists for a source/destination zone pair:
`beforeSystemDefined` and `afterSystemDefined`. A set replaces only the
before-system list unless `--after-system` selects the other list. Naming an
ID from the opposite list moves it across the system-rule boundary. Never
combine both lists into one set; preserve both for rollback. ACL reorder
also supports get/set, without
firewall zone-pair arguments.

Firewall zone network lists require Integration network UUIDs, not names.

Firewall groups use Session HTTP. Group records have Session IDs and
`external_id` UUIDs; policies reference the latter. Policy source/destination
port-group and address-group flags resolve group names to those UUIDs.
File payloads also support shorthand filter fields. Name resolution for
Session groups needs Session access even when the policy uses Integration.

NAT uses Session v2 HTTP, including in Session-only auth. Update fetches the
existing rule and merges changed fields. The mutually exclusive name and
description flags both set the display description. Masquerade uses the
outgoing interface address; destination NAT forwards to a translated address
and port. Read back the resulting rule to verify interface and match scope.

DNS record types on the CLI are lowercase (`a`, `aaaa`, `cname`, `mx`, `txt`,
`srv`, `forward`), even though API tokens differ. TTL accepts 0 through 86400.
Traffic-list create accepts ports, IPv4, or IPv6 lists. Supply required
create fields inline or through a file.

Hotspot create generates one or more vouchers with shared duration, guest,
data, and rate limits. Hotspot has no file payload flag. Purge deletes
vouchers matching a server-side filter after confirmation; inspect the
intended scope before authorizing it.

## Events, statistics, and system operations

Event watching requires password-based Session or Hybrid auth for its
WebSocket. A UniFi OS API key suffices for event listing, not the WebSocket.
Watch filters accept category names case-insensitively: Device, Client,
Network, System, Admin, Firewall, Vpn, Unknown. Event-name globs do not work.

Statistics require Session HTTP. Device/client queries take comma-separated
`--macs`, not positional MACs. Intervals are `5m`, `hourly`, `daily`, or
`monthly`. Site/gateway start/end accept RFC 3339 or Unix-second timestamps;
device/client stats currently ignore those flags despite accepting them.
DPI statistics group by app or category. DPI apps/categories are Integration reference data, while DPI
status/enable/disable use Session HTTP.

System info uses Integration when available and falls back to Session.
System health requires Session, so it is not a cloud connectivity probe.
Backups, reboot, and poweroff also use Session. Backup download accepts a
file path or an existing directory (create the directory first). Admin
revoke/update take a positional admin ID; fetch it with
admin list. Site creation requires both name and description.

## VPN and settings

VPN servers/tunnels are Integration inventories with get commands for detail.
VPN status and health provide Session-backed monitoring. Manual site-to-site,
remote-access server, and client profile CRUD use Session network records.
Their payloads are different from Integration inventory records.

VPN connection inventory and restart use Session v2. WireGuard peer writes
and get require a parent remote-access server ID; peer list can omit it to
list peers across servers. Peer subnets reports consumed subnets. Magic
site-to-site inventory
is read-only. Remote-access helpers suggest an OpenVPN port and download
configuration files; treat downloaded VPN files as sensitive.

VPN get/create output redacts sensitive fields and places controller details
under `fields`. A redacted record is not a complete update payload. VPN
settings patch accepts raw settings or the get wrapper (the inner `fields`
object is sent). Supply required secret material explicitly when updating.

Site settings use Session HTTP. Settings set reads the section, merges the
patch, and writes the section back. Positional values parse as bool, number,
null, or string; `--data` accepts a JSON object instead. Metadata is removed
before
writing. Exports always emit JSON, regardless of the global output format.

Settings hide `x_` credential fields by default. Use `--with-secrets` only
when needed. Omitted object credentials are preserved during updates, but
arrays containing stored credentials need special care: a stripped array
update is refused. Use a secret-inclusive export or explicitly authorize
removal with `--allow-secret-removal`. An array containing credentials is
taken as written, including changes to order and membership.

## Raw API and cloud

Raw API requires Session HTTP and is unavailable through cloud connector auth.
Paths are appended to the controller's platform prefix (`/proxy/network` on
UniFi OS), without automatic site substitution. Include the complete route:

```text
api/s/<site>/stat/device
api/s/<site>/cmd/stamgr
v2/api/site/<site>/traffic-flow-latest-statistics
integration/v1/sites/<site-uuid>/clients
```

Raw responses retain their API shape, including Session envelopes. Body data
is JSON for POST/PUT/PATCH. Delete does not require a body. Transport handles
CSRF; do not construct or cache CSRF tokens yourself.

Cloud fleet commands use Site Manager directly and need its API key, without
a host ID. Cloud switch selects a connector site and updates the active cloud
profile; it is a configuration write. Regular Integration commands in cloud
mode need a console host ID (automatically resolved when unambiguous).
Cloud device host filters can be repeated. ISP query reports partial-success
warnings when Site Manager returns incomplete results.

## Demo fleet

`--demo-fleet` (or `UNIFI_DEMO_FLEET=true`) answers from a synthetic,
offline network with no controller, credentials, or config file. Only these
read-only commands work: `system health|info`, `alarms list`, `wans list`,
`devices list|get|stats|ports|ports-export`,
`clients list|get|find|roams|wifi`, `networks list|get`,
`wifi list|get|neighbors|channels`, `firewall policies list|get`,
`firewall zones list|get`, `nat policies list|get`, `acl list|get`,
`events list`, `stats site|gateway|dpi`, `topology`, and `sites list`.
Each honors every output format except `devices ports-export`, which always
writes JSONC, as it does against a real controller. Any other command,
`devices port-set` included, exits 5 with "not available in demo mode"
before reading config or touching the network. The fleet has
problems worth diagnosing: an offline AP, an AP with firmware waiting, a
dark camera port on the core switch, and an unplugged second WAN. Alarms,
events, WANs, devices, and switch ports agree on them; health shows the
offline AP as a `wlan` warning. Demo stats rows carry the default
attributes and ignore `--attrs`.
`--demo-seed N` selects a different fleet; the same seed reproduces the
same names, MACs, IPs, and IDs. Timestamps follow the launch time, except
the `// last-seen` markers of `devices ports-export --with-clients`, which
carry a fixed 2026-03-18T15:30Z so a seed exports the same bytes on every
run. The data is fictional, so never present it as the user's network.
`unifly tui --demo-fleet` runs the dashboard against the same fleet, while
plain `--demo` sanitizes a real controller's data in the TUI. Controller and
cloud commands cannot sanitize their output, so in demo mode (`--demo`,
`UNIFI_DEMO`, or `[demo] enabled = true` in config) without `--demo-fleet`
they exit 5 before connecting or reading credentials. `config` commands still
run; turn the file switch off by setting `enabled = false` under `[demo]`.
