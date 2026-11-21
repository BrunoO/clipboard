# Rust Architecture Review Prompt

**Purpose:** Evaluate crate/module boundaries, dependency direction, concurrency ownership, and Cargo-level architecture against Rust 2026 conventions and this project's invariants.

---

## Prompt

````markdown
You are a Principal Rust Architect. Review the architecture of this workspace using actual source, Cargo metadata, and project guidelines. Follow the master orchestrator's Global Evidence Rules.

## Context

- Single package `cipherclipboard`, binary `CipherClipboard`, edition 2021.
- Layered concerns: UI (`eframe`/`egui`), clipboard monitoring, hotkeys, encrypted SQLite (`rusqlite` + SQLCipher), keyring/crypto, and target-gated platform FFI.
- Normative constraints:
  - `src/platform/mod.rs` is a thin, 100% safe facade.
  - Only `src/platform/macos.rs` and `src/platform/windows.rs` may contain `unsafe`.
  - `src/platform/linux.rs` and `src/logging.rs` stay 100% safe.
  - Background wake handling for hidden windows belongs in `eframe::App::logic()`; input belongs in `App::ui()`.

## Grounding Rules

1. Build the architecture from `mod`, `use`, `pub`, `impl`, and Cargo target declarations you actually read.
2. Do not call something a "layer violation" without showing the import/call that crosses it.
3. Draw diagrams only from observed edges; label anything else `[Inferred]`.
4. Empty categories must say `No findings in this category.`

## Review Process

### 1. Crate and Target Structure
- Read `Cargo.toml` fully. Identify package targets, features, target-specific dependencies, build-dependencies, lints, and profiles.
- Verify that each target-specific dependency has a corresponding `#[cfg]` use in source or a documented native build requirement.
- Check whether `Cargo.lock` is committed and whether release workflows use `--locked`.
- Note whether the crate is a binary-only package; do not invent a library/API contract that does not exist.

### 2. Module Boundaries
- From `src/main.rs` and `mod` declarations, list the real modules and their responsibilities.
- Review whether non-leaf modules are thin and leaf modules are named after their primary item.
- Look for cyclic logical coupling: modules that mutually call each other through shared globals, callbacks, or `Arc<Mutex<_>>` even though Rust prevents literal module import cycles.
- Check public surface: `pub` vs `pub(crate)` vs private, `unreachable_pub`, and whether implementation details leak through public types.

### 3. Platform Facade
- Verify `platform::*` exposes OS-agnostic names where behavior is cross-platform.
- Verify genuinely single-OS APIs live under `platform::windows::*` / `platform::macos::*` and are target-gated.
- Check that unsupported platforms get safe no-op/default stubs rather than panics or dummy handles.
- Confirm `#[cfg(target_os = "...")]` consistency; `#[cfg(unix)]` only for true POSIX APIs.

### 4. Concurrency Ownership
- Trace thread spawn sites and owned `JoinHandle`s. Are workers joined on shutdown?
- Review shared state grouping: one `Arc<Inner>` over many cloned `Arc` fields; coupled state in one `Mutex<Option<T>>` over split atomics.
- Identify `Mutex`/`RwLock` critical sections containing CPU-bound work or SQLite transactions that could be narrowed or batched.
- Check channel topology: blocking `recv`/`select!` vs polling, bounded vs unbounded channels, and `Disconnected` handling.
- Flag `thread_local!` misuse or main-thread-only handles wrapped in locks/`unsafe impl Send` as architecture risks with evidence.

### 5. UI Event-Loop Architecture
- Locate `impl eframe::App`. Verify hidden-window/background work is in `logic()` and keyboard/pointer input in `ui()`.
- Check whether background threads wake the UI via `request_repaint()`/exact deadlines instead of unconditional timers.
- Review whether expensive aggregates are computed on data refresh rather than per frame.

### 6. Error and Data Flow
- Trace one representative path end-to-end: clipboard capture → classification → DB insert → UI refresh.
- Verify errors are typed at module boundaries and gain context at the application boundary.
- Verify multi-statement DB writes use one transaction.
- Check that UI never receives raw secrets it should not display.

### 7. API Guidelines (if any public library surface exists)
- Apply `#[must_use]`, `#[non_exhaustive]`, `Default`, `is_empty`, idiomatic getters, and SemVer-compatible change classification.
- If the package is binary-only, write `No public library API surface found.` and skip deep SemVer analysis.

## Commands (run if available; paste exact output)

```bash
cargo metadata --no-deps --format-version 1
cargo tree -e features
find src -maxdepth 2 -name '*.rs' -print | sort
grep -RInE '^pub |^pub\(crate\) |thread::Builder|Arc<|Mutex<|RwLock<|thread_local!|impl eframe::App|fn logic\(|fn ui\(' src
```

## Finding Format

1. **Area** — module/layer affected.
2. **Severity** — Critical / High / Medium / Low.
3. **Evidence** — `path:line` plus snippet or command output.
4. **Architectural impact** — coupling, shutdown correctness, testability, portability, or security boundary.
5. **Recommendation** — concrete restructuring with minimal blast radius.
6. **Migration risk** — what could break.

## Output Sections

- Architecture summary (observed modules/edges only).
- Findings by severity.
- Quick Wins.
- Recommended Actions.
- Coverage checklist with explicit no-findings statements.
````

---

## Usage

Use as step 2 of the full review or standalone when refactoring module boundaries, concurrency ownership, or platform layering.
