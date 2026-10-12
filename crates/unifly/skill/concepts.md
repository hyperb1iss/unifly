# Authentication, identifiers, and data contracts

Load `unifly skill commands` for command discovery and `unifly skill workflows`
for examples. The guidance below explains behavior that command help alone
does not establish.

## Choose the transport

| Profile auth mode | Available paths                                                      |
| ----------------- | -------------------------------------------------------------------- |
| `integration`     | Integration API with an API key; Session HTTP also works on UniFi OS |
| `session`         | Session HTTP and WebSocket with username/password                    |
| `hybrid`          | Integration API plus authenticated Session HTTP and WebSocket        |
| `cloud`           | Site Manager fleet API and connector-routed Integration API          |

The Integration API handles networks, WiFi configuration, firewall policies
and zones, DNS, ACLs, traffic lists, vouchers, and VPN server/tunnel views.
Session HTTP handles NAT policies, firewall groups, switch port settings,
DHCP reservations, site settings, VPN configuration and observability,
historical stats, admin, backups, and device operations.

API-key mode on UniFi OS supports most HTTP operations without a password,
including client/device enrichment and WiFi observability. Self-hosted
controllers and other controller variants may need Session authentication
for those routes. Live `events watch` needs a Session cookie; use `session`
or `hybrid`. The TUI can poll without WebSocket access.

Cloud mode has no Session client. Session-only commands and the raw `api`
command cannot use the cloud connector. Fleet commands under `cloud` query
Site Manager directly. Select an Integration read for connector checks and
a fleet read for fleet checks; `system health` requires Session HTTP.

## Configuration and credentials

Configuration lives at `~/.config/unifly/config.toml` on Linux and macOS,
and `%APPDATA%\unifly\config\config.toml` on Windows. Use named profiles with
`-p NAME` or `UNIFI_PROFILE` for multiple controllers. Local setup uses
`unifly config init`; cloud setup uses `unifly config cloud-setup`.

Inspect saved configuration with `unifly config show -o table`. Table
output masks stored API keys and passwords; JSON/YAML output can expose
them. Always specify `-o table` here, even when it is normally the default,
because `UNIFI_OUTPUT` can select another format. The display does not
resolve all command-line or environment overrides. Account for those
overrides when checking the actual controller, site, or credentials.

Connection values generally resolve as flag/environment, profile,
`[defaults]`, then built-in fallback. The timeout fallback is 30 seconds.
TLS verification is enabled by default. A profile CA certificate overrides
an inherited `defaults.insecure = true`. The flag `--insecure=false` forces
verification over an insecure profile/default; bare `-k` disables it.

Connection environment variables use `UNIFI_` (for example `UNIFI_URL`,
`UNIFI_API_KEY`, `UNIFI_USERNAME`, `UNIFI_PASSWORD`, `UNIFI_SITE`,
`UNIFI_HOST_ID`, `UNIFI_TIMEOUT`, and `UNIFI_INSECURE`). Use `true` or `false`
for the insecure override. The theme variable is `UNIFLY_THEME`.

Store passwords in the OS keyring with `unifly config set-password PROFILE`,
or supply credentials through the configured environment sources. An
explicit profile `password` or `api_key` takes precedence over its keyring
entry. Do not print secrets while inspecting configuration or commands.
For MFA, the best source is a profile `totp_command`: an argv array (no
shell, 10 s limit) that prints the current code, for example
`totp_command = ["op", "item", "get", "UniFi", "--otp"]`. unifly runs it for
every login that needs a code, so long-running sessions such as the TUI
renew themselves when the cookie expires; a locked password manager may
prompt on each retry, about every 30 s while the session stays expired. A
one-off code via `UNIFI_TOTP`, `totp_env`, or the hidden `--totp` flag
serves only the first login and cannot renew a session. With 1Password's
`op read`, append `?attribute=otp` to the reference so it prints the code.
The flag `--no-cache` bypasses cached Session login cookies (useful after
password rotation).

## Identifiers and query scope

Integration records generally use UUIDs; Session records use legacy string
IDs. A network's Integration UUID is not interchangeable with its Session
ID. Devices and clients also have MAC addresses. Inspect the accepted
identifier in command help before composing a pipeline. In particular,
`clients block` expects a MAC, whereas plain list output emits entity IDs.

Client lookup with `clients find` searches name, hostname, IP, and MAC.
The observability commands `clients roams` and `clients wifi` resolve names
and other indirect identifiers against the current client snapshot.
Passing a full MAC to `clients roams` or an IPv4 address to `clients wifi`
bypasses snapshot lookup and can query clients absent from the snapshot.

List commands using common pagination default to 25 rows. Pass `--all`
when supported to enumerate the available collection. Some commands accept
only `--limit` (including events, alarms, and hotspot lists); consult their
help. A larger display limit cannot recover records missing from the
controller response or current snapshot.

Ordinary list filters run locally against serialized field names and
support one expression: `field.eq('value')`, `field.contains('value')`, or
`field.in('a','b')`. Equality and substring matching ignore case. Boolean
combinators and range comparisons are unsupported. Invalid expressions
can produce an empty result with exit status zero, so verify the filter
before concluding the collection is empty. The filter on `hotspot purge`
instead goes to the controller's Integration filter DSL.

## Output and secret handling

JSON list output is an array; JSON get output is usually an object.
Compact JSON serializes the whole result on one line. Event streaming is
different: each event is a separate JSON line. Quiet mode suppresses normal
output, so do not combine `-q` with commands whose JSON you need to capture.
Dynamic/raw endpoints may retain controller-specific schemas.

Device states and event severities serialize in PascalCase (for example
`Online`, `Offline`, `Warning`). Events expose `client_mac` and
`device_mac`. Site health is an array of subsystem records, not one object
containing CPU and memory fields.

Site settings hide nested `x_` credential fields by default. The opt-in
flag `--with-secrets` reveals them. Settings updates merge with existing
values, but replacing arrays can remove hidden credentials. Inspect the
relevant unredacted array privately before replacing it; use
`--allow-secret-removal` only when removing those credentials is intended.
The settings export command always emits JSON.

VPN get/create records redact keys and pre-shared secrets. A redacted
record is not a complete backup or a ready-to-replay secret-bearing
payload. Restore required secret fields from an authorized source before
an update. Downloaded VPN configurations can contain credentials; keep
them out of logs and source control.

## Raw API paths and controller quirks

The raw `api` command appends its path after the controller's platform
prefix. It does not add `api/s/<site>/` or replace a site in a literal
path. Use a Session site name for Session paths and the actual Integration
site UUID for Integration paths. Raw results keep controller envelopes;
typed commands may unwrap or transform them.

Session v2 observability endpoints generally return raw JSON rather than
the classic `{meta, data}` envelope. The endpoint `stat/rogueap` uses epoch
seconds. Client-connection logs duplicate the MAC in both URL path and
query parameter. The channel inventory is regulatory data, not a list of
each AP's current radio settings.

An authentication or permissions error does not prove a resource is
missing. Preserve stderr and distinguish missing transport, failed login,
forbidden operation, and TLS failure before changing credentials or
retrying. A write with an uncertain outcome needs a read-back before any
retry, especially for create commands.

Exit status is `0` on success. CLI errors use `1` for general failure, `2`
for usage/validation or missing confirmation, `3` for authentication, `4`
for a missing resource, `5` for unsupported/unimplemented operations, `6`
for conflict, `7` for connection/TLS failure, and `8` for timeout. Preserve
the diagnostic as well as the number: not every controller-side error maps
to a specialized exit code.
