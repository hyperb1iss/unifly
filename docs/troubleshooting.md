# 🔧 Troubleshooting

Common issues and how to fix them.

## 🌐 Connection Problems

### "Connection refused" or timeout

**Cause**: unifly can't reach the controller.

```bash
# Verify the URL is correct and reachable
curl -kI https://192.168.1.1

# Check your profile config
unifly config show
```

- Ensure the URL includes `https://` (not `http://`)
- Include the port if non-standard (e.g., `:8443` for self-hosted controllers)
- Check firewall rules between your machine and the controller

### "TLS error: self-signed certificate"

**Cause**: The controller uses a self-signed certificate (this is the default for most UniFi hardware).

```bash
# Quick fix: accept self-signed certs
unifly -k devices list

# Permanent fix: set insecure in your profile
unifly config set insecure true
```

For production, provide a custom CA certificate:

```toml
[profiles.production]
ca_cert = "/path/to/your-ca.pem"
```

### "403 Forbidden" on POST/PUT/DELETE

**Cause**: CSRF token is stale or missing. This happens when the Session API session has expired.

```bash
# Force a fresh login
unifly --no-cache devices list
```

If it persists, check that your credentials are still valid on the controller.

## 🔑 Authentication Issues

### "Unsupported { required: Integration API }"

**Cause**: You're running a command that needs an API key, but your profile is set to `session` mode.

```bash
# Check your auth mode
unifly config show

# Switch to hybrid or integration
unifly config set auth_mode hybrid
```

### "Unsupported { required: Session API }"

**Cause**: You're running a command that needs username/password, but your profile only has an API key.

```bash
# Switch to hybrid mode and add credentials
unifly config set auth_mode hybrid
unifly config set-password
```

### "Profile not found"

```bash
# List available profiles
unifly config profiles

# Check for typos in profile name
unifly -p <correct-name> devices list
```

### Keyring Errors (Linux)

**Cause**: The Secret Service daemon isn't running (common on headless servers, WSL, or minimal desktops).

```bash
# Check if a keyring daemon is available
dbus-send --session --dest=org.freedesktop.DBus \
  --type=method_call --print-reply \
  /org/freedesktop/DBus org.freedesktop.DBus.ListNames 2>/dev/null | grep -i secret
```

Workaround: Use environment variables instead of the keyring:

```bash
export UNIFI_API_KEY="your-key"
export UNIFI_URL="https://192.168.1.1"
```

## 📊 Missing or Empty Data

### Client list missing traffic/hostname/VLAN columns

**Cause**: You're using API Key mode. These fields come from the Session API and require Hybrid mode.

```bash
# Switch to Hybrid for enriched data
unifly config set auth_mode hybrid
unifly config set-password
```

### "events watch" hangs or shows nothing

**Cause**: Events require the Session API (WebSocket connection).

- Verify your profile has `auth_mode = "hybrid"` or `auth_mode = "session"` (`"legacy"` also works as a backwards-compatible alias for `"session"`)
- Check that the controller's WebSocket port is accessible
- Try `unifly events list` first to confirm Session API access works

### Results are truncated at 25 rows

**Cause**: Default list limit is 25. This is by design with a truncation hint.

```bash
# Show all results
unifly devices list --all

# Or set a higher limit
unifly clients list --limit 200
```

## 🖥️ TUI Issues

### TUI crashes or shows garbled output

**Cause**: Terminal doesn't support alternate screen mode or Unicode.

- Use a modern terminal emulator (Ghostty, Kitty, Alacritty, iTerm2, Windows Terminal)
- Ensure your locale supports UTF-8: `echo $LANG` should show something like `en_US.UTF-8`
- Try a larger terminal window (minimum ~120x40 recommended)

### "Where are the TUI logs?"

Logs go to a single file in your system temp directory (stderr is captured by the alternate screen):

```bash
# Default path (varies by OS)
# Linux:  /tmp/unifly-tui.log
# macOS:  $TMPDIR/unifly-tui.log  (e.g., /var/folders/.../unifly-tui.log)

# Find it programmatically
python3 -c "import tempfile; print(tempfile.gettempdir() + '/unifly-tui.log')"

# With verbose logging
unifly tui -v      # INFO level
unifly tui -vv     # DEBUG level
unifly tui -vvv    # TRACE level
```

### Theme doesn't look right

```bash
# Override the theme
UNIFLY_THEME=silkcircuit unifly tui

# Or use the theme selector: press , in the TUI to open Settings
```

## 🔐 MFA / TOTP

### "TOTP required but not provided"

**Cause**: The controller has two-factor authentication enabled.

```bash
# Pass TOTP via environment variable
UNIFI_TOTP=123456 unifly devices list

# Or with 1Password CLI
UNIFI_TOTP=$(op read "op://Vault/UniFi/one-time password") unifly devices list

# Configure in your config.toml profile for automatic resolution:
# [profiles.home]
# totp_env = "UNIFI_TOTP"
```

::: tip
`totp_env` must be set directly in `config.toml`. It is not yet supported by `unifly config set`.
:::

## 🎭 Common Gotchas

- **`events watch --types`** takes category names (`Device`, `Client`, `Network`), not `EVT_*` glob patterns
- **`nat policies update`** uses `--name` or `--description` (mutually exclusive) for the display label
- **`firewall policies patch`** is the fast path for toggling `enabled`/`logging`. Use it instead of `update` when only those fields change
- **`networks refs <id>`** checks what depends on a network before you delete it. No equivalent exists for other entities yet
- **`admin revoke`** takes a positional admin ID, not a `--email` flag
- **Controller reconnect** is currently broken in the TUI. If the connection drops, restart the TUI

## Still Stuck?

- Run with max verbosity: `unifly -vvv <command>` to see full request/response details
- Check [GitHub Issues](https://github.com/hyperb1iss/unifly/issues) for known problems
- Open a new issue with your unifly version, controller model/firmware, and the verbose output

## 🎯 Next Steps

- [Configuration](/guide/configuration): check your profile settings
- [Authentication](/guide/authentication): review which auth mode you need
- [CLI Commands](/reference/cli): full command reference
