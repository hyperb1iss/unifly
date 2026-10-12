+++
title = "Site Settings"
description = "Read and write every site-level settings section from the terminal"
weight = 8
+++

The `unifly settings` command reads and writes site-level settings: the same sections the controller web UI scatters across its Settings screens (management, DPI, IPS, guest access, radio AI, and the rest). Every subcommand talks to the Session API, so an API key on UniFi OS or session credentials is required.

```bash
unifly settings list                 # Summary of every settings section
unifly settings get <KEY>            # One section in full
unifly settings set <KEY> <FIELD> <VALUE>
unifly settings set <KEY> --data '{"field": "value"}'
unifly settings export               # Raw JSON dump of everything
```

## Discovering Sections

`settings list` shows a summary table of all sections with their key, field count, enabled status, and notable values. Keys are the controller's internal section names: `mgmt`, `dpi`, `ips`, `usg`, `radio_ai`, `guest_access`, and so on.

```bash
unifly settings list
unifly settings get dpi
unifly settings get ips -o json
```

{% tip() %}
Fields prefixed with `x_` hold credentials and internal secrets. The table view shows them as `***`, and JSON or YAML output leaves them out entirely. Add `--with-secrets` when you actually need the real values.
{% end %}

## Writing a Field

`set` performs a read-modify-write: it fetches the current section, patches the one field you named, and PUTs the full section back. Values are parsed as booleans (`true`/`false`), then numbers, then fall back to strings.

```bash
unifly settings set dpi enabled true
unifly settings set mgmt advanced_feature_enabled false
```

For multiple fields at once, `--data` merges a JSON object into the section instead of a single field/value pair (the two forms are mutually exclusive):

```bash
unifly settings set usg --data '{"mss_clamp": "auto", "broadcast_ping": false}'
```

Because the PUT replaces the entire section, unifly strips the `_id`, `site_id`, and `key` metadata fields before sending, so the payload round-trips cleanly.

{% warning() %}
Settings sections control controller-wide behavior — IPS modes, management access, guest portals. A wrong value can lock you out of a surface or disrupt clients. Read the section with `get` first, and prefer patching single fields over replaying large `--data` blobs.
{% end %}

## Exporting Everything

`settings export` dumps every section as JSON, regardless of the `--output` flag. Credential fields (`x_`-prefixed, at any depth) are left out by default, so the export is safe to commit for change tracking. A note on stderr says how many were removed.

```bash
# Snapshot for change tracking
unifly settings export > settings-$(date +%F).json

# One-off inspection
unifly settings export | jq '.[] | select(.key == "ips")'

# Full dump including credentials: treat as secret material
unifly settings export --with-secrets > settings-full.json
```

Leaving the fields out, rather than masking them, keeps edited exports safe to apply. When `set` writes a section back, it carries over any credential your JSON left out of an object, at any depth.

Arrays are all or nothing, because their entries have no identity to match on. If you send an array that holds credentials and your version includes any of them, unifly treats your array as authoritative, so rotating, clearing, reordering, and removing entries all work as written. If your version includes none, which is what a stripped export looks like, `set` refuses and names the credentials it would erase. Export that section with `--with-secrets` to edit such an array, or pass `--allow-secret-removal` when dropping them is the point.

## Relationship to Other Commands

Some commands are curated fronts over the same settings machinery: `dpi status|enable|disable` toggles the `dpi` section, and [`vpn settings`](/guide/vpn#magic-site-to-site-and-settings) manages the VPN-related sections with friendlier names. `settings` is the general-purpose surface for everything else.

## Next Steps

- [CLI Commands](/reference/cli): the condensed settings command list
- [Authentication](/guide/authentication): session gating explained
- [VPN](/guide/vpn): the VPN-specific settings front-end
