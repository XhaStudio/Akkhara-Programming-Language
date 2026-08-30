#!/bin/sh
# install.sh -- installer for the Akkhara ("akk") interpreter.
#
# Usage:
#   curl -fsSL https://akkhara-lang.dev/install.sh | sh
#
# What this does:
#   1. Detects your OS and CPU architecture.
#   2. Downloads the matching prebuilt "akk" binary from the latest
#      GitHub release.
#   3. Installs it to ~/.local/bin (or $AKK_INSTALL_DIR if set).
#   4. Tells you how to add that directory to PATH if it isn't already.
#
# Env vars you can override:
#   AKK_INSTALL_DIR   Where to put the binary (default: $HOME/.local/bin)
#   AKK_VERSION       A specific release tag to install (default: latest)

set -eu

# ---------------------------------------------------------------------
# Config -- change REPO to your actual GitHub "owner/repo"
# ---------------------------------------------------------------------
REPO="${AKK_REPO:-yourusername/akkhara}"
BIN_NAME="akk"
INSTALL_DIR="${AKK_INSTALL_DIR:-$HOME/.local/bin}"
VERSION="${AKK_VERSION:-latest}"

# ---------------------------------------------------------------------
# Helpers
# ---------------------------------------------------------------------
info()  { printf '\033[1;36m==>\033[0m %s\n' "$1"; }
ok()    { printf '    \033[1;32m[OK]\033[0m %s\n' "$1"; }
warn()  { printf '    \033[1;33m[!]\033[0m %s\n' "$1"; }
fail()  { printf '    \033[1;31m[FAILED]\033[0m %s\n' "$1"; exit 1; }

need_cmd() {
    if ! command -v "$1" >/dev/null 2>&1; then
        fail "required command '$1' not found. Please install it and re-run."
    fi
}

# ---------------------------------------------------------------------
# 1. Detect platform
# ---------------------------------------------------------------------
detect_platform() {
    _os="$(uname -s)"
    _arch="$(uname -m)"

    case "$_os" in
        Linux)  os="unknown-linux-gnu" ;;
        Darwin) os="apple-darwin" ;;
        *) fail "unsupported OS: $_os (Akkhara installer supports Linux and macOS; Windows users should use install.ps1)" ;;
    esac

    case "$_arch" in
        x86_64|amd64)  arch="x86_64" ;;
        arm64|aarch64) arch="aarch64" ;;
        *) fail "unsupported architecture: $_arch" ;;
    esac

    TARGET="${arch}-${os}"
}

# ---------------------------------------------------------------------
# 2. Resolve version + download URL
# ---------------------------------------------------------------------
resolve_download_url() {
    if [ "$VERSION" = "latest" ]; then
        API_URL="https://api.github.com/repos/${REPO}/releases/latest"
    else
        API_URL="https://api.github.com/repos/${REPO}/releases/tags/${VERSION}"
    fi

    ASSET="akk-${TARGET}.tar.gz"

    # Pull the matching asset's browser_download_url out of the release JSON
    # without requiring jq -- grep/sed is enough for this simple shape.
    DOWNLOAD_URL="$(curl -fsSL "$API_URL" \
        | grep "browser_download_url" \
        | grep "$ASSET" \
        | head -n1 \
        | sed -E 's/.*"browser_download_url": *"([^"]+)".*/\1/')"

    if [ -z "$DOWNLOAD_URL" ]; then
        fail "could not find a release asset named '$ASSET' for $REPO (version: $VERSION). Check https://github.com/${REPO}/releases"
    fi
}

# ---------------------------------------------------------------------
# 3. Download, verify, install
# ---------------------------------------------------------------------
install_binary() {
    tmp_dir="$(mktemp -d)"
    trap 'rm -rf "$tmp_dir"' EXIT

    info "Downloading akk ($TARGET) from $REPO"
    curl -fsSL "$DOWNLOAD_URL" -o "$tmp_dir/$ASSET" \
        || fail "download failed: $DOWNLOAD_URL"
    ok "Downloaded $ASSET"

    info "Extracting"
    tar -xzf "$tmp_dir/$ASSET" -C "$tmp_dir" \
        || fail "could not extract $ASSET"

    if [ ! -f "$tmp_dir/$BIN_NAME" ]; then
        fail "extracted archive did not contain a '$BIN_NAME' binary"
    fi

    mkdir -p "$INSTALL_DIR"
    mv "$tmp_dir/$BIN_NAME" "$INSTALL_DIR/$BIN_NAME"
    chmod +x "$INSTALL_DIR/$BIN_NAME"
    ok "Installed to $INSTALL_DIR/$BIN_NAME"
}

# ---------------------------------------------------------------------
# 4. PATH check
# ---------------------------------------------------------------------
check_path() {
    case ":$PATH:" in
        *":$INSTALL_DIR:"*)
            ok "$INSTALL_DIR is already on your PATH"
            ;;
        *)
            warn "$INSTALL_DIR is not on your PATH"
            shell_name="$(basename "${SHELL:-sh}")"
            case "$shell_name" in
                zsh)  profile="$HOME/.zshrc" ;;
                bash) profile="$HOME/.bashrc" ;;
                fish) profile="$HOME/.config/fish/config.fish" ;;
                *)    profile="your shell profile" ;;
            esac
            echo ""
            echo "    Add this line to $profile, then restart your shell:"
            echo ""
            echo "        export PATH=\"$INSTALL_DIR:\$PATH\""
            echo ""
            ;;
    esac
}

# ---------------------------------------------------------------------
# 5. Smoke test
# ---------------------------------------------------------------------
smoke_test() {
    if "$INSTALL_DIR/$BIN_NAME" --version >/dev/null 2>&1; then
        ok "akk runs correctly"
    else
        warn "installed but 'akk --version' didn't exit cleanly -- check manually"
    fi
}

main() {
    need_cmd curl
    need_cmd tar
    need_cmd uname
    need_cmd mktemp

    info "Installing Akkhara (akk)"
    detect_platform
    ok "Detected platform: $TARGET"

    resolve_download_url
    install_binary
    check_path
    smoke_test

    echo ""
    info "Install complete"
    echo "    Run:  akk myprogram.akk"
    echo "    (Restart your shell first if PATH was just updated above.)"
}

main "$@"
