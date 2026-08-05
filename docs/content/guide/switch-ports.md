+++
title = "Switch Ports"
description = "Switch port profiles as reviewable JSONC files: export, edit, apply, diff"
weight = 7
+++

Switch port configuration is a config-as-code surface: export a switch's port overrides to a JSONC file, keep it in git, edit it in review, and apply it back. Three commands make the loop:

```bash
unifly devices ports <SWITCH>              # Read current port config
unifly devices ports-export <SWITCH>       # Export overrides as JSONC
unifly devices port-set <SWITCH> -F ports.jsonc   # Apply a file
```

`<SWITCH>` accepts a device ID or MAC. Port indices are **1-based** throughout, matching the labels on the hardware and the controller's wire format.

## Reading Ports

```bash
unifly devices ports <SWITCH>                   # Table of ports and overrides
unifly devices ports <SWITCH> --with-clients    # Annotate with what's plugged in
unifly devices ports <SWITCH> -o json           # Full structured output
```

`--with-clients` adds a connections column counting the end-user clients and adopted devices (APs, downstream switches) observed on each port; JSON output carries a `connections` array with `kind`, `mac`, `name`, and, for clients, `ip` and `vlan_id`.

## Quick Single-Port Changes

For one-off tweaks, `port-set` takes the port index and flags directly:

```bash
unifly devices port-set <SWITCH> 9 --mode access --native-vlan IoT
unifly devices port-set <SWITCH> 1 --mode trunk --tagged-vlans "Infra,IoT,Cameras"
unifly devices port-set <SWITCH> 5 --name "camera-ne" --poe auto
unifly devices port-set <SWITCH> 3 --speed 2500
unifly devices port-set <SWITCH> 7 --reset      # Back to controller defaults
```

- `--native-vlan` and `--tagged-vlans` accept network **names** or IDs. Names are resolved against the controller's network list, and an ambiguous name errors out rather than picking one.
- `--mode trunk` without `--tagged-vlans` trunks **all** VLANs; passing an explicit list switches the port to a custom tagged set.
- `--poe` accepts `off`, `auto`, `pasv24`, and `passthrough`. There is no `on`: `auto` is the on/negotiate mode.
- `--speed auto` re-enables auto-negotiation; the numeric values pin a link speed.
- `--reset` removes the port's override entirely, returning it to controller defaults. It prompts for confirmation unless `-y` is passed.

## The JSONC Payload

`port-set -F` applies a JSONC file (comments and trailing commas are allowed) describing one or more ports on a single device:

```jsonc
{
  "ports": [
    // Trunk uplink carrying all VLANs
    {
      "index": 1,
      "name": "uplink",
      "mode": "trunk",
      "native_vlan": "Infra",
      "tagged_all": true,
      "poe": "off",
    },

    // Access port pinned to one VLAN
    {
      "index": 9,
      "name": "mac-mini",
      "mode": "access",
      "native_vlan": "Personal",
      "poe": "auto",
    },

    // Clear a stale override
    { "index": 7, "reset": true },
  ],
}
```

Per-port fields: `name`, `mode` (`access` / `trunk` / `mirror`), `native_network_id` (alias `native_vlan`), `tagged_network_ids` (alias `tagged_vlans`), `tagged_all`, `poe`, `speed`, and `reset`. Unknown fields are rejected with an error, so typos surface instead of silently disappearing.

### Splice Semantics

The payload is a splice, not a replacement:

- Ports **not listed** keep their existing override untouched.
- A listed port **merges** the given fields into its override; omitted fields are unchanged.
- An **empty** `tagged_network_ids: []` clears the tagged list (JSON Merge Patch style); omitting the field leaves it alone.
- `"reset": true` removes that port's override entry entirely.

This means a payload file can safely describe just the ports you care about, and applying it twice is idempotent.

## Export and Drift Detection

`ports-export` emits the device's current configuration in exactly the shape `port-set -F` accepts:

```bash
unifly devices ports-export <SWITCH> > ports.jsonc         # Only ports with overrides
unifly devices ports-export <SWITCH> --all > ports.jsonc   # Every port
unifly devices ports-export <SWITCH> --with-clients > ports.jsonc
```

The round-trip is non-destructive: export, then re-apply the same file, and the device configuration is unchanged.

With `--with-clients`, the export prepends a comment line above each port recording what was observed on it:

```jsonc
// last-seen 2026-08-05T17:20:00Z: aa:bb:cc:dd:ee:01 (Living Room AP, device)
{
  "index": 3,
  "name": "ap-living-room",
  ...
}
```

Commit the export to git and re-export on a schedule to catch moved cables, swapped APs, or surprise devices. Every export stamps a fresh timestamp into each `// last-seen` marker, so strip the timestamps before diffing to see only real occupant changes:

```bash
unifly devices ports-export <mac> --with-clients \
  | sed -E 's|// last-seen [^:]+: |// occupant: |' > ports/<mac>.jsonc
git diff ports/<mac>.jsonc
```

{% tip() %}
The `// last-seen ` prefix (with a single trailing space) is a stable parse anchor if you script against the export.
{% end %}

## Authentication

Port configuration lives on Session API routes; the Integration API does not expose port VLAN settings. On UniFi OS consoles an Integration API key satisfies these routes, so no username or password is needed. Classic standalone controllers require `auth_mode = "session"` or `"hybrid"`.

## Next Steps

- [CLI Commands](/reference/cli): the condensed switch-port command list
- [Networks](/reference/cli#networks): the VLANs your port profiles reference
- [Troubleshooting](/troubleshooting): auth-mode errors and fixes
