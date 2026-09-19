#!/usr/bin/env bash
# Publish docs/wiki/*.md to the GitHub wiki for this repository.
#
# The wiki sources live in the main repo (docs/wiki/) so they are reviewed and
# versioned with the code. This script mirrors them into the wiki git repo.
#
# Usage:
#   scripts/publish-wiki.sh ["commit message"]
#
# Requires: git, and network access to <origin>.wiki.git. If the wiki repo does
# not exist yet, enable the wiki in the repo settings and create one page in the
# web UI first (GitHub only provisions <repo>.wiki.git after the first page).
set -euo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
SRC="$REPO_ROOT/docs/wiki"
MSG="${1:-docs(wiki): sync from docs/wiki}"

if [ ! -d "$SRC" ]; then
  echo "error: $SRC not found" >&2
  exit 1
fi

# Derive the wiki URL from origin (https://github.com/OWNER/REPO[.git]).
ORIGIN="$(git -C "$REPO_ROOT" remote get-url origin)"
ORIGIN="${ORIGIN%.git}"
SLUG="$(printf '%s' "$ORIGIN" | sed -E 's#.*github\.com[:/]##')"
WIKI_URL="https://github.com/${SLUG}.wiki.git"

WORK="$(mktemp -d)"
trap 'rm -rf "$WORK"' EXIT

echo "==> cloning $WIKI_URL"
if ! git clone --depth 1 "$WIKI_URL" "$WORK/wiki" 2>/dev/null; then
  echo "error: could not clone $WIKI_URL" >&2
  echo "       enable the wiki and create the first page in the GitHub web UI," >&2
  echo "       then re-run this script." >&2
  exit 1
fi

# Mirror: copy every page, remove pages that no longer exist in docs/wiki.
rm -f "$WORK"/wiki/*.md
cp "$SRC"/*.md "$WORK"/wiki/

cd "$WORK/wiki"
if [ -z "$(git status --porcelain)" ]; then
  echo "==> wiki already up to date"
  exit 0
fi

git add -A
git commit -q -m "$MSG"
git push -q origin HEAD
echo "==> published $(ls -1 *.md | wc -l) pages to $WIKI_URL"
