#!/usr/bin/env bash
# Proof harness for the cargo-deny policy (deny.toml).
#
# Builds throwaway fixture crates in a temp dir and shows each cargo-deny gate go
# RED on the thing it is meant to catch, then confirms the real tree is GREEN.
# Fully offline: the "git source" case uses a LOCAL git repo (git+file://), which
# cargo-deny treats as an unknown git source exactly like a remote one.
#
# Run: tools/supplychain/deny-selftest.sh   (exit 0 = every gate fired as designed)
set -u

HERE="$(cd "$(dirname "$0")" && pwd)"
ROOT="$(cd "$HERE/../.." && pwd)"
DENY="$ROOT/deny.toml"
WORK="$(mktemp -d)"
trap 'rm -rf "$WORK"' EXIT

PASS=0; FAIL=0
ok()  { echo "  PASS: $1"; PASS=$((PASS+1)); }
bad() { echo "  FAIL: $1"; FAIL=$((FAIL+1)); }

if ! cargo deny --version >/dev/null 2>&1; then
  echo "cargo-deny not installed (cargo install cargo-deny --locked). Cannot run proofs."
  exit 3
fi

mkcrate() { # dir name license
  mkdir -p "$1/src"; : > "$1/src/lib.rs"
  cat > "$1/Cargo.toml" <<EOF
[package]
name = "$2"
version = "0.1.0"
edition = "2021"
license = "$3"
EOF
}

echo "=== deny.toml: $DENY ==="

# ---- LICENSES: a dependency with a non-allow-listed licence -> RED ----------
echo
echo "### licenses  a GPL-3.0 dependency must be REJECTED"
mkcrate "$WORK/lic/badlicense" badlicense "GPL-3.0"
mkcrate "$WORK/lic" fixture_lic "MIT"
cat >> "$WORK/lic/Cargo.toml" <<EOF
[dependencies]
badlicense = { path = "badlicense" }
EOF
( cd "$WORK/lic" && cargo generate-lockfile >/dev/null 2>&1 )
OUT="$(cargo deny --manifest-path "$WORK/lic/Cargo.toml" --config "$DENY" check licenses 2>&1)"; RC=$?
echo "$OUT" | grep -iE 'rejected|GPL-3.0|licenses FAILED' | sed 's/^/    /'
{ [ $RC -ne 0 ] && echo "$OUT" | grep -q "GPL-3.0" && echo "$OUT" | grep -q "licenses FAILED"; } \
  && ok "licenses gate rejected GPL-3.0 (exit $RC)" || bad "licenses gate did NOT fire"

# ---- SOURCES: a git dependency (local git+file://) -> RED -------------------
echo
echo "### sources  a git-sourced dependency must be REJECTED (crates.io only)"
mkcrate "$WORK/gitdep" gitdep "MIT"
( cd "$WORK/gitdep" && git init -q && git add -A \
  && GIT_AUTHOR_NAME=t GIT_AUTHOR_EMAIL=t@t GIT_COMMITTER_NAME=t GIT_COMMITTER_EMAIL=t@t \
     git commit -qm init )
mkcrate "$WORK/src" fixture_src "MIT"
cat >> "$WORK/src/Cargo.toml" <<EOF
[dependencies]
gitdep = { git = "file://$WORK/gitdep" }
EOF
( cd "$WORK/src" && cargo generate-lockfile >/dev/null 2>&1 )
OUT="$(cargo deny --manifest-path "$WORK/src/Cargo.toml" --config "$DENY" check sources 2>&1)"; RC=$?
echo "$OUT" | grep -iE 'source-not-allowed|sources FAILED' | sed 's/^/    /'
{ [ $RC -ne 0 ] && echo "$OUT" | grep -q "source-not-allowed"; } \
  && ok "sources gate rejected a git source (exit $RC)" || bad "sources gate did NOT fire"

# ---- BANS: a hard-banned crate present in the real tree -> RED --------------
echo
echo "### bans  a crate on the deny list must be REJECTED"
BANCFG="$WORK/deny_ban.toml"
sed 's/^deny = \[\]/deny = [{ name = "png" }]/' "$DENY" > "$BANCFG"
OUT="$(cargo deny --manifest-path "$ROOT/Cargo.toml" --config "$BANCFG" check bans 2>&1)"; RC=$?
echo "$OUT" | grep -iE "explicitly banned|bans FAILED" | sed 's/^/    /'
{ [ $RC -ne 0 ] && echo "$OUT" | grep -q "explicitly banned"; } \
  && ok "bans gate rejected a banned crate (exit $RC)" || bad "bans gate did NOT fire"

# ---- DUPLICATES: detection (warn, not fatal) on the real tree --------------
echo
echo "### duplicates  multiple versions are DETECTED (reported, not fatal)"
OUT="$(cargo deny --manifest-path "$ROOT/Cargo.toml" --config "$DENY" check bans 2>&1)"
echo "$OUT" | grep -iE 'duplicate' | head -4 | sed 's/^/    /'
echo "$OUT" | grep -qiE 'duplicate' \
  && ok "duplicate detection reported multiple versions" || bad "no duplicates reported"

# ---- CLEAN: the real tree with the real policy -> GREEN --------------------
echo
echo "### clean  the real tree must pass every section"
OUT="$(cargo deny --manifest-path "$ROOT/Cargo.toml" --config "$DENY" check 2>&1)"; RC=$?
echo "$OUT" | grep -iE 'advisories|bans|licenses|sources' | tail -1 | sed 's/^/    /'
{ [ $RC -eq 0 ] && echo "$OUT" | grep -q "sources ok"; } \
  && ok "real tree is green (exit 0)" || bad "real tree not green (exit $RC)"

echo
echo "=== $PASS passed, $FAIL failed ==="
[ $FAIL -eq 0 ]
