#!/usr/bin/env bash
# M004b combined security-correctness + performance qualification harness.
#
# Consumes the M004a pinned Eggsec binary and proves the M004b combined
# verdict precedence (plan section 24):
#   Case A — security Pass + performance Pass        => final Pass (exit 0)
#   Case B — security Fail + performance Pass        => final Fail (exit 6)
#   Case C — security Pass + performance Fail        => final Fail (exit 6)
#   Case D — invalid security evidence + perf Pass   => final Invalid (exit 8)
#   Case E — security Pass + performance Inconclusive => final Inconclusive (exit 7)
#   Case F — correctness-only candidate               => final from correctness (exit 0)
#
# Case B runs through the Rust live suite (`m004b_live`, real lifecycle +
# real binary + production CLI compare). Cases A/C/D/E/F run here through
# the production `eggbench` binary against managed `eggserve-origin`
# targets. All real WAF observations come from the M004a pinned binary.
#
# Qualification-only: loopback only, no user-global installs. Cleans up
# all children/dirs.
#
# Verdict model: PASS / STOPPED / NOT-EXECUTED, one line per item.
# Exit code: 0 only when every check passes; 10 on a plan stop condition.
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

WORK="$(mktemp -d "${TMPDIR:-/tmp}/m004b-live-qual.XXXXXX")"
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

# ---- 1. Pinned Eggsec binary (same contract as M004a) ------------------------
if [ -d "$EGSEC_REPO/.git" ]; then
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
  && verdict PASS "minimal eggsec-cli build" "pinned revision" \
  || { verdict STOPPED "minimal eggsec-cli build" "$(tail -5 "$WORK/eggsec-build.log")"; exit 10; }
EGSEC="$EGSEC_SRC/target/release/eggsec"
export PATH="$(dirname "$EGSEC"):$PATH"

# ---- 2. Case B through the Rust live suite (real lifecycle + CLI) ------------
cd "$REPO" || exit 2
if EGGBENCH_PARENT_SENTINEL_26CE=set-in-parent-process cargo test --locked -p eggbench-cli --test m004b_live >"$WORK/case-b.log" 2>&1; then
  verdict PASS "case-B security Fail + performance Pass => Fail/exit 6" "$(grep -E 'test result' "$WORK/case-b.log" | tail -1)"
else
  tail -20 "$WORK/case-b.log"
  verdict STOPPED "case-B security Fail + performance Pass => Fail/exit 6" "m004b_live failed"
  exit 10
fi

# ---- 3. Safe-run plans for Cases A/C/D/E/F -----------------------------------
[ -x "$EGGBENCH_BIN" ] || { verdict STOPPED "eggbench binary available" "$EGGBENCH_BIN"; exit 10; }
WS="$WORK/ws"
mkdir -p "$WS"
python3 - "$WS" <<'EOF'
import json, sys
ws = sys.argv[1]
def plan(name, metrics, measured=2):
    return {
      "schema_version": 6,
      "experiment": name,
      "subject": {"kind": "label", "label": name},
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
      "trials": {"measured": measured, "warmup": 0, "cooldown_ms": None,
                 "reset": {"kind": "none"},
                 "timeouts": {"measurement": 5000, "drain": 5000}},
      "telemetry": [],
      "metrics": metrics,
      "environment_policy": {"kind": "strict_same_testbed"},
      "seed": 21,
      "diagnostics": [],
      "security_checks": [{
        "id": "waf-sqli", "source": "eggsec-waf", "target": "origin",
        "test_type": "sqli", "max_successful_bypasses": 0,
        "concurrency": 2, "timeout_ms": 60000,
      }],
      "bounds": {"artifact_count": 256, "artifact_bytes": 16777216, "total_bytes": 268435456},
    }
lat = lambda gate: {
  "name": "latency_p99", "unit": "ms",
  "direction": {"kind": "lower_is_better"}, "intent": "primary", "gate": gate,
}
plans = {
  # A: performance Pass (generous absolute budget).
  "a": [lat({"kind": "absolute", "value": 60000.0})],
  # C: performance Fail (impossibly tight absolute budget).
  "c": [lat({"kind": "absolute", "value": 0.000001})],
  # E: performance Inconclusive (zero-allowance statistical gate over a
  #      self-comparison: the bootstrap interval genuinely runs and
  #      straddles the zero threshold).
  "e": [lat({"kind": "statistical_relative", "allowance": 0, "min_trials": 1})],
  # F: correctness-only (no performance gates).
  "f": [],
}
for key, metrics in plans.items():
    measured = 6 if key == "e" else 2
    open(f"{ws}/plan-{key}.json", "w").write(json.dumps(plan(f"m004b-case-{key}", metrics, measured), indent=2))
print("plans written")
EOF

run_case() { # $1 = key
  cd "$WS" || exit 2
  "$EGGBENCH_BIN" run "plan-$1.json" "case-$1.eggb" --json >"$WORK/run-$1.json" 2>/dev/null
  rc=$?
  cd "$REPO" || exit 2
  return "$rc"
}

for key in a c e f; do
  if run_case "$key"; then
    verdict PASS "case-$key run finalizes" "execution completed"
  else
    verdict STOPPED "case-$key run finalizes" "$(cat "$WORK/run-$key.json")"
    exit 10
  fi
done

compare_case() { # $1 = label, $2 = candidate, $3 = baseline-or-empty, $4 = expected-exit
  local label="$1" candidate="$2" baseline="$3" expected="$4"
  cd "$WS" || exit 2
  if [ -n "$baseline" ]; then
    "$EGGBENCH_BIN" compare "$baseline" "$candidate" --output "$WORK/receipt-$label.json" >"$WORK/compare-$label.out" 2>"$WORK/compare-$label.human"
  else
    "$EGGBENCH_BIN" compare --absolute-only "$candidate" --output "$WORK/receipt-$label.json" >"$WORK/compare-$label.out" 2>"$WORK/compare-$label.human"
  fi
  rc=$?
  cd "$REPO" || exit 2
  if [ "$rc" -eq "$expected" ]; then
    verdict PASS "case-$label exit $expected" "$(head -c 120 "$WORK/compare-$label.out")"
  else
    verdict STOPPED "case-$label exit $expected" "got exit $rc: $(cat "$WORK/compare-$label.out" | head -c 300)"
    exit 10
  fi
}

# Case E compares the bundle against itself: the statistical gate
# genuinely evaluates (bootstrap interval over real observations) with
# perfect comparability, and the zero-allowance interval straddles zero.
compare_case "A" "case-a.eggb" "" 0
compare_case "C" "case-c.eggb" "" 6
compare_case "E" "case-e.eggb" "case-e.eggb" 7
compare_case "F" "case-f.eggb" "" 0

# ---- 4. Case D: tampered security artifact => Invalid (exit 8) ----------------
# Flip the observed bypass count while keeping the stored Pass: the digest
# chain (result -> index -> manifest) is recomputed consistently so the
# loader reaches the stored-versus-recomputed check and reports Invalid
# with a stable reason instead of a digest failure.
python3 - "$WS" "$WORK" <<'EOF'
import json, shutil, hashlib, sys
ws, work = sys.argv[1], sys.argv[2]
shutil.copytree(f"{ws}/case-a.eggb", f"{ws}/case-d.eggb")
path = f"{ws}/case-d.eggb/security/waf-sqli.json"
res = json.load(open(path))
res["successful_bypasses"] = 2
res["sanitized_cases"][0]["bypass_successful"] = True
res["sanitized_cases"][1]["bypass_successful"] = True
data = json.dumps(res, indent=2).encode()
open(path, "wb").write(data)
result_digest = hashlib.sha256(data).hexdigest()
index_path = f"{ws}/case-d.eggb/security-checks.json"
index = json.load(open(index_path))
index["checks"][0]["successful_bypasses"] = 2
index["checks"][0]["artifact_sha256"] = result_digest
index_data = json.dumps(index, indent=2).encode()
open(index_path, "wb").write(index_data)
manifest_path = f"{ws}/case-d.eggb/manifest.json"
manifest = json.load(open(manifest_path))
for artifact in manifest["artifacts"]:
    if artifact["path"] == "security/waf-sqli.json":
        artifact["sha256"] = result_digest
        artifact["byte_size"] = len(data)
    if artifact["path"] == "security-checks.json":
        artifact["sha256"] = hashlib.sha256(index_data).hexdigest()
        artifact["byte_size"] = len(index_data)
json.dump(manifest, open(manifest_path, "w"), indent=2)
print("tampered: digest", result_digest[:16])
EOF
compare_case "D" "case-d.eggb" "" 8

# ---- 5. Combined receipt proofs ------------------------------------------------
python3 - "$WORK" <<'EOF'
import json, sys
work = sys.argv[1]
ok = []
def check(name, cond, detail=""):
    print(("[PASS]" if cond else "[STOPPED]") + " " + name + " -- " + detail)
    ok.append(cond)
def receipt(label):
    return json.load(open(f"{work}/receipt-{label}.json"))
a, c, e, f, d = (receipt(k) for k in "ACEFD")
check("receipts are schema v3", all(r["schema_version"] == 3 for r in (a, c, e, f, d)), "v3")
check("case-A Pass/Pass => Pass",
      a["performance_verdict"] == "pass" and a["correctness"]["aggregate_verdict"] == "pass"
      and a["aggregate_verdict"] == "pass", json.dumps(a["aggregate_verdict"]))
check("case-C Pass/Fail(perf) => Fail",
      c["performance_verdict"] == "fail" and c["correctness"]["aggregate_verdict"] == "pass"
      and c["aggregate_verdict"] == "fail", json.dumps(c["aggregate_verdict"]))
check("case-E Pass/Inconclusive(perf) => Inconclusive",
      e["performance_verdict"] == "inconclusive" and e["correctness"]["aggregate_verdict"] == "pass"
      and e["aggregate_verdict"] == "inconclusive", json.dumps(e["aggregate_verdict"]))
check("case-F correctness-only Pass => Pass",
      f.get("performance_verdict") is None and f["correctness"]["aggregate_verdict"] == "pass"
      and f["aggregate_verdict"] == "pass", json.dumps(f["aggregate_verdict"]))
check("case-D invalid correctness + perf Pass => Invalid",
      d["performance_verdict"] == "pass" and d["correctness"]["aggregate_verdict"] == "invalid"
      and d["aggregate_verdict"] == "invalid", json.dumps(d["aggregate_verdict"]))
check("case-D check carries a stable reason",
      d["correctness"]["checks"][0]["disposition"] == "invalid"
      and d["correctness"]["checks"][0]["reason"] == "disposition_mismatch",
      d["correctness"]["checks"][0].get("reason"))
check("correctness policy identifier is immutable",
      all(r["correctness"]["policy_id"] == "eggbench.security-correctness.v1" for r in (a, c, e, f, d)),
      "eggbench.security-correctness.v1")
check("no security data in TrialMetrics",
      all("successful_bypasses" not in json.dumps(r.get("metrics", [])) for r in (a, c, e, f, d)),
      "metrics carry no bypass counts")
check("rendering separates sections",
      all("aggregate_verdict" in r and "correctness" in r for r in (a, c, e, f, d))
      and all("performance_verdict" in r for r in (a, c, e, d)),
      "typed v3 fields (performance absent only for correctness-only F)")
sys.exit(0 if all(ok) else 1)
EOF
[ $? -eq 0 ] \
  && verdict PASS "cases A/C/D/E/F combined proofs" "see lines above" \
  || { verdict STOPPED "cases A/C/D/E/F combined proofs" "see lines above"; exit 10; }

# Human rendering spot check: the Case C failure names the primary gate,
# while the live Case B names the security gate (covered in m004b_live).
grep -q "primary gate" "$WORK/compare-C.human" \
  && verdict PASS "human rendering names the failing family" "$(cat "$WORK/compare-C.human")" \
  || { verdict STOPPED "human rendering names the failing family" "missing"; exit 10; }

# ---- 6. Summary ---------------------------------------------------------------
echo "pass=$pass stopped=$stopped notexec=$notexec"
[ "$stopped" -eq 0 ] && [ "$notexec" -eq 0 ] && [ "$pass" -gt 0 ] && exit 0
exit 10
