+++
title = "AI Agent Skill"
description = "Version-matched UniFi guidance for coding agents"
weight = 9
+++

The installable unifly skill is a discovery stub. It tells an agent to run
`unifly skill`, which prints the guide embedded in the installed binary.
Updating the CLI updates its guide at the same time.

## Install

Install the [unifly binary](/guide/installation) on the machine where the agent
runs, then add the discovery skill for your user:

```bash
unifly skill --install
```

This writes `~/.agents/skills/unifly/SKILL.md`, the shared user skills
directory. Claude Code reads its own directory, so target it with `--claude`
(`$CLAUDE_CONFIG_DIR/skills`, or `~/.claude/skills` by default), or pick any
skills directory with `--dir`:

```bash
unifly skill --install --claude
unifly skill --install --dir ~/work/agent-skills
```

The command works offline without configuration. It prints the installed path
on stdout and its status on stderr (`--quiet` keeps only the path). Running
it again is safe: an identical skill is left alone, and one installed by
`npx skills add` or an earlier release is upgraded in place. Any other
`SKILL.md` at that path, such as a locally edited copy, stays untouched and the
command exits with code 6 unless you pass `--force`. A symlinked skill
directory or `SKILL.md` means something else manages the skill (a checkout, or
`npx skills add` in link mode), so the command reports where the link points
and writes nothing, even with `--force`.

The quick install scripts run this step for you (set `UNIFLY_NO_SKILL=1` to
skip it). `install.sh` skips it when run as root, since that home directory may
belong to someone else. Homebrew and AUR install unifly for every user and cannot write into
your home directory, so they print a reminder instead. After `cargo install`,
run it yourself.

The [skills CLI](https://github.com/vercel-labs/skills) installs the same stub
for many agents at once:

```bash
npx skills add hyperb1iss/unifly
```

For Claude Code, the repository also provides a plugin with a network manager
agent and status/audit shortcuts:

```text
/plugin marketplace add hyperb1iss/unifly
```

Older binaries that do not recognize `unifly skill` need an update before
they can serve the guide. Until then, use the installed binary's `--help` for
syntax rather than assuming current online instructions match its behavior.

## Load Guidance as Needed

```bash
unifly skill
unifly skill commands
unifly skill concepts
unifly skill workflows
unifly skill examples
unifly skill examples switch-ports.jsonc
```

The core guide covers targeting, authentication, inspection, authorized
changes, and verification. Command notes explain operational exceptions;
the appended inventory is generated from the binary's Clap command tree.
Concepts covers configuration, identifiers, output, and secrets. Workflows
provides focused recipes for diagnostics, settings, switch ports, and firewall
ordering. Named examples print the raw template for saving and editing.

Every topic works offline without a configured controller. Commands used to
inspect or modify the network still need the appropriate credentials.
Configuration setup uses `unifly config init` locally or
`unifly config cloud-setup` for Site Manager.

Runtime guidance removes installed skill/binary version mismatch. Operational
prose still needs maintenance when behavior changes, and a payload that passes
local schema checks may require different values on a particular controller.

## Agent and Slash Commands

The plugin's network manager agent and both shortcuts load the same runtime
guide instead of carrying separate command recipes:

| Entry point           | Purpose                                                          |
| --------------------- | ---------------------------------------------------------------- |
| Network manager agent | Inspect, provision, and verify authorized infrastructure changes |
| `/unifly-status`      | Read-only health, device, client, and alarm summary              |
| `/unifly-audit`       | Read-only WiFi, firewall, firmware, and isolation review         |

The shortcuts preserve the requested profile and report checks that the
available authentication cannot perform. Audit findings include evidence and
impact; a failed query is not proof that a resource is unused.

## Example Requests

- "Create an IoT VLAN with a dedicated SSID and isolation from my trusted network."
- "Audit firewall policy ordering without changing anything."
- "Find the client named ring-doorbell and inspect its WiFi experience."
- "Export the switch port configuration so I can review the VLAN assignments."
- "List the consoles visible through my Site Manager profile."

The agent reads the relevant guide topic, inspects state, carries the user's
authorization through the task, and verifies changes afterward.

## Next Steps

- [Quick Start](/guide/quick-start): configure the CLI
- [Authentication](/guide/authentication): choose local or cloud access
- [CLI Commands](/reference/cli): inspect the available operations
