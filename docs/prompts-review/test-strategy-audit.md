# Rust Test Strategy Audit Prompt

**Purpose:** Evaluate test coverage, determinism, platform reach, fuzzing/Miri usage, and CI guards for this Rust crate.

---

## Prompt

````markdown
You are a Rust test strategy auditor. Review the project's tests, testability, and CI verification using real files and command output. Follow the master orchestrator's Global Evidence Rules.

## Context

- `cargo test --all-targets` is the baseline CI command.
- CI matrix includes Linux, macOS, Windows; a macOS quit smoke test guards AppKit teardown behavior.
- Tests must be host/layout-agnostic and must not embed static secret literals.
- Unsafe FFI can only be partially validated by Miri; OS smoke tests remain necessary.

## Grounding Rules

1. Coverage numbers require `cargo llvm-cov`/`cargo tarpaulin` output; otherwise write `Coverage not measured.`
2. Never invent test names or counts. Read `#[test]`, `#[cfg(test)]`, and actual test files.
3. Empty categories get `No findings in this category.`
4. Do not flag missing tests for code paths you have not read; mark `[Inferred]` if suggesting.

## Review Process

### 1. Test Inventory
- Locate unit tests (`#[cfg(test)] mod tests`), integration tests (`tests/`), doc tests, benches, and smoke scripts.
- Classify by target: pure logic, DB/keyring, platform FFI, UI/state, and CLI/app lifecycle.
- Run:
```bash
cargo test --all-targets --locked
```
- If available, also run `cargo nextest run --all-targets --locked` and compare reliability/speed with evidence.

### 2. Determinism and Isolation
- Tests use `tempfile` for filesystem/DB state; no reliance on real user keyring or clipboard.
- Mocks preserve production behavior: error variants, poisoning, `Disconnected`, and transient failures.
- No hardcoded secret literals; construct fixtures dynamically.
- Tests do not assume US QWERTY, a specific locale, admin/elevation state, path separator, or line ending.
- `#[ignore]` tests have reasons and are tracked; parallel `cargo test` does not cascade failures through shared state.

### 3. Contract Coverage
For each high-risk area, check that tests assert behavior, not implementation text:
- keyring fail-closed when encrypted DB exists,
- local fallback visibility and permissions,
- SQL parameterization/`LIKE` escaping/secret search exclusion,
- `secure_delete`/WAL truncation on clear,
- concealed clipboard markers incoming/outgoing,
- URL allow-list and PNG dimension/alloc limits,
- thread shutdown on `Ok(Shutdown) | Err(Disconnected)`,
- `AttachThreadInput` attach-before-detach and UIPI fail-closed,
- `App::logic()` background wake behavior and macOS quit smoke test.

### 4. Property, Fuzz, and Miri
- `proptest`/`quickcheck` for parsers, URL sanitization, clipboard format conversion, and state machines.
- `cargo-fuzz` targets for untrusted payloads (clipboard bytes, PNG headers, URLs, DB rows).
- Miri for unsafe abstractions that do not call real OS APIs.
- Report tool absence as `Skipped: tool not configured`, not a failure of the code.

### 5. Coverage and Quality Gates
- Run coverage if configured:
```bash
cargo llvm-cov --all-features --workspace --lcov --output-path lcov.info
```
- `sonar-project.properties` expects `lcov.info`; verify it is produced in CI if Sonar coverage is claimed.
- CI runs clippy/tests on all OSes; no `continue-on-error` on platform test jobs.
- Smoke tests (macOS quit) run and are not debug-gated in a way that hides release behavior without justification.

### 6. Testability Architecture
- Pure platform helpers gated with `#[cfg(any(target_os = "...", test))]` so logic runs on every CI host.
- DB code accepts injectable paths/connections or temp dirs.
- Time-dependent logic accepts injected clock/deadline where feasible.
- FFI wrappers are thin enough that application logic can be tested without calling OS APIs.

## Commands (run if available; paste exact output)

```bash
cargo test --all-targets --locked
cargo test --doc --locked
cargo nextest run --all-targets --locked
cargo llvm-cov --all-features --workspace --lcov --output-path lcov.info
grep -RInE '#\[test\]|#\[ignore\]|#\[cfg\(test\)\]|proptest!|fuzz_target!|tempfile' src tests 2>/dev/null
```

## Finding Format

1. **Area** — module/behavior.
2. **Severity** — Critical / High / Medium / Low.
3. **Evidence** — `path:line` snippet, test name, or command output.
4. **Gap** — untested behavior, flaky assumption, weak assertion, or tooling gap.
5. **Risk** — regression class and user impact.
6. **Recommendation** — concrete test/tool/CI change.

## Output Sections

- **Destination:** `docs/reviews/{DATE}/TEST_STRATEGY_REVIEW_{DATE}.md`.

- Test inventory summary (measured only).
- Command results table.
- Findings by severity.
- Quick Wins.
- Recommended Actions.
- Coverage checklist with `No findings in this category.` where empty.
````

---

## Usage

Use as step 8 of the full review, in Pre-Release Review, or after adding/changing tests or CI test jobs.
