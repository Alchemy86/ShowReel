#!/usr/bin/env bash
# One-time bootstrap for dropping this supply-chain kit into another Cargo repo.
#
# Portability model: every script here is generic — guard.py, audit.sh, check.sh,
# scan.sh and install-hooks.sh need NO per-repo edits. Only two things are
# per-repo, and this script produces/points at both:
#   1. tools/supplychain/buildscript-baseline.json  — generated from THIS repo
#   2. deny.toml's licence allow-list / exceptions   — tuned to THIS repo's tree
#
# To port into asciicity / gameboy / mapgb (or any Cargo repo):
#   cp -r <showreel>/tools/supplychain  <target>/tools/
#   cp    <showreel>/deny.toml          <target>/         # then tune licences
#   cd <target> && bash tools/supplychain/bootstrap.sh
#
# This runs from the target repo root (or is found relative to itself).
set -uo pipefail
HERE="$(cd "$(dirname "$0")" && pwd)"
ROOT="$(cd "$HERE/../.." && pwd)"
export PATH="$HOME/.cargo/bin:$PATH"
cd "$ROOT"

echo "== Supply-chain kit bootstrap for: $ROOT =="
[ -f Cargo.lock ] || { echo "No Cargo.lock at repo root — run 'cargo generate-lockfile' first."; exit 1; }

echo
echo "-- 1. Tooling --"
for t in cargo-audit cargo-deny; do
  if cargo "${t#cargo-}" --version >/dev/null 2>&1; then
    echo "  ok: $t ($(cargo "${t#cargo-}" --version 2>/dev/null))"
  else
    echo "  MISSING: $t   (install: cargo install $t --locked)"
  fi
done

echo
echo "-- 2. Fetch crate sources (so every build script can be inspected) --"
cargo fetch >/dev/null 2>&1 && echo "  cargo fetch ok" || echo "  cargo fetch failed (offline?) — baseline may be incomplete"

echo
echo "-- 3. Generate the build-script baseline for this repo --"
python3 "$HERE/guard.py" --update-baseline

echo
echo "-- 4. cargo deny status (tune deny.toml licences if this fails) --"
if [ -f "$ROOT/deny.toml" ] && cargo deny --version >/dev/null 2>&1; then
  if cargo deny --manifest-path "$ROOT/Cargo.toml" --config "$ROOT/deny.toml" check licenses 2>&1 | tail -1 | grep -q 'ok'; then
    echo "  licences: ok"
  else
    echo "  licences: NOT yet clean. Licences present in this tree:"
    cargo deny list 2>/dev/null | sed -n 's/ (.*//p' | sort -u | sed 's/^/      /'
    echo "  Add the missing ones to deny.toml [licenses].allow, then re-run."
  fi
else
  echo "  (skipped: deny.toml or cargo-deny missing)"
fi

echo
echo "-- 5. Prove the gates fire (self-tests) --"
bash "$HERE/selftest.sh"      >/dev/null 2>&1 && echo "  guard self-test: PASS" || echo "  guard self-test: FAIL (run tools/supplychain/selftest.sh)"
if cargo deny --version >/dev/null 2>&1 && [ -f "$ROOT/deny.toml" ]; then
  bash "$HERE/deny-selftest.sh" >/dev/null 2>&1 && echo "  deny  self-test: PASS" || echo "  deny  self-test: FAIL (run tools/supplychain/deny-selftest.sh)"
fi

echo
echo "-- Next steps --"
echo "  * review & commit: tools/supplychain/buildscript-baseline.json + buildscript-inventory.md + deny.toml"
echo "  * install the fast pre-push gate:  bash tools/supplychain/install-hooks.sh"
echo "  * run the full scan any time:       tools/supplychain/scan.sh"
