# Test Strategy Review - 2026-11-21-b

## Executive Summary
- **Health Score**: 8/10 — 97 unit tests run and pass under `cargo test --all-targets --locked` on Linux (`98` total `#[test]` functions across 17 source files, including 1 macOS-only unit test in `src/platform/macos.rs`), covering all high-risk contracts (keyring fail-closed, degraded fallback modes, SQLCipher `secure_delete`/WAL truncation, `ClearError::Delete` vs `ClearError::Compaction`, `prune_in_tx` rollback, schema migration error propagation, `LIKE` escaping + secret exclusion, URL allow-list, PNG decompression bomb guards, pure Windows concealed-marker/UIPI/icacls helpers, paste-worker shutdown, and `App::logic()` background dispatch); remaining gaps are two static hex literals in test code, lack of property/fuzz/Miri tooling, and minor CI coverage command flags.
- **Critical Issues**: 0
- **High Issues**: 0
- **Total Findings**: 3 (0 Critical, 0 High, 0 Medium, 3 Low)
- **Estimated Remediation Effort**: 2 hours
- **Test inventory summary (measured only)**:
  - Total `#[test]` attributes in `src/`: **98** across 17 source files (`grep -RIn '#\[test\]' src | wc -l` → `98`; `grep -RIn '#\[test\]' src | cut -d: -f1 | sort | uniq -c`):
    - `src/db.rs`: 22 (database encryption, deduplication, auto-pruning, `preserve_secrets_on_prune`, search `LIKE` escaping & secret exclusion, `secure_delete` + WAL truncation, `ClearError::Delete` and `ClearError::Compaction`, `prune_in_tx` rollback on trigger failure, duplicate-column classifier, schema migration failure abort)
    - `src/code.rs`: 9 (language detection and `syntect` highlighting with WCAG contrast)
    - `src/crypto.rs`: 9 (random key generation, `local_fallback_mode`, save/read/remove key file, fail-closed when DB exists, legacy key file & service migrations, mock keyring isolation)
    - `src/app/mod.rs`: 8 (display preview formatting, tray event handling & `is_quitting`, keyring explanation copy across all 3 `KeyStorageMode` variants, `copy_plain_text` toast, `monitor_update_needs_refresh`, `clipboard_clear_is_due`, `handle_monitor_updates`, `handle_clipboard_auto_clear`)
    - `src/color.rs`: 8 (HEX/RGB/HSL/8-digit ARGB parsing, CSS color extraction, false-positive avoidance)
    - `src/secret.rs`: 8 (token/key/JWT/PEM detection, URL credentials, `.env` secrets, high-entropy passwords, Windows path false-positive avoidance, backslash passwords)
    - `src/platform/mod.rs`: 7 (no-panic capture/restore, `concealed_marker_support_matches_ui_limitation`, `windows_concealed_marker_decision_helper`, `windows_concealed_marker_format_names`, `uipi_should_skip_paste_fails_closed`, `linux_mark_clipboard_concealed_runs_explicit_noop`, `paste_worker_shutdown_joins_and_is_idempotent`)
    - `src/password.rs`: 6 (BIP-39 2,048-word uniqueness, password length/classes/ambiguous exclusion/fallback, passphrase options, entropy & strength classification)
    - `src/platform/fs_security.rs`: 4 (`test_restricted_dir_permissions`, `test_resolve_windows_icacls_path`, `test_windows_icacls_grant_requires_username`, `test_windows_icacls_grant_qualifies_domain_accounts`)
    - `src/settings.rs`: 4 (settings default & DB roundtrip, hotkey preset conversions, custom hotkey parsing with layout-aware `Ctrl+ù`, theme mode legacy compatibility)
    - `src/media.rs`: 3 (PNG roundtrip, oversized image encode rejection, crafted 10,000×10,000 PNG header rejection before allocation)
    - `src/theme.rs`: 3 (dark/light palette colors, WCAG AAA/AA contrast audit, `windows_font_candidates_respects_windir`)
    - `src/app/history_view.rs`: 2 (`sanitize_browser_url` allow-list and non-HTTP scheme blocking)
    - `src/logging.rs`: 2 (`log_file_path` and `log_dir` suffix checks; `log_message` returns early under `cfg!(test)` so tests never pollute the user's `app.log`)
    - `src/instance.rs`: 1 (single-instance lock acquisition, `AlreadyRunning` PID reporting, release on drop)
    - `src/platform/macos.rs`: 1 (`lock_or_recover_recovers_poisoned_mutex`, compiled on `target_os = "macos"`)
    - `src/tray.rs`: 1 (`test_load_tray_icon_rgba_succeeds`)
  - `#[ignore]` tests: **0** (`grep -RIn '#\[ignore\]' src | wc -l` → `0`).
  - Integration tests (`tests/`), benchmarks (`benches/`), fuzz targets (`fuzz/`): **None** (binary-only crate `[[bin]] name = "CipherClipboard"`).
  - CI smoke tests: `macos-quit-smoke` job in `.github/workflows/ci.yml:74-114` exercises `CIPHERCLIPBOARD_SMOKE_QUIT_MS=5000` (`src/main.rs:232-254`).
  - **Coverage not measured locally** (`cargo-llvm-cov` is not installed on this VM; CI generates `lcov.info` in `.github/workflows/sonarcloud.yml:38`).
- **Evidence Commands Run**:
  - `cargo test --all-targets --locked` → `test result: ok. 97 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 8.13s`
  - `cargo test --doc --locked` → `error: no library targets found in package cipherclipboard` (exit 101; expected for binary-only crate)
  - `grep -RIn '#\[test\]' src | wc -l && grep -RIn '#\[test\]' src | cut -d: -f1 | sort | uniq -c && grep -RIn '#\[cfg(test)\]' src | cut -d: -f1 | sort | uniq -c && grep -RIn '#\[ignore\]' src | wc -l`
  - `grep -RIniE 'proptest|fuzz_target|quickcheck|criterion|iai_callgrind|nextest' src Cargo.toml .github 2>/dev/null; echo "test_tools_exit=$?"` → `test_tools_exit=1`
  - `which cargo-nextest cargo-llvm-cov` → **Skipped: tool not configured / not installed locally**.

### Command Results Table

| Command | Status | Result / Output Summary |
|---|---|---|
| `cargo test --all-targets --locked` | Passed (exit 0) | `97 passed; 0 failed; 0 ignored; finished in 8.13s` (97 on Linux + 1 `#[cfg(target_os = "macos")]` test in `src/platform/macos.rs:468`) |
| `cargo test --doc --locked` | Expected N/A (exit 101) | `error: no library targets found in package cipherclipboard` (`Cargo.toml` defines only `[[bin]]`) |
| `cargo nextest run --all-targets --locked` | Skipped | `Skipped: tool not configured` (`cargo-nextest` not installed) |
| `cargo llvm-cov --all-features --workspace --lcov --output-path lcov.info` | Skipped locally | `Coverage not measured.` (`cargo-llvm-cov` not installed locally; run in CI at `.github/workflows/sonarcloud.yml:38`) |

## Findings

### Critical
No findings in this category.

### High
No findings in this category.

### Medium
No findings in this category.

### Low

**L1. Two remaining static 64-hex key literals in `src/settings.rs` and `src/db.rs`**
- **Area**: Test fixture hygiene (`src/settings.rs`, `src/db.rs`).
- **Severity**: Low.
- **Evidence** — `docs/SECURITY_LESSONS.md:59` requires dynamic test fixtures instead of static secret literals. While `src/crypto.rs:320-322`, `src/db.rs:870-872,1194,1223,1230,1248,1307`, and `src/app/mod.rs:1872,1983,2040,2133` now generate keys and tokens dynamically at runtime, two static 64-character hex literals remain (values redacted per Global Evidence Rule 10):
  - `src/settings.rs:572` (`const TEST_KEY: &str = "<redacted-64-hex-literal>";`, used at `:578`)
  - `src/db.rs:1081` (`let wrong_key = "<redacted-64-hex-literal>";`, used at `:1082`)
- **Gap**: Static 64-hex strings in unit tests can trigger secret-scanner false positives.
- **Risk**: Minor secret-scanner noise (all other fixtures were already migrated to `test_hex_key(seed)`).
- **Recommendation**: Replace `TEST_KEY` in `src/settings.rs:572` with a dynamic `format!("{:016x}", 1_u64).repeat(4)` helper and `wrong_key` in `src/db.rs:1081` with `&test_hex_key(0xFFFF_FFFF_FFFF_FFFF)`.

**L2. `[Inferred]` No property-based (`proptest`) or fuzz (`cargo-fuzz`) tests for untrusted clipboard parsers**
- **Area**: Input parsers (`src/app/history_view.rs::sanitize_browser_url`, `src/media.rs::png_to_rgba`, `src/color.rs::parse_color`, `src/secret.rs::is_likely_secret`, `src/settings.rs::ParsedHotkey::parse`).
- **Severity**: Low.
- **Evidence** — `grep -RIniE 'proptest|fuzz_target|quickcheck|criterion|iai_callgrind|nextest' src Cargo.toml .github 2>/dev/null; echo "test_tools_exit=$?"` → `test_tools_exit=1`. Current parser tests are example-based (`src/app/history_view.rs:1117-1136`, `src/media.rs:68-115`, `src/color.rs:564-701`, `src/secret.rs:391-512`, `src/settings.rs:641-681`).
- **Gap**: `Skipped: tool not configured` — no `proptest` dev-dependency or `cargo-fuzz` harness is configured.
- **Risk**: `[Inferred]` Edge-case Unicode/UTF-8 slicing or malformed clipboard strings outside the curated example sets are not automatically explored via property generation.
- **Recommendation**: `[Inferred]` Add a `proptest` dev-dependency for `sanitize_browser_url`, `truncate_utf8`/`is_likely_secret`, `parse_color`, and `ParsedHotkey::parse` roundtrips.

**L3. SonarCloud coverage step in `.github/workflows/sonarcloud.yml` omits `--all-features --workspace`**
- **Area**: CI coverage command in `.github/workflows/sonarcloud.yml`.
- **Severity**: Low.
- **Evidence** — `.github/workflows/sonarcloud.yml:37-38` verbatim:
  ```yaml
        - name: Generate code coverage report
          run: cargo llvm-cov --all-targets --locked --lcov --output-path lcov.info
  ```
  Compare with `.github/workflows/ci.yml:38` (`cargo clippy --all-targets --all-features --locked -- -D warnings`) and the prompt's baseline (`cargo llvm-cov --all-features --workspace --lcov --output-path lcov.info`).
- **Gap**: `sonarcloud.yml:38` now includes `--locked` (fixing the lockfile drift from the first review), but does not pass `--all-features --workspace`.
- **Risk**: Currently `Cargo.toml` defines no optional `[features]` or secondary workspace members, so behavior is identical today; if features or sub-crates are added later, coverage would silently omit them.
- **Recommendation**: Add `--all-features --workspace` to `cargo llvm-cov` in `.github/workflows/sonarcloud.yml:38`.

## Quick Wins
1. Replace the last two static 64-hex literals in `src/settings.rs:572` and `src/db.rs:1081` with dynamic hex generators (L1; ~10 min).
2. Add `--all-features --workspace` to `cargo llvm-cov` in `.github/workflows/sonarcloud.yml:38` (L3; ~5 min).
3. Add `proptest` property tests for `sanitize_browser_url`, `parse_color`, `ParsedHotkey::parse`, and `is_likely_secret` (L2; ~1.5h).

## Recommended Actions
1. **L1 (Low, ~0.25h)**: Convert `src/settings.rs:572` and `src/db.rs:1081` to dynamic hex fixtures.
2. **L3 (Low, ~0.1h)**: Update `.github/workflows/sonarcloud.yml:38` to `cargo llvm-cov --all-targets --all-features --workspace --locked --lcov --output-path lcov.info`.
3. **L2 (Low, ~1.5h)**: Introduce `proptest` in `[dev-dependencies]` for pure parser and UTF-8 truncation invariants.

## Coverage
- **1. Test Inventory**:
  - 98 `#[test]` functions across 17 files in `src/` (97 executed on Linux in 8.13s, all passing; 1 macOS-gated test in `src/platform/macos.rs:468`), 0 `#[ignore]` tests.
  - Pure logic (`code.rs`, `color.rs`, `secret.rs`, `password.rs`, `theme.rs`, `settings.rs`), DB/keyring (`db.rs`, `crypto.rs`), platform/filesystem security (`platform/mod.rs`, `platform/fs_security.rs`, `platform/macos.rs`), UI state/dispatch (`app/mod.rs`, `app/history_view.rs`, `tray.rs`), and process lifecycle (`instance.rs`, `logging.rs`, plus CI `macos-quit-smoke`) are all represented.
  - No findings in this category.
- **2. Determinism and Isolation**:
  - Filesystem and SQLCipher tests use `tempfile::tempdir()` (`src/db.rs:876`, `src/crypto.rs:347`, `src/instance.rs:77`, `src/platform/fs_security.rs:245`, `src/settings.rs:576`, `src/app/mod.rs:1870`).
  - `mock_keyring` serializes access via `TEST_LOCK` and recovers from poisoned mutexes (`src/crypto.rs:212-233`).
  - `logging::log_message` returns early when `cfg!(test)` (`src/logging.rs:34-36`) so intentional failure tests (`test_clear_reports_compaction_failure_after_deleting_rows`, `lock_or_recover_recovers_poisoned_mutex`) never write to the user's real `app.log`.
  - Keyboard layout tests branch on `crate::platform::quote_key_is_ugrave()` (`src/settings.rs:669-675`) rather than assuming US QWERTY.
  - Finding L1 (two remaining static 64-hex literals).
- **3. Contract Coverage**:
  - Keyring fail-closed when encrypted DB exists: `src/crypto.rs:393-399` (`test_existing_db_without_key_fails_loudly`).
  - Local fallback visibility and permission degradation: `src/crypto.rs:332-341` (`test_local_fallback_mode_reports_permission_degradation`), `src/app/mod.rs:1949-1973` (`test_keyring_explanation_copy_matches_storage_mode`), `src/platform/fs_security.rs:241-291`.
  - SQL parameterization, `LIKE` escaping, secret search exclusion: `src/db.rs:1299-1328` (`test_search_escapes_wildcards_and_excludes_secrets`).
  - `secure_delete`, WAL truncation, and `ClearError` variants: `src/db.rs:1237-1296` (`test_secure_delete_and_wal_truncate_on_clear`, `test_clear_reports_delete_failure`, `test_clear_reports_compaction_failure_after_deleting_rows`).
  - Transaction rollback on `prune_in_tx` failure and schema migration error propagation: `src/db.rs:1331-1491` (`test_prune_in_tx_failure_rolls_back_insert`, `test_is_duplicate_column_error_classifier`, `test_schema_migration_unexpected_error_aborts_open`).
  - Incoming/outgoing concealed markers and UIPI fail-closed: `src/platform/mod.rs:670-732` (`test_concealed_marker_support_matches_ui_limitation`, `test_windows_concealed_marker_decision_helper`, `test_windows_concealed_marker_format_names`, `test_uipi_should_skip_paste_fails_closed`, `test_linux_mark_clipboard_concealed_runs_explicit_noop`).
  - URL allow-list and PNG header/allocation limits: `src/app/history_view.rs:1117-1136`, `src/media.rs:89-115`.
  - Thread shutdown and `App::logic()` background dispatch: `src/platform/mod.rs:735-764` (`test_paste_worker_shutdown_joins_and_is_idempotent`), `src/app/mod.rs:2090-2148` (`test_monitor_update_needs_refresh_helper`, `test_clipboard_clear_is_due_helper`, `test_handle_monitor_updates_dispatches_on_notification_and_query_change`, `test_handle_clipboard_auto_clear_dispatches_only_at_deadline`).
  - No findings in this category.
- **4. Property, Fuzz, and Miri**:
  - `Skipped: tool not configured` for `proptest`, `quickcheck`, `cargo-fuzz`, and `Miri`. Finding L2 records the optional `proptest` adoption opportunity.
- **5. Coverage and Quality Gates**:
  - CI runs `cargo clippy --all-targets --all-features --locked -- -D warnings` and `cargo test --all-targets --locked --verbose` on `ubuntu-latest`, `macos-latest`, and `windows-latest` with `fail-fast: false` and no `continue-on-error` (`.github/workflows/ci.yml:38-73`).
  - `macos-quit-smoke` verifies clean AppKit exit on `macos-latest` (`.github/workflows/ci.yml:74-114`).
  - `sonar-project.properties:10` (`sonar.rust.lcov.reportPaths=lcov.info`) matches `.github/workflows/sonarcloud.yml:38`.
  - Finding L3 (`--all-features --workspace` flag on `cargo llvm-cov`).
- **6. Testability Architecture**:
  - Pure platform helpers in `src/platform/mod.rs:218,332,340,348,358` and `src/platform/fs_security.rs:134,146` are gated with `#[cfg(any(target_os = "windows", test))]` so Windows decision logic is tested on Linux and macOS CI runs.
  - Database and filesystem helpers accept caller-supplied `Path` arguments, and deadline logic (`clipboard_clear_is_due(now, deadline)`, `src/app/mod.rs:1511-1513`) accepts an injected `Instant`.
  - No findings in this category.
