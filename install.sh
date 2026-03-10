#!/bin/sh
set -e

# ─────────────────────────────────────────────
#  todo — installer
#  Usage:  curl -fsSL https://raw.githubusercontent.com/Yasar195/mcply/release/install.sh | sh
#          curl -fsSL ... | sh -s -- --version v1.2.3
#          curl -fsSL ... | sh -s -- --install-dir ~/.local/bin
# ─────────────────────────────────────────────

REPO="Yasar195/mcply"          # ← change this
BINARY="mcply"
INSTALL_DIR="/usr/local/bin"
VERSION=""                          # empty = latest

# ── parse flags ──────────────────────────────
while [ $# -gt 0 ]; do
  case "$1" in
    --version)    VERSION="$2"; shift 2 ;;
    --install-dir) INSTALL_DIR="$2"; shift 2 ;;
    *) echo "Unknown option: $1" >&2; exit 1 ;;
  esac
done

# ── helpers ───────────────────────────────────
info()  { printf '\033[1;34m[info]\033[0m  %s\n' "$*"; }
ok()    { printf '\033[1;32m[ ok ]\033[0m  %s\n' "$*"; }
err()   { printf '\033[1;31m[err ]\033[0m  %s\n' "$*" >&2; exit 1; }

need() {
  command -v "$1" >/dev/null 2>&1 || err "Required tool not found: $1"
}

# ── detect downloader ─────────────────────────
if command -v curl >/dev/null 2>&1; then
  fetch() { curl -fsSL "$1" -o "$2"; }
  fetch_stdout() { curl -fsSL "$1"; }
elif command -v wget >/dev/null 2>&1; then
  fetch() { wget -qO "$2" "$1"; }
  fetch_stdout() { wget -qO- "$1"; }
else
  err "Neither curl nor wget found. Please install one and retry."
fi

# ── detect OS ─────────────────────────────────
OS="$(uname -s)"
case "$OS" in
  Linux)  OS="linux" ;;
  Darwin) OS="macos" ;;
  *)      err "Unsupported OS: $OS" ;;
esac

# ── detect arch ───────────────────────────────
ARCH="$(uname -m)"
case "$ARCH" in
  x86_64 | amd64)  ARCH="amd64" ;;
  arm64  | aarch64) ARCH="arm64" ;;
  *)  err "Unsupported architecture: $ARCH" ;;
esac

# ── musl vs glibc (Linux only) ─────────────────
VARIANT=""
if [ "$OS" = "linux" ] && [ "$ARCH" = "amd64" ]; then
  # prefer musl if ldd isn't available or it reports musl
  if ldd --version 2>&1 | grep -qi musl || ! command -v ldd >/dev/null 2>&1; then
    VARIANT="-musl"
  fi
fi

# ── resolve version ───────────────────────────
if [ -z "$VERSION" ]; then
  info "Fetching latest release tag…"
  VERSION="$(fetch_stdout "https://api.github.com/repos/${REPO}/releases/latest" \
    | grep '"tag_name"' | sed -E 's/.*"tag_name": *"([^"]+)".*/\1/')"
  [ -n "$VERSION" ] || err "Could not determine latest version."
fi

info "Installing $BINARY $VERSION ($OS-$ARCH)"

# ── build asset name & URL ────────────────────
ASSET="${BINARY}-${OS}${VARIANT}-${ARCH}.tar.gz"
URL="https://github.com/${REPO}/releases/download/${VERSION}/${ASSET}"

# ── download & extract ────────────────────────
TMP="$(mktemp -d)"
trap 'rm -rf "$TMP"' EXIT

info "Downloading $URL"
fetch "$URL" "$TMP/$ASSET"

info "Extracting…"
tar -xzf "$TMP/$ASSET" -C "$TMP"

# ── install ───────────────────────────────────
mkdir -p "$INSTALL_DIR"

DEST="$INSTALL_DIR/$BINARY"

if [ -w "$INSTALL_DIR" ]; then
  mv "$TMP/$BINARY" "$DEST"
else
  info "Requesting elevated privileges to write to $INSTALL_DIR …"
  sudo mv "$TMP/$BINARY" "$DEST"
fi

chmod +x "$DEST"

ok "$BINARY installed → $DEST"
info "Run '$BINARY --help' to get started."

# ── PATH hint ─────────────────────────────────
case ":$PATH:" in
  *":$INSTALL_DIR:"*) ;;
  *) info "Note: $INSTALL_DIR is not in your PATH. Add it with:"
     printf '  export PATH="%s:$PATH"\n' "$INSTALL_DIR" ;;
esac
