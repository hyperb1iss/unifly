+++
title = "Cloud & Site Manager"
description = "Fleet queries and connector-routed control through Ubiquiti's cloud"
weight = 5
+++

Unifly speaks Ubiquiti's [Site Manager](https://unifi.ui.com) cloud API. One Site Manager API key unlocks two distinct surfaces: **fleet queries** across every console your account can see, and a **cloud connector** that routes normal Integration commands to a console you have no direct network path to.

{% mermaid() %}
graph LR
UNIFLY["unifly"] --> FLEET["Fleet API<br/><i>api.ui.com/v1</i>"]
UNIFLY --> CONN["Cloud Connector<br/><i>/v1/connector/consoles/{host_id}</i>"]
FLEET --> ALL["All consoles:<br/>hosts, sites, devices, ISP, SD-WAN"]
CONN --> ONE["One console:<br/>Integration API commands"]

    style FLEET fill:#80ffea,color:#0a0a0f
    style CONN fill:#ff6ac1,color:#0a0a0f

{% end %}

## Setup

Create a Site Manager API key under your Ubiquiti account (not on a controller), then run the guided wizard:

```bash
unifly config cloud-setup
```

The wizard validates the key against the fleet API, lists the consoles the key can see, lets you pick a console and a site, and writes a profile with `auth_mode = "cloud"` and the chosen console's `host_id`. The key itself lands wherever you choose in the wizard: the OS keyring (default), a named environment variable (`api_key_env`), or plaintext in the config.

```toml
[profiles.cloud]
auth_mode = "cloud"
host_id = "ABC123..."       # The chosen console
site = "default"
# API key in the OS keyring, or via api_key_env
```

Cloud traffic goes to `api.ui.com` over normal public TLS; the `insecure` and `ca_cert` settings do not apply.

## Fleet Queries

The `cloud` command group hits the fleet API directly. No controller connection, no `host_id` needed:

```bash
unifly cloud hosts                      # Consoles the key can see
unifly cloud hosts get <ID>             # One console in detail
unifly cloud sites                      # Sites across the whole fleet
unifly cloud devices                    # Devices across every console
unifly cloud devices --host <ID>        # Scope to one console (repeatable)
unifly cloud isp                        # ISP metrics
unifly cloud isp --type 5m              # Higher resolution
unifly cloud isp query --sites site-a,site-b
unifly cloud sdwan                      # SD-WAN configs
unifly cloud sdwan get <ID>
unifly cloud sdwan status <ID>
```

These work from any machine with internet access, which makes them handy for cross-controller health snapshots and fleet dashboards.

## Connector-Routed Commands

With `auth_mode = "cloud"`, the regular Integration-backed commands transparently tunnel through the connector to the console named by `host_id`:

```bash
unifly -p cloud networks list
unifly -p cloud firewall policies list
unifly -p cloud wifi create --name "Guest" --network <ID> ...
```

### How `host_id` Resolves

The console ID comes from, in order: the `--host-id` flag (or `UNIFI_HOST_ID`), the profile's `host_id_env` variable, then the profile's `host_id` value. When none of those are set, unifly asks the fleet API and auto-resolves:

- The key sees exactly **one console**: that console is used.
- The key sees several, but you **own exactly one**: the owned console is used.
- Otherwise the command errors and lists the available consoles; pin one with `unifly config set host_id <ID>` or `--host-id`.

### Switching Sites

`cloud switch` repoints the active cloud profile at another site on the console, accepting a site name, internal reference, or UUID:

```bash
unifly cloud switch "Branch Office"
```

## What Stays Session-Only

The connector proxies the Integration API only. Session-backed features need a direct connection to the controller and do not work in cloud mode:

- `events list` / `events watch` and the TUI's live event stream
- `stats` (historical reports) and DPI status/control
- Device commands (`restart`, `adopt`, `locate`, `upgrade`, `speedtest`, ...)
- Switch port configuration, firewall groups, NAT, site settings, and the Session-backed VPN surface
- Admin management and backups

For those, use a direct profile (`integration`, `session`, or `hybrid`) when you're on a network that can reach the controller. Profiles switch per-command with `-p`, so a cloud profile and a direct profile for the same console coexist cleanly.

## Next Steps

- [Authentication](/guide/authentication): how cloud mode fits next to the other three modes
- [CLI Commands](/reference/cli): the condensed cloud command list
- [Troubleshooting](/troubleshooting): host_id ambiguity and permission-scoped keys
