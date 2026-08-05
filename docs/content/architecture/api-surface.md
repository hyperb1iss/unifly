+++
title = "API Surface"
description = "Integration API, Session API, and Site Manager endpoints"
weight = 3
+++

Unifly communicates with UniFi infrastructure through three distinct API interfaces, each with different authentication mechanisms and capabilities: the Integration API and Session API on the controller itself, and the Site Manager API in Ubiquiti's cloud.

## Integration API

The official REST API introduced in UniFi Network 7.x+.

| Aspect       | Details                                                                                         |
| ------------ | ----------------------------------------------------------------------------------------------- |
| **Auth**     | API key via `X-API-KEY` header                                                                  |
| **Base URL** | `https://<controller>/proxy/network/integration/v1/` (UniFi OS) or `/integration/v1/` (Classic) |
| **Format**   | JSON with typed responses                                                                       |
| **Methods**  | Full CRUD plus action endpoints (GET, POST, PUT, PATCH, DELETE)                                 |

### Covered Resources

Devices, clients, networks, WiFi (WLANs), firewall policies, firewall zones, ACL rules, DNS records, traffic matching lists, hotspot vouchers, VPN servers, VPN tunnels, RADIUS profiles, WAN interfaces, sites, system info, device actions, port actions, device statistics, countries, and DPI categories/applications.

### Limitations

- No event streaming
- No historical statistics
- No alarm management

The Session API still owns the live and historical monitoring surfaces: event streaming, stats/reporting, and admin workflows.

## Session API

The original UniFi controller API, session-based with cookie authentication.

| Aspect       | Details                                                                 |
| ------------ | ----------------------------------------------------------------------- |
| **Auth**     | Cookie session + CSRF token                                             |
| **Base URL** | `https://<controller>/proxy/network/` (UniFi OS) or root path (Classic) |
| **Format**   | JSON envelope: `{ "meta": {...}, "data": [...] }`                       |
| **CSRF**     | Required on all POST/PUT/DELETE via `X-CSRF-Token` header               |

### Exclusive Features

- `events/latest`: Recent events
- WebSocket event streaming
- `stat/device`: Device statistics with traffic data
- `stat/health`: System health with ISP info, DNS, gateway details
- `stat/report/`: Historical bandwidth and client reports
- `cmd/sitemgr`: Device commands (adopt, restart, upgrade)
- `stat/admin`: Administrator management
- `rest/firewallgroup`: Firewall groups (port and address groups)
- `rest/setting` / `set/setting/{key}`: Site-level settings sections
- Switch port overrides (`devices ports` / `port-set`): the Integration API does not expose port VLAN configuration

### v2 Endpoints

Network Application 9+ exposes a second generation of session-authenticated endpoints under `/v2/api/site/{site}/`. These return plain JSON (no `{ meta, data }` envelope).

| Endpoint                         | Description                                   |
| -------------------------------- | --------------------------------------------- |
| `traffic-flow-latest-statistics` | DPI traffic flow data by application/category |
| `nat`                            | NAT rule CRUD (DNAT, SNAT, masquerade)        |

Unifly's DPI cascade tries the v2 endpoint first, then falls back to `stat/sitedpi` and `stat/dpi` for older firmware. NAT rules use the v2 endpoint exclusively: the Integration API does not expose NAT on Network Application 10.1.89.

### CSRF Token Flow

{% mermaid() %}
sequenceDiagram
participant Client
participant Controller

    Client->>Controller: POST /api/login (credentials)
    Controller-->>Client: Set-Cookie + X-CSRF-Token header
    Client->>Controller: POST /api/s/default/stat/health<br/>X-CSRF-Token: <token>
    Controller-->>Client: Response + X-Updated-CSRF-Token
    Note over Client: Store rotated token for next request

{% end %}

{% warning() %}
GET requests work without CSRF tokens, but all POST/PUT/DELETE requests through `/proxy/network/` will fail with 403 without one.
{% end %}

## Hybrid Mode

When configured with both an API key and credentials, unifly uses each API for what it does best:

| Operation                               | API Used                      |
| --------------------------------------- | ----------------------------- |
| List/get/create/update/delete resources | Integration API (NAT uses v2) |
| Stream events                           | Session API (WebSocket)       |
| Query statistics                        | Session API                   |
| Device commands                         | Session API                   |
| System health (ISP, DNS, gateway)       | Session API                   |

This provides the most complete feature set while using the cleanest API surface available for each operation.

## Site Manager API

Ubiquiti's cloud API for fleets of consoles, spoken by the `SiteManagerClient`.

| Aspect       | Details                                        |
| ------------ | ---------------------------------------------- |
| **Auth**     | Site Manager API key via `X-API-KEY` header    |
| **Base URL** | `https://api.ui.com/v1/`                       |
| **Format**   | Plain JSON                                     |
| **Scope**    | Account-wide: every console the key can access |

Two surfaces run on the same key:

- **Fleet endpoints**: hosts (consoles), sites, devices across the fleet, ISP metrics, and SD-WAN configs. These back the `unifly cloud` command group and need no controller connection.
- **Cloud connector**: `https://api.ui.com/v1/connector/consoles/{host_id}` proxies the console's **Integration API**, so a cloud-mode profile can run the regular Integration-backed commands against a console with no direct network path. Session endpoints are not proxied; session-backed features require direct controller access.

## Error Handling

UniFi controllers return errors in different formats depending on the API:

- **Integration API**: Standard HTTP status codes with JSON error body
- **Session API**: Sometimes HTTP 200 with an error wrapper: `{"error":{"code":N,"message":"..."}}`

Unifly normalizes both into a unified `Error` type with 14 variants covering authentication failures, not-found, validation errors, rate limiting, and transport issues.
