#!/usr/bin/env bash
# M002a live SynVoid qualification harness.
#
# Two stages:
#   A. Synthetic orchestration smoke (always executes): copies the
#      checked-in qualification/synvoid/v1 workspace, patches free
#      loopback ports, and runs `eggbench qualify
#      validate/expand/run/inspect` (positive Pass) plus a temporary
#      expectation mutation (negative Fail). Proves the M002a profile
#      machinery on the hosted runner without any SynVoid checkout.
#   B. Real SynVoid reverse-proxy qualification (C002, closed contract):
#      checks out the pinned SynVoid source, builds the minimal
#      `--no-default-features` release profile, invokes the SynVoid-owned
#      qualification materializer, runs its check/configtest, verifies
#      the owner provenance pins, translates the owner corpus
#      mechanically (pinned policy, owner-declared Detect/Pass statuses),
#      executes the real profile (positive Pass), proves blocked requests
#      never reach the controlled origin via the harness-origin log,
#      runs the negative Fail proof, and proves teardown leaves nothing
#      behind. Every Stage B gate is proof-bearing: failure is STOPPED.
#   C-real. Performance/resource suite against the real proxy (C002 §5):
#      generates real perf/control/oracle scenarios from the synthetic
#      shapes (wiring + target paths only; trial/metric/gate policy
#      frozen), materializes explicit same-source baselines, and requires
#      Pass/Inconclusive, never Fail, plus the independent-oracle
#      procedure.
#
# Qualification-only: loopback only, no user-global installs, cleans up
# all children/dirs.
#
# Verdict model (one line per acceptance item on stdout):
#   PASS / STOPPED (plan stop condition hit) / NOT-EXECUTED (blocked by stop)
# Exit code: 0 when no STOPPED verdict fired; 10 otherwise.
#
# Env overrides:
#   EGGBENCH_BIN   path to built eggbench binary (default: <repo>/target/release/eggbench)
#   SYNVOID_REPO   git URL or local path for synvoid (default: https://github.com/dbowm91/synvoid.git)
#   SYNVOID_PIN    exact SynVoid source SHA (default below)
#   KEEP_WORK=1    keep the temp work dir for inspection (default: remove)
set -u
SYNVOID_PIN="${SYNVOID_PIN:-ae045481752b8f750d6e6079b185c526a09c91d5}"
POLICY_ID="${POLICY_ID:-synvoid.eggbench-qualification.v1}"

HERE="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO="$(cd "$HERE/../../.." && pwd)"
EGGBENCH_BIN="${EGGBENCH_BIN:-$REPO/target/release/eggbench}"
SYNVOID_REPO="${SYNVOID_REPO:-https://github.com/dbowm91/synvoid.git}"

WORK="$(mktemp -d "${TMPDIR:-/tmp}/m002a-live-qual.XXXXXX")"
PIDS=""
cleanup() {
  # shellcheck disable=SC2086
  for p in $PIDS; do kill "$p" 2>/dev/null || true; done
  sleep 1
  # shellcheck disable=SC2086
  for p in $PIDS; do kill -9 "$p" 2>/dev/null || true; done
  if [ "${KEEP_WORK:-0}" != "1" ]; then rm -rf "$WORK"; else echo "work kept at $WORK"; fi
  rm -f /tmp/fake-synvoid-*.delay_ms
}
trap cleanup EXIT

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
need git; need python3; need sha256sum
free_port() { python3 -c 'import socket; s=socket.socket(); s.bind(("127.0.0.1",0)); print(s.getsockname()[1])'; }

[ -x "$EGGBENCH_BIN" ] || { verdict STOPPED "eggbench binary available" "$EGGBENCH_BIN"; exit 10; }

# ---- Stage A. Synthetic orchestration smoke --------------------------------
WS="$WORK/ws"
cp -r "$REPO/qualification/synvoid/v1" "$WS" || { verdict STOPPED "stage profile workspace" "copy failed"; exit 10; }
PORT="$(free_port)"
python3 - "$WS" "$PORT" <<'EOF'
import json, glob, sys
ws, port = sys.argv[1], sys.argv[2]
for plan_path in glob.glob(ws + "/scenarios/*.json"):
    plan = json.load(open(plan_path))
    for service in plan["services"]:
        if service["name"] == "synvoid" and service["kind"]["kind"] == "command":
            service["kind"]["argv"] = ["stubs/fake_synvoid.py", "--port", port]
            host = service["http_url"].split("/")[2].split(":")[0]
            path = "/" + service["http_url"].split("/", 3)[-1]
            service["http_url"] = "http://%s:%s%s" % (host, port, path)
    json.dump(plan, open(plan_path, "w"), indent=2)
for name in ("target-config.json", "materialized/provenance.json"):
    path = ws + "/" + name
    doc = json.load(open(path))
    doc["listen"]["port"] = int(port)
    json.dump(doc, open(path, "w"), indent=2)
print("ports patched to %s" % port)
EOF
cd "$WS" || exit 2
"$EGGBENCH_BIN" qualify validate profile.json --json >/dev/null 2>&1 \
  && verdict PASS "stage-a profile validates" "qualify validate ok" \
  || { verdict STOPPED "stage-a profile validates" "validate failed"; exit 10; }
"$EGGBENCH_BIN" qualify expand profile.json --json >"$WORK/expand.json" 2>&1 \
  && verdict PASS "stage-a profile expands" "one scenario" \
  || { verdict STOPPED "stage-a profile expands" "$(head -c 200 "$WORK/expand.json")"; exit 10; }
"$EGGBENCH_BIN" qualify run profile.json --output "$WS/suite" --json >"$WORK/run.json" 2>&1
[ $? -eq 0 ] \
  && verdict PASS "stage-a positive run passes" "$(python3 -c "import json; print(json.load(open('$WORK/run.json'))['aggregate_verdict'])")" \
  || { verdict STOPPED "stage-a positive run passes" "$(head -c 300 "$WORK/run.json")"; exit 10; }
"$EGGBENCH_BIN" qualify inspect "$WS/suite/qualification-receipt.json" --json >/dev/null 2>&1 \
  && verdict PASS "stage-a receipt inspects" "inspect ok" \
  || { verdict STOPPED "stage-a receipt inspects" "inspect failed"; exit 10; }
# Negative proof: mutate one expectation, recompute the corpus identity,
# require qualification Fail.
python3 - "$WS" <<'EOF'
import json, hashlib, sys
ws = sys.argv[1]
raw_path = ws + "/corpus.json"
corpus = json.load(open(raw_path))
corpus["cases"][0]["expectation"] = {"status_exact": 404}
raw = json.dumps(corpus, indent=2).encode()
open(raw_path, "wb").write(raw)
filesha = hashlib.sha256(raw).hexdigest()
h = hashlib.sha256()
h.update(b"corpus.json"); h.update(b"\x00")
h.update(len(raw).to_bytes(8, "big")); h.update(b"\x00")
h.update(filesha.encode()); h.update(b"\x00")
plan_path = ws + "/scenarios/waf-correctness.json"
plan = json.load(open(plan_path))
plan["http_corpus_checks"][0]["corpus_sha256"] = h.hexdigest()
json.dump(plan, open(plan_path, "w"), indent=2)
print("expectation mutated")
EOF
"$EGGBENCH_BIN" qualify run profile.json --output "$WS/suite-negative" --json >"$WORK/run-negative.json" 2>&1
[ $? -eq 6 ] \
  && verdict PASS "stage-a negative mutation fails" "exit 6, correctness Fail" \
  || { verdict STOPPED "stage-a negative mutation fails" "expected exit 6"; exit 10; }
# Restore the pristine workspace: the negative proof mutates the shared
# corpus, and later stages require the committed expectations.
cp "$REPO/qualification/synvoid/v1/corpus.json" "$WS/corpus.json"
cp "$REPO/qualification/synvoid/v1/scenarios/waf-correctness.json" "$WS/scenarios/waf-correctness.json"
PORT2="$(free_port)"
python3 - "$WS" "$PORT2" <<'EOF'
import json, sys
ws, port = sys.argv[1], sys.argv[2]
plan_path = ws + "/scenarios/waf-correctness.json"
plan = json.load(open(plan_path))
for service in plan["services"]:
    if service["name"] == "synvoid":
        service["kind"]["argv"] = ["stubs/fake_synvoid.py", "--port", port]
        host = service["http_url"].split("/")[2].split(":")[0]
        path = "/" + service["http_url"].split("/", 3)[-1]
        service["http_url"] = "http://%s:%s%s" % (host, port, path)
json.dump(plan, open(plan_path, "w"), indent=2)
print("waf scenario restored on port %s" % port)
EOF
# ---- Stage C. Performance/resource suite (synthetic, M002b) --------------------
# Two-stage baseline workflow with the synthetic stand-in: same-source
# pairs prove orchestration mechanics (plan section 16). A same-source
# Fail is the section 8 stop condition, not a pass.
"$EGGBENCH_BIN" qualify run smoke.profile.json --output "$WS/suite-smoke" --json >"$WORK/smoke.json" 2>&1
[ $? -eq 0 ] \
  && verdict PASS "stage-c smoke profile passes" "5 scenarios, absolute gates" \
  || { verdict STOPPED "stage-c smoke profile passes" "$(head -c 300 "$WORK/smoke.json")"; exit 10; }
for scenario in perf-small-c1 perf-small-c8 perf-small-c32 \
    perf-large-c1 perf-large-c8 perf-large-c32 \
    control-small control-large; do
  "$EGGBENCH_BIN" run "scenarios/${scenario}.json" "baselines/${scenario}.eggb" --json >/dev/null 2>&1 \
    || { verdict STOPPED "stage-c baseline materialization" "$scenario"; exit 10; }
done
verdict PASS "stage-c explicit baselines materialized" "8 baseline bundles"
"$EGGBENCH_BIN" qualify run perf.profile.json --output "$WS/suite-perf" --json >"$WORK/perf.json" 2>&1
perf_rc=$?
perf_agg="$(python3 -c "import json; print(json.load(open('$WORK/perf.json'))['aggregate_verdict'])" 2>/dev/null)"
if { [ "$perf_rc" -eq 0 ] || [ "$perf_rc" -eq 7 ]; } && { [ "$perf_agg" = "pass" ] || [ "$perf_agg" = "inconclusive" ]; }; then
  verdict PASS "stage-c same-source perf pair" "aggregate $perf_agg (Pass/Inconclusive, never Fail)"
else
  verdict STOPPED "stage-c same-source perf pair" "aggregate $perf_agg violates section 8 repeatability"
  exit 10
fi
# Negative performance proof: qualification-only controlled delay in the
# subject harness without changing security semantics or the scenario
# plan (a plan-embedded delay would compare as incomparable drift).
# The stub reads its per-port sidecar file; the trap cleanup removes it.
echo "100" >"/tmp/fake-synvoid-${PORT}.delay_ms"
"$EGGBENCH_BIN" qualify run perf.profile.json --output "$WS/suite-perf-delayed" --json >"$WORK/perf-delayed.json" 2>&1
delayed_rc=$?
rm -f "/tmp/fake-synvoid-${PORT}.delay_ms"
[ "$delayed_rc" -eq 6 ] \
  && verdict PASS "stage-c performance-only regression fails" "exit 6 despite correctness Pass" \
  || { verdict STOPPED "stage-c performance-only regression fails" "expected exit 6, got $delayed_rc"; exit 10; }
# External-oracle procedure (deviation D4): independent driver, own baseline.
if command -v oha >/dev/null 2>&1; then
  "$EGGBENCH_BIN" run scenarios/oracle-oha-c8.json "$WS/oracle-oha-base.eggb" --workload-driver oha --json >/dev/null 2>&1 \
    && "$EGGBENCH_BIN" run scenarios/oracle-oha-c8.json "$WS/oracle-oha-cand.eggb" --workload-driver oha --json >/dev/null 2>&1 \
    && "$EGGBENCH_BIN" compare "$WS/oracle-oha-base.eggb" "$WS/oracle-oha-cand.eggb" --json >"$WORK/oracle-oha.json" 2>&1 \
    && verdict PASS "stage-c oha oracle procedure" "independent driver observation" \
    || { verdict STOPPED "stage-c oha oracle procedure" "oracle run/compare failed"; exit 10; }
else
  verdict NOT-EXECUTED "stage-c oha oracle procedure" "oha not installed"
fi
if command -v h2load >/dev/null 2>&1; then
  "$EGGBENCH_BIN" run scenarios/oracle-h2load-c8.json "$WS/oracle-h2load-cand.eggb" --workload-driver h2load --json >/dev/null 2>&1 \
    && verdict PASS "stage-c h2load oracle procedure" "independent driver observation" \
    || { verdict STOPPED "stage-c h2load oracle procedure" "oracle run failed"; exit 10; }
else
  verdict NOT-EXECUTED "stage-c h2load oracle procedure" "h2load not installed"
fi
# Gregg host telemetry: only where a qualified daemon is provisioned.
if python3 -c 'import socket; s=socket.create_connection(("127.0.0.1",11310), timeout=2); s.close()' 2>/dev/null; then
  verdict NOT-EXECUTED "stage-c gregg host telemetry" "daemon reachable but M002 profiles declare no collector (D5)"
else
  verdict NOT-EXECUTED "stage-c gregg host telemetry" "no daemon on 127.0.0.1:11310 (D5 limitation)"
fi
cd "$REPO" || exit 2

# ---- Stage B. Real SynVoid qualification (closed upstream contract) ----
# C002: the owner asset contract is closed, so every gate below is
# proof-bearing: any failure is STOPPED, never NOT-EXECUTED. Eggbench
# consumes only the owner export and never reinterprets Detect/Pass.
SYNVOID_SRC="$WORK/synvoid-src"
if ! git clone -q "$SYNVOID_REPO" "$SYNVOID_SRC" 2>/dev/null; then
  verdict STOPPED "stage-b synvoid source checkout" "clone failed for $SYNVOID_REPO"
  echo "pass=$pass stopped=$stopped notexec=$notexec"
  exit 10
fi
if ! git -C "$SYNVOID_SRC" checkout -q "$SYNVOID_PIN" 2>/dev/null; then
  verdict STOPPED "stage-b pinned synvoid revision" "$SYNVOID_PIN not found"
  echo "pass=$pass stopped=$stopped notexec=$notexec"
  exit 10
fi
verdict PASS "stage-b pinned synvoid revision" "$SYNVOID_PIN"
if [ ! -f "$SYNVOID_SRC/plans/eggbench_security_qualification_asset_contract.md" ]; then
  verdict STOPPED "stage-b upstream asset contract" \
    "plans/eggbench_security_qualification_asset_contract.md absent at $SYNVOID_PIN"
  echo "pass=$pass stopped=$stopped notexec=$notexec"
  exit 10
fi
verdict PASS "stage-b upstream asset contract" "closed contract present at $SYNVOID_PIN"
SYNBIN="$SYNVOID_SRC/target/release/synvoid"
if ! (cd "$SYNVOID_SRC" && cargo build --locked --release --no-default-features >"$WORK/synvoid-build.log" 2>&1); then
  verdict STOPPED "stage-b minimal synvoid build" "$(tail -c 200 "$WORK/synvoid-build.log")"
  echo "pass=$pass stopped=$stopped notexec=$notexec"
  exit 10
fi
[ -x "$SYNBIN" ] || { verdict STOPPED "stage-b minimal synvoid binary" "not executable"; echo "pass=$pass stopped=$stopped notexec=$notexec"; exit 10; }
SYNBIN_SHA="$(sha256sum "$SYNBIN" | cut -d' ' -f1)"
verdict PASS "stage-b minimal synvoid build" "release no-default-features sha $SYNBIN_SHA"

# Free ports are chosen first: the owner export binds them into the
# generated config, so export, harness origin, and scenario must agree.
LPORT="$(free_port)"
OPORT="$(free_port)"
EXPORT="$WORK/real-export"
if ! (cd "$SYNVOID_SRC" && cargo run -q --locked -p xtask -- eggbench-qualification export \
  --output "$EXPORT" --listen-port "$LPORT" --origin-port "$OPORT" >"$WORK/export.log" 2>&1); then
  verdict STOPPED "stage-b owner export materialized" "$(tail -c 200 "$WORK/export.log")"
  echo "pass=$pass stopped=$stopped notexec=$notexec"
  exit 10
fi
verdict PASS "stage-b owner export materialized" "listen $LPORT origin $OPORT"
if ! (cd "$SYNVOID_SRC" && cargo run -q --locked -p xtask -- eggbench-qualification check \
  --input "$EXPORT" --configtest --configtest-binary "$SYNBIN" >"$WORK/check.log" 2>&1); then
  verdict STOPPED "stage-b export check plus configtest" "$(tail -c 200 "$WORK/check.log")"
  echo "pass=$pass stopped=$stopped notexec=$notexec"
  exit 10
fi
verdict PASS "stage-b export check plus configtest" "check ok, configtest valid"
python3 - "$EXPORT/provenance.json" "$SYNVOID_PIN" "$POLICY_ID" <<'EOF'
import json, sys
prov_path, pin, policy = sys.argv[1], sys.argv[2], sys.argv[3]
p = json.load(open(prov_path))
assert p["policy_id"] == policy, p["policy_id"]
assert p["synvoid_git_sha"] == pin, p["synvoid_git_sha"]
assert p["synvoid_package_version"] == "1.1.0", p["synvoid_package_version"]
assert p["detect_status"] == 403 and p["pass_status"] == 200
assert len(p["source_fixtures"]) == 15 and len(p["excluded_fixtures"]) == 12
print("provenance asserts ok")
EOF
[ $? -eq 0 ] \
  && verdict PASS "stage-b provenance pinned" "$POLICY_ID at $SYNVOID_PIN, 15 selected / 12 excluded" \
  || { verdict STOPPED "stage-b provenance pinned" "owner provenance mismatch"; echo "pass=$pass stopped=$stopped notexec=$notexec"; exit 10; }

# Real correctness workspace: translator + routes + target-config +
# scenario/profile, all derived from the owner export.
RWS="$WORK/real-ws"
mkdir -p "$RWS"
cp "$HERE/translate-owner-corpus.py" "$HERE/controlled-origin.py" \
   "$HERE/build-real-waf-workspace.py" "$HERE/assert-origin-log.py" "$RWS/"
chmod +x "$RWS/controlled-origin.py"
python3 "$RWS/build-real-waf-workspace.py" "$EXPORT" "$RWS" "$LPORT" "$OPORT" \
  "$SYNBIN" "$EXPORT/config" "$POLICY_ID" "$SYNVOID_PIN" "$RWS/translate-owner-corpus.py" \
  >"$WORK/real-ws.log" 2>&1 \
  && verdict PASS "stage-b real workspace built" "$(tail -1 "$WORK/real-ws.log")" \
  || { verdict STOPPED "stage-b real workspace built" "$(tail -c 300 "$WORK/real-ws.log")"; echo "pass=$pass stopped=$stopped notexec=$notexec"; exit 10; }
cd "$RWS" || exit 2
"$EGGBENCH_BIN" qualify validate real-profile.json --json >/dev/null 2>&1 \
  && verdict PASS "stage-b real profile validates" "qualify validate ok" \
  || { verdict STOPPED "stage-b real profile validates" "validate failed"; echo "pass=$pass stopped=$stopped notexec=$notexec"; exit 10; }
"$EGGBENCH_BIN" qualify run real-profile.json --output "$RWS/suite" --json >"$WORK/real-run.json" 2>&1
[ $? -eq 0 ] \
  && verdict PASS "stage-b real positive run passes" "$(python3 -c "import json; print(json.load(open('$WORK/real-run.json'))['aggregate_verdict'])")" \
  || { verdict STOPPED "stage-b real positive run passes" "$(head -c 300 "$WORK/real-run.json")"; echo "pass=$pass stopped=$stopped notexec=$notexec"; exit 10; }
python3 "$RWS/assert-origin-log.py" "$RWS/corpus.json" 403 "$RWS/origin-requests.log" \
  && verdict PASS "stage-b blocked requests never reach origin" "origin-log proof" \
  || { verdict STOPPED "stage-b blocked requests never reach origin" "origin log shows a leak"; echo "pass=$pass stopped=$stopped notexec=$notexec"; exit 10; }
# Negative proof: mutate one expectation, recompute the Eggbench corpus
# identity, require correctness Fail and qualification Fail (exit 6).
python3 - "$RWS" <<'EOF'
import json, hashlib, sys
ws = sys.argv[1]
raw_path = ws + "/corpus.json"
corpus = json.load(open(raw_path))
corpus["cases"][0]["expectation"] = {"status_exact": 404}
raw = json.dumps(corpus, indent=2).encode()
open(raw_path, "wb").write(raw)
filesha = hashlib.sha256(raw).hexdigest()
h = hashlib.sha256()
h.update(b"corpus.json"); h.update(b"\x00")
h.update(len(raw).to_bytes(8, "big")); h.update(b"\x00")
h.update(filesha.encode()); h.update(b"\x00")
plan_path = ws + "/scenarios/real-waf-correctness.json"
plan = json.load(open(plan_path))
plan["http_corpus_checks"][0]["corpus_sha256"] = h.hexdigest()
json.dump(plan, open(plan_path, "w"), indent=2)
print("expectation mutated")
EOF
"$EGGBENCH_BIN" qualify run real-profile.json --output "$RWS/suite-negative" --json >"$WORK/real-run-negative.json" 2>&1
[ $? -eq 6 ] \
  && verdict PASS "stage-b real negative mutation fails" "exit 6, correctness Fail" \
  || { verdict STOPPED "stage-b real negative mutation fails" "expected exit 6"; echo "pass=$pass stopped=$stopped notexec=$notexec"; exit 10; }
# Teardown proof: no child process or bound port may survive the runs.
for port in "$LPORT" "$OPORT"; do
  if python3 -c "import socket,sys; sys.exit(0 if socket.socket().connect_ex(('127.0.0.1', $port)) == 0 else 1)" 2>/dev/null; then
    verdict STOPPED "stage-b teardown leaves no port behind" "127.0.0.1:$port still bound"
    echo "pass=$pass stopped=$stopped notexec=$notexec"
    exit 10
  fi
done
if pgrep -f "target/release/synvoid --foreground" >/dev/null 2>&1 || pgrep -f "controlled-origin.py" >/dev/null 2>&1; then
  verdict STOPPED "stage-b teardown leaves no child behind" "synvoid or origin process survives"
  echo "pass=$pass stopped=$stopped notexec=$notexec"
  exit 10
fi
verdict PASS "stage-b teardown leaves nothing behind" "ports free, no children"

# ---- Stage C-real. Performance/resource suite against the real proxy ----
# Restores the pristine translated corpus (the negative proof mutated the
# shared file) and regenerates the scenario identity before proceeding.
python3 "$RWS/build-real-waf-workspace.py" "$EXPORT" "$RWS" "$LPORT" "$OPORT" \
  "$SYNBIN" "$EXPORT/config" "$POLICY_ID" "$SYNVOID_PIN" "$RWS/translate-owner-corpus.py" \
  >/dev/null 2>&1 \
  || { verdict STOPPED "stage-c real workspace restored" "rebuild failed"; echo "pass=$pass stopped=$stopped notexec=$notexec"; exit 10; }
python3 "$HERE/generate-real-scenarios.py" \
  "$REPO/qualification/synvoid/v1/scenarios" "$RWS/scenarios" \
  "$LPORT" "$OPORT" "$SYNBIN" "$EXPORT/config" \
  >"$WORK/real-scenarios.log" 2>&1 \
  && verdict PASS "stage-c real scenarios generated" "$(tail -1 "$WORK/real-scenarios.log")" \
  || { verdict STOPPED "stage-c real scenarios generated" "$(tail -c 300 "$WORK/real-scenarios.log")"; echo "pass=$pass stopped=$stopped notexec=$notexec"; exit 10; }
python3 - "$RWS" "$POLICY_ID" "$SYNVOID_PIN" <<'EOF'
import json, sys
ws, policy, pin = sys.argv[1], sys.argv[2], sys.argv[3]
owner = "SynVoid owner export %s (%s)" % (policy, pin)
def entry(sid, plan, base=None):
    e = {"id": sid, "plan": plan}
    if base:
        e["baseline_bundle"] = base
    return e
smoke = {"schema_version": 1, "id": "synvoid-smoke-real-v1", "owner": owner,
  "scenarios": [
    entry("synvoid-waf-correctness-real", "scenarios/real-waf-correctness.json"),
    entry("synvoid-benign-small-native-c1-real", "scenarios/smoke-small-real.json"),
    entry("synvoid-large-response-native-c1-real", "scenarios/smoke-large-real.json")],
  "corpus": {"path": "corpus.json"}, "target_config": {"path": "target-config.json"}}
perf = {"schema_version": 1, "id": "synvoid-perf-real-v1", "owner": owner,
  "scenarios": [
    entry("synvoid-waf-correctness-real", "scenarios/real-waf-correctness.json"),
    entry("synvoid-benign-small-native-c1-real", "scenarios/perf-small-c1-real.json", "baselines/perf-small-c1.eggb"),
    entry("synvoid-benign-small-native-c8-real", "scenarios/perf-small-c8-real.json", "baselines/perf-small-c8.eggb"),
    entry("synvoid-benign-small-native-c32-real", "scenarios/perf-small-c32-real.json", "baselines/perf-small-c32.eggb"),
    entry("synvoid-large-response-native-c1-real", "scenarios/perf-large-c1-real.json", "baselines/perf-large-c1.eggb"),
    entry("synvoid-large-response-native-c8-real", "scenarios/perf-large-c8-real.json", "baselines/perf-large-c8.eggb"),
    entry("synvoid-large-response-native-c32-real", "scenarios/perf-large-c32-real.json", "baselines/perf-large-c32.eggb"),
    entry("origin-benign-small-native-control-real", "scenarios/control-small-real.json", "baselines/control-small.eggb"),
    entry("origin-large-response-native-control-real", "scenarios/control-large-real.json", "baselines/control-large.eggb")],
  "corpus": {"path": "corpus.json"}, "target_config": {"path": "target-config.json"}}
json.dump(smoke, open(ws + "/real-smoke.profile.json", "w"), indent=2)
json.dump(perf, open(ws + "/real-perf.profile.json", "w"), indent=2)
print("real smoke/perf profiles written")
EOF
"$EGGBENCH_BIN" qualify run real-smoke.profile.json --output "$RWS/suite-smoke" --json >"$WORK/real-smoke.json" 2>&1
[ $? -eq 0 ] \
  && verdict PASS "stage-c real smoke profile passes" "correctness + small/large proxy, absolute gates" \
  || { verdict STOPPED "stage-c real smoke profile passes" "$(head -c 300 "$WORK/real-smoke.json")"; echo "pass=$pass stopped=$stopped notexec=$notexec"; exit 10; }
mkdir -p "$RWS/baselines"
for scenario in perf-small-c1 perf-small-c8 perf-small-c32 \
    perf-large-c1 perf-large-c8 perf-large-c32 \
    control-small control-large; do
  "$EGGBENCH_BIN" run "scenarios/${scenario}-real.json" "baselines/${scenario}.eggb" --json >/dev/null 2>&1 \
    || { verdict STOPPED "stage-c real baseline materialization" "$scenario"; echo "pass=$pass stopped=$stopped notexec=$notexec"; exit 10; }
done
verdict PASS "stage-c real explicit baselines materialized" "8 baseline bundles"
"$EGGBENCH_BIN" qualify run real-perf.profile.json --output "$RWS/suite-perf" --json >"$WORK/real-perf.json" 2>&1
perf_rc=$?
perf_agg="$(python3 -c "import json; print(json.load(open('$WORK/real-perf.json'))['aggregate_verdict'])" 2>/dev/null)"
if { [ "$perf_rc" -eq 0 ] || [ "$perf_rc" -eq 7 ]; } && { [ "$perf_agg" = "pass" ] || [ "$perf_agg" = "inconclusive" ]; }; then
  verdict PASS "stage-c real same-source perf pair" "aggregate $perf_agg (Pass/Inconclusive, never Fail)"
else
  verdict STOPPED "stage-c real same-source perf pair" "aggregate $perf_agg violates repeatability"
  echo "pass=$pass stopped=$stopped notexec=$notexec"
  exit 10
fi
# External-oracle procedure (deviation D4): independent driver, own baseline.
if command -v oha >/dev/null 2>&1; then
  "$EGGBENCH_BIN" run scenarios/oracle-oha-c8-real.json "$RWS/oracle-oha-base.eggb" --workload-driver oha --json >/dev/null 2>&1 \
    && "$EGGBENCH_BIN" run scenarios/oracle-oha-c8-real.json "$RWS/oracle-oha-cand.eggb" --workload-driver oha --json >/dev/null 2>&1 \
    && "$EGGBENCH_BIN" compare "$RWS/oracle-oha-base.eggb" "$RWS/oracle-oha-cand.eggb" --json >"$WORK/real-oracle-oha.json" 2>&1 \
    && verdict PASS "stage-c real oha oracle procedure" "independent driver observation" \
    || { verdict STOPPED "stage-c real oha oracle procedure" "oracle run/compare failed"; echo "pass=$pass stopped=$stopped notexec=$notexec"; exit 10; }
else
  verdict NOT-EXECUTED "stage-c real oha oracle procedure" "oha not installed"
fi
if command -v h2load >/dev/null 2>&1; then
  "$EGGBENCH_BIN" run scenarios/oracle-h2load-c8-real.json "$RWS/oracle-h2load-cand.eggb" --workload-driver h2load --json >/dev/null 2>&1 \
    && verdict PASS "stage-c real h2load oracle procedure" "independent driver observation" \
    || { verdict STOPPED "stage-c real h2load oracle procedure" "oracle run failed"; echo "pass=$pass stopped=$stopped notexec=$notexec"; exit 10; }
else
  verdict NOT-EXECUTED "stage-c real h2load oracle procedure" "h2load not installed"
fi
cd "$REPO" || exit 2
echo "pass=$pass stopped=$stopped notexec=$notexec"
[ "$stopped" -eq 0 ] && exit 0
exit 10
