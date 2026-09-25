#!/usr/bin/env bash
# M003 live external-tool qualification harness (C001, evidence-only).
#
# Executes the Eggbench M003 adapters against REAL pinned sibling binaries:
#   EggReplay @ d39f4b794620a2d0647688a914e0e7a6be42e184
#   Eggprobe  positive v0.1.1 / 53ea53d (schema 0.3)
#   Eggprobe  negative 0ce9597aa2acad9a61c45a70c3ffaf56333cf3d5 (schema 0.4)
#
# Qualification-only: never a production dependency, never alters Eggbench
# runtime behavior, never copies sibling implementation code, never requires
# root, loopback only, no user-global installs. Cleans up all children/dirs.
#
# Verdict model (one line per acceptance item on stdout):
#   PASS / STOPPED (plan stop condition hit) / NOT-EXECUTED (blocked by stop)
# Exit code: 0 only when every executed check passes; 10 when a plan stop
# condition from section 22 fires (expected C001 outcome: adapter defect).
#
# Env overrides:
#   EGGBENCH_BIN   path to built eggbench binary (default: <repo>/target/release/eggbench)
#   EGREPLAY_REPO  git URL or local path for eggreplay (default: https://github.com/eggstack/eggreplay.git)
#   EGGPROBE_REPO  git URL or local path for eggprobe  (default: https://github.com/eggstack/eggprobe.git)
#   KEEP_WORK=1    keep the temp work dir for inspection (default: remove)
set -u
EGGREPLAY_PIN="d39f4b794620a2d0647688a914e0e7a6be42e184"
EGGPROBE_POS_TAG="v0.1.1"
EGGPROBE_POS_SHA="53ea53d14560c150d0ebc10de83eca10de37202d"
EGGPROBE_NEG_SHA="0ce9597aa2acad9a61c45a70c3ffaf56333cf3d5"

HERE="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO="$(cd "$HERE/../../.." && pwd)"
EGGBENCH_BIN="${EGGBENCH_BIN:-$REPO/target/release/eggbench}"
EGREPLAY_REPO="${EGREPLAY_REPO:-https://github.com/eggstack/eggreplay.git}"
EGGPROBE_REPO="${EGGPROBE_REPO:-https://github.com/eggstack/eggprobe.git}"

WORK="$(mktemp -d "${TMPDIR:-/tmp}/m003-live-qual.XXXXXX")"
PIDS=""
cleanup() {
  # shellcheck disable=SC2086
  for p in $PIDS; do kill "$p" 2>/dev/null || true; done
  sleep 1
  # shellcheck disable=SC2086
  for p in $PIDS; do kill -9 "$p" 2>/dev/null || true; done
  if [ "${KEEP_WORK:-0}" != "1" ]; then rm -rf "$WORK"; else echo "work kept at $WORK"; fi
}
trap cleanup EXIT
track() { PIDS="$PIDS $1"; }

pass=0; stopped=0; notexec=0
verdict() { # $1 = PASS|STOPPED|NOT-EXECUTED, $2 = label, $3 = detail
  echo "[$1] $2 -- $3"
  case "$1" in
    PASS) pass=$((pass+1)) ;;
    STOPPED) stopped=$((stopped+1)) ;;
    *) notexec=$((notexec+1)) ;;
  esac
}
need() { command -v "$1" >/dev/null 2>&1 || { echo "missing required tool: $1" >&2; exit 2; }; }
need git; need cargo; need rustc; need python3; need curl; need sha256sum

echo "== C001 live qualification =="
echo "eggbench: $EGGBENCH_BIN"
echo "work: $WORK"
[ -x "$EGGBENCH_BIN" ] || { echo "eggbench binary not executable: $EGGBENCH_BIN" >&2; exit 2; }

# ---- 1. pinned sources + builds -------------------------------------------
echo "== pinned sources =="
git clone -q "$EGREPLAY_REPO" "$WORK/eggreplay" 2>/dev/null || git clone -q "$EGREPLAY_REPO" "$WORK/eggreplay"
git -C "$WORK/eggreplay" checkout -q "$EGGREPLAY_PIN"
ER_SHA="$(git -C "$WORK/eggreplay" rev-parse HEAD)"
[ "$ER_SHA" = "$EGGREPLAY_PIN" ] || { echo "eggreplay pin mismatch: $ER_SHA" >&2; exit 2; }
ER_LOCK="$(sha256sum "$WORK/eggreplay/Cargo.lock" | cut -d' ' -f1)"
cargo build --locked --release -p eggreplay-cli --manifest-path "$WORK/eggreplay/Cargo.toml" >/dev/null 2>"$WORK/build-er.log" \
  || { tail -5 "$WORK/build-er.log" >&2; exit 2; }
EGGREPLAY="$WORK/eggreplay/target/release/eggreplay"

git clone -q "$EGGPROBE_REPO" "$WORK/eggprobe" 2>/dev/null || git clone -q "$EGGPROBE_REPO" "$WORK/eggprobe"
git -C "$WORK/eggprobe" checkout -q "$EGGPROBE_POS_TAG"
EP_SHA="$(git -C "$WORK/eggprobe" rev-parse HEAD)"
[ "$EP_SHA" = "$EGGPROBE_POS_SHA" ] || { echo "eggprobe positive pin mismatch: $EP_SHA" >&2; exit 2; }
EP_LOCK="$(sha256sum "$WORK/eggprobe/Cargo.lock" | cut -d' ' -f1)"
cargo build --locked --release -p eggprobe-cli --manifest-path "$WORK/eggprobe/Cargo.toml" >/dev/null 2>"$WORK/build-ep.log" \
  || { tail -5 "$WORK/build-ep.log" >&2; exit 2; }
EGGPROBE_POS="$WORK/eggprobe/target/release/eggprobe"
cp "$EGGPROBE_POS" "$WORK/eggprobe-positive"
git -C "$WORK/eggprobe" checkout -q "$EGGPROBE_NEG_SHA"
EPN_LOCK="$(sha256sum "$WORK/eggprobe/Cargo.lock" | cut -d' ' -f1)"
cargo build --locked --release -p eggprobe-cli --manifest-path "$WORK/eggprobe/Cargo.toml" >/dev/null 2>"$WORK/build-epn.log" \
  || { tail -5 "$WORK/build-epn.log" >&2; exit 2; }
cp "$WORK/eggprobe/target/release/eggprobe" "$WORK/eggprobe-negative"
EGGPROBE_NEG="$WORK/eggprobe-negative"
git -C "$WORK/eggprobe" checkout -q "$EGGPROBE_POS_TAG"

echo "== provenance =="
echo "eggreplay src=$ER_SHA lock=$ER_LOCK bin=$(sha256sum "$EGGREPLAY" | cut -d' ' -f1) size=$(stat -c%s "$EGGREPLAY") version=$("$EGGREPLAY" --version)"
echo "eggprobe+ src=$EP_SHA lock=$EP_LOCK bin=$(sha256sum "$WORK/eggprobe-positive" | cut -d' ' -f1) size=$(stat -c%s "$WORK/eggprobe-positive") version=$("$WORK/eggprobe-positive" --version)"
echo "eggprobe- src=$EGGPROBE_NEG_SHA lock=$EPN_LOCK bin=$(sha256sum "$EGGPROBE_NEG" | cut -d' ' -f1) size=$(stat -c%s "$EGGPROBE_NEG") version=$("$EGGPROBE_NEG" --version)"
echo "rustc: $(rustc --version) | cargo: $(cargo --version)"
verdict PASS "criterion-1/2 exact binaries built with provenance" "see lines above"

# ---- 2. isolated tool dirs --------------------------------------------------
mkdir -p "$WORK/positive-tools" "$WORK/negative-tools"
ln -sf "$EGGREPLAY" "$WORK/positive-tools/eggreplay"
ln -sf "$WORK/eggprobe-positive" "$WORK/positive-tools/eggprobe"
ln -sf "$EGGREPLAY" "$WORK/negative-tools/eggreplay"
ln -sf "$EGGPROBE_NEG" "$WORK/negative-tools/eggprobe"
export PATH="$WORK/positive-tools:$PATH"
[ "$(command -v eggreplay)" = "$WORK/positive-tools/eggreplay" ] || { echo "PATH isolation failed" >&2; exit 2; }
[ "$(command -v eggprobe)" = "$WORK/positive-tools/eggprobe" ] || { echo "PATH isolation failed" >&2; exit 2; }

# ---- 3. deterministic origin + real fixture --------------------------------
python3 "$HERE/origin_raw.py" 200 64 >"$WORK/origin.port" 2>"$WORK/origin.err" &
track $!
sleep 1
ORIGIN_PORT="$(cat "$WORK/origin.port")"
[ -n "$ORIGIN_PORT" ] || { echo "origin failed to start" >&2; cat "$WORK/origin.err" >&2; exit 2; }
curl -sf -o /dev/null "http://127.0.0.1:$ORIGIN_PORT/bench" || { echo "origin self-check failed" >&2; exit 2; }

"$EGGREPLAY" record --listen 127.0.0.1:0 --upstream "http://127.0.0.1:$ORIGIN_PORT" \
  --fixture "$WORK/fixture" --route direct --output json >"$WORK/record.json" 2>"$WORK/record.stderr" &
REC=$!
sleep 2
GATEWAY="$(sed -n 's/recording on //p' "$WORK/record.stderr" | tr -d '\r\n')"
[ -n "$GATEWAY" ] || { echo "no recorder readiness line" >&2; cat "$WORK/record.stderr" >&2; kill -INT "$REC"; exit 2; }
curl -sf -o /dev/null -w "%{http_code} %{size_download}\n" "http://$GATEWAY/bench" | grep -q "^200 64$" \
  || { echo "gateway request failed" >&2; kill -INT "$REC"; exit 2; }
kill -INT "$REC"; wait "$REC" 2>/dev/null || true
sleep 2
python3 -c "import json;d=json.load(open('$WORK/record.json'));assert d['schema_version']==1 and d['command']=='record' and d['success'] is True, d" \
  || { echo "record finalization failed"; cat "$WORK/record.json"; exit 2; }

VALIDATE_OUT="$("$EGGREPLAY" validate --fixture "$WORK/fixture" --output json)"
echo "$VALIDATE_OUT" | python3 -c "import json,sys;d=json.load(sys.stdin);assert d['schema_version']==1 and d['command']=='validate' and d['success'] is True and d['payload']['flow_count']==1 and d['payload']['schema_version'] in (1,2), d"
verdict PASS "criterion-3 real EggReplay validates fixture" "$VALIDATE_OUT"

# ---- 4. standalone replay match / mismatch ----------------------------------
MATCH_OUT="$("$EGGREPLAY" replay --fixture "$WORK/fixture" --target "http://127.0.0.1:$ORIGIN_PORT/bench" --route direct --output json)"
echo "$MATCH_OUT" | python3 -c "import json,sys;d=json.load(sys.stdin);assert d['schema_version']==1 and d['command']=='replay' and all(r['schema_version']==2 for r in d['payload']['reports']) and d['payload']['finding_count']==0, d" \
  && verdict PASS "criterion-4 matching replay yields zero findings" "$(echo "$MATCH_OUT" | head -c 200)" \
  || verdict STOPPED "criterion-4 matching replay" "$MATCH_OUT"

python3 "$HERE/origin_raw.py" 201 64 >"$WORK/mm.port" 2>/dev/null &
track $!
sleep 1
MM_PORT="$(cat "$WORK/mm.port")"
MM_OUT="$("$EGGREPLAY" replay --fixture "$WORK/fixture" --target "http://127.0.0.1:$MM_PORT/bench" --route direct --output json)"
echo "$MM_OUT" | python3 -c "import json,sys;d=json.load(sys.stdin);assert d['success'] is True and d['payload']['finding_count']>0, d" \
  && verdict PASS "criterion-5 mismatch replay yields nonzero findings" "$(echo "$MM_OUT" | head -c 200)" \
  || verdict STOPPED "criterion-5 mismatch replay" "$MM_OUT"

# ---- 5. standalone eggprobe: correct shape vs Eggbench shape ----------------
python3 -c "
import json
plan = {
  'schema_version': '0.3',
  'target': {'host': '127.0.0.1', 'port': $ORIGIN_PORT},
  'route': {'kind': 'direct'},
  'probes': [{'kind': 'tcp', 'port': $ORIGIN_PORT}, {'kind': 'http', 'url': 'http://127.0.0.1:$ORIGIN_PORT/bench'}],
  'execution': {'deadline': 5000000, 'repetitions': 1, 'retries': 0},
  'assertions': [],
}
print(json.dumps(plan))" >"$WORK/correct-plan.json"
if "$WORK/eggprobe-positive" run - <"$WORK/correct-plan.json" >"$WORK/correct-report.json" 2>"$WORK/correct.err"; then
  python3 -c "
import json;d=json.load(open('$WORK/correct-report.json'))
assert d['schema_version']=='0.3' and d['tool']['name']=='eggprobe' and d['tool']['version'] and d['route']['kind']=='direct', d
print('report status:', d['status'])" \
  && verdict PASS "reference real eggprobe v0.1.1 accepts typed schema-0.3 plan" "$(head -c 160 "$WORK/correct-report.json")" \
  || verdict STOPPED "reference real eggprobe accepts typed plan" "$(cat "$WORK/correct-report.json")"
else
  verdict STOPPED "reference real eggprobe accepts typed plan" "$(cat "$WORK/correct.err")"
fi

# Exact Eggbench-generated shape (build_probe_plan): must be accepted for C001.
python3 -c "
import json
plan = {
  'schema_version': '0.3',
  'target': {'host': '127.0.0.1', 'port': $ORIGIN_PORT, 'tls': False, 'http_url': 'http://127.0.0.1:$ORIGIN_PORT/bench'},
  'route': {'kind': 'direct'},
  'probes': ['tcp', 'http'],
  'execution': {'repetitions': 1, 'retries': 0, 'deadline_ms': 5000},
  'assertions': [],
}
print(json.dumps(plan))" >"$WORK/eggbench-plan.json"
if "$WORK/eggprobe-positive" run - <"$WORK/eggbench-plan.json" >"$WORK/eb-plan.out" 2>"$WORK/eb-plan.err"; then
  verdict PASS "criterion-6 real eggprobe accepts Eggbench schema-0.3 plan" "$(head -c 160 "$WORK/eb-plan.out")"
else
  verdict STOPPED "criterion-6 real eggprobe accepts Eggbench schema-0.3 plan (stop: rejects exact plan)" "exit nonzero; stderr: $(cat "$WORK/eb-plan.err")"
fi

# ---- 6. Eggbench workspace + combined runs ----------------------------------
WS="$WORK/ws"
mkdir -p "$WS"
cp "$REPO/examples/eggstack-diagnostics.json" "$WS/plan.json"
rm -rf "$WS/fixtures"
# Regenerate the real fixture at the workspace-relative path via the recorder.
"$EGGREPLAY" record --listen 127.0.0.1:0 --upstream "http://127.0.0.1:$ORIGIN_PORT" \
  --fixture "$WS/fixtures/replay" --route direct --output json >"$WORK/record-ws.json" 2>"$WORK/record-ws.stderr" &
REC2=$!
sleep 2
GW2="$(sed -n 's/recording on //p' "$WORK/record-ws.stderr" | tr -d '\r\n')"
curl -sf -o /dev/null "http://$GW2/bench"
kill -INT "$REC2"; wait "$REC2" 2>/dev/null || true
sleep 2

export PATH="$WORK/positive-tools:$PATH"
if "$EGGBENCH_BIN" run "$WS/plan.json" "$WS/positive.eggb" --workload-driver eggreplay-semantic --json >"$WORK/combined-positive.json" 2>"$WORK/combined-positive.human"; then
  verdict PASS "criterion-8 combined positive run finalizes" "$(head -c 200 "$WORK/combined-positive.json")"
else
  verdict STOPPED "criterion-8 combined positive run (stop: pre-startup contract failure)" "$(cat "$WORK/combined-positive.json")"
fi

export PATH="$WORK/negative-tools:$PATH"
if "$EGGBENCH_BIN" run "$WS/plan.json" "$WS/negative.eggb" --workload-driver eggreplay-semantic --json >"$WORK/combined-negative.json" 2>/dev/null; then
  verdict STOPPED "criterion-7 schema-0.4 negative control must be rejected" "unexpectedly accepted"
else
  if grep -q "diagnostic_contract_unsupported" "$WORK/combined-negative.json" && [ ! -e "$WS/negative.eggb" ]; then
    verdict PASS "criterion-7 schema-0.4 binary rejected before startup" "$(cat "$WORK/combined-negative.json")"
  else
    verdict STOPPED "criterion-7 schema-0.4 rejection shape" "$(cat "$WORK/combined-negative.json")"
  fi
fi

# ---- summary ----------------------------------------------------------------
echo "== summary: PASS=$pass STOPPED=$stopped NOT-EXECUTED=$notexec =="
echo "criteria 9-16 (trial-unit/timing/diagnostics/cancellation/gates) require a"
echo "successful combined run and are NOT-EXECUTED while criterion 6/8 stop."
if [ "$stopped" -gt 0 ]; then
  echo "RESULT: STOPPED -- plan section 22 stop condition fired; register C002."
  exit 10
fi
echo "RESULT: PASS"
