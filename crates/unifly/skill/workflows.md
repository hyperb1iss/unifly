# Focused operational workflows

Use the user's requested controller and site throughout a task. The
examples below use a profile named `home`; replace it with the intended
profile. Commands containing example identifiers or payload filenames
need values obtained from inspection. Run mutations only within the
user's authorized scope.

## Inspect the target

```bash
unifly config profiles
unifly config show -o table
unifly -p home devices list --all -o json
unifly -p home clients list --all -o json
```

The explicit table format masks stored credentials and overrides any
`UNIFI_OUTPUT` setting. JSON/YAML config output can expose stored secrets.
The config display shows saved configuration; account for flag and
environment overrides separately. With Session HTTP access, inspect
subsystem health:

```bash
unifly -p home system health -o json | jq '.[] | {subsystem, status}'
```

For a cloud profile, use `unifly -p cloud cloud hosts -o json` for fleet
access or an Integration-backed list for the selected connector. Cloud
mode cannot run Session health or raw API commands.

## Apply a prepared payload and capture its ID

Retrieve a bundled template by exact filename, then edit it for the
intended site before applying:

```bash
unifly skill examples
unifly skill examples network-iot-vlan.json > network.json
```

Validate the edited JSON, inspect existing networks to avoid duplicates,
and apply the approved payload:

```bash
set -euo pipefail
jq empty network.json
unifly -p home networks list --all -o json
NETWORK_ID=$(unifly -p home networks create -F network.json -o json |
  jq -er '.id | select(type == "string" and length > 0)')
unifly -p home networks get "$NETWORK_ID" -o json
```

Create commands normally print their result on stdout and confirmation on
stderr. Site creation and backup creation return no entity. Voucher
creation returns an array. If ID capture fails after the controller
accepted a create, inspect the collection before trying again; a second
create may duplicate the first.

## Export and edit switch ports

```bash
unifly -p home devices ports SWITCH --with-clients -o json
unifly -p home devices ports-export SWITCH --with-clients > ports.jsonc
```

Replace `SWITCH` with the inspected device identifier. Review and edit the
export, then apply it and read back the affected ports:

```bash
unifly -p home devices port-set SWITCH -F ports.jsonc --yes
unifly -p home devices ports SWITCH --with-clients -o json
```

Port indices are 1-based. Unlisted ports keep their overrides; a per-port
`"reset": true` removes that port's override. The default export includes
ports with overrides; add `--all` to include other ports. PoE uses `auto`
for on/negotiate, not `on`. A trunk with no explicit tagged VLAN list
allows all VLANs. Check the management/uplink port before changing VLANs.
JSONC supports export comments; plain `jq` cannot validate JSONC directly.

## Make a narrow settings update

```bash
unifly -p home settings get dpi -o json
unifly -p home settings set dpi enabled true
unifly -p home settings get dpi -o json
```

For multiple fields, use `settings set KEY --data '{"field": "value"}'`.
Updates preserve fields outside the requested change. Redacted exports
omit credentials; do not replay them as a full backup. Read the credential
and array caveats with `unifly skill concepts` before replacing arrays or
using `--allow-secret-removal`.

## Reorder firewall policies

Snapshot the current ordering for the exact zone pair:

```bash
unifly -p home firewall policies reorder \
  --source-zone SOURCE_ZONE --dest-zone DEST_ZONE --get -o json > order-before.json
```

The result contains two arrays: `beforeSystemDefined` and
`afterSystemDefined`. Inspect the policies in each array and prepare the
complete desired ordering for the half you intend to change. The default
`--set` replaces the before-system half; add `--after-system` to replace
the after-system half. For example, change the before-system half and
read back both halves:

```bash
unifly -p home firewall policies reorder \
  --source-zone SOURCE_ZONE --dest-zone DEST_ZONE --set 'ID1,ID2,ID3'
unifly -p home firewall policies reorder \
  --source-zone SOURCE_ZONE --dest-zone DEST_ZONE --get -o json
```

Naming an ID from the opposite half moves that policy across the system
rules boundary. Keep the arrays separate; concatenating them into a single
`--set` changes their placement relative to system rules. Order affects
first-match behavior.

Preserve both original arrays for rollback. When both originals are
nonempty, restore them with separate writes: one `--set` with the original
before-system IDs, and one `--set --after-system` with the original
after-system IDs. An empty string is not a valid policy UUID; do not use
`--set ''` to clear a half. Moving policies back to their original half
removes them from the opposite half. Re-fetch and compare both arrays
after the writes. For enabled/logging changes alone, prefer the narrow
`firewall policies patch` command over a full policy update.

## Trace a client's connectivity

```bash
unifly -p home clients find laptop -o json
unifly -p home clients roams CLIENT --limit 100 -o json
unifly -p home clients wifi CLIENT -o json
unifly -p home wifi neighbors --all -o json
```

Replace `CLIENT` with the identified client. With Session WebSocket
authentication, stream events:

```bash
unifly -p home events watch --types 'Client,Firewall' -o json
```

The stream uses JSON Lines. Event filters take categories such as `Client`
and `Firewall`, not `EVT_*` globs. To inspect recent stored events for a MAC:

```bash
unifly -p home events list --within 1 --limit 500 -o json |
  jq --arg mac 'aa:bb:cc:dd:ee:ff' '.[] | select(.client_mac == $mac)'
```

The event list is a bounded snapshot. An empty result does not prove the
absence of an incident. Historical stats retain endpoint-specific fields;
inspect their output before aggregating. A reporting interval such as
`daily` selects bucket size, not a guaranteed last-24-hours window.

## Query an unwrapped endpoint

Use typed commands when available. For a direct controller profile, raw
paths include their own site segment:

```bash
unifly -p home api 'api/s/default/stat/health' -o json
```

Replace `default` with the intended Session site name. The profile site
does not rewrite this literal path. Integration raw routes instead need
the site's Integration UUID, and raw output keeps its original envelope.
The raw command supports writes, but does not supply a preview or an
automatic confirmation gate.

## Verify before retrying or deleting

After a mutation, re-fetch the affected entity or collection. If a write
fails after dispatch, inspect the actual state before retrying. Keep
stderr separate from JSON stdout and preserve the original error.

Before deleting a network, run `networks refs ID -o json` and inspect all
returned references. A failed reference query leaves dependency status
unknown. For backups, confirm a newly created backup is listed and can be
downloaded before considering retention cleanup. Do not assume the list
is sorted or that a fixed sleep proves creation is complete.
