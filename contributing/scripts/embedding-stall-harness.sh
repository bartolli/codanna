#!/bin/bash
# Embedding-stall harness. Runs repeated fresh-workspace index passes with a
# wall-clock watchdog; a run past the watchdog is sampled (stack capture,
# macOS `sample`) and killed. Exit code is nonzero if any run wedged.
#
# Usage: embedding-stall-harness.sh [target-repo] [runs] [watchdog-secs]
#   target-repo    directory to index (default: this repo)
#   runs           number of runs (default: 10)
#   watchdog-secs  kill threshold; use >3x median wall time (default: 90)

set -e

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"
BIN="$REPO_ROOT/target/release/codanna"

TARGET="${1:-$REPO_ROOT}"
RUNS="${2:-10}"
WATCHDOG="${3:-90}"

if [ ! -x "$BIN" ]; then
    echo "release binary missing: $BIN (run cargo build --release)" >&2
    exit 1
fi

WORKSPACE="$(mktemp -d)"
echo "workspace: $WORKSPACE (stall samples land here)"
cd "$WORKSPACE"

wedged_total=0
for i in $(seq 1 "$RUNS"); do
    rm -rf .codanna
    "$BIN" init >/dev/null 2>&1
    start=$(date +%s)
    "$BIN" index "$TARGET" --no-progress >/dev/null 2>&1 &
    pid=$!
    wedged=0
    while kill -0 "$pid" 2>/dev/null; do
        now=$(date +%s)
        if [ $((now - start)) -ge "$WATCHDOG" ]; then
            wedged=1
            if command -v sample >/dev/null 2>&1; then
                sample "$pid" 5 -file "$WORKSPACE/stall-run$i.txt" >/dev/null 2>&1 || true
            fi
            kill -9 "$pid" 2>/dev/null || true
            break
        fi
        sleep 2
    done
    wait "$pid" 2>/dev/null || true
    end=$(date +%s)
    if [ "$wedged" -eq 1 ]; then
        wedged_total=$((wedged_total + 1))
        echo "RUN $i: WEDGED after ${WATCHDOG}s (sample: $WORKSPACE/stall-run$i.txt)"
    else
        echo "RUN $i: ok $((end - start))s"
    fi
done

echo ""
echo "$wedged_total/$RUNS wedged"
[ "$wedged_total" -eq 0 ]
