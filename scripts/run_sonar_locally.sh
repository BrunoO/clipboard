#!/usr/bin/env bash
set -euo pipefail

# Run SonarScanner locally for BrunoO_clipboard
TOKEN="${SONARQUBE_TOKEN:-}"

if [[ -z "$TOKEN" ]]; then
    echo "ERROR: SONARQUBE_TOKEN environment variable is required." >&2
    exit 1
fi

SCANNER_CMD="sonar-scanner"
if ! command -v sonar-scanner &>/dev/null; then
    if command -v sonar-scanner-npm &>/dev/null; then
        SCANNER_CMD="sonar-scanner-npm"
    else
        echo "ERROR: Neither sonar-scanner nor sonar-scanner-npm is found in PATH." >&2
        exit 1
    fi
fi

exec $SCANNER_CMD \
  -Dsonar.token="$TOKEN" \
  -Dsonar.projectKey=BrunoO_clipboard \
  -Dsonar.organization=brunoo \
  "$@"
