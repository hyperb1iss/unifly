#!/usr/bin/env bash
set -euo pipefail

# Generate llms.txt and llms-full.txt from Zola content.
# Run after `zola build` — outputs to public/.

DOCS_DIR="$(cd "$(dirname "$0")/.." && pwd)"
CONTENT_DIR="$DOCS_DIR/content"
PUBLIC_DIR="$DOCS_DIR/public"
BASE_URL="https://hyperb1iss.github.io/unifly"

TITLE="Unifly"
DESCRIPTION="CLI + TUI for UniFi Network Controllers"

extract_field() {
  local file="$1" field="$2"
  sed -n '/^+++$/,/^+++$/p' "$file" | grep "^${field} " | head -1 | sed "s/^${field} = \"\(.*\)\"/\1/"
}

strip_frontmatter() {
  awk 'BEGIN{n=0} /^\+\+\+$/{n++; next} n>=2{print}' "$1"
}

strip_shortcodes() {
  sed -E 's/\{%[^%]*%\}//g'
}

url_for() {
  local file="$1"
  local rel="${file#"$CONTENT_DIR/"}"
  rel="${rel%.md}"
  rel="${rel%_index}"
  rel="${rel%/}"
  [ -z "$rel" ] && echo "$BASE_URL/" && return
  echo "$BASE_URL/${rel}/"
}

# ── llms.txt (index) ─────────────────────────────────────────────

{
  echo "# $TITLE"
  echo ""
  echo "> $DESCRIPTION"
  echo ""
  echo "Unifly is a Rust CLI and TUI for managing Ubiquiti UniFi network"
  echo "infrastructure. One binary with CLI commands, a TUI dashboard, and a"
  echo "triple-path API engine (Integration + Session + Site Manager cloud)."
  echo ""

  for section_index in "$CONTENT_DIR"/*/_index.md; do
    [ -f "$section_index" ] || continue
    section_title="$(extract_field "$section_index" "title")"
    echo "## $section_title"

    section_dir="$(dirname "$section_index")"
    for page in "$section_dir"/*.md; do
      [ "$page" = "$section_index" ] && continue
      [ -f "$page" ] || continue
      page_title="$(extract_field "$page" "title")"
      page_desc="$(extract_field "$page" "description")"
      page_url="$(url_for "$page")"
      if [ -n "$page_desc" ]; then
        echo "- [$page_title]($page_url): $page_desc"
      else
        echo "- [$page_title]($page_url)"
      fi
    done
    echo ""
  done

  if [ -f "$CONTENT_DIR/troubleshooting.md" ]; then
    ts_title="$(extract_field "$CONTENT_DIR/troubleshooting.md" "title")"
    ts_url="$(url_for "$CONTENT_DIR/troubleshooting.md")"
    echo "## Other"
    echo "- [$ts_title]($ts_url): Common issues and how to fix them"
    echo ""
  fi
} > "$PUBLIC_DIR/llms.txt"

# ── llms-full.txt (complete content) ─────────────────────────────

{
  echo "# $TITLE — Complete Documentation"
  echo ""
  echo "> $DESCRIPTION"
  echo ""

  for section_index in "$CONTENT_DIR"/*/_index.md; do
    [ -f "$section_index" ] || continue
    section_title="$(extract_field "$section_index" "title")"
    echo "---"
    echo ""
    echo "# $section_title"
    echo ""
    body="$(strip_frontmatter "$section_index" | strip_shortcodes)"
    [ -n "$body" ] && echo "$body" && echo ""

    section_dir="$(dirname "$section_index")"
    for page in "$section_dir"/*.md; do
      [ "$page" = "$section_index" ] && continue
      [ -f "$page" ] || continue
      page_title="$(extract_field "$page" "title")"
      page_url="$(url_for "$page")"
      echo "---"
      echo ""
      echo "## $page_title"
      echo "Source: $page_url"
      echo ""
      strip_frontmatter "$page" | strip_shortcodes
      echo ""
    done
  done

  if [ -f "$CONTENT_DIR/troubleshooting.md" ]; then
    echo "---"
    echo ""
    echo "## Troubleshooting"
    echo "Source: $(url_for "$CONTENT_DIR/troubleshooting.md")"
    echo ""
    strip_frontmatter "$CONTENT_DIR/troubleshooting.md" | strip_shortcodes
    echo ""
  fi
} > "$PUBLIC_DIR/llms-full.txt"

echo "Generated llms.txt ($(wc -l < "$PUBLIC_DIR/llms.txt") lines) and llms-full.txt ($(wc -l < "$PUBLIC_DIR/llms-full.txt") lines)"
