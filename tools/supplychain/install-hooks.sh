#!/usr/bin/env bash
# Install a git pre-push hook that runs the FAST supply-chain gate (check.sh).
# Idempotent; backs up any existing pre-push hook it would replace. Run once per
# clone. Emergency override for a push is `git push --no-verify`.
set -euo pipefail
HERE="$(cd "$(dirname "$0")" && pwd)"
ROOT="$(cd "$HERE/../.." && pwd)"
HOOKS="$(cd "$ROOT" && git rev-parse --git-path hooks)"
# git-path may be relative to the repo root
case "$HOOKS" in /*) ;; *) HOOKS="$ROOT/$HOOKS" ;; esac
mkdir -p "$HOOKS"
HOOK="$HOOKS/pre-push"

if [ -e "$HOOK" ] && ! grep -q 'supplychain/check.sh' "$HOOK" 2>/dev/null; then
  cp "$HOOK" "$HOOK.bak.$(date +%s)"
  echo "Backed up existing pre-push hook to $HOOK.bak.*"
fi

cat > "$HOOK" <<'EOF'
#!/usr/bin/env sh
# Supply-chain fast gate (installed by tools/supplychain/install-hooks.sh).
# Runs the offline build-script drift + typosquat check before every push.
root="$(git rev-parse --show-toplevel)"
if [ -x "$root/tools/supplychain/check.sh" ]; then
  if ! "$root/tools/supplychain/check.sh"; then
    echo ""
    echo "PUSH BLOCKED: supply-chain drift detected (tools/supplychain/check.sh)."
    echo "If the change is legitimate and reviewed, re-baseline:"
    echo "    python3 tools/supplychain/guard.py --update-baseline"
    echo "Emergency override (use sparingly): git push --no-verify"
    exit 1
  fi
fi
EOF
chmod +x "$HOOK"
echo "Installed pre-push hook: $HOOK"
echo "It runs tools/supplychain/check.sh before every push."
