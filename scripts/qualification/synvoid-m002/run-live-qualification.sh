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
#   B. Real SynVoid reverse-proxy qualification (plan section 14):
#      checks out the pinned SynVoid source, builds the minimal
#      `--no-default-features` profile, invokes the SynVoid-owned
#      qualification materializer, runs its check/configtest, executes
#      the profile, verifies positive Pass plus the negative Fail
#      proof, and cleans up. Stage B requires the closed SynVoid-owned
#      asset contract
#      (dbowm91/synvoid:plans/eggbench_security_qualification_asset_contract.md).
#      While that contract is open the harness reports NOT-EXECUTED for
#      stage B and exits 0, so the CI job stays green-in-form without
#      claiming live qualification that never happened.
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
SYNVOID_PIN="${SYNVOID_PIN:-7f1b79452a683e758e0b4ea1e70f6c0f2463f0d1}"

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

# ---- Stage B. Real SynVoid qualification (blocked on upstream contract) ----
SYNVOID_SRC="$WORK/synvoid-src"
if ! git clone -q "$SYNVOID_REPO" "$SYNVOID_SRC" 2>/dev/null; then
  verdict NOT-EXECUTED "stage-b synvoid source checkout" "clone failed for $SYNVOID_REPO"
  echo "pass=$pass stopped=$stopped notexec=$notexec"
  [ "$stopped" -eq 0 ] && exit 0
  exit 10
fi
if ! git -C "$SYNVOID_SRC" checkout -q "$SYNVOID_PIN" 2>/dev/null; then
  verdict NOT-EXECUTED "stage-b pinned synvoid revision" "$SYNVOID_PIN not found"
  echo "pass=$pass stopped=$stopped notexec=$notexec"
  [ "$stopped" -eq 0 ] && exit 0
  exit 10
fi
verdict PASS "stage-b pinned synvoid revision" "$SYNVOID_PIN"
if [ ! -f "$SYNVOID_SRC/plans/eggbench_security_qualification_asset_contract.md" ]; then
  verdict NOT-EXECUTED "stage-b upstream asset contract" \
    "plans/eggbench_security_qualification_asset_contract.md absent at $SYNVOID_PIN; Eggbench must not translate SynVoid Detect/Pass semantics itself"
  echo "pass=$pass stopped=$stopped notexec=$notexec"
  [ "$stopped" -eq 0 ] && exit 0
  exit 10
fi
# The contract exists: the remainder is the plan section 14 live sequence
# (build minimal profile, materialize, check, qualify, negative proof).
# It is intentionally unimplemented until the upstream materializer lands,
# so a stale assumption can never be mistaken for qualification.
verdict NOT-EXECUTED "stage-b live reverse-proxy run" \
  "upstream contract present but Eggbench live sequence not yet re-qualified against it"
echo "pass=$pass stopped=$stopped notexec=$notexec"
[ "$stopped" -eq 0 ] && exit 0
exit 10
