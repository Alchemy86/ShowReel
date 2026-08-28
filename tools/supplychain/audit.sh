#!/usr/bin/env bash
# Known-vulnerability scan (cargo audit / RustSec advisory DB) with LOUD offline
# handling.
#
# The whole point of this wrapper: `cargo audit` returns exit 1 for BOTH
# "a vulnerability was found" and "the advisory database could not be loaded".
# A naive `cargo audit || <handle>` therefore cannot tell a real finding from a
# check that never ran — and the classic mistake (`cargo audit || true`, added to
# stop unmaintained-warnings breaking CI) turns a failed fetch into a silent PASS.
# This script instead classifies the outcome from the output and NEVER reports a
# green when the scan could not actually run.
#
# Exit codes:
#   0  scan ran, no vulnerabilities (warnings, e.g. unmaintained, are reported, not fatal)
#   2  scan ran, VULNERABILITIES found
#   3  scan COULD NOT RUN (advisory DB unavailable and no cached copy) — loud, never silent
#
# Offline behaviour: if the live fetch fails but a cached advisory DB exists, the
# scan still runs against the cache and warns LOUDLY that results may be stale
# (printing the DB's age). Only when there is no DB at all does it fail with 3.
set -u

CARGO_HOME_DIR="${CARGO_HOME:-$HOME/.cargo}"
DB="$CARGO_HOME_DIR/advisory-db"
# Optional: warn if a (stale-path) DB is older than this many days.
STALE_DAYS="${SUPPLYCHAIN_AUDIT_STALE_DAYS:-14}"

say() { printf '%s\n' "$*"; }
rule() { printf '%s\n' "------------------------------------------------------------"; }

db_age_days() {
  [ -d "$DB/.git" ] || { echo "unknown"; return; }
  local last now
  last="$(git -C "$DB" log -1 --format=%ct 2>/dev/null)" || { echo "unknown"; return; }
  now="$(date +%s)"
  echo $(( (now - last) / 86400 ))
}

classify_and_exit() {
  # $1 = output text, $2 = cargo-audit exit code, $3 = "stale" or "fresh"
  local out="$1" rc="$2" mode="$3"
  if printf '%s' "$out" | grep -qiE 'vulnerabilit(y|ies) found'; then
    rule; say "AUDIT RESULT: RED — vulnerabilities found (advisory DB: $mode)."
    [ "$mode" = "stale" ] && say "WARNING: run against a STALE advisory DB (age ${_AGE} days) — a newer advisory may exist."
    exit 2
  fi
  if [ "$rc" -eq 0 ]; then
    rule
    if printf '%s' "$out" | grep -qiE 'warning'; then
      say "AUDIT RESULT: GREEN (no vulnerabilities) — but advisory WARNINGS present above (unmaintained/yanked). Not fatal; review them."
    else
      say "AUDIT RESULT: GREEN — no known vulnerabilities (advisory DB: $mode)."
    fi
    [ "$mode" = "stale" ] && say "WARNING: run against a STALE advisory DB (age ${_AGE} days) — re-run online to refresh."
    exit 0
  fi
  # Non-zero and NOT a vulnerability finding: the scan did not complete.
  return 1
}

say "== Known-vulnerability scan (cargo audit) =="
if ! command -v cargo-audit >/dev/null 2>&1 && ! cargo audit --version >/dev/null 2>&1; then
  rule
  say "AUDIT COULD NOT RUN: cargo-audit is not installed."
  say "Install it once with:  cargo install cargo-audit --locked"
  say "(This is a LOUD failure on purpose — a missing scanner must not read as a pass.)"
  exit 3
fi

# --- Attempt 1: normal run (fetches the advisory DB) -----------------------
OUT="$(cargo audit --color never "$@" 2>&1)"; RC=$?
say "$OUT"
_AGE="$(db_age_days)"
classify_and_exit "$OUT" "$RC" "fresh" || true

# We only reach here if the run did not complete (couldn't load/fetch the DB).
# --- Attempt 2: fall back to the cached DB, if any -------------------------
if [ -d "$DB/crates" ] || [ -d "$DB/.git" ]; then
  rule
  say "NOTE: live advisory fetch failed; retrying against the CACHED database (offline-tolerant path)."
  OUT2="$(cargo audit --color never --stale --no-fetch "$@" 2>&1)"; RC2=$?
  say "$OUT2"
  _AGE="$(db_age_days)"
  if [ "$_AGE" != "unknown" ] && [ "$_AGE" -gt "$STALE_DAYS" ]; then
    say "WARNING: cached advisory DB is ${_AGE} days old (> ${STALE_DAYS})."
  fi
  classify_and_exit "$OUT2" "$RC2" "stale" || true
fi

# --- Could not run at all --------------------------------------------------
rule
say "AUDIT COULD NOT RUN: the RustSec advisory database is unavailable and no cached"
say "copy exists at: $DB"
say "This scan is reporting a LOUD FAILURE rather than a false GREEN. Restore network"
say "access (or 'cargo audit --url <mirror>') and re-run."
exit 3
