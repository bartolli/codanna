#!/usr/bin/env bash
# Run a homebrew-core formula for codanna through Homebrew's own CI shape
# before opening the pull request: style, from-source install, test, audit.
#
# Usage:
#   ./contributing/scripts/brew-formula-check.sh <formula.rb> [--out DIR] [--tarball FILE]
#
# The formula is the one that will be submitted (GitHub url, sha256 may be a
# placeholder). The script does not modify it: a copy with the url pointing at
# a local tarball of the COMMITTED tree (git archive HEAD) and that tarball's
# sha256 is what runs inside the container. Pass --tarball to check a release
# tarball instead.
#
# Everything below is a rule earned at the v0.17.1 dry run; each one is a check
# here, not advice:
# - `brew update --force` runs first: the ghcr.io/homebrew/brew:main image can
#   carry a brew months behind main, and under an old brew a formula's `def
#   fetch` hook never runs, so the sandboxed `cargo install` is denied the
#   registry and the failure reads like DNS. It is the image, not DNS.
# - `brew tap homebrew/core` without --force; --force fails on a remote-case
#   mismatch inside the image.
# - The formula copy carries no `version` line: the tarball name carries the
#   version and `brew audit --strict` flags the redundancy.
# - The network-free build is proven by construction: `std_cargo_args` adds
#   `--offline` whenever a `fetch` hook is defined, so `cargo install` can only
#   use what `cargo fetch` cached. The script asserts both the fetch log and the
#   `--offline` argument. Homebrew's Linux sandbox (bubblewrap, Landlock) is
#   unavailable under Docker's default profile and under the OrbStack kernel,
#   so sandbox enforcement itself is only proven on real CI.
# - The installed binary must link libonnxruntime from the onnxruntime formula
#   and nothing named ssl.
# The image is amd64 only; on an arm64 host it runs emulated (a codanna build
# takes about three minutes). Exit status is nonzero if any step or check fails.
set -euo pipefail

usage() { sed -n '2,/^set -euo/p' "$0" | sed '$d' | sed 's/^# \{0,1\}//'; }

FORMULA=""
OUT=""
TARBALL=""
while [ $# -gt 0 ]; do
  case "$1" in
    --out) OUT="$2"; shift 2 ;;
    --tarball) TARBALL="$2"; shift 2 ;;
    -h|--help) usage; exit 0 ;;
    *) FORMULA="$1"; shift ;;
  esac
done
[ -n "$FORMULA" ] || { usage; exit 2; }
[ -f "$FORMULA" ] || { echo "no such formula file: $FORMULA" >&2; exit 2; }
command -v docker >/dev/null || { echo "requires docker" >&2; exit 2; }
docker info >/dev/null 2>&1 || { echo "docker daemon not running" >&2; exit 2; }

REPO="$(cd "$(dirname "$0")/../.." && pwd -P)"
NAME="$(sed -n 's/^class \([A-Za-z0-9]*\) < Formula/\1/p' "$FORMULA" | tr '[:upper:]' '[:lower:]')"
[ -n "$NAME" ] || { echo "cannot read the formula class name from $FORMULA" >&2; exit 2; }
VERSION="$(sed -E -n 's/.*\/v?([0-9][0-9.]*)\.tar\.(gz|xz)".*/\1/p' "$FORMULA" | head -1)"
[ -n "$VERSION" ] || { echo "cannot read the version from the url in $FORMULA" >&2; exit 2; }
OUT="${OUT:-/private/tmp/brew-formula-check-$NAME}"
rm -rf "$OUT"
mkdir -p "$OUT/src" "$OUT/logs"

if [ -n "$TARBALL" ]; then
  cp "$TARBALL" "$OUT/src/$NAME-$VERSION.tar.gz"
else
  git -C "$REPO" archive --format=tar.gz --prefix="$NAME-$VERSION/" -o "$OUT/src/$NAME-$VERSION.tar.gz" HEAD
  echo "tarball: git archive of $(git -C "$REPO" rev-parse --short HEAD) (committed tree only)"
fi
SHA="$(shasum -a 256 "$OUT/src/$NAME-$VERSION.tar.gz" | cut -d' ' -f1)"

# Local copy: file:// url, real sha256, no version line.
sed -e "s|^\(  url \).*|\1\"file:///src/$NAME-$VERSION.tar.gz\"|" \
    -e "s|^\(  sha256 \).*|\1\"$SHA\"|" \
    -e '/^  version "/d' \
    "$FORMULA" > "$OUT/src/$NAME.rb"
echo "formula copy: $OUT/src/$NAME.rb"

LETTER="${NAME:0:1}"
set +e
docker run --rm -v "$OUT/src":/src:ro -v "$OUT/logs":/out "ghcr.io/homebrew/brew:main" bash -c '
  set -u
  name=$1; letter=$2
  export HOMEBREW_NO_INSTALL_FROM_API=1 HOMEBREW_NO_ANALYTICS=1 HOMEBREW_DEVELOPER=1
  fail=0
  step() { echo; echo "### $1"; }
  check() { if "$@"; then echo "CHECK ok: $*"; else echo "CHECK FAILED: $*"; fail=1; fi; }

  step "brew update (the image may be months behind main)"
  echo "before: $(brew --version | head -1)"
  brew update --force > /out/update.log 2>&1 || { echo "brew update failed, see update.log"; exit 3; }
  echo "after:  $(brew --version | head -1)"
  export HOMEBREW_NO_AUTO_UPDATE=1

  step "core tap and formula copy"
  brew tap homebrew/core > /out/tap.log 2>&1 || true
  target="$(brew --repository homebrew/core)/Formula/$letter/$name.rb"
  cp "/src/$name.rb" "$target" && echo "copied to $target"

  step "brew style"
  brew style "homebrew/core/$name" > /out/style.log 2>&1; rc=$?; tail -1 /out/style.log; check [ "$rc" -eq 0 ]

  step "brew install --build-from-source (fetch hook, then cargo install --offline)"
  brew install --build-from-source --verbose "$name" > /out/install.log 2>&1; rc=$?
  echo "exit $rc"; check [ "$rc" -eq 0 ]
  logdir="$HOME/.cache/Homebrew/Logs/$name"
  cp "$logdir"/* /out/ 2>/dev/null || true
  check ls "$logdir"/fetch.*.cargo.log
  check grep -q -- "--offline" "$logdir"/01.cargo.log

  step "brew test"
  brew test --verbose "$name" > /out/test.log 2>&1; rc=$?; echo "exit $rc"; check [ "$rc" -eq 0 ]

  step "brew audit --strict"
  brew audit --strict "$name" > /out/audit.log 2>&1; rc=$?; tail -3 /out/audit.log; check [ "$rc" -eq 0 ]

  step "installed binary"
  bin="$(brew --prefix "$name")/bin/$name"
  "$bin" --version || true
  ldd "$bin" > /out/ldd.log 2>&1 || true
  check grep -q "opt/onnxruntime/lib/libonnxruntime" /out/ldd.log
  check sh -c "! grep -qi ssl /out/ldd.log"

  echo
  if [ "$fail" -eq 0 ]; then echo "RESULT: all steps and checks passed"; else echo "RESULT: FAILED (see /out logs)"; fi
  exit "$fail"
' bash "$NAME" "$LETTER" 2>&1 | grep -v "requested image's platform"
STATUS=${PIPESTATUS[0]}
set -e
echo "logs: $OUT/logs"
exit "$STATUS"
