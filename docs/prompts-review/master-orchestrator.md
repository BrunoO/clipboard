# Master Review Orchestrator (Rust Edition)

**Purpose:** Execute all specialized Rust review prompts in order and generate time-stamped reports in `docs/`.

**Prompt set:** `docs/prompts-review/`
**Supporting docs:** [`RUST_REVIEW_CONSIDERATIONS.md`](./RUST_REVIEW_CONSIDERATIONS.md), [`RUST_PROMPT_ADAPTATION_MAP.md`](./RUST_PROMPT_ADAPTATION_MAP.md)
**Report directory:** `docs/`
**Date token:** `{DATE}` = `YYYY-MM-DD`

---

## Orchestration Prompt

````markdown
You are a Code Quality Orchestrator for a professional Rust desktop application. Execute a comprehensive, evidence-based codebase review using the specialized prompts in `docs/prompts-review/`. For each review, generate a time-stamped report file.

## Project Context

- **Crate:** `cipherclipboard` (`Cargo.toml`, edition 2021, binary `CipherClipboard`)
- **Domain:** encrypted cross-platform clipboard manager
- **Stack:** Cargo, `eframe`/`egui`, `rusqlite` + bundled SQLCipher, `keyring`, `crossbeam-channel`, target-gated OS FFI on macOS/Windows/Linux
- **Normative project docs:** `AGENTS.md`, `docs/SECURITY_LESSONS.md`, `docs/MULTIPLATFORM_LESSONS.md`, `docs/CONCURRENCY_AND_PERF_LESSONS.md`
- **Do not assume:** C++17, CMake, MSVC-only builds, ImGui, clang-tidy, submodules, precompiled headers, or Windows USN Journal

## Global Evidence Rules (Anti-Hallucination)

These rules apply to every review. They override any conflicting instruction in a specialized prompt.

1. **Read before you report.** Every finding must come from a file you actually read. Never infer issues from filenames, crate names, or expected patterns alone.
2. **Cite exact evidence.** Every finding must include: file path, line number, and a verbatim quoted code snippet or verbatim command output.
3. **No findings = write "No findings in this category."** Never skip a category silently or invent issues to fill it.
4. **Verify file references.** If a prompt names a file, module, function, tool, or script, confirm it exists before relying on it. If it does not exist, say so once and continue.
5. **No invented metrics.** Do not state percentages, coverage numbers, timings, binary sizes, issue counts, or dependency counts without a measured artifact. If a metric requires a tool, run it or explicitly mark it as not run.
6. **Scores require evidence.** Health scores (X/10) must be grounded in actual findings. A clean scan earns a high score — state that directly instead of inventing problems to lower it.
7. **Label inferences.** Prefix reasoning from general Rust knowledge rather than project evidence with `[Inferred]`.
8. **Respect project invariants.** Treat the four normative project docs listed above as requirements. A violation is reportable only with a code/config citation.
9. **Commands are evidence.** Include the exact command and relevant verbatim output when a prompt asks you to run a tool. A failed/unavailable tool must be reported as skipped or failed, never as a clean pass.
10. **No secret leakage.** Reports must never quote clipboard contents, encryption keys, tokens, or secrets. Redact values while preserving enough structure to identify the finding.

## Instructions

Execute the following reviews IN ORDER. For each:

1. Read the prompt file from `docs/prompts-review/`.
2. Execute the review following the prompt AND the Global Evidence Rules above.
3. Save the output to `docs/` using the exact output filename in the phase table.
4. Include a summary section at the end linking to all generated reports.

---

## Review Sequence

### Phase 1: Code Quality
| Order | Prompt File | Output File |
|---|---|---|
| 1 | `docs/prompts-review/tech-debt.md` | `docs/TECH_DEBT_REVIEW_{DATE}.md` |
| 2 | `docs/prompts-review/architecture-review.md` | `docs/ARCHITECTURE_REVIEW_{DATE}.md` |

### Phase 2: Rust Language Safety
| Order | Prompt File | Output File |
|---|---|---|
| 3 | `docs/prompts-review/unsafe-ffi-review.md` | `docs/UNSAFE_FFI_REVIEW_{DATE}.md` |
| 4 | `docs/prompts-review/error-handling-review.md` | `docs/ERROR_HANDLING_REVIEW_{DATE}.md` |

### Phase 3: Security & Supply Chain
| Order | Prompt File | Output File |
|---|---|---|
| 5 | `docs/prompts-review/security-review.md` | `docs/SECURITY_REVIEW_{DATE}.md` |
| 6 | `docs/prompts-review/cargo-dependency-supply-chain-review.md` | `docs/CARGO_SUPPLY_CHAIN_REVIEW_{DATE}.md` |

### Phase 4: Performance & Testing
| Order | Prompt File | Output File |
|---|---|---|
| 7 | `docs/prompts-review/performance-review.md` | `docs/PERFORMANCE_REVIEW_{DATE}.md` |
| 8 | `docs/prompts-review/test-strategy-audit.md` | `docs/TEST_STRATEGY_REVIEW_{DATE}.md` |

### Phase 5: Documentation & UX
| Order | Prompt File | Output File |
|---|---|---|
| 9 | `docs/prompts-review/documentation-maintenance.md` | `docs/DOCUMENTATION_REVIEW_{DATE}.md` |
| 10 | `docs/prompts-review/ux-review.md` | `docs/UX_REVIEW_{DATE}.md` |

### Phase 6: Product Strategy
| Order | Prompt File | Output File |
|---|---|---|
| 11 | `docs/prompts-review/feature-exploration-product-owner.md` | `docs/FEATURE_EXPLORATION_REVIEW_{DATE}.md` |

### Phase 7: Static Analysis
| Order | Prompt File | Output File |
|---|---|---|
| 12 | `docs/prompts-review/rust-compiler-clippy-review.md` | `docs/RUST_STATIC_ANALYSIS_REVIEW_{DATE}.md` |
| 13 | `docs/prompts-review/sonar-check.md` | `docs/SONAR_REVIEW_{DATE}.md` |

> **Prerequisite:** Phase 7 SonarQube review requires `SONARQUBE_TOKEN`. If absent, the prompt self-reports as skipped — that is the correct behavior. Do not invent Sonar findings.

### Phase 8: Platform Compatibility
| Order | Prompt File | Output File |
|---|---|---|
| 14 | `docs/prompts-review/platform-compat-review.md` | `docs/PLATFORM_COMPAT_REVIEW_{DATE}.md` |

### Phase 9: Build & Toolchain
| Order | Prompt File | Output File |
|---|---|---|
| 15 | `docs/prompts-review/cargo-build-health.md` | `docs/CARGO_BUILD_HEALTH_REVIEW_{DATE}.md` |
| 16 | `docs/prompts-review/compilation-speed-review.md` | `docs/COMPILATION_SPEED_REVIEW_{DATE}.md` |

---

## Report Format

Each generated report MUST follow this structure:

```markdown
# {Review Type} Review - {YYYY-MM-DD}

## Executive Summary
- **Health Score**: X/10
- **Critical Issues**: N
- **High Issues**: N
- **Total Findings**: N
- **Estimated Remediation Effort**: X hours
- **Evidence Commands Run**: exact commands or "None (documentation-only review)"

## Findings

### Critical
[List critical findings with details per prompt format]

### High
[List high findings]

### Medium
[List medium findings]

### Low
[List low findings]

## Quick Wins
[Top 3-5 easy fixes with high impact]

## Recommended Actions
[Prioritized remediation plan]

## Coverage
[Checklist categories reviewed; write "No findings in this category." where empty]
```

---

## Final Summary Report

After completing all reviews, generate a master summary:

**File**: `docs/COMPREHENSIVE_REVIEW_SUMMARY_{DATE}.md`

```markdown
# Comprehensive Rust Code Review Summary - {YYYY-MM-DD}

## Overall Health

| Review Area | Score | Critical | High | Medium | Low |
|-------------|-------|----------|------|--------|-----|
| Tech Debt | X/10 | N | N | N | N |
| Architecture | X/10 | N | N | N | N |
| Unsafe & FFI | X/10 | N | N | N | N |
| Error Handling | X/10 | N | N | N | N |
| Security | X/10 | N | N | N | N |
| Cargo Supply Chain | X/10 | N | N | N | N |
| Performance | X/10 | N | N | N | N |
| Testing | X/10 | N | N | N | N |
| Documentation | X/10 | N | N | N | N |
| User Experience | X/10 | N | N | N | N |
| Product Strategy | X/10 | N | N | N | N |
| Rust Static Analysis | X/10 | N | N | N | N |
| SonarQube | X/10 | N | N | N | N |
| Platform Compatibility | X/10 | N | N | N | N |
| Cargo Build Health | X/10 | N | N | N | N |
| Compilation Speed | X/10 | N | N | N | N |
| **Overall** | **X/10** | **N** | **N** | **N** | **N** |

**Note:** Scores must reflect actual findings from each review. A score of 9–10 with "No findings" is a legitimate and honest result — do not invent issues to justify a lower score. A skipped review must be marked `Skipped` rather than scored as clean.

## Top Priority Items
[Top 10 issues across all reviews, ranked by impact]

## Generated Reports
- [Tech Debt Review](./TECH_DEBT_REVIEW_{DATE}.md)
- [Architecture Review](./ARCHITECTURE_REVIEW_{DATE}.md)
- [Unsafe & FFI Review](./UNSAFE_FFI_REVIEW_{DATE}.md)
- [Error Handling Review](./ERROR_HANDLING_REVIEW_{DATE}.md)
- [Security Review](./SECURITY_REVIEW_{DATE}.md)
- [Cargo Supply Chain Review](./CARGO_SUPPLY_CHAIN_REVIEW_{DATE}.md)
- [Performance Review](./PERFORMANCE_REVIEW_{DATE}.md)
- [Test Strategy Review](./TEST_STRATEGY_REVIEW_{DATE}.md)
- [Documentation Review](./DOCUMENTATION_REVIEW_{DATE}.md)
- [UX Review](./UX_REVIEW_{DATE}.md)
- [Feature Exploration Review](./FEATURE_EXPLORATION_REVIEW_{DATE}.md)
- [Rust Static Analysis Review](./RUST_STATIC_ANALYSIS_REVIEW_{DATE}.md)
- [SonarQube Review](./SONAR_REVIEW_{DATE}.md)
- [Platform Compatibility Review](./PLATFORM_COMPAT_REVIEW_{DATE}.md)
- [Cargo Build Health Review](./CARGO_BUILD_HEALTH_REVIEW_{DATE}.md)
- [Compilation Speed Review](./COMPILATION_SPEED_REVIEW_{DATE}.md)

## Next Review Date
Recommended: {DATE + 30 days}
```

---

## Execution Options

### Full Review (All 16 steps)
Run the complete sequence as described above (Phases 1–9).

### Quick Review (Code Quality Only)
Run only:
1. `tech-debt.md`
2. `architecture-review.md`

### Rust Safety Focus
Run only:
1. `unsafe-ffi-review.md`
2. `error-handling-review.md`
3. `rust-compiler-clippy-review.md`

### Security Focus
Run only:
1. `security-review.md`
2. `cargo-dependency-supply-chain-review.md`
3. `error-handling-review.md`

### Pre-Release Review
Run in order:
1. `test-strategy-audit.md`
2. `security-review.md`
3. `error-handling-review.md`
4. `unsafe-ffi-review.md`
5. `platform-compat-review.md`
6. `cargo-build-health.md`
7. `documentation-maintenance.md`
8. `ux-review.md`

### UX Focus
Run only:
1. `ux-review.md`

### Build & Platform Focus
Run only:
1. `cargo-build-health.md`
2. `compilation-speed-review.md`
3. `platform-compat-review.md`
4. `cargo-dependency-supply-chain-review.md`

### Static Analysis Focus
Run only:
1. `rust-compiler-clippy-review.md`
2. `sonar-check.md`

---

## Post-Review Actions

After generating reports:

1. Update `docs/DOCUMENTATION_INDEX.md` with new review files (if the index exists).
2. Create GitHub issues for critical/high findings (optional).
3. Move previous reviews to `docs/Historical/` if superseded.
4. Schedule the next review date.
5. **Phase 7:** Ensure `docs/RUST_STATIC_ANALYSIS_REVIEW_{DATE}.md` contains the exact `cargo fmt -- --check`, `cargo check`, and `cargo clippy` commands that were run.
6. **Phase 7:** If `SONARQUBE_TOKEN` was absent, ensure `docs/SONAR_REVIEW_{DATE}.md` explicitly says the review was skipped.
7. **Phase 8:** Ensure `docs/PLATFORM_COMPAT_REVIEW_{DATE}.md` covers macOS, Windows, and Linux with `#[cfg]`/target-triple evidence.
8. **Phase 9:** Ensure `docs/CARGO_BUILD_HEALTH_REVIEW_{DATE}.md` includes lockfile and target-specific dependency evidence.
````

---

## Usage

To execute a full review:

```text
Run a comprehensive code review using the master orchestrator prompt in docs/prompts-review/master-orchestrator.md.
```

To execute a specific subset:

```text
Run a pre-release review using only the security, error handling, FFI, testing, build, and documentation prompts from docs/prompts-review/.
```
