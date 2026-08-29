#!/bin/sh
# netdx installer for Linux/macOS
#
# Usage:
#   curl -fsSL https://github.com/duncanunderwood/netdx/releases/latest/download/install.sh | sh
#
# Downloads the latest netdx release tarball for the detected OS/arch,
# extracts the `netdx` binary into $HOME/.local/bin, and verifies it runs.

set -eu

REPO="duncanunderwood/netdx"
INSTALL_DIR="${NETDX_INSTALL_DIR:-$HOME/.local/bin}"

err() {
    printf 'netdx: error: %s\n' "$1" >&2
    exit 1
}

info() {
    printf 'netdx: %s\n' "$1"
}

# --- detect OS -------------------------------------------------------------

os_raw="$(uname -s)"
case "$os_raw" in
    Linux)
        os="unknown-linux-gnu"
        ;;
    Darwin)
        os="apple-darwin"
        ;;
    *)
        err "unsupported OS '$os_raw'. netdx ships prebuilt binaries for Linux and macOS only. On other platforms, install with: cargo install netdx"
        ;;
esac

# --- detect architecture -----------------------------------------------------

arch_raw="$(uname -m)"
case "$arch_raw" in
    x86_64 | amd64)
        arch="x86_64"
        ;;
    aarch64 | arm64)
        arch="aarch64"
        ;;
    *)
        err "unsupported architecture '$arch_raw'. netdx ships prebuilt binaries for x86_64 and aarch64/arm64 only. Try: cargo install netdx"
        ;;
esac

target="${arch}-${os}"
asset="netdx-${target}.tar.gz"
url="https://github.com/${REPO}/releases/latest/download/${asset}"

info "detected platform: ${target}"
info "downloading ${url}"

# --- download and extract ---------------------------------------------------

tmpdir="$(mktemp -d)"
trap 'rm -rf "$tmpdir"' EXIT INT TERM

if command -v curl >/dev/null 2>&1; then
    curl -fsSL "$url" -o "$tmpdir/$asset" || err "download failed. Does a release exist for ${target}? (${url})"
elif command -v wget >/dev/null 2>&1; then
    wget -q "$url" -O "$tmpdir/$asset" || err "download failed. Does a release exist for ${target}? (${url})"
else
    err "neither curl nor wget is available; install one and retry"
fi

tar -xzf "$tmpdir/$asset" -C "$tmpdir" || err "failed to extract ${asset}"

if [ ! -f "$tmpdir/netdx" ]; then
    # fall back to searching the archive in case it was packaged under a subdir
    found="$(find "$tmpdir" -type f -name 'netdx' | head -n 1)"
    [ -n "$found" ] || err "archive did not contain a 'netdx' binary"
    mv "$found" "$tmpdir/netdx"
fi

# --- install -----------------------------------------------------------------

mkdir -p "$INSTALL_DIR"
mv "$tmpdir/netdx" "$INSTALL_DIR/netdx"
chmod +x "$INSTALL_DIR/netdx"

info "installed to ${INSTALL_DIR}/netdx"

case ":$PATH:" in
    *":$INSTALL_DIR:"*)
        ;;
    *)
        printf '\n'
        info "${INSTALL_DIR} is not on your PATH."
        info "add it by appending this line to your shell profile (~/.bashrc, ~/.zshrc, etc.):"
        printf '\n    export PATH="%s:$PATH"\n\n' "$INSTALL_DIR"
        ;;
esac

# --- verify --------------------------------------------------------------

if "$INSTALL_DIR/netdx" --version >/dev/null 2>&1; then
    version="$("$INSTALL_DIR/netdx" --version)"
    info "install verified: ${version}"
else
    err "installed binary at ${INSTALL_DIR}/netdx failed to run 'netdx --version'"
fi

info "done. run 'netdx --help' to get started."
