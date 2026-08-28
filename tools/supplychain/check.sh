#!/usr/bin/env bash
# FAST supply-chain gate — cheap enough to run on every build / commit / push.
#
# Runs only the offline, sub-second structural check (guard.py): build-script
# drift + typosquat detection against the committed baseline. No network, no
# advisory fetch, no cargo metadata resolve. This is the half that would have
# fired on the 2026-08-20 arrayref attack on day one, before any advisory existed.
#
# The heavier, networked scan (cargo audit + cargo deny) lives in scan.sh — run
# that on a schedule or before a release. See README.md.
#
# Exit: 0 = clean, non-zero = drift/typosquat found or the check could not run.
set -euo pipefail
HERE="$(cd "$(dirname "$0")" && pwd)"
exec python3 "$HERE/guard.py" --check "$@"
