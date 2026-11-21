# Security Review - 2026-11-21

## Executive Summary
- **Health Score**: 8/10 — the core confidentiality controls (keyring fail-closed, zeroization, SQLCipher deletion hygiene, concealed-clipboard markers, URL allow-list, PNG limits, release logging gate, SHA-pinned CI) are implemented with exact code evidence; four residual/hardening findings remain.
- **Critical Issues**: 0
- **High Issues**: 0
- **Total Findings**: 4 (2 Medium, 2 Low)
- **Estimated Remediation Effort**: 4 hours
- **Evidence Commands Run**: `cd /Users/brunoorsier/dev/clipboard && grep -RIni 'safety' src/platform; echo "exit=$?"` → `exit=1`; `cd /Users/brunoorsier/dev/clipboard && git ls-files Cargo.lock deny.toml .github/workflows && wc -l Cargo.lock && grep -c '^\[\[package\]\]' Cargo.lock`; read/grep tool inspections of `Cargo.toml`, `deny.toml`, `.github/workflows/ci.yml`, `.github/workflows/release.yml`, `src/crypto.rs`, `src/db.rs`, `src/media.rs`, `src/monitor.rs`, `src/logging.rs`, `src/main.rs`, `src/secret.rs`, `src/app/{mod.rs,history_view.rs}`, `src/platform/{mod.rs,macos.rs,windows.rs,linux.rs}`, `docs/SECURITY_LESSONS.md`, `docs/MULTIPLATFORM_LESSONS.md`, and the three captured evidence logs. **No cargo, clippy, or test commands were run**; those results belong to the separate static-analysis report. Parent-supplied graph evidence (tier=Verify, project `Users-brunoorsier-dev-clipboard`, generation `2026-11-21T07:10:32Z`) was used only for orientation; every cited path was independently read.

## Findings

### Critical
No findings in this category.

### High
No findings in this category.

### Medium

**M1. Unsafe FFI blocks have no `// SAFETY:` annotations despite a normative project invariant**
- **Threat / asset**: Correctness of the macOS/Windows FFI boundary (`src/platform/macos.rs`, `src/platform/windows.rs`); a future edit that violates an unstated pointer/lifetime/thread invariant can cause UB.
- **Severity**: Medium.
- **Evidence** — `docs/SECURITY_LESSONS.md:59` verbatim: `- **Every \`unsafe\` block has a meaningful \`// SAFETY:\` comment.**`
  Verbatim command output: `$ grep -RIni 'safety' src/platform; echo "exit=$?"` → `exit=1` (no matches anywhere under `src/platform`). Concrete blocks exist, e.g. `/private/var/folders/04/j6_9v0hn5mz2syff3s6sdwp80000gp/T/opencode/review-evidence/unsafe-inventory.log:2` verbatim: `src/platform/macos.rs:37:    unsafe {` and `:29` verbatim: `src/platform/windows.rs:138:    unsafe {`.
- **Failure mode**: Reviewers and clippy cannot distinguish validated invariants from unverified `unsafe`; this is a documentation/auditability gap, not a demonstrated exploit.
- **Impact**: Integrity/availability (latent UB risk on platform code changes); no confidentiality impact shown.
- **Recommendation / test**: Add a short `// SAFETY:` comment to each `unsafe` block in the two FFI files, starting with the ones listed in the unsafe-inventory log (macOS 37/47/62/75/132, Windows 138/156/186/659/690/706/736/752). Add a lightweight CI grep that fails when an `unsafe` block in `src/platform/{macos,windows}.rs` is not preceded by a `SAFETY` marker.

**M2. Windows fallback-key ACL hardening is silently skipped and non-fatal**
- **Threat / asset**: The local `.encryption_key` fallback file on Windows; inherited broad ACLs could expose the key to other local accounts if DPAPI is bypassed or the file is copied.
- **Severity**: Medium.
- **Evidence** — `src/crypto.rs:400-407` verbatim:
  ```rust
      let username = std::env::var("USERNAME").unwrap_or_default();
      if username.trim().is_empty() {
          crate::logging::log_message(
              "WARN",
              "Skipping key file ACL hardening: USERNAME environment variable is empty",
          );
          return;
      }
  ```
  `src/crypto.rs:424-431` verbatim:
  ```rust
          Ok(output) => {
              let msg = format!(
                  "Failed to restrict key file ACLs via {} (exit {:?})",
                  icacls_path.display(),
                  output.status.code()
              );
              eprintln!("⚠️ {msg}");
              crate::logging::log_message("WARN", &msg);
  ```
- **Attack path / failure mode**: If `USERNAME` is unset (domain/headless/service contexts) or `icacls.exe` fails, the function returns normally and key creation continues (`src/crypto.rs:332-338` still applies DPAPI encryption at `:334-337`, so the plaintext key is not written). The residual risk is purely ACL-layer: broad inherited ACLs remain on the DPAPI blob.
- **Impact**: Local privilege-boundary degradation for the degraded fallback mode; not a remote or cross-user plaintext disclosure on its own.
- **Recommendation / test**: Surface the degraded ACL state in `KeyStorageMode`/UI or fall back to a user-scoped path when `USERNAME` is empty; add a Windows-only unit test for the empty-`USERNAME` branch (the resolver is already test-gated at `src/crypto.rs:370-380`).

### Low

**L1. Debug-build clipboard previews depend on heuristic redaction [Inferred]**
- **Threat / asset**: Clipboard text printed to stdout in debug builds.
- **Severity**: Low.
- **Evidence** — `src/monitor.rs:236-248` verbatim:
  ```rust
  #[cfg(debug_assertions)]
  fn log_captured_text(id: i64, text: &str) {
      if crate::secret::is_likely_secret(text) {
          println!("🔒 Captured new secret clipboard entry [id: {id}] (<redacted>)");
      } else {
          let preview = if text.chars().count() > 40 {
              let truncated: String = text.chars().take(40).collect();
              format!("{truncated}...")
          } else {
              text.to_string()
          };
          println!("📋 Captured new text clipboard entry [id: {id}]: {preview:?}");
      }
  }
  ```
  `src/monitor.rs:251-252` verbatim: `#[cfg(not(debug_assertions))]
const fn log_captured_text(_id: i64, _text: &str) {}`
- **Failure mode**: `is_likely_secret` is a heuristic; a secret it misses is printed (up to 40 chars) in debug builds. Release builds compile the function to a no-op, so this is not a release leak.
- **Impact**: Confidentiality limited to debug/local runs.
- **Recommendation / test**: Print only length/kind metadata in debug builds, or require an explicit opt-in env var; add a regression test asserting a known secret-shaped fixture is never echoed.

**L2. Static secret-shaped literals in unit tests**
- **Threat / asset**: Secret-scanner signal quality; real leaks can be hidden behind accepted false positives.
- **Severity**: Low.
- **Evidence** — `docs/SECURITY_LESSONS.md:59` verbatim ends with: `avoid static secret literals in unit tests (construct test fixtures dynamically so secret scanners don't mask real leaks behind false positives).`
  `src/db.rs:930-936` verbatim (the literal at `:937` is redacted per the no-secret-leakage rule):
  ```rust
      #[test]
      fn test_insert_secret_and_toggle_secret() {
          let dir = tempdir().unwrap();
          let db_path = dir.path().join("secret_test.db");
          let db = Database::open(&db_path, TEST_KEY).unwrap();

          let id = db
  ```
  `src/db.rs:937` and `:945` pass a static secret-shaped literal to `db.insert_secret(...)` (values redacted); `src/db.rs:964`, `:971`, `:989`, `:1011`, `src/crypto.rs:522`, `:539`, `:558`, and `src/app/mod.rs:1720` embed further static token/key-shaped literals (values redacted).
- **Failure mode**: Scanners learn to ignore these strings; a genuine pasted secret in source can be dismissed as "another test fixture".
- **Impact**: Detection blind spot, not a runtime disclosure.
- **Recommendation / test**: Build fixtures dynamically (e.g., generated hex/token strings) in the listed tests; tracked in detail in `./TEST_STRATEGY_REVIEW_2026-11-21.md`.

## Quick Wins
1. Add `// SAFETY:` comments to the two FFI files' `unsafe` blocks (M1; ~1 hour).
2. Make the Windows ACL failure branch visible in the UI/log with an actionable message (M2; ~1 hour).
3. Replace debug preview text with metadata-only output in `log_captured_text` (L1; ~30 min).
4. Replace static secret-shaped test literals with generated fixtures in `db.rs`/`crypto.rs`/`app/mod.rs` (L2; ~1 hour).

## Recommended Actions
1. **M1 (Medium, ~1h)**: satisfy `docs/SECURITY_LESSONS.md:59` on all `unsafe` blocks; add the CI grep guard so regressions fail fast.
2. **M2 (Medium, ~1h)**: make the `USERNAME`-empty and `icacls` failure paths fail closed or surface degraded state; add the Windows-only regression test.
3. **L1 (Low, ~0.5h)**: remove content from debug previews; keep release no-op.
4. **L2 (Low, ~1h)**: dynamic fixtures; coordinate with the test-strategy remediation so both reports close together.

## Coverage
- **Key management and secret lifetime**: Keyring fail-closed verified at `src/crypto.rs:102-104` verbatim: `    if db_exists {
        return Err(missing_key_error_message(data_dir).into());
    }`; `Zeroizing` used for keys (`src/crypto.rs:6,36,347-352`), pending clipboard secret (`src/app/mod.rs:173,386`), generated passwords (`src/password.rs:114-142`), transient DB text (`src/db.rs:108,241`), and DPAPI plaintext (`src/platform/windows.rs:715,754-755`). Key is scoped to `open_encrypted_database` and dropped on return (`src/main.rs:253-262`). Local fallback visibility exists via `KeyStorageMode::LocalFileFallback` (`src/crypto.rs:14-19`). Degraded ACL handling: finding M2.
- **Database and deletion hygiene**: Parameterized SQL everywhere reviewed (`src/db.rs:362-364`, `src/db.rs:616`), `PRAGMA secure_delete = ON` (`src/db.rs:146-150`), `VACUUM` + `PRAGMA wal_checkpoint(TRUNCATE)` on clear (`src/db.rs:622-630`), `LIKE` escaping of `\`, `%`, `_` plus `ESCAPE '\'` and `WHERE kind != 'secret'` (`src/db.rs:491-498`, `src/db.rs:516-523`), coupled writes in one transaction (`src/db.rs:333`, `:383`, `:432`). No findings in this category.
- **Clipboard privacy boundaries**: Incoming markers macOS `org.nspasteboard.ConcealedType/AutoGeneratedType/TransientType` + 1Password (`src/platform/macos.rs:393-399`), Windows three formats (`src/platform/windows.rs:467-483`), Linux `x-kde-passwordManagerHint` (`src/platform/linux.rs:234-292`); gating at `src/monitor.rs:74-78`; outgoing markers `src/platform/mod.rs:130-139` called from `src/app/mod.rs:464,500`; UIPI aligned buffer and fail-closed on denied `TOKEN_QUERY` (`src/platform/windows.rs:586-638`); macOS cleanup + `process::exit(0)` (`src/app/mod.rs:1454-1465`). No findings in this category.
- **Unsafe and FFI safety**: `#![allow(unsafe_code)]` confined to `src/platform/macos.rs:1` and `src/platform/windows.rs:1` per `unsafe-inventory.log:1,26`; safe facade/backend per `src/platform/mod.rs` and `src/platform/linux.rs`; no `unsafe impl Send/Sync` in the code read. Finding M1 (missing SAFETY comments).
- **Input validation and resource exhaustion**: URL allow-list before launch (`src/app/history_view.rs:1099-1108`, call site `:186-194`; hardcoded constant URL at `src/app/settings_view.rs:564`); PNG header check + `image::Limits::max_alloc` before decode (`src/media.rs:4,31-50`); text bound `MAX_TEXT_LENGTH = 500_000` (`src/monitor.rs:12,154`). No findings in this category.
- **Logging, telemetry, observability**: `app.log` 1 MiB rotation (`src/logging.rs:7,36-43`), `0600` file in `0700` dir (`src/logging.rs:16,46-50`; `src/crypto.rs:355-359`), panic-safe locking (`src/logging.rs:23-34`). Release clipboard logging compiled out (`src/monitor.rs:251-252`). Findings L1, L2.
- **Cryptographic choices**: OS RNG via `rand::rng().fill_bytes` (`src/crypto.rs:347-352`); SQLCipher key applied with `pragma_update` and validated before use (`src/db.rs:128-137`). No bespoke crypto or clock seeding found. `[Inferred]` no KDF parameter review possible from the files read. No findings in this category.
- **CI/CD and release trust**: All reviewed workflows pin actions to full SHAs (`.github/workflows/ci.yml:22,30,49,63,81,84,121,124`; `release.yml:31,67,157,164,185,251,256`; `sonarcloud.yml:20,30,35,41,49`), workflow-level `permissions: contents: read` (`ci.yml:9-10`, `release.yml:14-15`), no `continue-on-error` in the three workflows read, cargo-deny action present (`ci.yml:123-126`), release SHA-256/SLSA/Authenticode/OpenSSL checks verified in `release.yml:97-101,112-115,139-154,156-161,241-253`. The single Sonar issue is deliberately not analyzed here. No findings in this category.
