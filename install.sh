#!/usr/bin/env bash
# Kinetix installer (Linux x86_64 / aarch64).
#
#   curl -fsSL https://raw.githubusercontent.com/PrightCord/kinetix/main/install.sh | bash
#
# It installs the `kinetix` binary to ~/.local/bin, creates the XDG config/data/
# state directories, and prints the generated admin password once. Everything is
# then configurable through the CLI — no .env file, no dashboard required.
#
# Environment overrides:
#   KINETIX_VERSION   git tag/branch to install (default: main)
#   KINETIX_PREFIX    install prefix (default: ~/.local)
#   KINETIX_REPO      git URL (default: https://github.com/PrightCord/kinetix)
set -euo pipefail

REPO="${KINETIX_REPO:-https://github.com/PrightCord/kinetix}"
VERSION="${KINETIX_VERSION:-main}"
PREFIX="${KINETIX_PREFIX:-$HOME/.local}"
BIN_DIR="$PREFIX/bin"

log() { printf '\033[1;36m==>\033[0m %s\n' "$*"; }
err() { printf '\033[1;31merror:\033[0m %s\n' "$*" >&2; exit 1; }

case "$(uname -s)" in
  Linux) : ;;
  *) err "this installer currently supports Linux only (detected $(uname -s))" ;;
esac
case "$(uname -m)" in
  x86_64|aarch64) : ;;
  *) err "unsupported architecture $(uname -m) (need x86_64 or aarch64)" ;;
esac

command -v cargo >/dev/null 2>&1 || err "cargo (Rust) is required to build Kinetix; install from https://rustup.rs"
command -v git   >/dev/null 2>&1 || err "git is required"

WORK="$(mktemp -d)"
trap 'rm -rf "$WORK"' EXIT

log "Fetching Kinetix ($VERSION)"
git clone --depth 1 --branch "$VERSION" "$REPO" "$WORK/kinetix" 2>/dev/null \
  || git clone --depth 1 "$REPO" "$WORK/kinetix"

cd "$WORK/kinetix"
log "Building release binary (this may take a few minutes)"
cargo build --release --locked 2>/dev/null || cargo build --release

mkdir -p "$BIN_DIR"
install -m 0755 target/release/kinetix "$BIN_DIR/kinetix"
log "Installed $BIN_DIR/kinetix"

# Ensure ~/.local/bin is on PATH for future shells.
if ! printf '%s' ":$PATH:" | grep -q ":$BIN_DIR:"; then
  case "${SHELL:-}" in
    */zsh)  RC="$HOME/.zshrc" ;;
    */bash) RC="$HOME/.bashrc" ;;
    *)      RC="$HOME/.profile" ;;
  esac
  if [ -f "$RC" ] && ! grep -q "$BIN_DIR" "$RC" 2>/dev/null; then
    printf '\nexport PATH="%s:$PATH"\n' "$BIN_DIR" >> "$RC"
    log "Added $BIN_DIR to PATH in $RC (restart your shell or run: export PATH=\"$BIN_DIR:\$PATH\")"
  fi
fi

log "Initializing (creates config/data/state directories)"
"$BIN_DIR/kinetix" init

cat <<EOF

Next steps:
  1. Add an upstream provider and its credential:
       kinetix provider add --name "My Provider" --base-url https://.../v1 \\
         --wire-format openai --auth-scheme bearer --api-key sk-... --account-label primary
  2. Add a model it serves:
       kinetix model add --provider "My Provider" --upstream-id <model-id> --display-name "<Model>"
  3. Issue a virtual key for your client:
       kinetix key create --name "pi" --owner me
  4. Run the proxy:
       kinetix serve
     (In production, keep it on localhost behind cloudflared; see deploy/README.md.)

  Run \`kinetix --help\` for the full command surface.
EOF
