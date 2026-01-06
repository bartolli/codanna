#!/bin/sh
set -eu

# codanna installer - reads dist-manifest.json from GitHub releases

REPO="bartolli/codanna"
INSTALL_DIR="${CODANNA_INSTALL_DIR:-$HOME/.local/bin}"

# Colors (respects NO_COLOR and non-terminal output)
if [ -t 1 ] && [ -z "${NO_COLOR:-}" ]; then
    GREEN='\033[0;32m'
    BLUE='\033[0;34m'
    YELLOW='\033[0;33m'
    RED='\033[0;31m'
    BOLD='\033[1m'
    RESET='\033[0m'
else
    GREEN='' BLUE='' YELLOW='' RED='' BOLD='' RESET=''
fi

say() { printf "%b\n" "${BLUE}codanna:${RESET} $1"; }
err() { printf "%b\n" "${RED}codanna: ERROR:${RESET} $1" >&2; exit 1; }

# Detect platform
detect_platform() {
    os=$(uname -s | tr '[:upper:]' '[:lower:]')
    arch=$(uname -m)

    case "$os" in
        darwin) os="macos" ;;
        linux) os="linux" ;;
        *) err "unsupported OS: $os (use install.ps1 for Windows)" ;;
    esac

    case "$arch" in
        x86_64|amd64) arch="x64" ;;
        aarch64|arm64) arch="arm64" ;;
        *) err "unsupported arch: $arch" ;;
    esac

    echo "${os}-${arch}"
}

# Get latest release tag
get_latest_version() {
    curl -sL "https://api.github.com/repos/$REPO/releases/latest" \
        | grep '"tag_name"' | head -1 | cut -d'"' -f4
}

# Check for existing installation
check_existing() {
    ver="${version#v}"  # strip 'v' prefix for display
    if command -v codanna >/dev/null 2>&1; then
        current=$(codanna --version 2>/dev/null | cut -d' ' -f2 | sed 's/^v//' || echo "unknown")
        say "updating ${BOLD}$current${RESET} -> ${GREEN}$ver${RESET}"
    else
        say "installing ${GREEN}$ver${RESET}"
    fi
}

# Main
main() {
    platform=$(detect_platform)
    version="${CODANNA_VERSION:-$(get_latest_version)}"

    check_existing
    say "platform: $platform"

    # Fetch manifest
    manifest_url="https://github.com/$REPO/releases/download/$version/dist-manifest.json"
    manifest=$(curl -sLf "$manifest_url") || err "failed to fetch manifest"

    # Find matching artifact using awk (no jq dependency)
    artifact_vars=$(echo "$manifest" | awk -v platform="$platform" '
    BEGIN { found=0 }
    /"platform"/ && index($0, platform) { p=1 }
    /"url"/ { gsub(/.*"url"[^"]*"|"[^"]*$/, ""); url=$0 }
    /"sha256"/ { gsub(/.*"sha256"[^"]*"|"[^"]*$/, ""); sha=$0 }
    /\}/ {
        if (p && url && sha) {
            print "url=\"" url "\""
            print "sha256=\"" sha "\""
            found=1
            exit
        }
        p=0; url=""; sha=""
    }
    END { if (!found) exit 1 }
    ') || err "no artifact found for $platform"

    eval "$artifact_vars"
    [ -z "${url:-}" ] && err "no artifact found for $platform"
    filename=$(basename "$url")

    # Download
    tmpdir=$(mktemp -d)
    trap 'rm -rf "$tmpdir"' EXIT

    say "downloading $filename"
    curl -sLf "$url" -o "$tmpdir/$filename" || err "download failed"

    # Verify checksum
    say "verifying checksum"
    if command -v sha256sum >/dev/null; then
        actual=$(sha256sum "$tmpdir/$filename" | cut -d' ' -f1)
    else
        actual=$(shasum -a 256 "$tmpdir/$filename" | cut -d' ' -f1)
    fi
    [ "$actual" = "$sha256" ] || err "checksum mismatch: expected $sha256, got $actual"

    # Extract
    say "extracting"
    case "$filename" in
        *.tar.xz) tar -xJf "$tmpdir/$filename" -C "$tmpdir" ;;
        *.zip) unzip -q "$tmpdir/$filename" -d "$tmpdir" ;;
    esac

    # Install
    mkdir -p "$INSTALL_DIR"
    binary=$(find "$tmpdir" -name 'codanna' -type f | head -1)
    [ -z "$binary" ] && err "binary not found in archive"
    cp "$binary" "$INSTALL_DIR/"
    chmod +x "$INSTALL_DIR/codanna" 2>/dev/null || true

    say "${GREEN}installed${RESET} to $INSTALL_DIR/codanna"

    # PATH check
    case ":$PATH:" in
        *":$INSTALL_DIR:"*) ;;
        *) say "${YELLOW}note:${RESET} add $INSTALL_DIR to your PATH" ;;
    esac
}

main "$@"
