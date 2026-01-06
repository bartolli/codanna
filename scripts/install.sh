#!/bin/sh
set -eu

# codanna installer - reads dist-manifest.json from GitHub releases

REPO="bartolli/codanna"
INSTALL_DIR="${CODANNA_INSTALL_DIR:-$HOME/.local/bin}"

say() { echo "codanna: $1"; }
err() { say "ERROR: $1" >&2; exit 1; }

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

# Main
main() {
    platform=$(detect_platform)
    version="${CODANNA_VERSION:-$(get_latest_version)}"

    say "installing codanna $version for $platform"

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

    say "installed to $INSTALL_DIR/codanna"

    # PATH check
    case ":$PATH:" in
        *":$INSTALL_DIR:"*) ;;
        *) say "note: add $INSTALL_DIR to your PATH" ;;
    esac
}

main "$@"
