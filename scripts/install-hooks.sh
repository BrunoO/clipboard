#!/usr/bin/env bash
# ==============================================================================
# Installs CipherClipboard Git hooks (e.g. pre-push guardrail).
# ==============================================================================
set -euo pipefail

REPO_ROOT="$(git rev-parse --show-toplevel)"
HOOK_FILE="$REPO_ROOT/.git/hooks/pre-push"

cat << 'EOF' > "$HOOK_FILE"
#!/usr/bin/env bash
set -e
echo "Running CipherClipboard local pre-flight guardrails before push..."
"$(git rev-parse --show-toplevel)/scripts/preflight.sh"
EOF

chmod +x "$HOOK_FILE"
echo "✓ Git pre-push hook successfully installed at .git/hooks/pre-push."
echo "Every 'git push' will now run formatting, Clippy, cargo-deny, and tests automatically."
