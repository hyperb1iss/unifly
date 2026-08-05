# Automation Workflows

Runnable automation recipes using unifly. Each recipe is copy-paste-ready.
For command-level flag details, consult `commands.md`. For auth and
architecture, consult `concepts.md`.

## Pre-Flight Checks

Before any automation, verify connectivity and the resolved auth mode:

```bash
# Tool present and version
unifly --version

# Resolved profile and auth_mode
unifly config show

# Controller reachable, site healthy
unifly system health -o json -q
```

Always pass `-o json` for machine-parseable output and `-y`/`--yes` to
skip confirmation prompts in non-interactive scripts.

## Payload-Driven Provisioning with `--from-file`

Most entities accept `--from-file` (alias `-F`) for a complete JSON payload
instead of flag salad. This is the preferred pattern for agents managing
complex configurations because payloads can be version-controlled,
templated, and validated before apply.

```bash
# Validate the payload with jq first
jq empty examples/network-iot-vlan.json && echo "Valid JSON"

# Apply
unifly networks create -F examples/network-iot-vlan.json -o json

# For updates, pass the ID positionally
unifly networks update <network-id> -F examples/network-iot-vlan.json
```

## Network Provisioning

### Create a Complete VLAN Segment

Network, firewall zone, WiFi SSID, and an isolation policy in one script:

```bash
#!/usr/bin/env bash
set -euo pipefail

# 1. Create the network. Create commands print the created entity on
#    stdout in the requested format, so the ID captures directly.
NETWORK_ID=$(unifly networks create \
  --name "IoT" \
  --vlan 30 \
  --management gateway \
  --ipv4-host 10.0.30.1/24 \
  --dhcp --dhcp-start 10.0.30.100 --dhcp-stop 10.0.30.254 \
  --dns 1.1.1.1 --dns 1.0.0.1 \
  -o json | jq -r '.id')

# 2. Create a firewall zone that owns it
ZONE_ID=$(unifly firewall zones create \
  --name "IoT Zone" --networks "$NETWORK_ID" -o json | jq -r '.id')

# 3. Grab the Internal zone ID (default LAN zone)
INTERNAL_ZONE_ID=$(unifly firewall zones list -o json | \
  jq -r '.[] | select(.name == "Internal") | .id')

# 4. Block IoT from reaching Internal
unifly firewall policies create \
  --name "Block IoT to Internal" \
  --action block \
  --source-zone "$ZONE_ID" \
  --dest-zone "$INTERNAL_ZONE_ID" \
  --description "Contain IoT devices to their own zone" \
  --logging

# 5. Create an IoT-optimized SSID on the network
unifly wifi create \
  --name "IoT-WiFi" \
  --broadcast-type iot-optimized \
  --security wpa2-personal \
  --passphrase "IoTSecure2024!" \
  --network "$NETWORK_ID"

echo "IoT VLAN 30 provisioned: network=$NETWORK_ID zone=$ZONE_ID"
```

### Bulk DNS Records from CSV

```bash
# CSV format: domain,type,value,ttl
# --record-type values are lowercase (a, aaaa, cname, ...), so normalize.
while IFS=',' read -r domain type value ttl; do
  [ "$domain" = "domain" ] && continue  # skip header
  unifly dns create \
    --domain "$domain" \
    --record-type "$(echo "$type" | tr '[:upper:]' '[:lower:]')" \
    --value "$value" \
    --ttl "${ttl:-3600}"
done < dns_records.csv
```

## Bulk DHCP Reservations from JSON

One of the most-requested UniFi automation patterns. Drive reservations
from a source of truth (Ansible inventory, Terraform state, spreadsheet
export) instead of clicking through the web UI.

```bash
#!/usr/bin/env bash
# reservations.json format:
# [
#   {"mac": "aa:bb:cc:dd:ee:01", "ip": "10.0.30.10", "network": "IoT", "name": "Printer"},
#   {"mac": "aa:bb:cc:dd:ee:02", "ip": "10.0.30.11", "network": "IoT", "name": "NAS"}
# ]
set -euo pipefail

jq -c '.[]' reservations.json | while read -r entry; do
  mac=$(echo "$entry" | jq -r '.mac')
  ip=$(echo "$entry" | jq -r '.ip')
  network=$(echo "$entry" | jq -r '.network')
  name=$(echo "$entry" | jq -r '.name // empty')

  # Set the IP reservation (creates the client record if needed)
  unifly clients set-ip "$mac" --ip "$ip" --network "$network" || {
    echo "Failed to reserve $ip for $mac" >&2
    continue
  }

  echo "Reserved $ip -> $mac ($name)"
done
```

## Port Forwarding via NAT Policies

Port forwarding is `nat policies create --type destination`. No
dedicated `port-forward` command exists.

```bash
# Forward external TCP 443 -> internal 10.0.10.50:8443
unifly nat policies create \
  --name "HTTPS to webserver" \
  --type destination \
  --protocol tcp \
  --dst-port 443 \
  --translated-address 10.0.10.50 \
  --translated-port 8443
```

To modify a NAT policy, use `nat policies update <ID>` with any
combination of flags. Only the specified fields are changed.

## Real-Time Event Streaming

unifly's WebSocket event stream is one of its most differentiating
capabilities. Agents can watch events live and react to network changes.

### Stream All Events

```bash
unifly events watch
```

### Filter by Category

The `--types` flag matches `EventCategory` enum values case-insensitively.
Valid values: `Device`, `Client`, `Network`, `System`, `Admin`,
`Firewall`, `Vpn`, `Unknown`.

```bash
# Watch firewall and admin events only
unifly events watch --types "Firewall,Admin"

# Device events for fleet health monitoring
unifly events watch --types Device
```

### Pipe Events Into Alerting

```bash
# Forward warning-level client events to a webhook as line-delimited JSON
# (severity serializes PascalCase: Info, Warning, Error, Critical)
unifly events watch --types Client -o json | \
  jq -c 'select(.severity == "Warning" or .severity == "Error")' | \
  while read -r event; do
    curl -fsSL -X POST "$SLACK_WEBHOOK" \
      -H "Content-Type: application/json" \
      -d "{\"text\": $(echo "$event" | jq '.message')}"
  done
```

### Incident Triage Loop

```bash
# Watch for firewall events and correlate the client MAC when present
unifly events watch --types Firewall -o json | while read -r event; do
  mac=$(echo "$event" | jq -r '.client_mac // empty')
  [ -z "$mac" ] && continue
  echo "Firewall event for $mac"
  unifly clients find "$mac" -o json | jq '.[] | {name, hostname, mac, ip}'
done
```

## Safe Firewall Policy Reorder

Firewall policy order matters: first match wins. The `reorder --get` /
`reorder --set` pattern is round-trippable and safe.

```bash
#!/usr/bin/env bash
set -euo pipefail

SRC_ZONE="iot-zone-id"
DST_ZONE="internal-zone-id"

# 1. Snapshot current order
BEFORE=$(unifly firewall policies reorder \
  --source-zone "$SRC_ZONE" --dest-zone "$DST_ZONE" --get -o json)

echo "Current order:"
echo "$BEFORE" | jq .

# 2. Compute new order (move the last policy to the top)
NEW_ORDER=$(echo "$BEFORE" | jq -r '[.[-1]] + .[:-1] | join(",")')

# 3. Apply the new order
unifly firewall policies reorder \
  --source-zone "$SRC_ZONE" --dest-zone "$DST_ZONE" \
  --set "$NEW_ORDER"

echo "Reordered."
```

## DNS Ad-Blocking via Policies and Traffic Lists

Unique to unifly's coverage: use a traffic matching list of bad domains
and a DNS policy to redirect them. Most competing tools cannot touch the
modern Policy Table.

```bash
# 1. Create a traffic list of sinkhole targets (or use ports)
unifly traffic-lists create \
  --name "AdDomains" \
  --list-type ipv4 \
  --items "0.0.0.0,127.0.0.1"

# 2. Create DNS records that sinkhole each bad domain
# (driven from a curated blocklist file)
while read -r domain; do
  [ -z "$domain" ] || [[ "$domain" =~ ^# ]] && continue
  unifly dns create \
    --domain "$domain" \
    --record-type a \
    --value "0.0.0.0" \
    --ttl 300
done < blocklist.txt
```

## Cafe / Airbnb Voucher Flow

Generate vouchers, export as a printable list with QR codes.

```bash
#!/usr/bin/env bash
set -euo pipefail

BATCH="Cafe-$(date +%Y-%m-%d)"
# hotspot create prints the generated vouchers (codes included) on stdout
VOUCHERS=$(unifly hotspot create \
  --name "$BATCH" \
  --count 20 \
  --minutes 1440 \
  --rx-limit-kbps 20000 --tx-limit-kbps 5000 \
  -o json)

# Extract codes
echo "$VOUCHERS" | jq -r '.[].code' > vouchers.txt

# Build a printable markdown table
{
  echo "| Code | Duration | Bandwidth |"
  echo "| ---- | -------- | --------- |"
  echo "$VOUCHERS" | jq -r '.[] | "| \(.code) | 24h | 20 Mbps |"'
} > vouchers.md

# Clean up unused vouchers after 7 days
unifly hotspot purge --filter "status.eq('UNUSED') && created_at.lt('$(date -d '7 days ago' -Iseconds)')"
```

## Device Fleet Operations

### Fleet Firmware Upgrade (Staggered)

```bash
# Upgrade all online switches with 30s between operations
# (device_type is "Switch"/"AccessPoint"/"Gateway"; state is PascalCase)
unifly devices list --all -o json | \
  jq -r '.[] | select(.device_type == "Switch" and .state == "Online") | .mac' | \
while read -r mac; do
  echo "Upgrading $mac..."
  unifly devices upgrade "$mac" --yes
  sleep 30
done
```

### Adopt All Pending Devices

```bash
unifly devices pending -o json | jq -r '.[].mac' | \
  xargs -n1 unifly devices adopt
```

### PoE Port Reset for a Stuck Device

```bash
# Power-cycle port 5 on a specific switch
unifly devices port-cycle "switch-mac" 5
```

### Locate a Device in a Rack

```bash
# Blink the LED
unifly devices locate "aa:bb:cc:dd:ee:ff" --on true

# Stop blinking
unifly devices locate "aa:bb:cc:dd:ee:ff" --on false
```

## Client Management

### Find Clients Quickly

```bash
# Substring match against IP, name, hostname, MAC
unifly clients find "macbook"
unifly clients find "10.4.22"
unifly clients find "dc:a6:32"        # vendor MAC prefix (Raspberry Pi)
```

### Block Unknown Clients by MAC Prefix Allowlist

```bash
ALLOWED_PREFIX="aa:bb:cc"

unifly clients list --all -o json | \
  jq -r --arg prefix "$ALLOWED_PREFIX" \
    '.[] | select(.mac | startswith($prefix) | not) | .mac' | \
  xargs -n1 unifly clients block
```

### Isolate a Compromised Client

```bash
MAC="aa:bb:cc:dd:ee:ff"

# 1. Block the client immediately
unifly clients block "$MAC"

# 2. Kick from WiFi (if wireless)
unifly clients kick "$MAC"

# 3. Gather forensic context from recent events
unifly events list --within 1 -o json | \
  jq --arg mac "$MAC" '.[] | select(.client // "" | contains($mac))'
```

## Monitoring and Alerting

### Health Check Script

```bash
#!/usr/bin/env bash
set -euo pipefail

# system health returns an ARRAY of per-subsystem summaries
UNHEALTHY=$(unifly system health -o json | \
  jq '[.[] | select(.status != "ok")]')

if [ "$(echo "$UNHEALTHY" | jq 'length')" -gt 0 ]; then
  echo "ALERT: unhealthy subsystems:"
  echo "$UNHEALTHY" | jq .
  # Hand off to alerting (PagerDuty, Slack, etc.)
fi

# Check for offline devices (state serializes PascalCase)
OFFLINE=$(unifly devices list --all -o json | \
  jq '[.[] | select(.state == "Offline")] | length')

if [ "$OFFLINE" -gt 0 ]; then
  echo "ALERT: $OFFLINE devices offline"
  unifly devices list --all -o json | \
    jq '.[] | select(.state == "Offline") | {name, mac, last_seen}'
fi
```

### Top Bandwidth Consumers

```bash
# Top 10 clients by total traffic (last 24h)
unifly stats client --interval daily -o json | \
  jq 'sort_by(-(.rx_bytes + .tx_bytes)) | .[0:10] | .[] | {mac, rx_bytes, tx_bytes}'
```

### DPI Traffic Breakdown

```bash
# Top 10 applications by traffic
unifly stats dpi --group-by by-app -o json | \
  jq 'sort_by(-.bytes) | .[0:10]'

# Enable DPI first if needed
unifly dpi status
# If disabled: unifly dpi enable
```

## Backup and Recovery

```bash
#!/usr/bin/env bash
set -euo pipefail

# Create a backup
unifly system backup create --yes

# Wait and download the latest
sleep 30
LATEST=$(unifly system backup list -o json | jq -r '.[0].filename')
unifly system backup download "$LATEST" --path ./backups/

echo "Backup saved: ./backups/$LATEST"

# Rotate: keep the 5 most recent
unifly system backup list -o json | \
  jq -r '.[5:] | .[].filename' | \
  xargs -n1 -I{} unifly system backup delete "{}" --yes
```

## Security Audit

### Firewall Policy Audit

```bash
# All allow policies (potential exposure); enums serialize PascalCase
unifly firewall policies list --all -o json | \
  jq '.[] | select(.action == "Allow") | {name, source, destination}'

# Policies without logging (blind spots)
unifly firewall policies list --all -o json | \
  jq '.[] | select(.logging_enabled == false) | {id, name}'

# Detect open WiFi SSIDs
unifly wifi list -o json | \
  jq '.[] | select(.security == "Open") | {name, id}'
```

### Unused Network Detection

```bash
unifly networks list --all -o json | jq -c '.[]' | while read -r net; do
  NET_ID=$(echo "$net" | jq -r '.id')
  NAME=$(echo "$net" | jq -r '.name')
  # refs returns an object of reference categories; count all values
  REFS=$(unifly networks refs "$NET_ID" -o json 2>/dev/null)
  if [ "$(echo "$REFS" | jq '[.. | arrays | length] | add // 0')" -eq 0 ]; then
    echo "Unused network: $NAME ($NET_ID)"
  fi
done
```

## Multi-Controller Operations

Manage multiple controllers via named profiles. Credentials live in the
OS keyring per profile.

```bash
# Cross-controller health snapshot
for profile in home office warehouse; do
  echo "=== $profile ==="
  unifly -p "$profile" system health -o json | jq '{status, cpu, mem}'
  unifly -p "$profile" devices list --all -o json | jq '[.[] | {name, state}]'
done

# Or via env var
for profile in home office warehouse; do
  UNIFI_PROFILE="$profile" unifly system info
done
```

## TUI Handoff Pattern

Unique to unifly: propose a change, let a human visually confirm in the
TUI before committing.

```bash
#!/usr/bin/env bash
# Agent proposes a firewall rule change
echo "Proposed change:"
cat << 'EOF'
  - Policy: Block IoT to Management
  - Source zone: IoT
  - Dest zone: Internal
  - Action: block
EOF

echo "Open the TUI and review screen 5 (Firewall)."
echo "Press Enter when ready to apply, Ctrl-C to abort."
read -r

unifly firewall policies create \
  --name "Block IoT to Management" \
  --action block \
  --source-zone "$IOT_ZONE" \
  --dest-zone "$INTERNAL_ZONE" \
  --logging
```

## Raw API Escape Hatch

When unifly does not wrap an endpoint yet, fall back to `unifly api`. It
routes through the Session client with CSRF handling.

```bash
# Query raw traffic flow statistics (Session v2 endpoint)
unifly api "v2/api/site/default/traffic-flow-latest-statistics" -o json | \
  jq '.[] | {mac, rx, tx}'

# Force-reconnect a client via Session command endpoint
unifly api "cmd/stamgr" -m post -d '{"cmd":"kick-sta","mac":"aa:bb:cc:dd:ee:ff"}'

# Raw Integration API call
unifly api "integration/v1/sites/default/clients" -o json
```

## Error Handling Patterns

### Retry with Exponential Backoff

```bash
retry_unifly() {
  local max_attempts=3
  local delay=5
  local attempt=1

  while [ $attempt -le $max_attempts ]; do
    if output=$("$@" 2>&1); then
      echo "$output"
      return 0
    fi
    echo "Attempt $attempt failed, retrying in ${delay}s..." >&2
    sleep $delay
    delay=$((delay * 2))
    attempt=$((attempt + 1))
  done

  echo "Failed after $max_attempts attempts" >&2
  return 1
}

retry_unifly unifly devices list -o json
```

### Read Before Write

```bash
# Confirm entity exists before mutating
NETWORK=$(unifly networks get "$NETWORK_ID" -o json 2>/dev/null) || {
  echo "Network $NETWORK_ID not found" >&2
  exit 1
}

# Safe to proceed
unifly networks update "$NETWORK_ID" --name "Updated Name"
```

### Fresh Login After Password Rotation

```bash
# Force a fresh Session login, bypassing the session cache
unifly --no-cache devices list
```

## Agent Best Practices

1. **Inspect before mutating.** Always `get` or `list` an entity before
   `create`, `update`, or `delete`.
2. **Capture IDs from create output.** Create commands print the created
   entity on stdout: `unifly ... create -o json | jq -r '.id'`, or
   `-o plain` for the bare ID. (Exception: `sites create` returns no
   record from the controller.)
3. **Verify after changes.** Re-fetch to confirm state.
4. **Stagger bulk device operations.** Add `sleep` between restarts,
   upgrades, and port cycles to avoid overwhelming the controller.
5. **Always pass `-o json` in automation.** Never parse tables.
6. **Always pass `--all`** on list commands for enumeration to defeat the
   25-row default limit.
7. **Use profiles** (`-p home`) to target different controllers without
   reconfiguring.
8. **Check `unifly config show`** to confirm the resolved auth mode
   before running commands that depend on Session or Integration
   specifically.
9. **For destructive operations** (delete, reboot, poweroff, purge),
   summarize the planned change to the user before running even with
   `--yes`.
10. **Use `--from-file`** for complex creates. Payload files are easier
    to review, version, and replay than long flag lists.
