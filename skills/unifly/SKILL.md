---
name: unifly
description: >-
  Inspect and manage UniFi Network infrastructure with the unifly CLI: devices,
  clients, VLANs, WiFi, firewall, VPN, switch ports, site settings, and cloud
  fleets. Use for UniFi network configuration, monitoring, and troubleshooting,
  not UniFi Protect camera or recording management.
---

# unifly

Before using unifly, load the guide shipped with the installed binary:

```bash
unifly skill
```

The guide provides authentication boundaries, inspection and change workflows,
and commands for loading further topics and payload examples. Command discovery
is generated from the installed CLI. Skill output works offline without requiring
controller configuration or credentials.

If the executable is missing, install unifly using the project's installation
instructions: <https://github.com/hyperb1iss/unifly#-install>. If `skill` is an
unknown command, the installed binary predates runtime guidance; update it to a
release that supports `unifly skill`. Until then, use that binary's `--help` for
syntax and avoid assuming the latest online guide matches its behavior.
