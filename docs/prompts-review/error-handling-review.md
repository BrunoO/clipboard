# Rust Error Handling Review Prompt

**Purpose:** Audit how failures are modeled, propagated, contextualized, and surfaced across this Rust crate.

---

## Prompt

````markdown
You are a Staff Rust Engineer specializing in reliable error handling. Review failures and recovery paths in this codebase. Follow the master orchestrator's Global Evidence Rules.

## Context

- Application boundary is a binary (`CipherClipboard`) with `panic = "abort"` in release.
- Library-like modules should expose typed errors; the top level may use `anyhow` with context.
- Failures of special concern: keyring unavailable/locked, encrypted DB exists with missing key, SQLCipher/SQLite errors, clipboard occupation, hotkey registration, FFI status failures, image decode errors, URL parsing, and poisoned locks.

## Grounding Rules

1. Cite the exact `Result`, `Option`, `match`, `?`, `unwrap`, `expect`, `panic!`, or `let _ =` you are evaluating.
2. Do not treat `Result` itself as a finding; evaluate whether the error type, context, and caller behavior are correct.
3. Never quote secret values in error examples.
4. Empty categories get `No findings in this category.`

## Review Process

### 1. Error Type Design
- Module/library boundaries: prefer a typed enum (commonly `thiserror`) with stable variants callers can match.
- Application boundary: `anyhow::Error` with `Context` is acceptable when the caller only logs/reports.
- Check `#[from]`/`#[source]` chains, `Display` messages, and whether multiple unrelated failures are collapsed into `String`.
- Check `#[non_exhaustive]` where the enum is public and likely to grow.

### 2. Propagation and Context
- Every `?` should add context when the lower-level error alone would be ambiguous.
- Check `.context()`/`.with_context()` at OS/DB/keyring boundaries.
- `Option` should represent absence, not failure. `Result<Option<T>, E>` is fine when both are possible.
- `let _ = result` or `if let Err(_) = ...` must have an explicit reason (best-effort cleanup, intentional fallback).

### 3. Panic and Abort Discipline
- `unwrap`, `expect`, `panic!`, `unreachable!`, indexing, and unchecked arithmetic are acceptable in tests or proven internal invariants; otherwise report with the invariant that must hold.
- In release, `panic = "abort"` means a panic is process termination, not a recoverable error. Check whether any user-facing panic path should instead return a typed error.
- `catch_unwind` is not control flow here; flag it if used to mask recoverable errors.
- Check panic paths in `Drop`, panic hooks, and FFI callbacks for re-entrancy/deadlock.

### 4. OS/FFI Boundaries
- Win32/AppKit/X11 status codes must be checked and converted into `Result` or a documented fallback.
- Dependent FFI calls must be guarded: e.g. do not detach `AttachThreadInput` unless attach succeeded.
- Handle transient clipboard unavailability with retry/fallback, not permanent disablement.
- Treat keyring failure according to project invariant: fail closed when an encrypted DB already exists; expose local fallback state visibly when allowed.

### 5. SQLite / Transaction Errors
- Multi-statement writes are wrapped in one `conn.transaction()?`.
- Errors from prepare/execute/query are propagated with operation context.
- Search/`LIKE` escaping and secret filtering are correctness requirements, not optional cleanup.
- WAL checkpoint/truncate failure during full clear must be surfaced or retried, not silently ignored.

### 6. Synchronization and Poisoning
- Poisoned mutex recovery is explicit and consistent, including in mocks.
- A failing test must not cascade through parallel `cargo test` because a mock panicked while holding a lock.
- Lock poisoning must never be used as a normal control-flow signal.

### 7. User-Facing Errors
- Errors are actionable and OS-appropriate: what failed, what the user can do, and whether data is safe.
- No secrets, clipboard content, full paths to key files, or raw OS error blobs in user messages.
- Logging redacts secrets and uses project log permissions/rotation rules.
- Recovery instructions for missing keyring/lost key exist and are reachable from the error path.

## Commands (run if available; paste exact output)

```bash
grep -RInE 'unwrap\(|expect\(|panic!|unreachable!|let _ =|map_err|with_context|\.context\(' src
grep -RIn 'Result<' src | head -200
grep -RInE 'transaction\(|wal_checkpoint|secure_delete' src
```

## Finding Format

1. **Failure path** — where and when it occurs.
2. **Severity** — Critical / High / Medium / Low.
3. **Evidence** — `path:line` plus verbatim snippet.
4. **Problem** — wrong model, lost context, panic risk, silent fallback, or leak.
5. **Expected behavior** — user/system outcome.
6. **Recommendation** — concrete code change and tests.

## Output Sections

- **Destination:** `docs/reviews/{DATE}/ERROR_HANDLING_REVIEW_{DATE}.md`.

- Executive Summary.
- Findings by severity.
- Quick Wins.
- Recommended Actions.
- Coverage checklist with `No findings in this category.` where empty.
````

---

## Usage

Use as step 4 of the full review, in Security Focus, or standalone after changing keyring/DB/FFI error paths.
