+++
title = "Introduction"
description = "Stop context-switching to the UniFi web dashboard. Unifly puts your entire network at your fingertips from the terminal you're already in."
sort_by = "weight"
template = "section.html"
+++

- **`unifly <command>`**: CLI for scripting, automation, and quick lookups
- **`unifly tui`**: real-time terminal dashboard for monitoring

Both are powered by a shared async engine that speaks every UniFi API dialect, so you never have to think about which endpoint to hit.

## The Problem

UniFi infrastructure speaks three completely different APIs:

{% mermaid() %}
graph LR
subgraph "Integration API"
I["REST + API Key"]
I1["Networks, WiFi, Firewall"]
I2["DNS, ACL, Traffic Lists"]
end

    subgraph "Session API"
        S["Cookie + CSRF"]
        S1["Events, Stats, DPI, NAT"]
        S2["Device Commands, Admin"]
    end

    subgraph "Site Manager"
        M["api.ui.com + API Key"]
        M1["Fleet: Consoles, Sites"]
        M2["Cloud Connector"]
    end

    subgraph unifly
        U["Unified Interface"]
    end

    I --> U
    S --> U
    M --> U
    U --> CLI["CLI Output"]
    U --> TUI["TUI Dashboard"]

{% end %}

- **Integration API**: RESTful, API-key authenticated, covers CRUD for most resources
- **Session API**: Session-based with cookie/CSRF, required for events, statistics, and device commands
- **Site Manager API**: Ubiquiti's cloud fleet API, for multi-console queries and connector-routed access without a VPN to the controller

Most tools speak one dialect, maybe two. The web dashboard is slow and can't be scripted. Unifly handles the routing, authentication, and data merging across all three automatically.

## What You Can Do

| Capability                | Description                                                                                       |
| ------------------------- | ------------------------------------------------------------------------------------------------- |
| **Device Management**     | List, inspect, restart, upgrade, and provision devices                                            |
| **Client Monitoring**     | See connected clients with signal, traffic, and VLAN info                                         |
| **Network Configuration** | Manage VLANs, subnets, DHCP, and IPv6 settings                                                    |
| **WiFi Management**       | Create and modify SSIDs, scan neighbors, analyze channels, track client roams and WiFi experience |
| **Firewall**              | Manage policies, zones, groups, and ACL rules                                                     |
| **NAT**                   | Masquerade, source NAT, and destination NAT rules (via Session v2 API)                            |
| **VPN**                   | Site-to-site, remote-access, WireGuard peers, client VPNs, and OpenVPN config export              |
| **Switch Ports**          | Port profiles as reviewable JSONC files: export, edit, apply, diff                                |
| **Site Settings**         | Read and write every site-level settings section from the terminal                                |
| **Cloud Fleet**           | Query consoles, sites, and devices across your whole fleet via Site Manager                       |
| **Events & Alarms**       | Stream live events, acknowledge and archive alarms                                                |
| **Statistics**            | Query bandwidth, client counts, and DPI data over time                                            |
| **Raw API Access**        | Hit any controller endpoint directly with `unifly api`                                            |
| **Real-Time Dashboard**   | Monitor everything with live octant charts and status bars                                        |

## Architecture at a Glance

{% mermaid() %}
graph TD
UNIFLY["unifly<br/><i>CLI + TUI (single binary)</i>"]
API["unifly-api<br/><i>Library crate on crates.io</i>"]

    UNIFLY --> API

    API --> INT["Integration Client<br/>REST + API Key"]
    API --> SES["Session Client<br/>Cookie + CSRF"]
    API --> WS["WebSocket<br/>Live Events"]
    API --> DS["DataStore<br/>DashMap + watch channels"]

{% end %}

Two crates with a clean dependency chain. The library is published independently for Rust developers building custom integrations. See the [Architecture](/architecture/) section for the full picture.

## Next Steps

- [Installation](/guide/installation): get unifly on your system
- [Quick Start](/guide/quick-start): configure and run your first commands
- [Authentication](/guide/authentication): understand API key vs password vs hybrid
- [AI Agent Skill](/guide/agents): let your coding agent manage your network
