#!/usr/bin/env bash
set -euo pipefail

cd "$(dirname "$0")/.."
brand="docs/static/brand"

if ! command -v resvg >/dev/null; then
  echo "Install resvg to export the SVG artwork." >&2
  exit 1
fi

resvg --width 1280 "$brand/repo-card.svg" "$brand/repo-card.png"
resvg --width 32 "$brand/favicon.svg" "$brand/favicon-32.png"
resvg --width 180 "$brand/favicon.svg" "$brand/apple-touch-icon.png"
resvg --width 512 "$brand/avatar.svg" "$brand/avatar.png"
resvg --width 1280 "$brand/dragonfly.svg" "$brand/dragonfly.png"
resvg --width 1120 "$brand/logo-dark.svg" "$brand/logo-dark.png"
resvg --width 1120 "$brand/logo-light.svg" "$brand/logo-light.png"
