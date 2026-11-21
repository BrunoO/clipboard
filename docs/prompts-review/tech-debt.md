# Rust Technical Debt Review Prompt

**Purpose:** Identify maintainability debt in the Rust crate without inventing problems or restating compiler-enforced style rules.

---

## Prompt

````markdown
You are a Senior Rust Maintainability Engineer. Review this codebase for technical debt using a Rust/Cargo reference frame. Follow the master orchestrator's Global Evidence Rules.

## Context

- Crate: `cipherclipboard`, edition 2021, binary `CipherClipboard`.
- Review `Cargo.toml`, `Cargo.lock`, `.rustfmt.toml`, `deny.toml`, `AGENTS.md`, and `docs/*LESSONS.md` before judging conventions.
- Do not search for C++ concepts (`unique_ptr`, RAII classes, CMake, PCH, MSVC, clang-tidy) as if they applied here.

## Grounding Rules

1. Every finding needs `path:line` plus a verbatim snippet or verbatim command output.
2. Do not report a line count, file size, dependency count, or `TODO` count unless you measured it.
3. A clean category gets `No findings in this category.`
4. `[Inferred]` prefixes any recommendation not directly tied to project evidence.
5. Never quote secrets, clipboard contents, or full key material.

## Review Process

### 1. Debt Markers and Suppressions
- Search source and config for `TODO`, `FIXME`, `HACK`, `XXX`, `NOTE`, `#[allow(...)]`, `#![allow(...)]`, and `#[cfg(test)]` workarounds.
- For each hit, decide whether it is:
  - an actionable debt item,
  - an intentional documented invariant,
  - a false positive.
- Every `#[allow]` that weakens rustc/clippy must have a narrow scope and a reason. Flag broad or unexplained suppressions.

### 2. Module and Ownership Cohesion
- Map `src/*.rs` and `src/platform/*.rs` responsibilities from actual `mod`/`use` declarations.
- Look for modules that own too many unrelated concerns (clipboard + DB + UI + FFI in one file) or thin modules that exist only to forward calls.
- Review repeated `clone()` calls that exist solely to satisfy borrow checking. Cite the call and explain the ownership boundary problem only if the code shows it.
- Review `Arc<Mutex<_>>`, `Arc<RwLock<_>>`, `Rc<RefCell<_>>`, and `unsafe impl Send/Sync` usage. Debt exists when the abstraction leaks or lacks a rationale.

### 3. API and Type Debt
- Public functions taking `&String`, `&Vec<T>`, `&Box<T>`, or `&Option<T>` where slices/iterators/`Option<&T>` would be clearer.
- Missing `#[must_use]`, `#[non_exhaustive]`, `Default`, `is_empty`, `From`, or `TryFrom` where the public contract implies them.
- Stringly typed state machines or booleans that should be enums/newtypes.
- `Result<_, String>`/`Result<_, Box<dyn Error>>` in library boundaries where a typed error would let callers react.

### 4. Error and Panic Debt
- `unwrap()`, `expect()`, `panic!()`, indexing, and arithmetic assumptions outside tests.
- `let _ = ...` or ignored `Result` without a recorded reason.
- Duplicated error conversion boilerplate that could be centralized with `thiserror`/`From`.

### 5. Cargo and Feature Debt
- Unused or redundant features in `Cargo.toml`.
- Target-specific dependencies without matching `#[cfg]` usage.
- Duplicate or conflicting dependency versions (`cargo tree -d`) that increase binary size or create trait-bound friction.
- Release profile settings that no longer match documented goals (binary size, startup time, crash reporting).

### 6. Platform and Testing Debt
- Repeated `#[cfg(target_os = "...")]` branches that could be consolidated through a safe facade or `const fn platform_select`.
- FFI logic outside the allowed backend modules (`src/platform/macos.rs`, `src/platform/windows.rs`).
- `#[ignore]`, empty tests, or tests that assert implementation details rather than contracts.
- Mocks that diverge from production error/poison behavior.

### 7. Documentation Debt
- Comments that contradict `AGENTS.md` or `docs/*LESSONS.md`.
- Public APIs lacking `# Errors`, `# Panics`, or `# Safety` where applicable.
- README/quickstart steps that cannot be verified from Cargo metadata.

## Commands (run if available; paste exact output)

```bash
cargo metadata --format-version 1
cargo tree -d
grep -RInE 'TODO|FIXME|HACK|XXX|#\[allow|#!\[allow' src Cargo.toml .github 2>/dev/null
wc -l src/*.rs src/platform/*.rs 2>/dev/null
```

Do not treat command output as findings by itself; interpret and cite it.

## Finding Format

For each finding:

1. **Title** — concise debt name.
2. **Severity** — Critical / High / Medium / Low.
3. **Evidence** — `path:line` and verbatim snippet/output.
4. **Why it is debt** — concrete maintainability, correctness, or onboarding cost.
5. **Recommendation** — smallest safe remediation.
6. **Effort** — S / M / L with a short justification.

## Output Sections

- Executive Summary with health score and counts.
- Findings by severity.
- Quick Wins (3–5).
- Recommended Actions ordered by payoff.
- Coverage checklist with `No findings in this category.` where empty.
````

---

## Usage

Use this prompt as step 1 of the full Rust review or as a standalone maintainability audit.
