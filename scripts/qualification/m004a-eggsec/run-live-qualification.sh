#!/usr/bin/env bash
# M004a live Eggsec qualification harness.
#
# Executes the Eggbench M004a adapter against the REAL pinned Eggsec binary:
#   Eggsec @ 0509ac668adfd78e9899cd3428a807d0b3c9f27b (workspace 0.1.0)
# built with `cargo build --locked --release -p eggsec-cli --no-default-features`.
#
# Proves (plan section 24):
#   1. `eggsec --version`
#   2. strict guarded preflight allowed for the in-scope local target
#   3. the selected WAF JSON shape parses
#   4. the safe fixture yields zero successful bypasses and Pass
#   5. the permissive fixture yields >= 1 Eggsec-declared bypass and Fail
#   6. both are valid execution observations
#   7. no raw payload appears in the finalized Eggbench bundle
#
# Qualification-only: never a production dependency, never alters Eggbench
# runtime behavior, never copies sibling implementation code, never requires
# root, loopback only, no user-global installs. Cleans up all children/dirs.
#
# Verdict model (one line per acceptance item on stdout):
#   PASS / STOPPED (plan stop condition hit) / NOT-EXECUTED (blocked by stop)
# Exit code: 0 only when every check passes; 10 when a plan stop condition
# from M004a section 29 fires.
#
# Env overrides:
#   EGGBENCH_BIN   path to built eggbench binary (default: <repo>/target/release/eggbench)
#   EGSEC_REPO     git URL or local path for eggsec (default: https://github.com/eggstack/eggsec.git)
#   KEEP_WORK=1    keep the temp work dir for inspection (default: remove)
set -u
EGSEC_PIN="0509ac668adfd78e9899cd3428a807d0b3c9f27b"

HERE="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO="$(cd "$HERE/../../.." && pwd)"
EGGBENCH_BIN="${EGGBENCH_BIN:-$REPO/target/release/eggbench}"
EGSEC_REPO="${EGSEC_REPO:-https://github.com/eggstack/eggsec.git}"

WORK="$(mktemp -d "${TMPDIR:-/tmp}/m004a-live-qual.XXXXXX")"
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
need git; need cargo; need rustc; need python3; need sha256sum

# ---- 1. Build the exact pinned Eggsec source revision ------------------------
if [ -d "$EGSEC_REPO/.git" ]; then
  git -C "$EGSEC_REPO" rev-parse --verify "$EGSEC_PIN^{commit}" >/dev/null 2>&1 \
    || { verdict STOPPED "pinned Eggsec revision available" "$EGSEC_PIN not found in $EGSEC_REPO"; exit 10; }
  EGSEC_SRC="$WORK/eggsec-src"
  git clone -q "$EGSEC_REPO" "$EGSEC_SRC" 2>/dev/null
  git -C "$EGSEC_SRC" checkout -q "$EGSEC_PIN"
else
  git clone -q "$EGSEC_REPO" "$WORK/eggsec-src" 2>/dev/null \
    || { verdict STOPPED "clone Eggsec repo" "clone failed"; exit 10; }
  EGSEC_SRC="$WORK/eggsec-src"
  git -C "$EGSEC_SRC" checkout -q "$EGSEC_PIN" \
    || { verdict STOPPED "checkout pinned Eggsec revision" "$EGSEC_PIN"; exit 10; }
fi
[ "$(git -C "$EGSEC_SRC" rev-parse HEAD)" = "$EGSEC_PIN" ] \
  && verdict PASS "pinned Eggsec source revision" "$EGSEC_PIN" \
  || { verdict STOPPED "pinned Eggsec source revision" "HEAD mismatch"; exit 10; }

cargo build --locked --release -p eggsec-cli --no-default-features --manifest-path "$EGSEC_SRC/Cargo.toml" >"$WORK/eggsec-build.log" 2>&1 \
  && verdict PASS "minimal eggsec-cli build" "cargo build --locked --release -p eggsec-cli --no-default-features" \
  || { verdict STOPPED "minimal eggsec-cli build" "$(tail -5 "$WORK/eggsec-build.log")"; exit 10; }
EGSEC="$EGSEC_SRC/target/release/eggsec"
[ -x "$EGSEC" ] || { verdict STOPPED "eggsec binary produced" "missing"; exit 10; }

# Bounded provenance (recorded for the closure record).
{
  echo "source_sha=$EGSEC_PIN"
  echo "cargo_lock_sha256=$(sha256sum "$EGSEC_SRC/Cargo.lock" | cut -d' ' -f1)"
  echo "binary_sha256=$(sha256sum "$EGSEC" | cut -d' ' -f1)"
  echo "binary_bytes=$(wc -c <"$EGSEC")"
  echo "eggsec_version=$("$EGSEC" --version 2>/dev/null)"
  echo "rustc=$(rustc --version)"
  echo "cargo=$(cargo --version)"
} >"$WORK/provenance.txt"
cat "$WORK/provenance.txt"
[ "$("$EGSEC" --version 2>/dev/null)" = "eggsec 0.1.0" ] \
  && verdict PASS "criterion-1 eggsec --version" "$("$EGSEC" --version)" \
  || { verdict STOPPED "criterion-1 eggsec --version" "unexpected version"; exit 10; }

export PATH="$(dirname "$EGSEC"):$PATH"

SAFE_FIXTURE="$REPO/crates/eggbench-drivers/tests/fixtures/eggsec_safe_fixture.py"
PERM_FIXTURE="$REPO/crates/eggbench-drivers/tests/fixtures/eggsec_permissive_fixture.py"
[ -f "$SAFE_FIXTURE" ] && [ -f "$PERM_FIXTURE" ] \
  && verdict PASS "qualification fixtures present" "safe + permissive" \
  || { verdict STOPPED "qualification fixtures present" "missing"; exit 10; }

# ---- 2. Adapter-level live contract (Rust suite, real binary) ----------------
cd "$REPO" || exit 2
if EGGBENCH_PARENT_SENTINEL_26CE=set-in-parent-process cargo test --locked -p eggbench-drivers --test eggsec_live >"$WORK/eggsec-live-tests.log" 2>&1; then
  verdict PASS "criteria-2/3/5/6 live adapter contract" "$(grep -E 'test result' "$WORK/eggsec-live-tests.log" | tail -1)"
else
  tail -30 "$WORK/eggsec-live-tests.log"
  verdict STOPPED "criteria-2/3/5/6 live adapter contract" "eggsec_live suite failed"
  exit 10
fi

# ---- 3. Full production run: safe fixture via managed eggserve-origin -------
WS="$WORK/ws"
mkdir -p "$WS"
python3 - "$WS" <<'EOF'
import json, sys
ws = sys.argv[1]
plan = {
  "schema_version": 6,
  "experiment": "m004a-live-safe",
  "subject": {"kind": "label", "label": "m004a-live-safe"},
  "services": [{
    "name": "origin",
    "kind": {"kind": "named", "service_type": "eggserve-origin"},
    "lifecycle": "managed",
    "depends_on": [],
    "config": {"path": "/bench", "body_bytes": "64", "status": "200"},
    "readiness": {"kind": "delay", "after_ms": 200},
    "shutdown": None,
    "working_directory": None,
    "log_limit_bytes": 65536,
  }],
  "workload": {"kind": "finite_count", "target": "origin", "requests": 10, "concurrency": 1},
  "trials": {"measured": 2, "warmup": 0, "cooldown_ms": None,
             "reset": {"kind": "none"},
             "timeouts": {"measurement": 5000, "drain": 5000}},
  "telemetry": [],
  "metrics": [],
  "environment_policy": {"kind": "strict_same_testbed"},
  "seed": 7,
  "diagnostics": [],
  "security_checks": [{
    "id": "waf-sqli", "source": "eggsec-waf", "target": "origin",
    "test_type": "sqli", "max_successful_bypasses": 0,
    "concurrency": 2, "timeout_ms": 60000,
  }],
  "bounds": {"artifact_count": 256, "artifact_bytes": 16777216, "total_bytes": 268435456},
}
open(ws + "/plan.json", "w").write(json.dumps(plan, indent=2))
print("plan written")
EOF

[ -x "$EGGBENCH_BIN" ] || { verdict STOPPED "eggbench binary available" "$EGGBENCH_BIN"; exit 10; }
cd "$WS" || exit 2
if "$EGGBENCH_BIN" validate plan.json >/dev/null 2>&1; then
  verdict PASS "schema-v6 security plan validates" "validate ok"
else
  verdict STOPPED "schema-v6 security plan validates" "validate failed"
  exit 10
fi
if "$EGGBENCH_BIN" run plan.json safe.eggb --json >"$WORK/combined-safe.json" 2>"$WORK/combined-safe.human"; then
  verdict PASS "criterion-4 full safe run finalizes" "$(head -c 160 "$WORK/combined-safe.json")"
else
  verdict STOPPED "criterion-4 full safe run finalizes" "$(cat "$WORK/combined-safe.json")"
  exit 10
fi
cd "$REPO" || exit 2

# ---- 4. Safe-bundle proofs ----------------------------------------------------
python3 - "$WS" "$WORK" <<'EOF'
import json, sys, glob, os
ws, work = sys.argv[1], sys.argv[2]
ok = []
def check(name, cond, detail=""):
    print(("[PASS]" if cond else "[STOPPED]") + " " + name + " -- " + detail)
    ok.append(cond)
m = json.load(open(ws + '/safe.eggb/manifest.json'))
check("safe bundle execution completed", m.get('execution_status') == 'completed', str(m.get('execution_status')))
check("performance trials intact", len(m.get('trials', [])) == 2, str(len(m.get('trials', []))))
sc = json.load(open(ws + '/safe.eggb/security-checks.json'))
check("criterion-4 safe check passes with zero bypasses",
      sc['checks'][0]['disposition'] == 'pass' and sc['checks'][0]['successful_bypasses'] == 0,
      json.dumps(sc['checks'][0]))
check("criterion-3 selected WAF shape with evaluated cases",
      sc['checks'][0]['evaluated_cases'] >= 1 and sc['driver'] == 'eggsec-waf' and sc['operation'] == 'waf --json --bypass',
      "evaluated=%s" % sc['checks'][0]['evaluated_cases'])
check("executable and scope provenance recorded",
      len(sc['executable_sha256']) == 64 and len(sc['scope_sha256']) == 64 and len(sc['executable_version']) > 0,
      sc['executable_version'])
res = json.load(open(ws + '/safe.eggb/security/waf-sqli.json'))
check("sanitized result carries digests not payloads",
      'payload' not in json.dumps({k: v for k, v in res.items() if k != 'sanitized_cases'})
      and all(len(c['payload_sha256']) == 64 for c in res['sanitized_cases']),
      "cases=%d" % len(res['sanitized_cases']))
ph = json.load(open(ws + '/safe.eggb/runner-phases.json'))
evs = ph['events'] if isinstance(ph, dict) and 'events' in ph else ph
order = [e.get('phase') for e in evs]
check("lifecycle placement outside measured timing",
      order.index('correctness_checks') < order.index('measured_trial'),
      " > ".join(order))
# Criterion 7: no raw payload anywhere in the finalized bundle.
markers = ["' OR 1=1", "<script", "../..", "echo:"]
leaked = []
for path in glob.glob(ws + '/safe.eggb/**/*', recursive=True):
    if os.path.isfile(path):
        try:
            data = open(path, 'rb').read()
        except OSError:
            continue
        for marker in markers:
            if marker.encode() in data:
                leaked.append((path, marker))
check("criterion-7 no raw payload in bundle", not leaked, str(leaked[:3]))
sys.exit(0 if all(ok) else 1)
EOF
[ $? -eq 0 ] \
  && verdict PASS "criteria-3/4/6/7 safe bundle proofs" "see lines above" \
  || { verdict STOPPED "criteria-3/4/6/7 safe bundle proofs" "see lines above"; exit 10; }

# ---- 5. Summary ---------------------------------------------------------------
echo "provenance:"
cat "$WORK/provenance.txt"
echo "pass=$pass stopped=$stopped notexec=$notexec"
[ "$stopped" -eq 0 ] && [ "$notexec" -eq 0 ] && [ "$pass" -gt 0 ] && exit 0
exit 10
