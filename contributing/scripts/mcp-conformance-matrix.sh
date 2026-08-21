#!/bin/bash
# MCP conformance matrix for the stateless migration:
#   {stdio, http} x {legacy, stateless} x {fresh, stale index}
# plus witness-debt cells (pipelined first write, cancelled-on-listen,
# HTTPS TLS, legacy-fallback reachability).
#
# Runs the release binary in scratch workspaces it seeds itself; never
# touches this repo's .codanna. Each cell reports PASS/FAIL; the script
# exits non-zero if any cell fails. Observational cells report OBSERVED
# with the outcome and never fail the run.
#
# Usage: ./contributing/scripts/mcp-conformance-matrix.sh
#   CODANNA_BIN=<path> overrides the binary under test.
#   CODANNA_LEGACY_BIN=<path> a < 0.13.0 server for the probe-death cell
#     (no default; the cell SKIPs without it).
#   CODANNA_PREVIOUS_BIN=<path> a >= 0.13.0 server for the previous-release
#     cells (default: /opt/homebrew/bin/codanna).

set -u

REPO_ROOT=$(cd "$(dirname "$0")/../.." && pwd -P)
BIN=${CODANNA_BIN:-"$REPO_ROOT/target/release/codanna"}
if [ ! -x "$BIN" ]; then
    echo "binary not found: $BIN (cargo build --release --all-features first)" >&2
    exit 2
fi

command -v timeout >/dev/null || { echo "requires 'timeout' (coreutils)" >&2; exit 2; }
command -v python3 >/dev/null || { echo "requires python3" >&2; exit 2; }

ROOT=$(mktemp -d)
trap 'rm -rf "$ROOT"' EXIT
ROOT=$(cd "$ROOT" && pwd -P)

PASS_COUNT=0
FAIL_COUNT=0
FAILED_CELLS=""

pass() { PASS_COUNT=$((PASS_COUNT + 1)); echo "PASS  $1"; }
fail() {
    FAIL_COUNT=$((FAIL_COUNT + 1))
    FAILED_CELLS="$FAILED_CELLS $1"
    echo "FAIL  $1: $2"
}

# --- workspace seeding -------------------------------------------------

seed_workspace() {
    local ws=$1
    mkdir -p "$ws/src" "$ws/.codanna"
    cat > "$ws/src/probe.rs" <<'FIXTURE'
pub fn matrix_probe_target() -> i32 {
    1
}
FIXTURE
    cat > "$ws/.codanna/settings.toml" <<SETTINGS
index_path = ".codanna/index"

[indexing]
indexed_paths = ["$ws/src"]

[semantic_search]
enabled = false
SETTINGS
    (cd "$ws" && "$BIN" index src --no-progress > /dev/null 2>&1) \
        || { echo "workspace seed failed: $ws" >&2; exit 2; }
}

tamper_stale() {
    local ws=$1
    python3 - "$ws/.codanna/index/index.meta" <<'PY'
import json, sys
path = sys.argv[1]
meta = json.load(open(path))
meta.pop("emission_version", None)
json.dump(meta, open(path, "w"), indent=2)
PY
}

# --- stdio helper ------------------------------------------------------

# stdio_session <workspace> <out-file> <extra-serve-args...> reads
# request lines from stdin, sends them to serve with an interline gap,
# and captures stdout. Returns serve's exit code.
stdio_session() {
    local ws=$1 out=$2
    shift 2
    local lines=()
    while IFS= read -r line; do lines+=("$line"); done
    (
        for line in "${lines[@]}"; do
            printf '%s\n' "$line"
            sleep 0.5
        done
        sleep 2
    ) | (cd "$ws" && timeout 30 "$BIN" serve "$@" 2>/dev/null) > "$out"
}

META='{"io.modelcontextprotocol/protocolVersion":"2026-07-28","io.modelcontextprotocol/clientCapabilities":{}}'

# assert_jsonl <out-file> <python-expr over `msgs`> — msgs is the list
# of parsed JSON lines. The expression must evaluate truthy.
assert_jsonl() {
    local out=$1 expr=$2
    python3 - "$out" "$expr" <<'PY'
import json, sys
msgs = []
for line in open(sys.argv[1]):
    line = line.strip()
    if not line:
        continue
    try:
        msgs.append(json.loads(line))
    except json.JSONDecodeError:
        pass
by_id = {m.get("id"): m for m in msgs if "id" in m}
sys.exit(0 if eval(sys.argv[2]) else 1)
PY
}

# tool_text <out-file> <id> — extract result.content[0].text for a
# tools/call response id.
tool_text() {
    python3 - "$1" "$2" <<'PY'
import json, sys
want = int(sys.argv[2])
for line in open(sys.argv[1]):
    line = line.strip()
    if not line:
        continue
    try:
        m = json.loads(line)
    except json.JSONDecodeError:
        continue
    if m.get("id") == want:
        print(m["result"]["content"][0]["text"])
        sys.exit(0)
sys.exit(1)
PY
}

echo "=== MCP conformance matrix ==="
echo "binary: $BIN"
"$BIN" --version
echo ""

# --- A: stdio x {legacy, stateless} x fresh ---------------------------

WS_FRESH="$ROOT/fresh"
seed_workspace "$WS_FRESH"

A1="$ROOT/a1-stdio-legacy-fresh.jsonl"
stdio_session "$WS_FRESH" "$A1" <<EOF
{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-11-25","capabilities":{},"clientInfo":{"name":"matrix-legacy","version":"0"}}}
{"jsonrpc":"2.0","method":"notifications/initialized"}
{"jsonrpc":"2.0","id":2,"method":"tools/list"}
{"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"name":"get_index_info","arguments":{}}}
EOF

if assert_jsonl "$A1" 'by_id[1]["result"]["serverInfo"]["name"] and len(by_id[2]["result"]["tools"]) == 9 and by_id[3]["result"]["content"][0]["text"]'; then
    pass "stdio x legacy x fresh"
else
    fail "stdio x legacy x fresh" "handshake/list/call assertions (see $A1)"
fi
if assert_jsonl "$A1" '"resultType" not in by_id[3]["result"]'; then
    pass "stdio x legacy x fresh: resultType stripped for legacy peer"
else
    fail "stdio x legacy x fresh" "legacy tools/call carries resultType"
fi

A2="$ROOT/a2-stdio-stateless-fresh.jsonl"
stdio_session "$WS_FRESH" "$A2" <<EOF
{"jsonrpc":"2.0","id":1,"method":"server/discover"}
{"jsonrpc":"2.0","id":2,"method":"tools/list","params":{"_meta":$META}}
{"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"_meta":$META,"name":"get_index_info","arguments":{}}}
EOF

if assert_jsonl "$A2" '"2026-07-28" in by_id[1]["result"]["supportedVersions"] and len(by_id[2]["result"]["tools"]) == 9 and by_id[3]["result"]["content"][0]["text"]'; then
    pass "stdio x stateless x fresh"
else
    fail "stdio x stateless x fresh" "discover/list/call assertions (see $A2)"
fi
if assert_jsonl "$A2" 'by_id[3]["result"]["resultType"] == "complete"'; then
    pass "stdio x stateless x fresh: resultType complete on the wire"
else
    fail "stdio x stateless x fresh" "stateless tools/call missing resultType"
fi

LEGACY_TEXT=$(tool_text "$A1" 3)
STATELESS_TEXT=$(tool_text "$A2" 3)
if [ -n "$LEGACY_TEXT" ] && [ "$LEGACY_TEXT" = "$STATELESS_TEXT" ]; then
    pass "stdio: tool results agree across generations"
else
    fail "stdio agreement" "legacy and stateless get_index_info text differ"
fi

# --- A: stdio x {legacy, stateless} x stale ---------------------------

WS_STALE="$ROOT/stale"
seed_workspace "$WS_STALE"
tamper_stale "$WS_STALE"

A3="$ROOT/a3-stdio-legacy-stale.jsonl"
stdio_session "$WS_STALE" "$A3" <<EOF
{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-11-25","capabilities":{},"clientInfo":{"name":"matrix-legacy-stale","version":"0"}}}
{"jsonrpc":"2.0","method":"notifications/initialized"}
EOF
A3_RC=$?

if assert_jsonl "$A3" '"INDEX STALE" in by_id[1]["result"]["instructions"] and "codanna index" in by_id[1]["result"]["instructions"]'; then
    pass "stdio x legacy x stale: heal guidance in initialize"
else
    fail "stdio x legacy x stale" "initialize lacks INDEX STALE heal guidance (see $A3)"
fi
if [ "$A3_RC" -eq 7 ]; then
    pass "stdio x legacy x stale: gate exit code 7"
else
    fail "stdio x legacy x stale" "expected exit 7, got $A3_RC"
fi

A4="$ROOT/a4-stdio-stateless-stale.jsonl"
stdio_session "$WS_STALE" "$A4" <<EOF
{"jsonrpc":"2.0","id":1,"method":"server/discover"}
EOF
A4_RC=$?

if assert_jsonl "$A4" '"INDEX STALE" in by_id[1]["result"]["instructions"] and "codanna index" in by_id[1]["result"]["instructions"]'; then
    pass "stdio x stateless x stale: heal guidance in discover"
else
    fail "stdio x stateless x stale" "discover lacks INDEX STALE heal guidance (see $A4)"
fi
if [ "$A4_RC" -eq 7 ]; then
    pass "stdio x stateless x stale: gate exit code 7"
else
    fail "stdio x stateless x stale" "expected exit 7, got $A4_RC"
fi

# --- HTTP helpers ------------------------------------------------------

free_port() {
    python3 -c 'import socket; s=socket.socket(); s.bind(("127.0.0.1",0)); print(s.getsockname()[1]); s.close()'
}

HTTP_PID=""
http_start() {
    local ws=$1 port=$2 scheme=$3
    shift 3
    # exec replaces the subshell so $! is the server itself; without it
    # http_stop kills the wrapper and the server outlives the run.
    (cd "$ws" && exec "$BIN" serve "$@" --bind "127.0.0.1:$port" > /dev/null 2>&1) &
    HTTP_PID=$!
    local i=0
    while [ $i -lt 50 ]; do
        if curl -ks "$scheme://127.0.0.1:$port/health" > /dev/null 2>&1; then
            return 0
        fi
        sleep 0.2
        i=$((i + 1))
    done
    return 1
}

http_stop() {
    [ -n "$HTTP_PID" ] && kill "$HTTP_PID" 2>/dev/null && wait "$HTTP_PID" 2>/dev/null
    HTTP_PID=""
}

# curl_mcp <scheme> <port> <method> <body> <payload-out> <headers-out> [extra -H args...]
curl_mcp() {
    local scheme=$1 port=$2 method=$3 body=$4 payload_out=$5 headers_out=$6
    shift 6
    curl -ks -X POST "$scheme://127.0.0.1:$port/mcp" \
        -H "Authorization: Bearer mcp-access-token-dummy" \
        -H "Content-Type: application/json" \
        -H "Accept: application/json, text/event-stream" \
        -H "Mcp-Method: $method" \
        "$@" \
        -D "$headers_out" \
        --data "$body" |
        python3 -c '
import json, sys
raw = sys.stdin.read()
# Legacy sessions prepend empty SSE keepalive frames ("data: " with
# id/retry fields) before the payload frame: take the first data
# frame that parses as JSON.
for line in raw.splitlines():
    if line.startswith("data: "):
        try:
            json.loads(line[6:])
        except json.JSONDecodeError:
            continue
        print(line[6:])
        sys.exit(0)
for line in raw.splitlines():
    if line.strip().startswith("{"):
        print(line.strip())
        sys.exit(0)
sys.exit(1)' > "$payload_out"
}

assert_json() {
    local file=$1 expr=$2
    python3 - "$file" "$expr" <<'PY'
import json, sys
m = json.load(open(sys.argv[1]))
sys.exit(0 if eval(sys.argv[2]) else 1)
PY
}

# --- B: http x {legacy, stateless} x fresh ----------------------------

B_PORT=$(free_port)
if http_start "$WS_FRESH" "$B_PORT" "http" --http; then
    B1_INIT="$ROOT/b1-init.json"; B1_HDRS="$ROOT/b1-init.hdrs"
    curl_mcp http "$B_PORT" initialize \
        '{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-11-25","capabilities":{},"clientInfo":{"name":"matrix-http-legacy","version":"0"}}}' \
        "$B1_INIT" "$B1_HDRS"
    SID=$(python3 -c '
import sys
for line in open(sys.argv[1]):
    k, _, v = line.partition(":")
    if k.strip().lower() == "mcp-session-id":
        print(v.strip()); break
' "$B1_HDRS")

    if [ -n "$SID" ] && assert_json "$B1_INIT" 'm["result"]["serverInfo"]["name"]'; then
        curl_mcp http "$B_PORT" notifications/initialized \
            '{"jsonrpc":"2.0","method":"notifications/initialized"}' \
            "$ROOT/b1-ack.json" "$ROOT/b1-ack.hdrs" -H "Mcp-Session-Id: $SID" || true
        B1_LIST="$ROOT/b1-list.json"
        curl_mcp http "$B_PORT" tools/list \
            '{"jsonrpc":"2.0","id":2,"method":"tools/list"}' \
            "$B1_LIST" "$ROOT/b1-list.hdrs" -H "Mcp-Session-Id: $SID"
        B1_CALL="$ROOT/b1-call.json"
        curl_mcp http "$B_PORT" tools/call \
            '{"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"name":"get_index_info","arguments":{}}}' \
            "$B1_CALL" "$ROOT/b1-call.hdrs" -H "Mcp-Session-Id: $SID"
        if assert_json "$B1_LIST" 'len(m["result"]["tools"]) == 9' \
            && assert_json "$B1_CALL" 'm["result"]["content"][0]["text"]'; then
            pass "http x legacy x fresh"
        else
            fail "http x legacy x fresh" "session list/call assertions (see $B1_LIST, $B1_CALL)"
        fi
    else
        fail "http x legacy x fresh" "initialize did not mint a session (see $B1_HDRS)"
    fi

    B2_LIST="$ROOT/b2-list.json"; B2_LIST_HDRS="$ROOT/b2-list.hdrs"
    curl_mcp http "$B_PORT" tools/list \
        "{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"tools/list\",\"params\":{\"_meta\":$META}}" \
        "$B2_LIST" "$B2_LIST_HDRS" -H "MCP-Protocol-Version: 2026-07-28"
    # Stateless HTTP requires Mcp-Name on name-bearing methods
    # (rmcp transport/common/mcp_headers.rs): -32020 without it.
    B2_CALL="$ROOT/b2-call.json"
    curl_mcp http "$B_PORT" tools/call \
        "{\"jsonrpc\":\"2.0\",\"id\":2,\"method\":\"tools/call\",\"params\":{\"_meta\":$META,\"name\":\"get_index_info\",\"arguments\":{}}}" \
        "$B2_CALL" "$ROOT/b2-call.hdrs" \
        -H "MCP-Protocol-Version: 2026-07-28" -H "Mcp-Name: get_index_info"

    if assert_json "$B2_LIST" 'len(m["result"]["tools"]) == 9' \
        && assert_json "$B2_CALL" 'm["result"]["content"][0]["text"]'; then
        pass "http x stateless x fresh"
    else
        fail "http x stateless x fresh" "sessionless list/call assertions (see $B2_LIST, $B2_CALL)"
    fi
    if python3 -c '
import sys
for line in open(sys.argv[1]):
    if line.partition(":")[0].strip().lower() == "mcp-session-id":
        sys.exit(1)
sys.exit(0)' "$B2_LIST_HDRS"; then
        pass "http x stateless x fresh: no session minted"
    else
        fail "http x stateless x fresh" "stateless request minted a session (see $B2_LIST_HDRS)"
    fi

    B1_TEXT=$(python3 -c 'import json,sys; print(json.load(open(sys.argv[1]))["result"]["content"][0]["text"])' "$B1_CALL" 2>/dev/null)
    B2_TEXT=$(python3 -c 'import json,sys; print(json.load(open(sys.argv[1]))["result"]["content"][0]["text"])' "$B2_CALL" 2>/dev/null)
    if [ -n "$B1_TEXT" ] && [ "$B1_TEXT" = "$B2_TEXT" ] && [ "$B1_TEXT" = "$LEGACY_TEXT" ]; then
        pass "http: tool results agree across generations and match stdio"
    else
        fail "http agreement" "http legacy/stateless/stdio get_index_info text differ"
    fi
else
    fail "http x fresh" "serve --http did not become healthy"
fi
http_stop

# --- B: http x stale --------------------------------------------------
# There is NO degraded HTTP mode: serve --http on a stale index
# fail-fasts with the gate exit code and heal message on stderr
# (story-bug-mcp-http-stale-no-degraded-mode owns the scenario
# divergence — the stdio lane serves degraded, the HTTP lane exits).
# This cell witnesses the fail-fast contract as shipped.

B_STALE_LOG="$ROOT/b-stale-serve.log"
(cd "$WS_STALE" && timeout 15 "$BIN" serve --http --bind "127.0.0.1:$(free_port)" > "$B_STALE_LOG" 2>&1)
B_STALE_RC=$?
if [ "$B_STALE_RC" -eq 7 ] && grep -q "codanna index" "$B_STALE_LOG"; then
    pass "http x stale: fail-fast exit 7 with heal message (no degraded HTTP mode; see story)"
else
    fail "http x stale" "expected fail-fast exit 7 with heal message, got $B_STALE_RC (see $B_STALE_LOG)"
fi

# --- C1: pipelined first write ----------------------------------------
# server/discover + a stateless request in ONE stdio write: both
# responses parse (witnesses the probe path's stdout ordering).

C1="$ROOT/c1-pipelined.jsonl"
(
    printf '%s\n%s\n' \
        '{"jsonrpc":"2.0","id":1,"method":"server/discover"}' \
        "{\"jsonrpc\":\"2.0\",\"id\":2,\"method\":\"tools/list\",\"params\":{\"_meta\":$META}}"
    sleep 2
) | (cd "$WS_FRESH" && timeout 30 "$BIN" serve 2>/dev/null) > "$C1"

if assert_jsonl "$C1" '"2026-07-28" in by_id[1]["result"]["supportedVersions"] and len(by_id[2]["result"]["tools"]) == 9'; then
    pass "pipelined first write: discover + stateless request in one write"
else
    fail "pipelined first write" "responses missing or unparseable (see $C1)"
fi

# --- C2: notifications/cancelled ends an active listen ----------------
# Cancel the listen request: the notification stream ends (a watched
# change after cancel produces nothing) and the session keeps serving.

WS_C2="$ROOT/c2"
seed_workspace "$WS_C2"
C2="$ROOT/c2-cancelled-listen.jsonl"
C2_FIFO="$ROOT/c2.fifo"
mkfifo "$C2_FIFO"
(cd "$WS_C2" && timeout 60 "$BIN" serve --watch < "$C2_FIFO" 2>/dev/null) > "$C2" &
C2_PID=$!
exec 3>"$C2_FIFO"
printf '%s\n' "{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"tools/list\",\"params\":{\"_meta\":$META}}" >&3
sleep 1
printf '%s\n' "{\"jsonrpc\":\"2.0\",\"id\":2,\"method\":\"subscriptions/listen\",\"params\":{\"_meta\":$META,\"notifications\":{\"resourcesListChanged\":true,\"resourceSubscriptions\":[\"file://src/probe.rs\"]}}}" >&3
sleep 1
printf '%s\n' '{"jsonrpc":"2.0","method":"notifications/cancelled","params":{"requestId":2}}' >&3
sleep 1
printf '%s\n' "{\"jsonrpc\":\"2.0\",\"id\":3,\"method\":\"tools/list\",\"params\":{\"_meta\":$META}}" >&3
sleep 1
printf '\npub fn c2_appendix() -> i32 {\n    2\n}\n' >> "$WS_C2/src/probe.rs"
sleep 6
exec 3>&-
wait "$C2_PID" 2>/dev/null

if assert_jsonl "$C2" 'any(m.get("method") == "notifications/subscriptions/acknowledged" for m in msgs) and len(by_id[3]["result"]["tools"]) == 9'; then
    pass "cancelled-on-listen: session keeps serving after cancel"
else
    fail "cancelled-on-listen" "ack or post-cancel request missing (see $C2)"
fi
if assert_jsonl "$C2" 'not any(str(m.get("method", "")).startswith("notifications/resources") for m in msgs)'; then
    pass "cancelled-on-listen: no notifications after cancel"
else
    fail "cancelled-on-listen" "notification leaked after cancel (see $C2)"
fi

# --- C3: HTTPS TLS cell -----------------------------------------------
# One stateless list over TLS (self-signed cert, curl -k): lifts the
# --https transport from compile-level verification to a wire witness.

C3_PORT=$(free_port)
if http_start "$WS_FRESH" "$C3_PORT" "https" --https; then
    C3="$ROOT/c3-https-list.json"
    curl_mcp https "$C3_PORT" tools/list \
        "{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"tools/list\",\"params\":{\"_meta\":$META}}" \
        "$C3" "$ROOT/c3.hdrs" -H "MCP-Protocol-Version: 2026-07-28"
    if assert_json "$C3" 'len(m["result"]["tools"]) == 9'; then
        pass "https x stateless x fresh: TLS cell"
    else
        fail "https x stateless x fresh" "sessionless list over TLS (see $C3)"
    fi
else
    fail "https x stateless x fresh" "serve --https did not become healthy"
fi
http_stop

# --- C4: cross-generation mcp-test cells ------------------------------
# mcp-test is Discover-only (no legacy initialize fallback). Two server
# generations exist on the shipped line:
#   single-generation (< 0.13.0, rmcp 2.x) dies on the pre-handshake
#     server/discover probe. Contract: the death is DIAGNOSED -- mcp-test
#     exits 1 naming the probe.
#   dual-generation (>= 0.13.0) answers the probe. Contract: the binary
#     under test completes against the previous release, and the previous
#     release's mcp-test completes against the binary under test.
# Each cell is keyed to its fixture's generation and SKIPs with a named
# reason otherwise. CODANNA_LEGACY_BIN has no default: brew autobump
# moves /opt/homebrew/bin/codanna with every release, so it is the
# previous release, never a legacy server.

server_generation() {
    local ver maj min
    ver=$("$1" --version 2>/dev/null | awk '{print $2}')
    maj=${ver%%.*}
    min=${ver#*.}
    min=${min%%.*}
    if [ "${maj:-0}" -gt 0 ] 2>/dev/null || [ "${min:-0}" -ge 13 ] 2>/dev/null; then
        echo dual
    else
        echo single
    fi
}

seed_with() {
    local bin=$1 ws=$2
    seed_workspace "$ws"
    (cd "$ws" && "$bin" index src --no-progress > /dev/null 2>&1)
}

negotiated_version() {
    grep -A1 'protocol_version' "$1" | tail -1 | tr -d ' ",'
}

LEGACY_BIN=${CODANNA_LEGACY_BIN:-}
if [ -z "$LEGACY_BIN" ] || [ ! -x "$LEGACY_BIN" ]; then
    echo "SKIP  probe-death diagnosis: set CODANNA_LEGACY_BIN to a < 0.13.0 binary"
elif [ "$(server_generation "$LEGACY_BIN")" != single ]; then
    echo "SKIP  probe-death diagnosis: $("$LEGACY_BIN" --version) is dual-generation (answers the probe); set CODANNA_LEGACY_BIN to a < 0.13.0 binary"
elif seed_with "$LEGACY_BIN" "$ROOT/c4-legacy"; then
    LEGACY_VERSION=$("$LEGACY_BIN" --version 2>/dev/null)
    C4L="$ROOT/c4-probe-death.out"
    (cd "$ROOT/c4-legacy" && timeout 30 "$BIN" mcp-test --server-binary "$LEGACY_BIN" > "$C4L" 2>&1)
    C4L_RC=$?
    if [ "$C4L_RC" -eq 1 ] && grep -q "pre-handshake server/discover probe" "$C4L"; then
        pass "probe death diagnosed against $LEGACY_VERSION"
    else
        fail "probe death diagnosis" "expected exit 1 naming the probe death against $LEGACY_VERSION, got $C4L_RC (see $C4L)"
    fi
else
    echo "SKIP  probe-death diagnosis: $LEGACY_BIN failed to seed a workspace"
fi

PREV_BIN=${CODANNA_PREVIOUS_BIN:-/opt/homebrew/bin/codanna}
if [ ! -x "$PREV_BIN" ]; then
    echo "SKIP  previous-release cells: no binary at $PREV_BIN (set CODANNA_PREVIOUS_BIN)"
elif [ "$(server_generation "$PREV_BIN")" != dual ]; then
    echo "SKIP  previous-release cells: $("$PREV_BIN" --version) is single-generation; point CODANNA_LEGACY_BIN at it instead"
elif seed_with "$PREV_BIN" "$ROOT/c4-prev"; then
    PREV_VERSION=$("$PREV_BIN" --version 2>/dev/null)
    C4P="$ROOT/c4-prev-server.out"
    (cd "$ROOT/c4-prev" && timeout 30 "$BIN" mcp-test --server-binary "$PREV_BIN" > "$C4P" 2>&1)
    C4P_RC=$?
    if [ "$C4P_RC" -eq 0 ] && grep -q '^Result:' "$C4P" && grep -q 'protocol_version' "$C4P"; then
        pass "mcp-test completes against $PREV_VERSION server (negotiated $(negotiated_version "$C4P"))"
    else
        fail "previous-release server" "expected exit 0 with tool output against $PREV_VERSION, got $C4P_RC (see $C4P)"
    fi
    C4Q="$ROOT/c4-prev-client.out"
    (cd "$ROOT/c4-prev" && timeout 30 "$PREV_BIN" mcp-test --server-binary "$BIN" > "$C4Q" 2>&1)
    C4Q_RC=$?
    if [ "$C4Q_RC" -eq 0 ] && grep -q '^Result:' "$C4Q" && grep -q 'protocol_version' "$C4Q"; then
        pass "$PREV_VERSION mcp-test completes against the binary under test (negotiated $(negotiated_version "$C4Q"))"
    else
        fail "previous-release client" "expected exit 0 with tool output from $PREV_VERSION mcp-test, got $C4Q_RC (see $C4Q)"
    fi
else
    echo "SKIP  previous-release cells: $PREV_BIN failed to seed a workspace"
fi

# --- summary -----------------------------------------------------------

echo ""
echo "=== matrix summary: $PASS_COUNT pass, $FAIL_COUNT fail ==="
[ -n "$FAILED_CELLS" ] && echo "failed:$FAILED_CELLS"
[ "$FAIL_COUNT" -eq 0 ]
