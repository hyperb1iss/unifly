---
name: unifly-status
description: Read-only health check of UniFi infrastructure, including devices and alerts
arguments:
  - name: profile
    description: "Config profile to use (e.g., home, office)"
    required: false
---

# UniFi Status Check

Load the installed operating guide with `unifly skill`, then use
`unifly skill workflows` for diagnostic patterns. Follow that binary's help
and output contracts instead of assuming field names or authentication support.

Inspect the requested profile's health, devices, clients, and available alarms.
Apply the optional profile argument consistently; when omitted, retain the
configured default. Use reads supported by the profile's local or cloud access.
Report device counts by observed state, offline devices, active alarms, and
any unavailable checks. Keep the status check read-only.

If `unifly skill` is unavailable, use `unifly --help` and explain that the
binary needs updating before it can supply version-matched guidance.
