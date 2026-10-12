#!/usr/bin/env bash
# Move the CHANGELOG's [Unreleased] notes under a dated version heading,
# open a fresh empty [Unreleased], and update the compare links.
#
# Usage: scripts/changelog-cut.sh <X.Y.Z> [YYYY-MM-DD] [CHANGELOG path]
set -euo pipefail

version="${1:-}"
date="${2:-$(date -u +%Y-%m-%d)}"
changelog="${3:-CHANGELOG.md}"

if [[ ! "$version" =~ ^[0-9]+\.[0-9]+\.[0-9]+$ ]]; then
  echo "usage: $0 <X.Y.Z> [YYYY-MM-DD] [CHANGELOG path]" >&2
  exit 2
fi
if [[ ! "$date" =~ ^[0-9]{4}-[0-9]{2}-[0-9]{2}$ ]]; then
  echo "error: date must be YYYY-MM-DD, got '$date'" >&2
  exit 2
fi
if [[ ! -f "$changelog" ]]; then
  echo "error: $changelog not found" >&2
  exit 1
fi
if grep -q "^## \[$version\]" "$changelog"; then
  echo "error: $changelog already has a [$version] section" >&2
  exit 1
fi

tmp="$(mktemp)"
trap 'rm -f "$tmp"' EXIT

awk -v version="$version" -v date="$date" '
  BEGIN { state = "before"; notes = 0; unreleased = 0; link = 0 }

  # The [Unreleased] heading: keep it, then open the new version section
  # right below it so every existing note now belongs to the release.
  state == "before" && /^## \[Unreleased\]/ {
    print
    print ""
    print "## [" version "] - " date
    state = "inside"
    unreleased = 1
    next
  }

  state == "inside" && /^## \[/ { state = "after" }
  state == "inside" && NF > 0 { notes = 1 }

  # Compare links: Unreleased now starts at the new tag, and the new
  # version spans from the previous tag to it.
  /^\[Unreleased\]: .*\/compare\/v[0-9.]+\.\.\.HEAD$/ {
    base = $0
    sub(/^\[Unreleased\]: /, "", base)
    sub(/\/compare\/.*/, "", base)
    previous = $0
    sub(/^.*\/compare\//, "", previous)
    sub(/\.\.\.HEAD$/, "", previous)
    print "[Unreleased]: " base "/compare/v" version "...HEAD"
    print "[" version "]: " base "/compare/" previous "...v" version
    link = 1
    next
  }

  { print }

  END {
    if (!unreleased) { print "error: no ## [Unreleased] heading" > "/dev/stderr"; exit 3 }
    if (!notes) { print "error: [Unreleased] has no notes to release" > "/dev/stderr"; exit 4 }
    if (!link) { print "error: no [Unreleased] compare link to update" > "/dev/stderr"; exit 5 }
  }
' "$changelog" >"$tmp"

cat "$tmp" >"$changelog"
echo "Cut [$version] - $date in $changelog"
