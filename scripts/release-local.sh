#!/usr/bin/env bash
# scripts/release-local.sh
# Build releases locally for x86_64-unknown-linux-gnu and aarch64-unknown-linux-gnu,
# package tarballs, compute SHA256SUMS, and optionally publish directly to GitHub Releases.
#
# Usage:
#   scripts/release-local.sh <tag> [--publish] [--draft]
# Example:
#   scripts/release-local.sh v0.1.1 --publish

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ROOT_DIR="$(cd "$SCRIPT_DIR/.." && pwd)"
DIST_DIR="$ROOT_DIR/target/dist"

log() { printf '\033[1;36m==>\033[0m %s\n' "$*"; }
err() { printf '\033[1;31m==>\033[0m %s\n' "$*" >&2; exit 1; }

if [ $# -lt 1 ]; then
  echo "Usage: $0 <tag> [--publish] [--draft]"
  echo "  <tag>        Release tag, e.g. v0.1.1"
  echo "  --publish    Upload artifacts directly to GitHub Release using gh CLI"
  echo "  --draft      Create release as draft when publishing"
  exit 1
fi

TAG="$1"
shift

PUBLISH=0
DRAFT_FLAG=""
while [ $# -gt 0 ]; do
  case "$1" in
    --publish) PUBLISH=1; shift ;;
    --draft) DRAFT_FLAG="--draft"; shift ;;
    *) err "Unknown option: $1" ;;
  esac
done

if ! printf '%s' "$TAG" | grep -qE '^v[0-9]+\.[0-9]+\.[0-9]+'; then
  err "Tag must match vX.Y.Z format (got '$TAG')"
fi

cd "$ROOT_DIR"

command -v cargo >/dev/null 2>&1 || err "cargo is required"
command -v npm >/dev/null 2>&1 || err "npm is required"
command -v tar >/dev/null 2>&1 || err "tar is required"
command -v sha256sum >/dev/null 2>&1 || err "sha256sum is required"

if [ "$PUBLISH" -eq 1 ]; then
  command -v gh >/dev/null 2>&1 || err "gh CLI is required to publish"
  gh auth status >/dev/null 2>&1 || err "gh CLI is not authenticated"
fi

TARGETS=("x86_64-unknown-linux-gnu" "aarch64-unknown-linux-gnu")

# 1. Build dashboard frontend once (rust-embed requires assets before compilation)
log "Building embedded dashboard frontend..."
(
  cd "$ROOT_DIR/dashboard"
  npm run build
)

rm -rf "$DIST_DIR"
mkdir -p "$DIST_DIR"

# 2. Build for each target
for TARGET in "${TARGETS[@]}"; do
  log "Building binary for target: $TARGET"

  if [ "$TARGET" = "x86_64-unknown-linux-gnu" ]; then
    rustup target add "$TARGET" >/dev/null 2>&1 || true
    cargo build --release --target "$TARGET"
    BIN_SRC="$ROOT_DIR/target/$TARGET/release/kinetix"
  else
    if command -v cross >/dev/null 2>&1; then
      log "Cross-compiling $TARGET via cross..."
      cross build --release --target "$TARGET"
      BIN_SRC="$ROOT_DIR/target/$TARGET/release/kinetix"
    else
      err "cross command not found. Install cross to build aarch64."
    fi
  fi

  [ -f "$BIN_SRC" ] || err "Built binary not found at $BIN_SRC"

  ARCHIVE_NAME="kinetix-$TAG-$TARGET.tar.gz"
  ARCHIVE_PATH="$DIST_DIR/$ARCHIVE_NAME"

  log "Creating archive: $ARCHIVE_NAME"
  TMP_STAGE="$(mktemp -d)"
  cp "$BIN_SRC" "$TMP_STAGE/kinetix"
  chmod 0755 "$TMP_STAGE/kinetix"
  tar -czf "$ARCHIVE_PATH" -C "$TMP_STAGE" kinetix
  rm -rf "$TMP_STAGE"

  # Single asset sha256 file
  (
    cd "$DIST_DIR"
    sha256sum "$ARCHIVE_NAME" > "$ARCHIVE_NAME.sha256"
  )
done

# 3. Create consolidated SHA256SUMS file
log "Generating canonical SHA256SUMS..."
(
  cd "$DIST_DIR"
  sha256sum kinetix-"$TAG"-*.tar.gz > SHA256SUMS
)

log "Release artifacts prepared in $DIST_DIR:"
ls -lh "$DIST_DIR"

# 4. Optional publish via gh
if [ "$PUBLISH" -eq 1 ]; then
  log "Publishing release $TAG to GitHub..."

  # Create or update git tag locally if it doesn't match HEAD
  if ! git rev-parse "$TAG" >/dev/null 2>&1; then
    log "Creating git tag $TAG..."
    git tag -a "$TAG" -m "Release $TAG"
  fi

  log "Pushing tag $TAG to origin..."
  git push origin "$TAG"

  log "Ensuring GitHub Release exists..."
  if ! gh release view "$TAG" >/dev/null 2>&1; then
    gh release create "$TAG" --title "Kinetix $TAG" --generate-notes $DRAFT_FLAG
  fi

  log "Uploading artifacts to release $TAG..."
  gh release upload "$TAG" "$DIST_DIR"/kinetix-"$TAG"-*.tar.gz "$DIST_DIR"/kinetix-"$TAG"-*.sha256 "$DIST_DIR"/SHA256SUMS --clobber

  log "Release $TAG published successfully!"
  gh release view "$TAG"
fi
