#!/bin/sh
# unifly installer
# Usage: curl -fsSL https://raw.githubusercontent.com/hyperb1iss/unifly/main/install.sh | sh
set -e

REPO="hyperb1iss/unifly"
BINARY="unifly"

# Colors (if terminal supports them)
if [ -t 1 ]; then
    PURPLE='\033[38;2;225;53;255m'
    CYAN='\033[38;2;128;255;234m'
    GREEN='\033[38;2;80;250;123m'
    RED='\033[38;2;255;99;99m'
    YELLOW='\033[38;2;241;250;140m'
    RESET='\033[0m'
else
    PURPLE='' CYAN='' GREEN='' RED='' YELLOW='' RESET=''
fi

info()  { printf "${CYAN}::${RESET} %s\n" "$1"; }
ok()    { printf "${GREEN}::${RESET} %s\n" "$1"; }
warn()  { printf "${YELLOW}::${RESET} %s\n" "$1"; }
error() { printf "${RED}::${RESET} %s\n" "$1" >&2; exit 1; }

# Detect platform
detect_platform() {
    OS=$(uname -s | tr '[:upper:]' '[:lower:]')
    ARCH=$(uname -m)

    case "$OS" in
        linux)  PLATFORM="linux" ;;
        darwin) PLATFORM="macos" ;;
        *)      error "Unsupported OS: $OS (use cargo install unifly instead)" ;;
    esac

    case "$ARCH" in
        x86_64|amd64)   ARCH_SUFFIX="amd64" ;;
        aarch64|arm64)  ARCH_SUFFIX="arm64" ;;
        *)              error "Unsupported architecture: $ARCH" ;;
    esac

    # macOS only ships arm64 builds
    if [ "$PLATFORM" = "macos" ] && [ "$ARCH_SUFFIX" = "amd64" ]; then
        error "macOS x86_64 builds are not available. Use Homebrew (brew install hyperb1iss/tap/unifly) or build from source."
    fi

    ASSET="${BINARY}-${PLATFORM}-${ARCH_SUFFIX}"
}

# Get latest version
get_latest_version() {
    VERSION=$(curl -fsSL "https://api.github.com/repos/${REPO}/releases/latest" \
        | grep '"tag_name"' | head -1 | sed 's/.*"tag_name": *"//;s/".*//')

    if [ -z "$VERSION" ]; then
        error "Failed to fetch latest version from GitHub"
    fi
}

# Choose install directory
choose_install_dir() {
    if [ -n "$INSTALL_DIR" ]; then
        # User specified
        return
    elif [ -w /usr/local/bin ]; then
        INSTALL_DIR="/usr/local/bin"
    elif [ -d "$HOME/.local/bin" ]; then
        INSTALL_DIR="$HOME/.local/bin"
    else
        mkdir -p "$HOME/.local/bin"
        INSTALL_DIR="$HOME/.local/bin"
    fi
}

# Download and install
install() {
    URL="https://github.com/${REPO}/releases/download/${VERSION}/${ASSET}"
    TMPDIR=$(mktemp -d)
    TMPFILE="${TMPDIR}/${BINARY}"

    info "Downloading ${ASSET} ${VERSION}..."
    if ! curl -fsSL -o "$TMPFILE" "$URL"; then
        rm -rf "$TMPDIR"
        error "Download failed. Check https://github.com/${REPO}/releases for available assets."
    fi

    chmod +x "$TMPFILE"

    info "Installing to ${INSTALL_DIR}/${BINARY}..."
    if [ -w "$INSTALL_DIR" ]; then
        mv "$TMPFILE" "${INSTALL_DIR}/${BINARY}"
    else
        sudo mv "$TMPFILE" "${INSTALL_DIR}/${BINARY}"
    fi

    rm -rf "$TMPDIR"
}

# Verify installation
verify() {
    if command -v "$BINARY" >/dev/null 2>&1; then
        INSTALLED_VERSION=$("$BINARY" --version 2>/dev/null | head -1)
        ok "Installed ${INSTALLED_VERSION}"
    else
        warn "Installed to ${INSTALL_DIR}/${BINARY} but it's not in your PATH"
        warn "Add this to your shell profile:"
        printf "  export PATH=\"%s:\$PATH\"\n" "$INSTALL_DIR"
    fi
}

# Install the agent skill for the current user. Package managers cannot
# reach a home directory, but this script runs as the user. Set
# UNIFLY_NO_SKILL=1 to skip it.
install_skill() {
    BIN_PATH="${INSTALL_DIR}/${BINARY}"
    if [ -n "$UNIFLY_NO_SKILL" ]; then
        return
    fi
    # Releases before `unifly skill --install` cannot do this.
    if ! "$BIN_PATH" skill --help 2>/dev/null | grep -q -- '--install'; then
        return
    fi
    if [ "$(id -u)" = "0" ]; then
        warn "Skipped the agent skill while running as root"
        warn "Run 'unifly skill --install' as your own user to add it"
        return
    fi
    if SKILL_STATUS=$("$BIN_PATH" skill --install 2>&1 >/dev/null); then
        printf '%s\n' "$SKILL_STATUS" | while IFS= read -r line; do ok "$line"; done
    else
        warn "Could not install the agent skill; run 'unifly skill --install' to see why"
    fi
}

# Main
printf "\n${PURPLE}  unifly installer${RESET}\n\n"

detect_platform
get_latest_version
choose_install_dir
install
verify
install_skill

printf "\n${CYAN}  Get started:${RESET}\n"
printf "    unifly config init    # Set up your controller\n"
printf "    unifly devices list   # List network devices\n"
printf "    unifly tui            # Launch the dashboard\n\n"
