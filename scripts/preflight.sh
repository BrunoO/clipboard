#!/usr/bin/env bash
# ==============================================================================
# CipherClipboard Local Pre-Flight CI Guardrail
# Run this before committing, pushing, or tagging a release.
# ==============================================================================
set -euo pipefail

# Ensure ~/.cargo/bin is in PATH if cargo-deny was installed there
export PATH="$HOME/.cargo/bin:$PATH"

RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
NC='\033[0m' # No Color

echo -e "${BLUE}=====================================================${NC}"
echo -e "${BLUE}       CipherClipboard Local Pre-Flight Checks        ${NC}"
echo -e "${BLUE}=====================================================${NC}"

# 1. Code Formatting
echo -e "\n${YELLOW}▶ [1/4] Checking code formatting (cargo fmt)...${NC}"
if cargo fmt --check; then
    echo -e "${GREEN}✓ Formatting is clean.${NC}"
else
    echo -e "${RED}✗ Formatting errors detected! Run 'cargo fmt' to fix automatically.${NC}"
    exit 1
fi

# 2. Clippy Linter
echo -e "\n${YELLOW}▶ [2/4] Running Clippy static analysis (strict warnings)...${NC}"
if cargo clippy --all-targets --all-features -- -D warnings; then
    echo -e "${GREEN}✓ Clippy passed with zero warnings.${NC}"
else
    echo -e "${RED}✗ Clippy warnings or errors detected! Resolve them before pushing.${NC}"
    exit 1
fi

# 3. Supply Chain & License Audit (cargo-deny)
echo -e "\n${YELLOW}▶ [3/4] Running supply-chain and license checks (cargo deny)...${NC}"
if command -v cargo-deny >/dev/null 2>&1 || cargo deny --version >/dev/null 2>&1; then
    if cargo deny check; then
        echo -e "${GREEN}✓ cargo-deny passed (licenses, advisories, bans, sources ok).${NC}"
    else
        echo -e "${RED}✗ cargo-deny detected license or security issues in deny.toml!${NC}"
        exit 1
    fi
else
    echo -e "${YELLOW}⚠ cargo-deny is not installed. To install: cargo install cargo-deny --locked${NC}"
fi

# 4. Unit & Integration Tests
echo -e "\n${YELLOW}▶ [4/4] Running unit and integration tests (cargo test)...${NC}"
if cargo test; then
    echo -e "${GREEN}✓ All tests passed.${NC}"
else
    echo -e "${RED}✗ Tests failed!${NC}"
    exit 1
fi

echo -e "\n${GREEN}=====================================================${NC}"
echo -e "${GREEN}  ✓ All local guardrails passed! Safe to commit & push.${NC}"
echo -e "${GREEN}=====================================================${NC}"
