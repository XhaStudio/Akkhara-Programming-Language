#!/bin/sh
#
# install.sh -- remote installer for the Akkhara ("akk") interpreter.
#
# Usage:
#   curl -fsSL https://raw.githubusercontent.com/XhaStudio/Akkhara-Programming-Language/main/scripts/install.sh | sh
#
# What this does:
#   1. Detects your OS and CPU architecture.
#   2. Downloads the matching prebuilt "akk" binary from the latest
#      GitHub release (with a progress bar).
#   3. Installs it to $InstallDir
#      (default: ~/.local/share/akkhara/bin).
#   4. Adds that folder to your shell's PATH if it isn't already there.
#   5. Runs a quick smoke test.
#
# Env vars you can override before piping into sh:
#   AKK_REPO         "owner/repo"
#                    (default: XhaStudio/Akkhara-Programming-Language)
#   AKK_VERSION      a release tag
#                    (default: latest)
#   AKK_INSTALL_DIR  install directory
#                    (default: $HOME/.local/share/akkhara/bin)
#
# NOTE:
# This script downloads a prebuilt binary.
# The Windows-only Visual C++ Redistributable step from the PowerShell
# installer does not apply here and has been omitted.

set -eu

# ---------------------------------------------------------------------
# Output helpers
# ---------------------------------------------------------------------

if [ -t 1 ]; then
    C_CYAN=$(printf '\033[36m')
    C_GREEN=$(printf '\033[32m')
    C_YELLOW=$(printf '\033[33m')
    C_RED=$(printf '\033[31m')
    C_MAGENTA=$(printf '\033[35m')
    C_RESET=$(printf '\033[0m')
else
    C_CYAN=""; C_GREEN=""; C_YELLOW=""; C_RED=""; C_MAGENTA=""; C_RESET=""
fi

step() { printf '\n%s==> %s%s\n' "$C_CYAN" "$1" "$C_RESET"; }
ok()   { printf '    %s[OK] %s%s\n' "$C_GREEN" "$1" "$C_RESET"; }
warn() { printf '    %s[!] %s%s\n' "$C_YELLOW" "$1" "$C_RESET"; }
fail() { printf '    %s[FAILED] %s%s\n' "$C_RED" "$1" "$C_RESET" >&2; exit 1; }

# ---------------------------------------------------------------------
# Download helper -- shows a live progress bar.
# Prefers curl; falls back to wget.
# ---------------------------------------------------------------------

download() {
    local url="$1"
    local out="$2"

    if command -v curl >/dev/null 2>&1; then
        curl -fL --progress-bar -o "$out" "$url"
    elif command -v wget >/dev/null 2>&1; then
        wget -O "$out" "$url"
    else
        fail "neither curl nor wget is available; please install one and retry"
    fi
}

# ---------------------------------------------------------------------
# Config
# ---------------------------------------------------------------------

REPO="${AKK_REPO:-XhaStudio/Akkhara-Programming-Language}"
VERSION="${AKK_VERSION:-latest}"
INSTALL_DIR="${AKK_INSTALL_DIR:-$HOME/.local/share/akkhara/bin}"
BIN_NAME="akk"

printf '%sAkkhara installer (Unix)%s\n' "$C_MAGENTA" "$C_RESET"

# ---------------------------------------------------------------------
# 1. Detect platform
# ---------------------------------------------------------------------

step "Detecting platform"

os=$(uname -s)
arch=$(uname -m)

case "$os" in
    Linux)  os_part="unknown-linux-gnu" ;;
    Darwin) os_part="apple-darwin" ;;
    *)      fail "unsupported OS: $os" ;;
esac

case "$arch" in
    x86_64|amd64)  arch_part="x86_64" ;;
    arm64|aarch64) arch_part="aarch64" ;;
    *)             fail "unsupported architecture: $arch" ;;
esac

target="${arch_part}-${os_part}"

ok "Detected $target"

# ---------------------------------------------------------------------
# 2. Resolve download URL
#     GitHub's /releases/latest/download/<asset> endpoint redirects to the
#     current latest release, so we don't need to parse the JSON API.
# ---------------------------------------------------------------------

step "Looking up release"

asset_name="akk-${target}.tar.gz"

if [ "$VERSION" = "latest" ]; then
    download_url="https://github.com/${REPO}/releases/latest/download/${asset_name}"
else
    download_url="https://github.com/${REPO}/releases/download/${VERSION}/${asset_name}"
fi

ok "Will download $asset_name ($VERSION)"

# ---------------------------------------------------------------------
# 3. Download, extract, install
# ---------------------------------------------------------------------

step "Downloading akk"

tmp_dir=$(mktemp -d "${TMPDIR:-/tmp}/akkhara_install.XXXXXX")
trap 'rm -rf "$tmp_dir"' EXIT

archive_path="$tmp_dir/$asset_name"

download "$download_url" "$archive_path" || \
    fail "could not download $download_url -- no asset named '$asset_name' in release '$VERSION', or a network error. Check https://github.com/$REPO/releases"

ok "Downloaded $asset_name"

step "Extracting and installing to $INSTALL_DIR"

mkdir -p "$tmp_dir/extract"
tar -xzf "$archive_path" -C "$tmp_dir/extract"

exe_source="$tmp_dir/extract/$BIN_NAME"

if [ ! -f "$exe_source" ]; then
    fail "extracted archive did not contain $BIN_NAME"
fi

mkdir -p "$INSTALL_DIR"

dest_path="$INSTALL_DIR/$BIN_NAME"

# Write to a temp name and rename into place. Renaming is atomic and works
# even when a copy of `akk` is currently running (overwriting in place
# would fail with ETXTBSY on Linux).
cp "$exe_source" "$dest_path.new" || fail "could not write to $INSTALL_DIR -- check permissions"
chmod 755 "$dest_path.new"
mv -f "$dest_path.new" "$dest_path"

ok "Installed to $dest_path"

# akk itself also creates this on every run, but making it here too means
# it's visible right away instead of only after the first `akk` invocation.
mkdir -p "$INSTALL_DIR/libraries"

# ---------------------------------------------------------------------
# 4. Add to PATH if needed
# ---------------------------------------------------------------------

step "Checking PATH"

path_updated=0
rc_file=""

# Check if INSTALL_DIR is already on PATH (split on ':' to avoid glob issues
# if the path contains special characters).
path_has_dir=0
old_ifs=$IFS
IFS=:
for p in $PATH; do
    if [ "$p" = "$INSTALL_DIR" ]; then
        path_has_dir=1
        break
    fi
done
IFS=$old_ifs

if [ "$path_has_dir" -eq 1 ]; then
    ok "$INSTALL_DIR is already on your PATH"
else
    shell_name=$(basename "${SHELL:-sh}")

    case "$shell_name" in
        zsh)
            rc_file="$HOME/.zshrc"
            ;;
        bash)
            if [ -f "$HOME/.bashrc" ]; then
                rc_file="$HOME/.bashrc"
            elif [ -f "$HOME/.bash_profile" ]; then
                rc_file="$HOME/.bash_profile"
            else
                rc_file="$HOME/.bashrc"
            fi
            ;;
        fish)
            rc_file="$HOME/.config/fish/config.fish"
            ;;
        *)
            rc_file="$HOME/.profile"
            ;;
    esac

    mkdir -p "$(dirname "$rc_file")"

    if [ -f "$rc_file" ] && grep -qF "$INSTALL_DIR" "$rc_file" 2>/dev/null; then
        ok "$INSTALL_DIR is already referenced in $rc_file"
    else
        if [ "$shell_name" = "fish" ]; then
            printf '\n# Added by Akkhara installer\nfish_add_path %s\n' "$INSTALL_DIR" >> "$rc_file"
        else
            printf '\n# Added by Akkhara installer\nexport PATH="%s:$PATH"\n' "$INSTALL_DIR" >> "$rc_file"
        fi
        ok "Added $INSTALL_DIR to PATH in $rc_file"
        path_updated=1
    fi

    # Make it usable in the current shell too.
    PATH="$INSTALL_DIR:$PATH"
    export PATH
fi

# ---------------------------------------------------------------------
# 5. Smoke test
# ---------------------------------------------------------------------

step "Verifying install"

akk_path="$INSTALL_DIR/$BIN_NAME"

if version_output=$("$akk_path" --version 2>&1); then
    ok "akk runs correctly ($version_output)"
else
    warn "installed but 'akk --version' didn't run cleanly -- check manually"
fi

# ---------------------------------------------------------------------
# 6. Complete
# ---------------------------------------------------------------------

step "Install complete"

printf '    Run:  %sakk myprogram.akk%s\n' "$C_GREEN" "$C_RESET"

if [ "$path_updated" -eq 1 ]; then
    printf '    %sRestart your shell (or run: . %s) for PATH changes to apply.%s\n' \
        "$C_YELLOW" "$rc_file" "$C_RESET"
fi
