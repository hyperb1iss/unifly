+++
title = "Installation"
description = "Install unifly via Homebrew, Cargo, AUR, or binary download"
weight = 1
+++

## Quick Install

```bash
# Linux / macOS
curl -fsSL https://raw.githubusercontent.com/hyperb1iss/unifly/main/install.sh | sh
```

```powershell
# Windows
irm https://raw.githubusercontent.com/hyperb1iss/unifly/main/install.ps1 | iex
```

The installer detects your platform, downloads the latest release binary, adds it to your `PATH`, and installs the [agent skill](/guide/agents) for your user. Set `UNIFLY_NO_SKILL=1` to skip the skill.

## Package Managers

```bash
# Homebrew
brew install hyperb1iss/tap/unifly
```

```bash
# AUR (Arch)
yay -S unifly-bin
```

```bash
# Cargo
cargo install unifly
```

## From Source

Requires Rust 1.94+ (edition 2024):

```bash
git clone https://github.com/hyperb1iss/unifly.git
cd unifly
cargo build --workspace --release
# Binary at target/release/unifly
```

Or install directly from git:

```bash
cargo install --git https://github.com/hyperb1iss/unifly.git unifly
```

## Shell Completions

```bash
# Bash
unifly completions bash > ~/.local/share/bash-completion/completions/unifly
```

```bash
# Zsh
unifly completions zsh > ~/.zfunc/_unifly
```

```bash
# Fish
unifly completions fish > ~/.config/fish/completions/unifly.fish
```

```powershell
# PowerShell
unifly completions powershell | Out-String | Invoke-Expression
```

## Agent Skill

Coding agents find unifly through a small discovery skill. Package managers
install unifly for every user and cannot write into your home directory, so
after Homebrew, AUR, Cargo, or a manual download, add it yourself:

```bash
unifly skill --install
```

It lands in `~/.agents/skills/unifly/SKILL.md`. See
[AI Agent Skill](/guide/agents) for Claude Code and other directories.

## Verify

```bash
unifly --version
```

## Next Steps

You're in! Now configure your first controller profile:

- [Quick Start](/guide/quick-start): run `unifly config init` and explore your network
- [Authentication](/guide/authentication): choose the right auth mode
- [Troubleshooting](/troubleshooting): if something goes wrong
