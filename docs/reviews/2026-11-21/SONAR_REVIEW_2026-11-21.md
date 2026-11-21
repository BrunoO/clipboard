# SonarQube Review - 2026-11-21

## Executive Summary

- **Health Score**: 8/10 — one open MAJOR `VULNERABILITY` in a CI workflow; no BLOCKER or CRITICAL issues in the returned dataset, so the score is not lower; not 9–10 because a security issue remains open.
- **Critical Issues**: 0
- **High Issues**: 1
- **Total Findings**: 1
- **Estimated Remediation Effort**: 1 hour (Sonar-reported `effort: "1h"` for the single issue; no separate estimate invented).
- **Evidence Commands Run**:
  - `echo "Token set: ${SONARQUBE_TOKEN:+yes}"` → `Token set: yes`
  - `./scripts/fetch_sonar_results.sh --format json --open-only` (run by parent; verbatim log at `/private/var/folders/04/j6_9v0hn5mz2syff3s6sdwp80000gp/T/opencode/review-evidence/sonar-fetch.log`) → `Total issues fetched: 1`, `SONAR_FETCH_EXIT=0`
  - `sed -n '86,88p' .github/workflows/ci.yml` → `      - name: Build debug binary` / `        run: cargo build`
  - Direct read of `sonar-results/sonarqube_issues.json` (42 lines; 1 open issue). No other Sonar issues were fetched or inferred.
  - No `cargo build`, `cargo clippy`, `cargo test`, or timing command was run for this report.

## Findings

### Critical

No findings in this category.

### High

**H-1 — `githubactions:S8549`: CI workflow build does not lock resolved dependency versions**

- **Issue ID**: `AaH6ohLcsU18O1jEuvWW`
- **Rule**: `githubactions:S8549` — verbatim from `sonar-results/sonarqube_issues.json:4`. This is a `githubactions:` rule because the issue is in a workflow; it is **not** a `rust:Sxxxx` or `cpp:Sxxxx` rule, despite the generic rule-ID example in `docs/prompts-review/sonar-check.md:18`.
- **Location**: `.github/workflows/ci.yml:87`; component `BrunoO_clipboard:.github/workflows/ci.yml` (`sonar-results/sonarqube_issues.json:6`).
- **Severity**: MAJOR (`sonar-results/sonarqube_issues.json:5`).
- **Type**: VULNERABILITY (`sonar-results/sonarqube_issues.json:25`).
- **Status**: OPEN (`sonar-results/sonarqube_issues.json:17`).
- **Created**: `2026-11-20T20:23:07+0000` (`sonar-results/sonarqube_issues.json:23`).
- **Message**: "Using dependencies without locking resolved versions is security-sensitive." (`sonar-results/sonarqube_issues.json:18`).
- **Effort**: `1h` (`sonar-results/sonarqube_issues.json:19-20`).
- **Verbatim JSON evidence** (`sonar-results/sonarqube_issues.json:3-8`):

```json
    "key": "AaH6ohLcsU18O1jEuvWW",
    "rule": "githubactions:S8549",
    "severity": "MAJOR",
    "component": "BrunoO_clipboard:.github/workflows/ci.yml",
    "project": "BrunoO_clipboard",
    "line": 87,
```

  and (`sonar-results/sonarqube_issues.json:17-19`, `:23`, `:25`):

```json
    "status": "OPEN",
    "message": "Using dependencies without locking resolved versions is security-sensitive.",
    "effort": "1h",
    "debt": "1h",
    "creationDate": "2026-11-20T20:23:07+0000",
    "type": "VULNERABILITY",
```

- **Verbatim command output** (`sed -n '86,88p' .github/workflows/ci.yml`):

```text
      - name: Build debug binary
        run: cargo build

```

  The flagged command at `.github/workflows/ci.yml:87` is exactly `        run: cargo build` — no `--locked` flag.
- **Verbatim fetch-log excerpt** (`/private/var/folders/04/j6_9v0hn5mz2syff3s6sdwp80000gp/T/opencode/review-evidence/sonar-fetch.log:12`, `:20`):

```text
Total issues fetched: 1
SONAR_FETCH_EXIT=0
```

- **Impact**: [Inferred] the macOS quit-smoke build at `.github/workflows/ci.yml:86-114` can resolve a semver-compatible dependency graph different from `Cargo.lock`, while the release builds use `--locked` (`.github/workflows/release.yml:70` and `:218`). No build result is claimed here.
- **Recommendation**: change `.github/workflows/ci.yml:87` from `run: cargo build` to `run: cargo build --locked`; then re-run the Sonar fetch after re-analysis. Expected fix cost is the Sonar-reported 1h. No claim is made that the issue is now closed.

### Medium

No findings in this category.

### Low

No findings in this category.

## Quick Wins

1. Change `.github/workflows/ci.yml:87` to `run: cargo build --locked` (one-line fix; Sonar effort `1h`).
2. [Inferred] Apply the same `--locked` convention to the other CI cargo gates that currently omit it: `.github/workflows/ci.yml:38`, `:69`, `:72`, and `.github/workflows/sonarcloud.yml:38` — consistency with `.github/workflows/release.yml:70`, `:218`.
3. Re-run `./scripts/fetch_sonar_results.sh --format json --open-only` after the fix and compare the issue count; do not report closure before the fetch result shows it.
4. Keep the rule-family detail accurate in notes and dashboards: this is `githubactions:S8549`, not a Rust rule.

## Recommended Actions

1. **Immediate (this week)**: implement the one-line `--locked` fix at `.github/workflows/ci.yml:87` and re-fetch Sonar results. This addresses the only open issue in the dataset.
2. **Short-term (this month)**: [Inferred] align all workflow cargo invocations with `--locked` so CI, coverage, and release resolve the same graph; verify by reading each workflow line cited above. The separate build-health review covers the same `--locked` gap from the reproducibility angle.
3. **Long-term (this quarter)**: [Inferred] add a workflow lint or review checklist item that flags `cargo` commands in `.github/workflows/` without `--locked`; no such hook was observed in `scripts/` (directory listing read), so this would be a new control.
4. **Rule coverage**: no `rust:Sxxxx` issue was returned, so this report makes no Rust-code claims. Any Rust-specific prevention ideas belong in the static-analysis report, not here.

## Coverage

- **Token availability**: verified — `echo "Token set: ${SONARQUBE_TOKEN:+yes}"` → `Token set: yes`.
- **Fetch status**: `./scripts/fetch_sonar_results.sh --format json --open-only` → 1 open issue, exit 0 (log path quoted above).
- **Dataset scope**: `sonar-results/sonarqube_issues.json` contains exactly one issue (`AaH6ohLcsU18O1jEuvWW`); no other issues are reported. Per parent coverage, this file is `excluded` from the graph index and was read directly.
- **Rule Violation Patterns**: No findings in this category. (Single issue; no repetition or hotspot possible.)
- **Severity Analysis**: No findings in this category. (One MAJOR issue; no BLOCKER/CRITICAL/MINOR/INFO issues.)
- **File/Module Analysis**: No findings in this category. (The only issue is in `.github/workflows/ci.yml`, not in target-gated source.)
- **Temporal Analysis**: No findings in this category. (One data point dated `2026-11-20T20:23:07+0000`; no trend support.)
- **Prevention / missing rules**: [Inferred] recommendations are listed in Recommended Actions; they are not Sonar findings.
- **Rust rule coverage (`rust:Sxxxx`)**: No findings in this category.
- **Executed build/clippy/test/timing commands**: None for this report; no pass/fail or timing result is claimed.
