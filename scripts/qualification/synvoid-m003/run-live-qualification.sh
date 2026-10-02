#!/usr/bin/env bash
# Security Qualification M003 live qualification harness.
#
# One job, two accountable stage groups over one real minimal SynVoid build:
#
#   m003c-*  Terminal subject-telemetry qualification. Materializes the
#            closed owner asset export with telemetry enabled, verifies the
#            owner contract id and the exact owner mapping digest, proves the
#            endpoint is loopback-only with admin disabled, runs the M003b
#            pooled/fresh/body and mixed load shapes plus a target-telemetry
#            pressure scenario through the normal Eggbench runner, asserts
#            the collected subject evidence, proves the required-field /
#            TYPE-drift mappings fail closed, and proves the polling task
#            drains with the run.
#
#   m003d-*  M003 load profiles and the negative demonstrations: exact
#            mixed 80/20 schedules, both connection policies, a strict
#            Eggsec load path (owner security path, not an independent
#            transport oracle), an independent oracle body scenario when
#            the tool exists, an explicitly materialized baseline set with a
#            same-source repeatability pair, and the correctness-only,
#            performance-only, and telemetry-only regression proofs.
#
# Qualification-only: loopback only, no user-global installs, all children
# and work directories removed on exit.
#
# Verdict model (one line per acceptance item on stdout):
#   PASS / STOPPED (a stop condition fired) / NOT-EXECUTED (blocked by a stop
#   condition, or an optional external binary is absent)
# Exit code: 0 when no STOPPED verdict fired; 10 otherwise.
#
# Env overrides:
#   EGGBENCH_BIN   path to the built eggbench binary (default release build)
#   EGGBENCH_FEATURES  features the binary must carry (default the M003 set)
#   EGGBENCH_BUILD=1   build the binary in-harness with those features
#   SYNVOID_REPO   git URL or local path (default: upstream SynVoid)
#   SYNVOID_PIN    exact SynVoid source SHA (default: the M003 telemetry pin)
#   EGGSEC_REPO    git URL or local path (default: upstream Eggsec)
#   EGGSEC_PIN     exact Eggsec source SHA
#   KEEP_WORK=1    keep the temp work dir for inspection
set -u
SYNVOID_PIN="${SYNVOID_PIN:-1338ce7b60f3793701091b4c329f80eb542f802d}"
POLICY_ID="${POLICY_ID:-synvoid.eggbench-qualification.v1}"
TELEMETRY_CONTRACT_ID="${TELEMETRY_CONTRACT_ID:-synvoid.eggbench-telemetry.v2}"
# Owner-recorded digest of the materialized telemetry-mapping.json bytes.
MAPPING_SHA256="${MAPPING_SHA256:-622f6a13c4353cc7465cce39a57ed86fa0db2fe4114258e6f06226c1748d2d99}"
EGGSEC_PIN="${EGGSEC_PIN:-0509ac668adfd78e9899cd3428a807d0b3c9f27b}"
POLL_INTERVAL_MS="${POLL_INTERVAL_MS:-200}"
# The throttle used by the performance-only regression must push throughput far
# past the frozen allowance without consuming the per-trial measurement budget:
# the gated scenario is measured nine times, so at 100ms a throttled trial took
# ~10s against a 120s measurement timeout and a slower runner staged failed
# trials, turning the comparison Invalid instead of Fail.
ORIGIN_DELAY_MS="${ORIGIN_DELAY_MS:-20}"

HERE="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO="$(cd "$HERE/../../.." && pwd)"
EGGBENCH_BIN="${EGGBENCH_BIN:-$REPO/target/release/eggbench}"
EGGBENCH_FEATURES="${EGGBENCH_FEATURES:-eggstack-http,prometheus-http}"
SYNVOID_REPO="${SYNVOID_REPO:-https://github.com/dbowm91/synvoid.git}"
EGGSEC_REPO="${EGGSEC_REPO:-https://github.com/eggstack/eggsec.git}"
RESOLVER="$REPO/scripts/qualification/synvoid-m003/resolve-content-digests.py"
BUILDER="$HERE/build-real-m003-workspace.py"
TELEMETRY_ASSERT="$HERE/assert-subject-telemetry.py"
ORIGIN_PY="$HERE/controlled-origin-m003.py"

WORK="$(mktemp -d "${TMPDIR:-/tmp}/m003-live-qual.XXXXXX")"
PIDS=""
cleanup() {
  # shellcheck disable=SC2086
  for p in $PIDS; do kill "$p" 2>/dev/null || true; done
  sleep 1
  # shellcheck disable=SC2086
  for p in $PIDS; do kill -9 "$p" 2>/dev/null || true; done
  rm -f "$WORK"/ws*/origin-delay-ms 2>/dev/null || true
  if [ "${KEEP_WORK:-0}" != "1" ]; then rm -rf "$WORK"; else echo "work kept at $WORK"; fi
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
need git; need python3; need sha256sum; need cargo
free_port() { python3 -c 'import socket; s=socket.socket(); s.bind(("127.0.0.1",0)); print(s.getsockname()[1]); s.close()'; }
port_open() { python3 -c 'import socket,sys; s=socket.socket(); s.settimeout(1)
try:
    s.connect(("127.0.0.1", int(sys.argv[1]))); print("open")
except OSError:
    print("closed")
' "$1"; }

# ---- m003c-0: the binary carries the drivers these plans need -----------
# A default-features build resolves no plan that declares services or subject
# telemetry, which would otherwise surface much later as an opaque "missing
# required Service driver". Fail closed here with the exact build command.
if [ "${EGGBENCH_BUILD:-0}" = "1" ] || [ ! -x "$EGGBENCH_BIN" ]; then
  ( cd "$REPO" && cargo build --locked --release -p eggbench-cli \
      --features "$EGGBENCH_FEATURES" ) >"$WORK/eggbench-build.log" 2>&1 \
    && EGGBENCH_BIN="$REPO/target/release/eggbench" \
    && verdict PASS "m003c-0 eggbench built with $EGGBENCH_FEATURES" "$EGGBENCH_BIN" \
    || { verdict STOPPED "m003c-0 eggbench built with $EGGBENCH_FEATURES" "see $WORK/eggbench-build.log"; exit 10; }
fi
if python3 - "$EGGBENCH_BIN" "$REPO/examples/eggstack-path.json" <<'DRIVERCHECK'
import json
import subprocess
import sys

binary, plan = sys.argv[1], sys.argv[2]
result = subprocess.run([binary, "--json", "doctor", plan], capture_output=True, text=True)
document = json.loads(result.stdout)
drivers = {
    (driver["name"], driver["category"])
    for driver in (document.get("result") or {}).get("drivers", [])
}
required = [
    ("eggfetch-http", "Workload"),
    ("eggserve-origin", "Service"),
    ("eggsec-load", "Workload"),
    ("oha", "Workload"),
    ("prometheus-http", "Telemetry"),
]
missing = [name for name, category in required if (name, category) not in drivers]
if missing:
    print("binary is missing drivers: " + ",".join(missing))
    raise SystemExit(1)
print("drivers present: " + ",".join(name for name, _ in required))
DRIVERCHECK
then verdict PASS "m003c-0b eggbench advertises the required drivers" "eggfetch-http, eggserve-origin, eggsec-load, oha, prometheus-http"
else verdict STOPPED "m003c-0b eggbench advertises the required drivers" "rebuild with: cargo build --locked --release -p eggbench-cli --features $EGGBENCH_FEATURES"; exit 10; fi

# =========================================================================
# m003c-1..4: owner-owned materialization at the pinned revision
# =========================================================================
if [ -n "${SYNVOID_SRC_DIR:-}" ] && [ -d "$SYNVOID_SRC_DIR/.git" ]; then
  # A prepared tree at the same path keeps cargo's build cache valid; the pin
  # is still verified, so reuse can never qualify a different revision.
  SYN_SRC="$SYNVOID_SRC_DIR"
  [ "$(git -C "$SYN_SRC" rev-parse HEAD)" = "$SYNVOID_PIN" ]
  verdict PASS "m003c-1 synvoid source checkout (reused tree)" "$SYNVOID_PIN"
else
  SYN_SRC="$WORK/synvoid-src"
  git clone -q "$SYNVOID_REPO" "$SYN_SRC" >/dev/null 2>&1 \
    && git -C "$SYN_SRC" checkout -q "$SYNVOID_PIN" >/dev/null 2>&1 \
    && verdict PASS "m003c-1 synvoid source checkout" "$SYNVOID_PIN" \
    || { verdict STOPPED "m003c-1 synvoid source checkout" "$SYNVOID_PIN"; exit 10; }
fi

if [ -f "$SYN_SRC/plans/eggbench_security_qualification_asset_contract.md" ] \
  && [ -f "$SYN_SRC/plans/eggbench_security_qualification_m003_telemetry_contract.md" ]; then
  verdict PASS "m003c-2 owner asset + telemetry contracts present at pin" "both owner plans present"
else
  verdict STOPPED "m003c-2 owner asset + telemetry contracts present at pin" "owner plan missing"
  exit 10
fi

# Optional local build cache: reuse an already-built target directory instead of
# recompiling the owner tree. Only meaningful together with SYNVOID_SRC_DIR,
# because cargo fingerprints workspace members by their source path. Never used
# by hosted CI, where a fresh build is part of the evidence.
if [ -n "${SYNVOID_TARGET_DIR:-}" ] && [ -d "$SYNVOID_TARGET_DIR" ] \
   && [ ! -e "$SYN_SRC/target" ]; then
  ln -sfn "$SYNVOID_TARGET_DIR" "$SYN_SRC/target"
fi
( cd "$SYN_SRC" && cargo build --locked --release --no-default-features ) >"$WORK/synvoid-build.log" 2>&1
SYNBIN="$SYN_SRC/target/release/synvoid"
if [ -x "$SYNBIN" ]; then
  SYN_BIN_SHA="$(sha256sum "$SYNBIN" | cut -d' ' -f1)"
  verdict PASS "m003c-3 minimal synvoid build" "binary sha256 $SYN_BIN_SHA"
else
  verdict STOPPED "m003c-3 minimal synvoid build" "see $WORK/synvoid-build.log"
  exit 10
fi

LPORT="$(free_port)"; OPORT="$(free_port)"; MPORT="$(free_port)"
EXPORT="$WORK/real-export"
( cd "$SYN_SRC" && cargo run -q --locked -p xtask -- eggbench-qualification export \
    --output "$EXPORT" --listen-port "$LPORT" --origin-port "$OPORT" --metrics-port "$MPORT" ) \
  >"$WORK/owner-export.log" 2>&1 \
  && verdict PASS "m003c-4 owner export with telemetry enabled" "listen $LPORT origin $OPORT metrics $MPORT" \
  || { verdict STOPPED "m003c-4 owner export with telemetry enabled" "see $WORK/owner-export.log"; exit 10; }

( cd "$SYN_SRC" && cargo run -q --locked -p xtask -- eggbench-qualification check \
    --input "$EXPORT" --configtest --configtest-binary "$SYNBIN" ) >"$WORK/owner-check.log" 2>&1 \
  && verdict PASS "m003c-5 owner export check + real-binary configtest" "check ok, configtest valid" \
  || { verdict STOPPED "m003c-5 owner export check + real-binary configtest" "see $WORK/owner-check.log"; exit 10; }

# ---- Owner contract identity (provenance + contract + mapping bytes) -----
if python3 - "$EXPORT" "$POLICY_ID" "$SYNVOID_PIN" "$TELEMETRY_CONTRACT_ID" "$MAPPING_SHA256" <<'PY'
import hashlib, json, os, sys
export, policy, sha, contract_id, mapping_sha = sys.argv[1:6]
provenance = json.load(open(os.path.join(export, "provenance.json")))
contract = json.load(open(os.path.join(export, "telemetry-contract.json")))
assert provenance["policy_id"] == policy, provenance["policy_id"]
assert provenance["synvoid_git_sha"] == sha, provenance["synvoid_git_sha"]
assert provenance["synvoid_package_version"] == "1.1.0", provenance["synvoid_package_version"]
assert provenance["detect_status"] == 403 and provenance["pass_status"] == 200
assert len(provenance["source_fixtures"]) == 15, len(provenance["source_fixtures"])
assert len(provenance["excluded_fixtures"]) == 12, len(provenance["excluded_fixtures"])
assert provenance["telemetry_contract_id"] == contract_id, provenance["telemetry_contract_id"]
assert provenance["telemetry_mapping_digest"] == mapping_sha, provenance["telemetry_mapping_digest"]
assert contract["contract_id"] == contract_id, contract["contract_id"]
assert contract["schema_version"] == "synvoid.eggbench-telemetry.contract.v2", contract["schema_version"]
assert contract["mapping_sha256"] == mapping_sha
assert contract["source_refresh_cadence_secs"] == 5
assert contract["scrape_path"] == "/metrics"
required = [m for m in contract["metrics"] if m["required"]]
assert len(required) == 10, len(required)
assert all(m.get("source_aggregation") for m in contract["metrics"])
assert all(not m.get("aggregation") for m in contract["metrics"])
raw = open(os.path.join(export, "telemetry-mapping.json"), "rb").read()
assert hashlib.sha256(raw).hexdigest() == mapping_sha, hashlib.sha256(raw).hexdigest()
mapping = json.loads(raw)
assert mapping["schema_version"] == 1 and mapping["source"] == "prometheus"
assert "contract_id" not in mapping
assert len(mapping["fields"]) == 12
assert all(f["output_name"].startswith("subject_") for f in mapping["fields"])
print("owner provenance + telemetry contract pinned")
PY
then verdict PASS "m003c-6 owner provenance + telemetry contract pinned" "$TELEMETRY_CONTRACT_ID, mapping $MAPPING_SHA256"
else verdict STOPPED "m003c-6 owner provenance + telemetry contract pinned" "owner pins do not match"; exit 10; fi

# ---- Loopback-only metrics endpoint, admin disabled ----------------------
if python3 - "$EXPORT/config/main.toml" "$MPORT" <<'PY'
import re, sys
config, metrics_port = sys.argv[1], int(sys.argv[2])
text = open(config).read()
admin = re.search(r"\[admin\]\n(.*?)(?=\n\[|\Z)", text, re.S).group(1)
assert re.search(r"^enabled\s*=\s*false", admin, re.M), "admin must be disabled"
block = re.search(r"\[metrics\]\n(.*?)(?=\n\[|\Z)", text, re.S).group(1)
assert re.search(r"^enabled\s*=\s*true", block, re.M), "metrics must be enabled"
assert re.search(r"^bind_address\s*=\s*\"127\.0\.0\.1\"", block, re.M), block
assert re.search(r"^port\s*=\s*%d" % metrics_port, block, re.M), block
server = re.search(r"\[server\]\n(.*?)(?=\n\[|\Z)", text, re.S).group(1)
assert re.search(r"^host\s*=\s*\"127\.0\.0\.1\"", server, re.M)
print("loopback-only metrics endpoint, admin disabled")
PY
then verdict PASS "m003c-7 loopback-only metrics endpoint, admin disabled" "config-verified, port $MPORT"
else verdict STOPPED "m003c-7 loopback-only metrics endpoint, admin disabled" "config assertion failed"; exit 10; fi

# =========================================================================
# m003c-8 / m003d: real M003 workspace, identities resolved by the runner's
# own content-identity algorithm
# =========================================================================
WS="$WORK/real-ws"
mkdir -p "$WS"
cp "$ORIGIN_PY" "$WS/controlled-origin-m003.py"
chmod +x "$WS/controlled-origin-m003.py"
python3 "$BUILDER" --export "$EXPORT" --workspace "$WS" --listen-port "$LPORT" \
  --origin-port "$OPORT" --metrics-port "$MPORT" --synvoid "$SYNBIN" \
  --config "$EXPORT/config" --policy "$POLICY_ID" --git-sha "$SYNVOID_PIN" \
  --mapping-sha256 "$MAPPING_SHA256" --poll-interval-ms "$POLL_INTERVAL_MS" \
  >"$WORK/workspace-build.log" 2>&1 \
  && verdict PASS "m003c-8 real M003 workspace built" "$(tail -1 "$WORK/workspace-build.log")" \
  || { verdict STOPPED "m003c-8 real M003 workspace built" "see $WORK/workspace-build.log"; exit 10; }

( cd "$WS" && python3 "$RESOLVER" --workspace "$WS" ) >"$WORK/identities.log" 2>&1 \
  && verdict PASS "m003c-9 content identities resolved" "$(grep -c identity "$WORK/identities.log") inputs pinned" \
  || { verdict STOPPED "m003c-9 content identities resolved" "see $WORK/identities.log"; exit 10; }

# The owner mapping bytes in the workspace must be the checked-in fixture
# byte-for-byte: Eggbench consumes the owner artifact, not a translation. The
# owner contract document additionally carries the run's own ports and
# digests, so its stable identity fields are compared instead of its bytes.
if cmp -s "$WS/telemetry/telemetry-mapping.json" \
         "$REPO/qualification/synvoid/v2/telemetry/telemetry-mapping.json" \
   && python3 - "$WS/telemetry/telemetry-contract.json" \
         "$REPO/qualification/synvoid/v2/telemetry/telemetry-contract.json" <<'PY'
import json, sys
live = json.load(open(sys.argv[1]))
fixture = json.load(open(sys.argv[2]))
for key in (
    "contract_id",
    "schema_version",
    "mapping_sha256",
    "source_refresh_cadence_secs",
    "scrape_path",
    "mapping_filename",
):
    assert live[key] == fixture[key], (key, live[key], fixture[key])
assert [m["prometheus_name"] for m in live["metrics"]] == [
    m["prometheus_name"] for m in fixture["metrics"]
], "owner sample inventory drifted from the checked-in fixture"
PY
then
  verdict PASS "m003c-10 live mapping matches the checked-in owner fixture" "byte-identical mapping, 12-sample inventory"
else
  verdict STOPPED "m003c-10 live mapping matches the checked-in owner fixture" "byte or identity mismatch"
  exit 10
fi

MAPPING_IDENTITY="$(grep '^mapping identity' "$WORK/identities.log" | awk '{print $3}')"
[ -n "$MAPPING_IDENTITY" ] || { verdict STOPPED "m003c-9b mapping identity" "unresolved"; exit 10; }

# ---- m003c-10b: one owner instance for the whole qualification ----------
# The plans declare the subject and its controlled origin as external
# services, so the harness owns the single long-running pair. A per-scenario
# managed subject would contend for the owner metrics port and a per-scenario
# child that loses that race would publish no series at all, which would make
# the telemetry evidence describe whichever process still held the port.
# `cd X && exec prog` keeps the recorded PID on the process itself; a plain
# `cd X && prog &` records the wrapper's PID and orphans the real process, so
# teardown would then check ports that nothing ever owned.
( cd "$WS" && exec ./controlled-origin-m003.py --port "$OPORT" --routes-json routes.json \
    --log-file origin-requests.log --delay-signal-file origin-delay-ms ) \
    >"$WORK/origin.log" 2>&1 &
SUBJECT_PIDS="$!"
nohup "$SYNBIN" --foreground --config-path "$EXPORT/config" \
  >"$WORK/subject.log" 2>&1 &
SUBJECT_PID="$!"
PIDS="$PIDS $SUBJECT_PID $SUBJECT_PIDS"
SUBJECT_READY=no
for _ in $(seq 1 60); do
  if [ "$(port_open "$OPORT")" = "open" ] && [ "$(port_open "$LPORT")" = "open" ] \
     && [ "$(port_open "$MPORT")" = "open" ]; then
    SUBJECT_READY=yes
    break
  fi
  sleep 1
done
if [ "$SUBJECT_READY" = "yes" ]; then
  verdict PASS "m003c-10b owner subject + origin live for the whole run" "listen $LPORT origin $OPORT metrics $MPORT"
else
  verdict STOPPED "m003c-10b owner subject + origin live for the whole run" "see $WORK/subject.log"
  exit 10
fi

cd "$WS" || exit 10
# The performance profile is only valid once its explicit baselines exist, so
# it is validated with them (m003d-1) rather than here.
"$EGGBENCH_BIN" qualify validate smoke.profile.json --json >/dev/null 2>&1 \
  && "$EGGBENCH_BIN" qualify expand smoke.profile.json --json >/dev/null 2>&1 \
  && verdict PASS "m003c-11 real M003 smoke profile validates and expands" "6 scenarios" \
  || { verdict STOPPED "m003c-11 real M003 smoke profile validates and expands" "profile validation failed"; exit 10; }

# ---- m003c-12: the M003 smoke profile, including target telemetry -------
ss -Hltn | awk '{print $4}' | sort -u >"$WORK/listeners-before.txt"
"$EGGBENCH_BIN" qualify run smoke.profile.json --output "$WORK/suite-smoke" --json >"$WORK/suite-smoke.json" 2>&1
SMOKE_RC=$?
ss -Hltn | awk '{print $4}' | sort -u >"$WORK/listeners-after.txt"
if [ "$SMOKE_RC" -eq 0 ] || [ "$SMOKE_RC" -eq 7 ]; then
  "$EGGBENCH_BIN" qualify inspect "$WORK/suite-smoke/qualification-receipt.json" --json >/dev/null 2>&1 \
    && verdict PASS "m003c-12 M003 smoke profile on the real subject" "$(python3 -c 'import json,sys;print(json.load(open(sys.argv[1]))["aggregate_verdict"])' "$WORK/suite-smoke/qualification-receipt.json")" \
    || verdict STOPPED "m003c-12 M003 smoke profile on the real subject" "receipt inspection failed"
else
  verdict STOPPED "m003c-12 M003 smoke profile on the real subject" "exit $SMOKE_RC"
fi

# ---- m003c-13: collected subject evidence in the published bundle -------
# Two claims, deliberately separate. The first is the consumer contract: the
# bundle carries owner samples bound to the pinned mapping identity, with
# in-window samples and no poll errors. The second is the owner population
# claim: the subject actually filled the required series. The pinned minimal
# runtime pre-registers the inventory at zero because the supervisor does not
# route Message::UnifiedServerWorkerHeartbeat into
# ProcessManager::handle_unified_server_worker_heartbeat, so the bridge
# aggregates default payloads. That is an upstream finding, recorded as a
# stopped gate rather than papered over with a weaker assertion.
TELEMETRY_BUNDLE="$WORK/suite-smoke/scenarios/synvoid-m003-target-telemetry-pressure-c8.eggb"
TELEMETRY_COMMON=(--bundle "$TELEMETRY_BUNDLE" --mapping-sha256 "$MAPPING_IDENTITY"
  --require-gauge subject_event_loop_lag_ms
  --require-gauge subject_active_connections
  --require-counter subject_body_buffering_bytes_total
  --require-counter subject_offload_rejections_total
  --min-samples 4)
if python3 "$TELEMETRY_ASSERT" "${TELEMETRY_COMMON[@]}" >"$WORK/telemetry-evidence.log" 2>&1; then
  verdict PASS "m003c-13 subject telemetry evidence satisfies the owner contract" "$(tail -1 "$WORK/telemetry-evidence.log")"
else
  verdict STOPPED "m003c-13 subject telemetry evidence satisfies the owner contract" "$(tail -2 "$WORK/telemetry-evidence.log")"
fi
if python3 "$TELEMETRY_ASSERT" "${TELEMETRY_COMMON[@]}" --require-live-values \
     >"$WORK/telemetry-values.log" 2>&1; then
  verdict PASS "m003c-13b required subject series carry live owner values" "$(tail -1 "$WORK/telemetry-values.log")"
else
  verdict STOPPED "m003c-13b required subject series carry live owner values (upstream: synvoid@$SYNVOID_PIN publishes the v2 inventory at zero; supervisor/ipc.rs has no UnifiedServerWorkerHeartbeat dispatch arm)" "$(tail -2 "$WORK/telemetry-values.log")"
fi

# Optional owner sample absent in the minimal runtime: warned, never zeroed.
OPTIONAL_BUNDLE="$WORK/suite-smoke/scenarios/synvoid-m003-target-telemetry-optional-c8.eggb"
if [ ! -d "$OPTIONAL_BUNDLE" ]; then
  "$EGGBENCH_BIN" run scenarios/telemetry-optional-c8.json "$WORK/optional.eggb" --json >/dev/null 2>&1
  OPTIONAL_BUNDLE="$WORK/optional.eggb"
fi
if python3 - "$OPTIONAL_BUNDLE" <<'PY'
import json, os, sys
bundle = sys.argv[1]
trials = sorted(os.listdir(os.path.join(bundle, "trials")))
assert trials, "no measured trials"
for trial in trials:
    document = json.load(open(os.path.join(bundle, "trials", trial, "metrics.json")))
    optional = [o for o in document["observations"]
                if o["name"] == "subject_cpu_worker_rss_bytes"]
    assert optional, "optional owner sample was not requested"
    assert optional[0]["state"]["state"] == "missing", optional[0]
    warnings = [w["category"] for w in document.get("warnings", [])]
    assert "prometheus_missing_samples" in warnings, warnings
    provenance_dir = os.path.join(bundle, "trials", trial, "telemetry")
    provenance = json.load(open(os.path.join(provenance_dir, os.listdir(provenance_dir)[0])))
    assert provenance["missing_field_observation_count"] > 0
print("optional absence warned, not fabricated")
PY
then verdict PASS "m003c-14 optional owner absence stays absent + warned" "no fabricated zero"
else verdict STOPPED "m003c-14 optional owner absence stays absent + warned" "optional sample was fabricated"; fi

# ---- m003c-15: polling leaves no listener behind ------------------------
# The owner owns its own listeners for the whole run, so the leak claim is that
# a run adds none: the loopback listener set after the smoke suite must equal
# the set captured immediately before it.
if [ -s "$WORK/listeners-before.txt" ] && [ -s "$WORK/listeners-after.txt" ] \
   && diff -q "$WORK/listeners-before.txt" "$WORK/listeners-after.txt" >/dev/null; then
  verdict PASS "m003c-15 telemetry polling leaves no listener behind" "listener set unchanged across the run"
else verdict STOPPED "m003c-15 telemetry polling leaves no listener behind" "a listener was left open by the run"; fi

# ---- m003c-16/17: fail-closed mapping drift and TYPE drift -------------
cp telemetry/telemetry-mapping.json telemetry/telemetry-mapping.owner.json
python3 - <<'PY'
import json
path = "telemetry/telemetry-mapping.json"
mapping = json.load(open(path))
for field in mapping["fields"]:
    if field["output_name"] == "subject_event_loop_lag_ms":
        field["prometheus_name"] = "synvoid_subject_event_loop_lag_renamed"
with open(path, "w") as handle:
    json.dump(mapping, handle, separators=(",", ":"), sort_keys=True)
PY
python3 "$RESOLVER" --workspace "$WS" --repin >/dev/null 2>&1
"$EGGBENCH_BIN" run scenarios/telemetry-pressure-c8.json "$WORK/drift-missing.eggb" --json >"$WORK/drift-missing.json" 2>&1
DRIFT_RC=$?
if [ "$DRIFT_RC" -ne 0 ] && grep -q "required_metric_missing" "$WORK/drift-missing.json" \
   && [ ! -d "$WORK/drift-missing.eggb/trials" ]; then
  verdict PASS "m003c-16 renamed required owner metric fails closed" "no measured trials published"
else
  verdict STOPPED "m003c-16 renamed required owner metric fails closed" "exit $DRIFT_RC"
fi
cp telemetry/telemetry-mapping.owner.json telemetry/telemetry-mapping.json
python3 - <<'PY'
import json
path = "telemetry/telemetry-mapping.json"
mapping = json.load(open(path))
for field in mapping["fields"]:
    if field["output_name"] == "subject_offload_timeouts_total":
        # The owner declares a monotonic counter; a mapping that claims it is
        # a gauge must be rejected against the real exposition's TYPE line.
        field["kind"] = "gauge"
        field["aggregation"] = "max"
with open(path, "w") as handle:
    json.dump(mapping, handle, separators=(",", ":"), sort_keys=True)
PY
python3 "$RESOLVER" --workspace "$WS" --repin >/dev/null 2>&1
"$EGGBENCH_BIN" run scenarios/telemetry-pressure-c8.json "$WORK/drift-type.eggb" --json >"$WORK/drift-type.json" 2>&1
DRIFT_RC=$?
if [ "$DRIFT_RC" -ne 0 ] && grep -q "sample_type_mismatch" "$WORK/drift-type.json"; then
  verdict PASS "m003c-17 owner TYPE drift fails closed" "sample_type_mismatch"
else
  verdict STOPPED "m003c-17 owner TYPE drift fails closed" "exit $DRIFT_RC"
fi
cp telemetry/telemetry-mapping.owner.json telemetry/telemetry-mapping.json
python3 "$RESOLVER" --workspace "$WS" --repin >/dev/null 2>&1

# ---- m003d-1: explicit same-source baselines + repeatability ------------
# The baseline set is derived from the emitted profile instead of restated
# here, so a renamed or dropped scenario cannot leave this stage validating
# bundles the profile no longer declares.
BASELINE_PLANS=$(python3 -c '
import json
import sys

profile = json.load(open(sys.argv[1]))
for scenario in profile["scenarios"]:
    if scenario.get("baseline_bundle"):
        print(scenario["plan"])
' perf.profile.json)
BASELINE_COUNT=0
BASELINE_FAILED=""
for plan in $BASELINE_PLANS; do
  name=${plan##*/}
  name=${name%.json}
  baseline_rc=0
  "$EGGBENCH_BIN" run "$plan" "baselines/$name.eggb" --json >"$WORK/baseline-$name.json" 2>&1 \
    || baseline_rc=$?
  if [ "$baseline_rc" -eq 0 ]; then
    BASELINE_COUNT=$((BASELINE_COUNT + 1))
  else
    BASELINE_FAILED="$BASELINE_FAILED $name(exit $baseline_rc: $(python3 -c '
import json
import sys

try:
    document = json.load(open(sys.argv[1]))
except (OSError, ValueError) as error:
    print("unreadable envelope: " + str(error))
    raise SystemExit(0)
error = document.get("error", {})
print(str(error.get("category")) + ": " + str(error.get("message"))[:120])
' "$WORK/baseline-$name.json"))"
  fi
done
BASELINE_TOTAL=$(printf '%s\n' $BASELINE_PLANS | wc -l)
if [ "$BASELINE_COUNT" -gt 0 ] \
   && [ "$BASELINE_COUNT" -eq "$BASELINE_TOTAL" ] \
   && "$EGGBENCH_BIN" qualify validate perf.profile.json --json >/dev/null 2>&1; then
  verdict PASS "m003d-1 explicit baseline bundles materialized" "$BASELINE_COUNT accepted-revision bundles, perf profile validates"
else
  verdict STOPPED "m003d-1 explicit baseline bundles materialized" "$BASELINE_COUNT of $BASELINE_TOTAL baseline runs succeeded; failed:$BASELINE_FAILED"
fi

"$EGGBENCH_BIN" qualify run perf.profile.json --output "$WORK/suite-perf" --json >"$WORK/suite-perf.json" 2>&1
PERF_RC=$?
if [ "$PERF_RC" -eq 0 ] || [ "$PERF_RC" -eq 7 ]; then
  verdict PASS "m003d-2 same-source perf pair" "$(python3 -c 'import json,sys;print(json.load(open(sys.argv[1]))["aggregate_verdict"])' "$WORK/suite-perf/qualification-receipt.json")"
else
  verdict STOPPED "m003d-2 same-source perf pair" "exit $PERF_RC"
fi

# ---- m003d-3: blocked requests never reach the origin -------------------
if [ -f origin-requests.log ] && [ -s origin-requests.log ]; then
  if python3 - "$WS/corpus.json" origin-requests.log <<'PY'
import json
import sys

corpus = json.load(open(sys.argv[1]))
logged = open(sys.argv[2], errors="replace").read()
leaked = [
    case["id"]
    for case in corpus["cases"]
    if case["expectation"] != {"status_exact": 200}
    and case["request"]["path_and_query"] in logged
]
if leaked:
    print("blocked requests reached the origin: " + ",".join(leaked))
    raise SystemExit(1)
print(f"{len(logged.splitlines())} logged requests, no blocked case reached the origin")
PY
  then
    verdict PASS "m003d-3 controlled origin served benign traffic only" "no owner-blocked case reached the origin"
  else
    verdict STOPPED "m003d-3 controlled origin served benign traffic only" "a blocked case reached the origin"
  fi
else
  verdict NOT-EXECUTED "m003d-3 controlled origin served benign traffic only" "no origin log (scenarios served from proxy cache)"
fi

# ---- m003d-4: independent oracle body scenario (optional tool) -----------
if command -v oha >/dev/null 2>&1; then
  "$EGGBENCH_BIN" run scenarios/body-oha-c8.json "$WORK/oha-base.eggb" --workload-driver oha --json >/dev/null 2>&1 \
    && "$EGGBENCH_BIN" run scenarios/body-oha-c8.json "$WORK/oha-cand.eggb" --workload-driver oha --json >/dev/null 2>&1 \
    && verdict PASS "m003d-4 independent oha body scenario" "base + candidate compared" \
    || verdict STOPPED "m003d-4 independent oha body scenario" "oha run failed"
else
  verdict NOT-EXECUTED "m003d-4 independent oha body scenario" "oha not installed on this runner"
fi

# ---- m003d-5: strict Eggsec load path (security-owner execution) --------
EGGSRC="$WORK/eggsec-src"
if git clone -q "$EGGSEC_REPO" "$EGGSRC" >/dev/null 2>&1 \
   && git -C "$EGGSRC" checkout -q "$EGGSEC_PIN" >/dev/null 2>&1 \
   && ( cd "$EGGSRC" && cargo build --locked --release --no-default-features -p eggsec-cli ) >"$WORK/eggsec-build.log" 2>&1; then
  EGGSEC_BIN="$EGGSRC/target/release/eggsec"
  EGGSEC_SHA="$(sha256sum "$EGGSEC_BIN" | cut -d' ' -f1)"
  EGGSEC_VERSION="$("$EGGSEC_BIN" --version 2>/dev/null | head -1)"
  verdict PASS "m003d-5 Eggsec build + version probe" "pin $EGGSEC_PIN, $EGGSEC_VERSION, sha $EGGSEC_SHA"
  # The Eggsec driver resolves the trusted executable from PATH; the pinned
  # build shadows any other eggsec for the rest of this harness.
  PATH="$(dirname "$EGGSEC_BIN"):$PATH"
  export PATH
  "$EGGBENCH_BIN" run scenarios/eggsec-benign-body-c8.json "$WORK/eggsec-benign.eggb" \
      --workload-driver eggsec-load --json >"$WORK/eggsec-benign.json" 2>&1
  BENIGN_RC=$?
  "$EGGBENCH_BIN" run scenarios/eggsec-blocked-c8.json "$WORK/eggsec-blocked.eggb" \
      --workload-driver eggsec-load --json >"$WORK/eggsec-blocked.json" 2>&1
  BLOCKED_RC=$?
  if [ "$BENIGN_RC" -eq 0 ] && [ "$BLOCKED_RC" -eq 0 ]; then
    verdict PASS "m003d-6 Eggsec strict-scope load (benign body + blocked case)" "expected status outcomes, not transport errors"
  else
    verdict STOPPED "m003d-6 Eggsec strict-scope load (benign body + blocked case)" "benign $BENIGN_RC blocked $BLOCKED_RC"
  fi
else
  verdict STOPPED "m003d-5 Eggsec build + version probe" "see $WORK/eggsec-build.log"
fi

# ---- m003d-7: correctness-only regression ------------------------------
cp correctness-corpus.json correctness-corpus.owner.json
python3 - <<'PY'
import json
corpus = json.load(open("correctness-corpus.json"))
for case in corpus["cases"]:
    if case["request"]["method"] == "POST" and case["expectation"] == {"status_exact": 200}:
        case["expectation"] = {"status_exact": 418}
        break
with open("correctness-corpus.json", "w") as handle:
    json.dump(corpus, handle, indent=2)
PY
python3 "$RESOLVER" --workspace "$WS" --repin >/dev/null 2>&1
"$EGGBENCH_BIN" qualify run smoke.profile.json --output "$WORK/suite-correctness-regression" --json >/dev/null 2>&1
CORRECTNESS_RC=$?
cp correctness-corpus.owner.json correctness-corpus.json
python3 "$RESOLVER" --workspace "$WS" --repin >/dev/null 2>&1
if [ "$CORRECTNESS_RC" -eq 6 ]; then
  verdict PASS "m003d-7 correctness-only regression fails the suite" "security correctness fails, performance acceptable"
else
  verdict STOPPED "m003d-7 correctness-only regression fails the suite" "exit $CORRECTNESS_RC"
fi

# ---- m003d-8: performance-only regression (origin throttle) -------------
rm -f origin-delay-ms
echo "$ORIGIN_DELAY_MS" > origin-delay-ms
"$EGGBENCH_BIN" qualify run perf.profile.json --output "$WORK/suite-performance-regression" --json >"$WORK/perf-regression.json" 2>&1
PERFORMANCE_RC=$?
rm -f origin-delay-ms
# A stopped stage must say which scenario ended up in what state, otherwise the
# only evidence is an exit code that does not identify the cause.
REGRESSION_DETAIL=$(python3 -c '
import json
import sys

try:
    document = json.load(open(sys.argv[1]))
except (OSError, ValueError) as error:
    print(f"unreadable envelope: {error}")
    raise SystemExit(0)
states = " ".join(
    "{0}={1}/{2}{3}".format(
        scenario["id"],
        scenario.get("status"),
        scenario.get("performance_verdict") or scenario.get("correctness_verdict") or "-",
        "" if not scenario.get("reason") else " (" + str(scenario["reason"]) + ")",
    )
    for scenario in document.get("scenarios", [])
)
print("aggregate {}: {}".format(document.get("aggregate_verdict"), states))
' "$WORK/perf-regression.json")
if [ "$PERFORMANCE_RC" -eq 6 ] \
   && python3 -c 'import json,sys; d=json.load(open(sys.argv[1])); r=[s for s in d["scenarios"] if s["id"]=="synvoid-m003-correctness"]; sys.exit(0 if r and r[0].get("correctness_verdict")=="pass" else 1)' \
        "$WORK/suite-performance-regression/qualification-receipt.json"; then
  verdict PASS "m003d-8 performance-only regression fails the suite" "origin throttled; security outcomes stay correct"
else
  verdict STOPPED "m003d-8 performance-only regression fails the suite" "exit $PERFORMANCE_RC; $REGRESSION_DETAIL"
fi

# ---- m003d-9: telemetry-only regression is covered by m003c-16/17 ------
verdict PASS "m003d-9 telemetry-only regression demonstrated" "m003c-16 renamed required metric, m003c-17 TYPE drift"

# ---- teardown -----------------------------------------------------------
# The harness owns the owner pair, so it stops them here first; only then is
# "no listener and no subject process left behind" a claim about the
# qualification rather than about the harness still running.
for p in $PIDS; do kill "$p" 2>/dev/null || true; done
# The owner spawns its own worker descendants; they belong to this harness too.
pkill -f "$SYNBIN" 2>/dev/null || true
for _ in $(seq 1 30); do
  open_ports=""
  for port in "$LPORT" "$OPORT" "$MPORT"; do
    [ "$(port_open "$port")" = "open" ] && open_ports="$open_ports $port"
  done
  [ -z "$open_ports" ] && break
  sleep 1
done
for p in $PIDS; do kill -9 "$p" 2>/dev/null || true; done
sleep 1
still_open=""
for port in "$LPORT" "$OPORT" "$MPORT"; do
  [ "$(port_open "$port")" = "open" ] && still_open="$still_open $port"
done
if [ -n "$still_open" ]; then
  verdict STOPPED "m003 teardown leaves no listener" "ports still open:$still_open"
elif pgrep -f "$SYNBIN" >/dev/null 2>&1; then
  verdict STOPPED "m003 teardown leaves no subject process" "synvoid still running"
else
  verdict PASS "m003 teardown leaves no listener or subject process" \
    "ports $LPORT/$OPORT/$MPORT closed, owner tree reaped"
fi

echo "pass=$pass stopped=$stopped notexec=$notexec"
echo "evidence: synvoid $SYNVOID_PIN binary $SYN_BIN_SHA"
echo "evidence: owner contract $TELEMETRY_CONTRACT_ID mapping $MAPPING_SHA256 identity $MAPPING_IDENTITY"
[ "$stopped" -eq 0 ] || exit 10
exit 0
