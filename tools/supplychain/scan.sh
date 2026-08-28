#!/usr/bin/env bash
# THOROUGH supply-chain scan — run on a schedule or before a release, or any time
# explicitly. Slower than check.sh because it fetches the RustSec advisory DB and
# resolves the dependency graph.
#
# Runs all three gates and reports a combined result. Every gate is LOUD: a gate
# that cannot run (e.g. advisory DB unreachable with no cache) fails the scan
# rather than passing silently.
#
#   1. structural drift   guard.py     (build-script drift + typosquats; offline)
#   2. known vulns        audit.sh     (cargo audit / RustSec; network, offline-tolerant)
#   3. standing policy    cargo deny   (sources + licences + duplicates)
#
# Exit: 0 = all gates green, non-zero = at least one gate red or unable to run.
set -uo pipefail
HERE="$(cd "$(dirname "$0")" && pwd)"
ROOT="$(cd "$HERE/../.." && pwd)"
export PATH="$HOME/.cargo/bin:$PATH"

hr() { printf '\n============================================================\n'; }
declare -a RESULTS

run_gate() { # label command...
  local label="$1"; shift
  hr; echo "## $label"; echo
  "$@"; local rc=$?
  if [ $rc -eq 0 ]; then RESULTS+=("OK    $label"); else RESULTS+=("FAIL($rc) $label"); fi
  return 0
}

# 1) structural drift (fast, offline)
run_gate "Build-script drift + typosquats (guard.py)" \
  python3 "$HERE/guard.py" --check

# 2) known vulnerabilities (cargo audit, offline-tolerant, loud on can't-run)
run_gate "Known vulnerabilities (cargo audit)" \
  bash "$HERE/audit.sh"

# 3) standing policy (cargo deny)
if cargo deny --version >/dev/null 2>&1; then
  run_gate "Source / licence / duplicate policy (cargo deny)" \
    cargo deny --manifest-path "$ROOT/Cargo.toml" --config "$ROOT/deny.toml" check
else
  hr; echo "## Source / licence / duplicate policy (cargo deny)"
  echo "cargo-deny not installed — install with: cargo install cargo-deny --locked"
  RESULTS+=("FAIL(3) cargo deny (not installed)")
fi

hr
echo "## SUPPLY-CHAIN SCAN SUMMARY"
echo
fail=0
for r in "${RESULTS[@]}"; do
  echo "  $r"
  [[ "$r" == FAIL* ]] && fail=1
done
echo
if [ $fail -eq 0 ]; then
  echo "RESULT: GREEN — every supply-chain gate passed."
else
  echo "RESULT: RED — at least one gate is red or could not run. See sections above."
fi
exit $fail
