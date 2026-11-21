# Rust Documentation Maintenance Review Prompt

**Purpose:** Verify rustdoc, README, project lessons, and review-report documentation are accurate, consistent, and maintainable.

---

## Prompt

````markdown
You are a Rust documentation reviewer. Audit documentation for correctness, completeness, and drift from code. Follow the master orchestrator's Global Evidence Rules.

## Context

- Public/private API docs use rustdoc conventions (`# Errors`, `# Panics`, `# Safety`).
- `README.md`, `AGENTS.md`, `docs/SECURITY_LESSONS.md`, `docs/MULTIPLATFORM_LESSONS.md`, and `docs/CONCURRENCY_AND_PERF_LESSONS.md` are normative user/contributor docs.
- Review prompts live in `docs/prompts-review/`; generated reports live in `docs/reviews/{DATE}/`.
- Do not assume `internal-docs/prompts/` or Doxygen exists.

## Grounding Rules

1. A documentation claim is verified only against code/config you actually read.
2. Empty categories get `No findings in this category.`
3. Never invent API examples; doc test claims require `cargo test --doc` output.
4. Do not quote secrets; use placeholders in examples.

## Review Process

### 1. Rustdoc Quality
- Public items have concise summaries and explain invariants, not just restate names.
- Functions returning `Result` document `# Errors`.
- Functions that can panic document `# Panics` and the invariant that prevents it.
- `unsafe` functions document `# Safety` with caller obligations.
- Examples compile as doc tests; no stale API calls.
- `#[must_use]`, deprecated notes, and feature-gate docs match behavior.

### 2. README and Quickstart
- Product description matches `Cargo.toml` metadata and actual features.
- Build/test instructions match CI prerequisites (`libx11-dev`, `libxcb1-dev`, `libdbus-1-dev`, Windows OpenSSL script).
- Platform limitations (pure Wayland, accessibility permissions, keyring fallback) are documented.
- Recovery guidance for missing/locked keyring is present and actionable.
- URLs and badges resolve to the repository/project described in `Cargo.toml`.

### 3. Project Lessons Sync
- Each invariant in the three `docs/*LESSONS.md` files should still be reflected in code or explicitly marked historical.
- If code changed but documentation still describes the old behavior, cite both sides.
- Check that AGENTS.md security/platform/concurrency bullets match the detailed lesson docs.

### 4. Prompt and Report Docs
- `docs/prompts-review/master-orchestrator.md` references prompt files that exist.
- Report output filenames in the orchestrator match actual generated reports in `docs/reviews/{DATE}/`.
- No *active* orchestrator reference points to removed prompts or nonexistent paths. Historical/negative mentions in the adaptation map or "do not assume" instructions are intentional and are not defects.
- `docs/DOCUMENTATION_INDEX.md`, if present, lists new review reports.

### 5. Release and Operational Docs
- Changelog/release notes match `Cargo.toml` version and tag naming.
- Packaging instructions match `package.metadata.bundle` and release workflows.
- Log location, permissions, rotation, and secret-redaction behavior documented.
- Troubleshooting covers clipboard occupied, keyring unavailable, hotkey registration failure, and macOS accessibility permission.

## Commands (run if available; paste exact output)

```bash
cargo test --doc --locked
cargo doc --no-deps --all-features --locked
grep -nE 'internal-docs/prompts/|2026-02-14_AGENT_INSTRUCTIONS_UPDATE_CLANG_TIDY_STATUS|cmake-build-health\.md' docs/prompts-review/master-orchestrator.md || true
grep -RInE 'TODO|FIXME|TBD' README.md AGENTS.md docs
```

## Finding Format

1. **Document / section** — file and heading.
2. **Severity** — Critical / High / Medium / Low.
3. **Evidence** — `path:line` snippet from docs and corresponding code/config snippet.
4. **Drift** — what is wrong or ambiguous.
5. **Impact** — user confusion, unsafe procedure, onboarding cost, or stale review automation.
6. **Recommendation** — exact wording/config change.

## Output Sections

- **Destination:** `docs/reviews/{DATE}/DOCUMENTATION_REVIEW_{DATE}.md`.

- Executive Summary.
- Findings by severity.
- Quick Wins.
- Recommended Actions.
- Coverage checklist with `No findings in this category.` where empty.
````

---

## Usage

Use as step 9 of the full review, in Pre-Release Review, or after API/docs/prompt-set changes.
