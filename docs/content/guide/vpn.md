+++
title = "VPN"
description = "Site-to-site tunnels, remote access, WireGuard peers, and VPN settings from the CLI"
weight = 6
+++

Unifly manages the full UniFi VPN surface: site-to-site tunnels, remote-access servers, WireGuard peers, client VPN profiles, live connections, and the site-level VPN feature toggles. The read-only inventory rides the Integration API; everything else is Session-backed, so an API key on UniFi OS or session credentials unlocks the whole surface.

{% mermaid() %}
graph LR
subgraph "Integration API"
SRV["vpn servers"]
TUN["vpn tunnels"]
end

    subgraph "Session API"
        S2S["vpn site-to-site"]
        RA["vpn remote-access"]
        CL["vpn clients"]
        SET["vpn settings"]
    end

    subgraph "Session v2 API"
        CONN["vpn connections"]
        PEERS["vpn peers"]
        MAGIC["vpn magic-site-to-site"]
    end

    style SRV fill:#80ffea,color:#0a0a0f
    style TUN fill:#80ffea,color:#0a0a0f
    style S2S fill:#50fa7b,color:#0a0a0f
    style RA fill:#50fa7b,color:#0a0a0f

{% end %}

## Inventory and Health

Start with the read-only views to see what exists:

```bash
unifly vpn servers                    # VPN servers (Integration API)
unifly vpn tunnels                    # Site-to-site tunnel inventory (Integration API)
unifly vpn status                     # Live IPsec tunnel status (security associations)
unifly vpn health                     # VPN subsystem health
```

## Site-to-Site Tunnels

Site-to-site records are payload-driven: `create` and `update` take a JSON file via `--from-file` / `-F` rather than a wall of flags.

```bash
unifly vpn site-to-site list
unifly vpn site-to-site get <ID>
unifly vpn site-to-site create -F ipsec.json
unifly vpn site-to-site update <ID> -F ipsec.json
unifly vpn site-to-site delete <ID>
```

A minimal IPsec payload names the peer, the pre-shared key, and the remote subnets:

```json
{
  "name": "Branch Office Tunnel",
  "vpn_type": "ipsec-vpn",
  "enabled": true,
  "x_ipsec_pre_shared_key": "REPLACE_ME",
  "ipsec_peer_ip": "203.0.113.42",
  "ipsec_key_exchange": "ikev2",
  "remote_vpn_subnets": ["10.20.0.0/24"]
}
```

The [examples/](https://github.com/hyperb1iss/unifly/tree/main/skills/unifly/examples) directory in the repository has complete templates with the full cipher configuration.

## Remote Access

Remote-access servers (WireGuard, OpenVPN, Teleport) follow the same payload-driven CRUD, plus two OpenVPN helpers:

```bash
unifly vpn remote-access list
unifly vpn remote-access get <ID>
unifly vpn remote-access create -F wireguard.json
unifly vpn remote-access update <ID> -F wireguard.json
unifly vpn remote-access delete <ID>

unifly vpn remote-access suggest-port              # Free ports for a new OpenVPN server
unifly vpn remote-access download-config <ID>      # Export an .ovpn client config
```

`download-config` writes `<ID>.ovpn` in the current directory by default; pass `--path` to choose another location.

## WireGuard Peers

Peers hang off a remote-access server, so every mutation takes the parent server ID first:

```bash
unifly vpn peers list                        # All peers, every server
unifly vpn peers list <SERVER_ID>            # Scoped to one server
unifly vpn peers get <SERVER_ID> <ID>
unifly vpn peers create <SERVER_ID> -F peer.json
unifly vpn peers update <SERVER_ID> <ID> -F peer.json
unifly vpn peers delete <SERVER_ID> <ID>
unifly vpn peers subnets                     # Subnets already consumed by peers
```

A peer payload is small: a name, the client's public key, and its tunnel address.

```json
{
  "name": "Bliss Laptop",
  "interface_ip": "10.255.0.2",
  "public_key": "REPLACE_WITH_CLIENT_PUBLIC_KEY",
  "allowed_ips": ["10.255.0.2/32"]
}
```

Run `peers subnets` before assigning addresses to avoid colliding with an existing peer.

## Client VPNs and Connections

Client VPN profiles make the gateway dial out to another provider. Live connections are a separate, restartable inventory:

```bash
unifly vpn clients list                      # Configured client VPN profiles
unifly vpn clients create -F client.json
unifly vpn clients update <ID> -F client.json
unifly vpn clients delete <ID>

unifly vpn connections list                  # Active VPN client connections
unifly vpn connections get <ID>
unifly vpn connections restart <ID>          # Bounce one connection
```

## Magic Site-to-Site and Settings

Magic site-to-site (Ubiquiti's auto-configured mesh between consoles) is read-only; site-level VPN toggles are read-write:

```bash
unifly vpn magic-site-to-site list
unifly vpn magic-site-to-site get <ID>

unifly vpn settings list                     # Teleport, OpenVPN, peer-to-peer, ...
unifly vpn settings get teleport
unifly vpn settings set teleport --enabled true
unifly vpn settings patch peer-to-peer -F payload.json
```

`settings patch` accepts either a raw session setting body or the wrapper shape emitted by `settings get` (`{"key": ..., "enabled": ..., "fields": {...}}`), so a get-edit-patch round-trip works without reshaping.

{% warning(title="Secrets Are Redacted") %}
WireGuard private keys, IPsec pre-shared keys, and similar sensitive material are redacted in both `get` and `create` output. When you build an update payload from a `get`, reconstruct those fields explicitly if the controller requires them unchanged; a redacted placeholder is not a valid value.
{% end %}

## Authentication

`vpn servers` and `vpn tunnels` need the Integration API (an API key). Every other subcommand is Session-backed: an API key works on UniFi OS consoles, while classic standalone controllers need `auth_mode = "session"` or `"hybrid"` with credentials. Cloud-connector profiles cannot reach the Session-backed VPN surface.

## Next Steps

- [CLI Commands](/reference/cli): the condensed VPN command list with gotchas
- [Authentication](/guide/authentication): which auth mode enables which commands
- [Site Settings](/guide/settings): the general settings machinery behind `vpn settings`
