---
name: unifly-audit
description: Read-only audit of UniFi WiFi security, firewall policy, and network isolation
arguments:
  - name: profile
    description: "Config profile to use (e.g., home, office)"
    required: false
---

# UniFi Security Audit

Load `unifly skill` and `unifly skill commands` before inspecting the network.
Use the installed guide and command help for authentication, enumeration,
identifiers, and output fields. Inspect returned schemas before filtering.

Apply the optional profile argument consistently; when omitted, retain the
configured default. Review WiFi security settings, firewall policy scope and
ordering, device firmware availability, and network isolation. Distinguish
intended guest access from an unintended exposure. Assess findings against
the user's network requirements instead of assigning severity from a single
field such as missing logging or WPA2 support.

Keep the audit read-only. Report evidence, impact, proposed remediation, and
checks unavailable with the current access. Failed dependency queries leave
resource usage unknown; they do not establish that deletion is safe.

If `unifly skill` is unavailable, use `unifly --help` and explain that the
binary needs updating before it can supply version-matched guidance.
