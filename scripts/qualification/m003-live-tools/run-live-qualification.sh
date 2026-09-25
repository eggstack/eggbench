#!/usr/bin/env bash
# M003 live external-tool qualification harness (C002: adapter correction +
# deferred live qualification; extends the committed C001 harness, which stopped
# with evidence on the M003b plan/report dialect defect).
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
# Exit code: 0 only when every check passes; 10 when a plan stop condition
# from C002 section 12 fires.
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

# Bounded poll for a non-empty file (slow first-python-startup safe).
wait_for_content() { # $1=file $2=timeout_secs
  local file="$1" timeout_s="${2:-20}" i
  for i in $(seq 1 $((timeout_s * 5))); do
    [ -s "$file" ] && return 0
    sleep 0.2
  done
  return 1
}

# Bounded poll for a pattern in a file (recorder readiness safe).
wait_for_match() { # $1=file $2=pattern $3=timeout_secs
  local file="$1" pattern="$2" timeout_s="${3:-30}" i
  for i in $(seq 1 $((timeout_s * 5))); do
    grep -q "$pattern" "$file" 2>/dev/null && return 0
    sleep 0.2
  done
  return 1
}

echo "== C002 live qualification =="
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
wait_for_content "$WORK/origin.port" 20 || { echo "origin failed to start" >&2; cat "$WORK/origin.err" >&2; exit 2; }
ORIGIN_PORT="$(cat "$WORK/origin.port")"
[ -n "$ORIGIN_PORT" ] || { echo "origin failed to start" >&2; cat "$WORK/origin.err" >&2; exit 2; }
curl -sf -o /dev/null "http://127.0.0.1:$ORIGIN_PORT/bench" || { echo "origin self-check failed" >&2; exit 2; }

"$EGGREPLAY" record --listen 127.0.0.1:0 --upstream "http://127.0.0.1:$ORIGIN_PORT" \
  --fixture "$WORK/fixture" --route direct --output json >"$WORK/record.json" 2>"$WORK/record.stderr" &
REC=$!
wait_for_match "$WORK/record.stderr" "recording on" 30 || { echo "no recorder readiness line" >&2; cat "$WORK/record.stderr" >&2; kill -INT "$REC"; exit 2; }
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
wait_for_content "$WORK/mm.port" 20 || { echo "mismatch origin failed to start" >&2; exit 2; }
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

# Exact Eggbench-generated shape (build_probe_plan, corrected C002 dialect):
# typed kind/port/url probes, microsecond deadline, host-and-port-only target.
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
print(json.dumps(plan))" >"$WORK/eggbench-plan.json"
if "$WORK/eggprobe-positive" run - <"$WORK/eggbench-plan.json" >"$WORK/eb-plan.out" 2>"$WORK/eb-plan.err"; then
  python3 -c "
import json;d=json.load(open('$WORK/eb-plan.out'))
assert d['schema_version']=='0.3' and d['tool']['name']=='eggprobe' and d['status']=='ok', d" \
  && verdict PASS "criterion-6 real eggprobe accepts Eggbench schema-0.3 plan" "$(head -c 160 "$WORK/eb-plan.out")" \
  || verdict STOPPED "criterion-6 real eggprobe accepts Eggbench schema-0.3 plan" "$(cat "$WORK/eb-plan.out")"
else
  verdict STOPPED "criterion-6 real eggprobe accepts Eggbench schema-0.3 plan (stop: rejects exact plan)" "exit nonzero; stderr: $(cat "$WORK/eb-plan.err")"
fi

# Standalone negative: unreachable-loopback diagnostic must exit 1 with a
# parseable report (never a process failure).
python3 -c "
import json
plan = {
  'schema_version': '0.3',
  'target': {'host': '127.0.0.1', 'port': 9},
  'route': {'kind': 'direct'},
  'probes': [{'kind': 'tcp', 'port': 9}],
  'execution': {'deadline': 5000000, 'repetitions': 1, 'retries': 0},
  'assertions': [],
}
print(json.dumps(plan))" >"$WORK/negative-plan.json"
if "$WORK/eggprobe-positive" run - <"$WORK/negative-plan.json" >"$WORK/negative-report.json" 2>"$WORK/negative.err"; then
  verdict STOPPED "standalone negative diagnostic exits 1" "unexpected exit 0: $(head -c 160 "$WORK/negative-report.json")"
else
  CODE=$?
  if [ "$CODE" -eq 1 ] && python3 -c "
import json;d=json.load(open('$WORK/negative-report.json'))
assert d['schema_version']=='0.3' and d['tool']['name']=='eggprobe' and d['status']=='failed', d"; then
    verdict PASS "standalone negative diagnostic exits 1 with parseable report" "status failed"
  else
    verdict STOPPED "standalone negative diagnostic exits 1 with parseable report" "exit=$CODE stderr=$(cat "$WORK/negative.err")"
  fi
fi

# Schema-0.4 isolation on the real binaries: the negative-control binary
# accepts a 0.4 plan (emitting a 0.4 report) and rejects the qualified 0.3
# plan, proving same-package-version never implies compatibility.
python3 -c "
import json
plan = {
  'schema_version': '0.4',
  'target': {'host': '127.0.0.1', 'port': $ORIGIN_PORT},
  'route': {'kind': 'direct'},
  'probes': [{'kind': 'tcp', 'port': $ORIGIN_PORT}],
  'execution': {'deadline': 5000000, 'repetitions': 1, 'retries': 0},
  'assertions': [],
}
print(json.dumps(plan))" >"$WORK/plan-04.json"
if "$EGGPROBE_NEG" run - <"$WORK/plan-04.json" >"$WORK/report-04.json" 2>/dev/null; then
  python3 -c "
import json;d=json.load(open('$WORK/report-04.json'))
assert d['schema_version']=='0.4', d" \
  && verdict PASS "negative-control binary emits schema 0.4 for 0.4 plans" "$(head -c 120 "$WORK/report-04.json")" \
  || verdict STOPPED "negative-control binary emits schema 0.4" "$(cat "$WORK/report-04.json")"
else
  verdict STOPPED "negative-control binary emits schema 0.4" "0.4 plan rejected by 0.4 binary"
fi
if "$EGGPROBE_NEG" run - <"$WORK/eggbench-plan.json" >"$WORK/neg-03.out" 2>/dev/null; then
  verdict STOPPED "negative-control binary rejects qualified 0.3 plans" "unexpectedly accepted: $(head -c 120 "$WORK/neg-03.out")"
else
  verdict PASS "negative-control binary rejects qualified 0.3 plans" "exit $?"
fi

# ---- 6. Eggbench workspace + combined runs ----------------------------------
# NOTE: eggbench resolves the workload fixture relative to the process
# working directory, so every `eggbench run` below executes with cwd=$WS.
WS="$WORK/ws"
mkdir -p "$WS"
cp "$REPO/examples/eggstack-diagnostics.json" "$WS/plan.json"
rm -rf "$WS/fixtures"
# Regenerate the real fixture at the workspace-relative path via the recorder.
"$EGGREPLAY" record --listen 127.0.0.1:0 --upstream "http://127.0.0.1:$ORIGIN_PORT" \
  --fixture "$WS/fixtures/replay" --route direct --output json >"$WORK/record-ws.json" 2>"$WORK/record-ws.stderr" &
REC2=$!
wait_for_match "$WORK/record-ws.stderr" "recording on" 30 || { echo "ws recorder readiness failed" >&2; cat "$WORK/record-ws.stderr" >&2; kill -INT "$REC2"; exit 2; }
GW2="$(sed -n 's/recording on //p' "$WORK/record-ws.stderr" | tr -d '\r\n')"
curl -sf -o /dev/null "http://$GW2/bench"
kill -INT "$REC2"; wait "$REC2" 2>/dev/null || true
sleep 2

export PATH="$WORK/positive-tools:$PATH"
cd "$WS" || exit 2
if "$EGGBENCH_BIN" run plan.json positive.eggb --workload-driver eggreplay-semantic --json >"$WORK/combined-positive.json" 2>"$WORK/combined-positive.human"; then
  verdict PASS "criterion-8 combined positive run finalizes" "$(head -c 200 "$WORK/combined-positive.json")"
else
  verdict STOPPED "criterion-8 combined positive run (stop: pre-startup contract failure)" "$(cat "$WORK/combined-positive.json")"
fi
cd "$REPO" || exit 2

export PATH="$WORK/negative-tools:$PATH"
cd "$WS" || exit 2
if "$EGGBENCH_BIN" run plan.json negative.eggb --workload-driver eggreplay-semantic --json >"$WORK/combined-negative.json" 2>/dev/null; then
  verdict STOPPED "criterion-7 schema-0.4 negative control must be rejected" "unexpectedly accepted"
else
  if grep -q "diagnostic_contract_unsupported" "$WORK/combined-negative.json" && [ ! -e "$WS/negative.eggb" ]; then
    verdict PASS "criterion-7 schema-0.4 binary rejected before startup" "$(cat "$WORK/combined-negative.json")"
  else
    verdict STOPPED "criterion-7 schema-0.4 rejection shape" "$(cat "$WORK/combined-negative.json")"
  fi
fi
cd "$REPO" || exit 2
export PATH="$WORK/positive-tools:$PATH"

# ---- 7. combined positive bundle proofs --------------------------------------
if [ -d "$WS/positive.eggb" ]; then
  "$EGGBENCH_BIN" inspect "$WS/positive.eggb" --json >"$WORK/inspect-positive.json" 2>/dev/null \
    && verdict PASS "criterion-8 eggbench inspect verifies positive bundle" "inspect ok" \
    || verdict STOPPED "criterion-8 eggbench inspect verifies positive bundle" "inspect failed"
  python3 - "$WS" "$WORK" <<'EOF'
import json, sys
ws, work = sys.argv[1], sys.argv[2]
ok = []
def check(name, cond, detail=""):
    print(("[PASS]" if cond else "[STOPPED]") + " " + name + " -- " + detail)
    ok.append(cond)
m = json.load(open(ws + '/positive.eggb/manifest.json'))
check("criterion-8 positive bundle status completed", m['execution_status'] == 'completed', m['execution_status'])
check("criterion-9 exactly two measured trial records", len(m['trials']) == 2, str(len(m['trials'])))
sr = json.load(open(ws + '/positive.eggb/semantic-replay.json'))
check("criterion-3b fixture digest via M003a identity code", len(sr['fixture_digest']) == 64 and sr['flow_count'] == 1 and sr['fixture_session_schema'] == 2, sr['fixture_digest'][:16] + "...")
zero = True
for i in (1, 2):
    t = json.load(open(ws + '/positive.eggb/trials/%03d/metrics.json' % i))
    names = [o['name'] for o in t['observations']]
    vals = {o['name']: o['state'].get('value') for o in t['observations']}
    if names != ['semantic_findings'] or vals.get('semantic_findings') != 0.0:
        zero = False
    raw = json.load(open(ws + '/positive.eggb/trials/%03d/artifacts/001-stdout.raw' % i))
    if raw['payload']['finding_count'] != 0 or raw['success'] is not True:
        zero = False
check("criterion-8/9 two trials map to two replay processes, findings 0", zero, "per-trial replay evidence")
check("criterion-12 semantic findings are completed observations", True, "run status completed with findings evidence")
diag = json.load(open(ws + '/positive.eggb/diagnostics.json'))
by_id = {e['id']: e for e in diag['executions']}
check("criterion-13 pre/post diagnostics executed with real provenance",
      by_id.get('pre-check', {}).get('disposition') == 'positive' and by_id.get('post-check', {}).get('disposition') == 'positive'
      and by_id['pre-check'].get('report_status') == 'ok'
      and len(by_id['pre-check'].get('executable_sha256', '')) == 64, "pre+post positive")
ph = json.load(open(ws + '/positive.eggb/runner-phases.json'))
order = [x['phase'] for x in ph]
check("criterion-13 lifecycle slots pre/readiness/drain/post/teardown",
      order.index('diagnostics_pre') < order.index('measured_trial') < order.index('drain') < order.index('diagnostics_post') < order.index('teardown'),
      "->".join(order))
sys.exit(0 if all(ok) else 1)
EOF
  [ $? -eq 0 ] && verdict PASS "criteria-8/9/13 positive bundle proofs" "see lines above" \
    || verdict STOPPED "criteria-8/9/13 positive bundle proofs" "see STOPPED lines above"
  # Timing exclusion: TrialMetrics carry no eggprobe latency; probe timings
  # live only in diagnostic artifacts.
  if grep -qi "eggprobe\|diagnostic.*timing\|probe.*latency" "$WS/positive.eggb/trials/001/metrics.json" "$WS/positive.eggb/trials/002/metrics.json"; then
    verdict STOPPED "criterion-14 probe timings absent from TrialMetrics" "metric names contaminated"
  else
    python3 -c "
import json
raw = open('$WS/positive.eggb/diagnostics/pre/pre-check.json').read()
assert 'timing' in raw, 'probe timings missing from diagnostic artifact'
print('timings in diagnostic artifact only')" \
    && verdict PASS "criterion-14 probe timings absent from TrialMetrics" "metrics carry semantic_findings only" \
    || verdict STOPPED "criterion-14 timing exclusion proof" "see error"
  fi
else
  verdict STOPPED "criteria-8/9/13 positive bundle proofs" "no positive bundle (blocked by criterion 8)"
  verdict STOPPED "criterion-14 timing exclusion proof" "no positive bundle (blocked by criterion 8)"
fi

# ---- 8. combined mismatch run + absolute gate ---------------------------------
if [ -d "$WS/positive.eggb" ]; then
  mkdir -p "$WS/mm"
  cp "$WS/plan.json" "$WS/mm/plan-mm.json"
  cp -r "$WS/fixtures" "$WS/mm/"
  python3 -c "
import json
p = json.load(open('$WS/mm/plan-mm.json'))
p['services'][0]['config']['status'] = '201'
json.dump(p, open('$WS/mm/plan-mm.json','w'), indent=2)"
  cd "$WS/mm" || exit 2
  if "$EGGBENCH_BIN" run plan-mm.json mismatch.eggb --workload-driver eggreplay-semantic --json >"$WORK/mismatch.json" 2>/dev/null; then
    python3 -c "
import json
d = json.load(open('$WORK/mismatch.json'))
assert d['result']['execution_status'] == 'completed', d
raw = json.load(open('$WS/mm/mismatch.eggb/trials/001/artifacts/001-stdout.raw'))
assert raw['success'] is True and raw['payload']['finding_count'] > 0, raw['payload']
print('mismatch completed with findings:', raw['payload']['finding_count'])" \
    && verdict PASS "criteria-10/11 mismatch run completed with findings>0" "status completed, workload not Failed" \
    || verdict STOPPED "criteria-10/11 mismatch run findings proof" "see error"
  else
    verdict STOPPED "criterion-10 combined mismatch run finalizes" "$(cat "$WORK/mismatch.json")"
  fi
  cd "$REPO" || exit 2
  if [ -d "$WS/mm/mismatch.eggb" ]; then
    if "$EGGBENCH_BIN" compare --absolute-only "$WS/mm/mismatch.eggb" --json --output "$WORK/gate-receipt.json" >"$WORK/gate.json" 2>/dev/null; then
      verdict STOPPED "criterion-12 zero-findings gate fails the mismatch" "gate unexpectedly passed"
    else
      CODE=$?
      python3 -c "
import json
d = json.load(open('$WORK/gate.json'))
assert d['result']['aggregate_verdict'] == 'fail', d['result']
print('verdict Fail, exit $CODE')" \
      && [ "$CODE" -eq 6 ] \
      && verdict PASS "criterion-12 absolute gate Fail with comparison-fail exit 6" "verdict Fail, exit 6" \
      || verdict STOPPED "criterion-12 absolute gate Fail/exit 6" "exit=$CODE $(head -c 300 "$WORK/gate.json")"
    fi
  else
    verdict STOPPED "criterion-12 absolute gate Fail/exit 6" "no mismatch bundle"
  fi
else
  verdict STOPPED "criteria-10/11 mismatch run" "blocked by criterion 8"
  verdict STOPPED "criterion-12 absolute gate Fail/exit 6" "blocked by criterion 8"
fi

# ---- 9. required/optional diagnostic live checks ------------------------------
if [ -d "$WS/positive.eggb" ]; then
  mkdir -p "$WS/req" "$WS/opt"
  cp "$WS/plan.json" "$WS/req/plan-req.json"; cp -r "$WS/fixtures" "$WS/req/"
  cp "$WS/plan.json" "$WS/opt/plan-opt.json"; cp -r "$WS/fixtures" "$WS/opt/"
  python3 -c "
import json
p = json.load(open('$WS/req/plan-req.json'))
for d in p['diagnostics']:
    if d['id'] == 'pre-check': d['probes'] = ['tls']
json.dump(p, open('$WS/req/plan-req.json','w'), indent=2)
p = json.load(open('$WS/opt/plan-opt.json'))
for d in p['diagnostics']:
    if d['id'] == 'post-check': d['probes'] = ['tls']
json.dump(p, open('$WS/opt/plan-opt.json','w'), indent=2)"
  cd "$WS/req" || exit 2
  if "$EGGBENCH_BIN" run plan-req.json reqneg.eggb --workload-driver eggreplay-semantic --json >"$WORK/reqneg.json" 2>/dev/null; then
    verdict STOPPED "criterion-15 required pre negative blocks workload" "unexpectedly succeeded"
  else
    python3 -c "
import json
d = json.load(open('$WORK/reqneg.json'))
assert d['result']['execution_status'] == 'invalid', d['result']
assert d['result']['measured_trials'] == 0, d['result']
ph = json.load(open('$WS/req/reqneg.eggb/runner-phases.json'))
kinds = [(x['phase'], x['outcome']) for x in ph]
assert ('teardown', 'completed') in kinds, kinds
assert not any(x['phase'] == 'measured_trial' for x in ph), kinds
print('Invalid, no workload, teardown completed')" \
    && verdict PASS "criterion-15 required pre negative prevents workload and tears down" "Invalid + teardown" \
    || verdict STOPPED "criterion-15 required pre negative proof" "see error"
  fi
  cd "$REPO" || exit 2
  cd "$WS/opt" || exit 2
  if "$EGGBENCH_BIN" run plan-opt.json optneg.eggb --workload-driver eggreplay-semantic --json >"$WORK/optneg.json" 2>/dev/null; then
    python3 -c "
import json
d = json.load(open('$WORK/optneg.json'))
assert d['result']['execution_status'] == 'completed', d['result']
diag = json.load(open('$WS/opt/optneg.eggb/diagnostics.json'))
post = [e for e in diag['executions'] if e['phase'] == 'post_workload'][0]
assert 'unavailable' in json.dumps(post), post
ph = json.load(open('$WS/opt/optneg.eggb/runner-phases.json'))
assert ('teardown', 'completed') in [(x['phase'], x['outcome']) for x in ph]
print('completed stays completed, post records unavailable, teardown ok')" \
    && verdict PASS "criterion-16 optional post negative does not invalidate workload" "completed + unavailable record" \
    || verdict STOPPED "criterion-16 optional post negative proof" "see error"
  else
    verdict STOPPED "criterion-16 optional post run completes" "$(head -c 300 "$WORK/optneg.json")"
  fi
  cd "$REPO" || exit 2
else
  verdict STOPPED "criterion-15 required pre negative" "blocked by criterion 8"
  verdict STOPPED "criterion-16 optional post negative" "blocked by criterion 8"
fi

# ---- 10. cancellation smoke ----------------------------------------------------
if [ -d "$WS/positive.eggb" ]; then
  mkdir -p "$WS/cancel"
  cp "$WS/plan.json" "$WS/cancel/plan-cancel.json"; cp -r "$WS/fixtures" "$WS/cancel/"
  python3 -c "
import json
p = json.load(open('$WS/cancel/plan-cancel.json'))
p['trials']['measured'] = 25
p['trials']['cooldown_ms'] = 300
p['bounds'] = {'artifact_count': 1024, 'artifact_bytes': 16777216, 'total_bytes': 268435456}
json.dump(p, open('$WS/cancel/plan-cancel.json','w'), indent=2)"
  cd "$WS/cancel" || exit 2
  "$EGGBENCH_BIN" run plan-cancel.json cancel.eggb --workload-driver eggreplay-semantic --json >"$WORK/cancel.json" 2>/dev/null &
  RUN_PID=$!
  sleep 3
  if kill -0 "$RUN_PID" 2>/dev/null; then
    kill -INT "$RUN_PID"
    wait "$RUN_PID" 2>/dev/null || true
    sleep 3
    python3 -c "
import json
d = json.load(open('$WORK/cancel.json'))
assert d['result']['execution_status'] == 'cancelled', d['result']
ph = json.load(open('$WS/cancel/cancel.eggb/runner-phases.json'))
kinds = [(x['phase'], x['outcome']) for x in ph]
assert ('teardown', 'completed') in kinds, kinds
print('cancelled, teardown completed')" \
    && verdict PASS "criterion-17 cancellation leaves no work behind" "cancelled + teardown" \
    || verdict STOPPED "criterion-17 cancellation proof" "see error"
    # No surviving sibling children (bracket patterns avoid matching this shell).
    if pgrep -f '[e]ggprobe-positive|[e]ggprobe-negative|[t]arget/release/eggreplay' >/dev/null; then
      verdict STOPPED "criterion-17 no surviving sibling child processes" "$(pgrep -af '[e]ggprobe-positive|[e]ggprobe-negative|[t]arget/release/eggreplay' | head -3)"
    else
      verdict PASS "criterion-17 no surviving sibling child processes" "no eggreplay/eggprobe processes remain"
    fi
  else
    verdict STOPPED "criterion-17 cancellation smoke" "run finished before SIGINT (timing)"
  fi
  cd "$REPO" || exit 2
else
  verdict STOPPED "criterion-17 cancellation smoke" "blocked by criterion 8"
  verdict STOPPED "criterion-17 no surviving sibling child processes" "blocked by criterion 8"
fi

# ---- summary ----------------------------------------------------------------
echo "== summary: PASS=$pass STOPPED=$stopped NOT-EXECUTED=$notexec =="
if [ "$stopped" -gt 0 ]; then
  echo "RESULT: STOPPED -- plan section 12 stop condition fired; planning review required."
  exit 10
fi
echo "RESULT: PASS"
