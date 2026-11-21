# Test Strategy Review - 2026-11-21

## Executive Summary
- **Health Score**: 7/10 — 74 measured unit tests cover the high-risk security contracts (keyring fail-closed, secret deletion, search escaping, URL/PNG validation) with isolated temp dirs and poison-recovering mocks; gaps are fixture hygiene, weak platform/UI assertions, and unmeasured tooling.
- **Critical Issues**: 0
- **High Issues**: 0
- **Total Findings**: 4 (2 Medium, 2 Low)
- **Estimated Remediation Effort**: 6 hours
- **Test inventory summary (measured only)**: 74 `#[test]` functions across 15 source files: `crypto.rs` 10, `db.rs` 16, `secret.rs` 8, `code.rs` 9, `color.rs` 8, `password.rs` 6, `settings.rs` 4, `media.rs` 3, `theme.rs` 3, `history_view.rs` 2, `app/mod.rs` 2, plus 1 each in `instance.rs`, `tray.rs`, `platform/mod.rs` (and `#[cfg(test)]` modules concentrated in the same files). 0 `#[ignore]` tests. No `tests/`, `benches/`, or `fuzz/` directories exist. CI: `cargo test --all-targets --verbose` on Linux/macOS/Windows (`.github/workflows/ci.yml:40-72`) plus a debug-only macOS quit smoke job (`:74-114`). **Coverage not measured.** CI generates `lcov.info` (`.github/workflows/sonarcloud.yml:34-38`; consumed by `sonar-project.properties:10`) but no local coverage artifact was produced here.
- **Evidence Commands Run**: `cd /Users/brunoorsier/dev/clipboard && grep -RIn '#\[test\]' src | wc -l && grep -RIn '#\[test\]' src | cut -d: -f1 | sort | uniq -c && grep -RIn '#\[cfg(test)\]' src | cut -d: -f1 | sort | uniq -c && grep -RIn '#\[ignore\]' src | wc -l` → `74`, the per-file counts above, `0`; `cd /Users/brunoorsier/dev/clipboard && grep -RIniE 'proptest|fuzz_target|quickcheck|criterion|iai_callgrind|nextest' src Cargo.toml .github 2>/dev/null; echo "exit=$?"` → `exit=1`; `cd /Users/brunoorsier/dev/clipboard && ls -la && ls -la tests benches fuzz .config .cargo 2>/dev/null || true` → no `tests/`, `benches/`, `fuzz/`, `.config/`, or `.cargo/` entries; read/grep inspections of the test modules in `src/db.rs`, `src/crypto.rs`, `src/media.rs`, `src/app/{mod.rs,history_view.rs}`, `src/platform/mod.rs`, the three workflow files, `sonar-project.properties`, `Cargo.toml`, `docs/SECURITY_LESSONS.md`, `docs/MULTIPLATFORM_LESSONS.md`, and the captured evidence logs. **No `cargo test`, `cargo nextest`, `cargo llvm-cov`, or doc-test commands were run**; those results belong to the parent's static-analysis report.

## Findings

### Critical
No findings in this category.

### High
No findings in this category.

### Medium

**M1. Static secret-shaped literals in unit tests**
- **Area**: DB, crypto, and app test fixtures.
- **Severity**: Medium.
- **Evidence** — `docs/SECURITY_LESSONS.md:59` verbatim: `- **Enforce \`cargo-deny\`** with \`wildcards = "deny"\`, \`unknown-registry = "deny"\`, and \`unknown-git = "deny"\`, and avoid static secret literals in unit tests (construct test fixtures dynamically so secret scanners don't mask real leaks behind false positives).`
  `src/db.rs:930-936` verbatim (the value at `:937` is redacted per the no-secret-leakage rule):
  ```rust
      #[test]
      fn test_insert_secret_and_toggle_secret() {
          let dir = tempdir().unwrap();
          let db_path = dir.path().join("secret_test.db");
          let db = Database::open(&db_path, TEST_KEY).unwrap();

          let id = db
  ```
  `src/crypto.rs:515-521` verbatim (the 64-hex key-shaped value at `:522` is redacted):
  ```rust
      #[test]
      fn test_release_mode_reads_legacy_key_file_when_keyring_unavailable() {
          let _lock = mock_keyring::TEST_LOCK.lock().unwrap();
          mock_keyring::reset();
          mock_keyring::set_fail_writes(true);
          let dir = tempdir().unwrap();
          let key_file = dir.path().join(".encryption_key");
  ```
  Additional redacted literals: `src/db.rs:945,964,971,989,1011`, `src/crypto.rs:539,558,578,581`, `src/app/mod.rs:1720`.
- **Gap**: Fixtures embed secret/token/key-shaped strings directly in source instead of constructing them dynamically, contrary to the normative invariant.
- **Risk**: Secret scanners learn to suppress these patterns, reducing the chance of catching a genuine leak; also normalizes pasting realistic credentials into source control.
- **Recommendation**: Generate fixtures at test runtime (e.g., derive a deterministic hex string from a counter via the existing `hex`/`rand` deps) and assert behavior, not the literal value.

**M2. Platform and UI contract tests use no-panic / flag-only assertions for high-risk behavior**
- **Area**: `src/platform/mod.rs` tests and `src/app/mod.rs` tests versus the contract list in `docs/prompts-review/test-strategy-audit.md:44-54`.
- **Severity**: Medium.
- **Evidence** — the only platform unit test, `src/platform/mod.rs:326-339` verbatim:
  ```rust
      #[test]
      fn test_platform_capture_and_restore_no_panic() {
          // Calling capture and restore should never panic on any platform
          capture_active_window();
          restore_focus_and_paste(false, false, 120);
          #[cfg(target_os = "macos")]
          assert!(macos::is_trusted() || !macos::is_trusted());
          let _ = is_app_active();
          let _ = is_clipboard_concealed();
          let _ = take_reopen_requested();
          hide_application();
          order_front_main_window();
          setup_reopen_observer();
      }
  ```
  The two app tests are `src/app/mod.rs:1696` (`test_format_display_preview_strips_variation_selectors_and_truncates`) and `:1710` (`test_quit_sets_is_quitting_flag`), with the Quit flag asserted at `:1787-1790` verbatim:
  ```rust
          // Verify TrayAction::Quit sets is_quitting
          tray_tx.send(super::TrayAction::Quit).unwrap();
          app.handle_tray_events(&ctx);
          assert!(app.is_quitting);
  ```
- **Gap**: No unit test asserts incoming/outgoing concealed-marker decisions, UIPI fail-closed behavior, or `App::logic()` background-wake handling; the platform test's `assert!(macos::is_trusted() || !macos::is_trusted())` is tautological.
- **Risk**: A regression in password-manager exclusion or elevated-paste suppression could pass CI. The macOS quit smoke test (`.github/workflows/ci.yml:74-114`) covers only AppKit teardown.
- **Recommendation**: Extract pure decision helpers behind `#[cfg(any(target_os = "windows", test))]` (per `docs/MULTIPLATFORM_LESSONS.md:15`) so concealed-marker parsing, UIPI level comparison, and `handle_monitor_updates`/`handle_clipboard_auto_clear` dispatch can be asserted on every CI host; keep the OS smoke tests for real FFI.

### Low

**L1. CI test/coverage commands drift from the documented baseline**
- **Area**: CI verification commands.
- **Severity**: Low.
- **Evidence** — `.github/workflows/ci.yml:71-72` verbatim:
  ```yaml
        - name: Run unit and integration tests
          run: cargo test --all-targets --verbose
  ```
  `.github/workflows/sonarcloud.yml:37-38` verbatim:
  ```yaml
        - name: Generate code coverage report
        run: cargo llvm-cov --all-targets --lcov --output-path lcov.info
  ```
  vs. the prompt's reference command `docs/prompts-review/test-strategy-audit.md:65` verbatim: `cargo llvm-cov --all-features --workspace --lcov --output-path lcov.info`.
- **Gap**: No `--locked` on tests, and coverage omits `--all-features --workspace`; `sonar-project.properties:10` still expects the produced `lcov.info`.
- **Risk**: Test resolution can drift from `Cargo.lock`, and feature-gated code is not exercised in the coverage run.
- **Recommendation**: Use `cargo test --locked --all-targets --verbose` and `cargo llvm-cov --locked --all-features --workspace --all-targets --lcov --output-path lcov.info`; verify `lcov.info` is still uploaded to Sonar.

**L2. `[Inferred]` No property-based or fuzz tests for untrusted-input parsers**
- **Area**: URL sanitization and PNG header parsing.
- **Severity**: Low.
- **Evidence** — verbatim command output: `$ grep -RIniE 'proptest|fuzz_target|quickcheck|criterion|iai_callgrind|nextest' src Cargo.toml .github 2>/dev/null; echo "exit=$?"` → `exit=1`; no `fuzz/` or `benches/` directory in the `ls` output. Code paths read: `src/app/history_view.rs:1101-1108` (`sanitize_browser_url`) and `src/media.rs:28-52` (`png_to_rgba`), each with fixed example-based tests at `:1114-1134` and `:80-107` respectively.
- **Gap / status**: `Skipped: tool not configured` (proptest/quickcheck/cargo-fuzz/Miri/nextest are not declared in `Cargo.toml`); this is a tooling gap, not a code failure.
- **Risk**: `[Inferred]` malformed URLs or PNG headers outside the hand-picked examples could regress without detection.
- **Recommendation**: `[Inferred]` add `proptest` cases for arbitrary URL strings and crafted PNG headers, and `cargo-fuzz` targets for clipboard byte payloads, when the tools are introduced.

## Quick Wins
1. Replace secret-shaped test literals with runtime-generated fixtures in `db.rs`/`crypto.rs`/`app/mod.rs` (M1; ~1h).
2. Replace the tautological macOS assertion and add pure-helper assertions for concealed/UIPI decisions (M2; ~2h).
3. Add `--locked` to CI tests and `--all-features --workspace` to coverage (L1; ~30 min).
4. Document the skipped property/fuzz/Miri tooling explicitly in the test README rather than leaving it implicit (L2; ~30 min).

## Recommended Actions
1. **M1 (Medium, ~1h)**: dynamic fixtures; pair with the security report's L2 remediation.
2. **M2 (Medium, ~2h)**: injectable pure helpers + behavioral assertions for the contract list at `docs/prompts-review/test-strategy-audit.md:44-54`; retain the macOS quit smoke job.
3. **L1 (Low, ~0.5h)**: align CI test/coverage commands and confirm `lcov.info` consumption.
4. **L2 (Low, ~1h when adopted)**: introduce proptest/cargo-fuzz for the two parsers; Miri remains **Skipped: tool not configured** for FFI code that calls real OS APIs.
5. Re-run `cargo test --all-targets --locked` (parent's static-analysis pass) before closing any item; no test result is claimed in this report.

## Coverage
- **Test inventory**: 74 `#[test]` functions in 15 files; 0 `#[ignore]`; no `tests/`, `benches/`, or `fuzz/` directories. No findings in this category.
- **Command results**: `grep -RIn '#\[test\]' src | wc -l` → `74`; `grep -RIn '#\[ignore\]' src | wc -l` → `0`; proptest/fuzz/nextest grep → `exit=1`; directory listing → no test tool config directories. Coverage not measured. No findings in this category.
- **Determinism and isolation**: `tempfile::tempdir()` used for DB/key tests (`src/db.rs:667`, `src/crypto.rs:465`); keyring mock recovers from poisoning (`src/crypto.rs:233-235`) and is serialised by `TEST_LOCK` (`src/crypto.rs:227,508-509`); transient clipboard failures handled in monitor tests. Finding M1 (fixture literals). No findings in this category.
- **Contract coverage**: Keyring fail-closed asserted at `src/crypto.rs:506-513` (`test_existing_db_without_key_fails_loudly`), local fallback visibility at `:516-531`, secure delete + WAL truncation at `src/db.rs:977-1000`, LIKE escaping/secret search exclusion at `:1002-1030`, URL allow-list at `src/app/history_view.rs:1114-1134`, PNG dimension/alloc guard at `src/media.rs:80-107`. Finding M2 for platform/UI contracts.
- **Property, fuzz, and Miri**: `Skipped: tool not configured` — proptest/quickcheck/cargo-fuzz/Miri are absent per the `exit=1` grep and directory listing; finding L2 is an adoption suggestion only.
- **Coverage and quality gates**: CI runs clippy/tests on Linux, macOS, Windows (`.github/workflows/ci.yml:40-72`) with no `continue-on-error`; macOS quit smoke job at `:74-114` (debug-only `CIPHERCLIPBOARD_SMOKE_QUIT_MS` hook at `src/main.rs:229-250`); `sonar-project.properties:10` expects `lcov.info`, produced by `.github/workflows/sonarcloud.yml:38`. Finding L1. **Coverage not measured locally.**
- **Testability architecture**: Pure helpers test-gated for non-native hosts (`src/crypto.rs:370-380` `resolve_windows_icacls_path` with `#[cfg(any(target_os = "windows", test))]`; theme helpers in `src/theme.rs`); DB accepts injectable temp paths (`Database::open(&db_path, ...)` in every DB test). No findings in this category.
