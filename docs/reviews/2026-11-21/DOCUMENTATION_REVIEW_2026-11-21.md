# Documentation Review - 2026-11-21

## Executive Summary
- **Health Score**: 7/10 ([Inferred] weighting of 0 critical, 0 high, 4 medium and 2 low evidence-backed findings; a clean prompt-path scan prevents a lower score)
- **Critical Issues**: 0
- **High Issues**: 0
- **Total Findings**: 6
- **Estimated Remediation Effort**: 3–5 hours [Inferred]
- **Evidence Commands Run**:
  - `grep -nE 'internal-docs/prompts/|2026-02-14_AGENT_INSTRUCTIONS_UPDATE_CLANG_TIDY_STATUS|cmake-build-health\.md' docs/prompts-review/master-orchestrator.md || true` → `(no output)`
  - `grep -RInE 'TODO|FIXME|TBD' README.md AGENTS.md docs` → only prompt-instruction mentions:
    ```text
    docs/prompts-review/tech-debt.md:21:2. Do not report a line count, file size, dependency count, or `TODO` count unless you measured it.
    docs/prompts-review/tech-debt.md:29:- Search source and config for `TODO`, `FIXME`, `HACK`, `XXX`, `NOTE`, `#[allow(...)]`, `#![allow(...)]`, and `#[cfg(test)]` workarounds.
    docs/prompts-review/tech-debt.md:75:grep -RInE 'TODO|FIXME|HACK|XXX|#\[allow|#!\[allow' src Cargo.toml .github 2>/dev/null
    docs/prompts-review/documentation-maintenance.md:66:grep -RInE 'TODO|FIXME|TBD' README.md AGENTS.md docs
    ```
  - `grep -RInE '/// # Errors|/// # Panics|/// # Safety|//! # Errors|//! # Panics|//! # Safety' src || echo '(no matches)'` → `(no matches)`
  - `grep -nA1 -E '^name = "(egui|eframe)"$' Cargo.lock` →
    ```text
    1097:name = "eframe"
    1098-version = "0.37.0"
    --
    1134:name = "egui"
    1135-version = "0.37.0"
    ```
  - `ls -1 src/platform docs/windows scripts/windows` →
    ```text
    docs/windows:
    WINDOWS_REVIEW.md
    WINDOWS_TESTING.md

    scripts/windows:
    setup-openssl-ci.ps1
    setup.iss

    src/platform:
    linux.rs
    macos.rs
    mod.rs
    windows.rs
    ```
  - `grep -niE 'missing key|lost key|locked key|keyring (missing|unavailable|locked)|recover' README.md || true` → `(no output)`
  - `grep -oE 'docs/prompts-review/[a-z0-9-]+\.md' docs/prompts-review/master-orchestrator.md | sort -u | while read -r f; do [ -f "$f" ] && echo "OK $f" || echo "MISSING $f"; done` → 17 lines, all `OK ...`; no `MISSING ...`.
- **Not run by this subagent**: `cargo test --doc --locked` and `cargo doc --no-deps --all-features --locked`; the parent is gathering static-analysis evidence, so no doc-test or rustdoc build result is claimed here.

## Findings

### Critical
No findings in this category.

### High
No findings in this category.

### Medium

**D1 — Rustdoc `# Errors` / `# Safety` sections are absent across `src/`**  
- **Document / section**: API docs in `src/crypto.rs`, `src/platform/windows.rs`, and elsewhere.  
- **Evidence**: `grep -RInE '/// # Errors|/// # Panics|/// # Safety|//! # Errors|//! # Panics|//! # Safety' src || echo '(no matches)'` → `(no matches)`. Concrete sites:
  - `src/crypto.rs:33-36`:
    ```rust
    pub(crate) fn get_or_create_encryption_key(
        data_dir: &Path,
        db_exists: bool,
    ) -> Result<(Zeroizing<String>, KeyStorageMode), Box<dyn std::error::Error>> {
    ```
  - `src/platform/windows.rs:671-673`:
    ```rust
    pub(crate) fn dpapi_protect(plaintext: &[u8]) -> Result<Vec<u8>, String> {
        let in_len = u32::try_from(plaintext.len())
            .map_err(|_| "DPAPI plaintext exceeds u32::MAX bytes".to_string())?;
    ```
  - `src/platform/windows.rs:72-75`:
    ```rust
    /// Resolves the keyboard layout (HKL) for the thread whose layout the user is
    /// currently typing with, preferring the foreground window's thread (or the
    /// captured target window when our own window is foreground) over our UI thread.
    unsafe fn target_keyboard_layout() -> HKL {
    ```
- **Drift**: The documentation prompt requires `Result`-returning functions to document `# Errors` and `unsafe` functions to document caller obligations under `# Safety`; no such sections exist in any source file.
- **Impact**: Contributors and reviewers must read implementations to learn failure modes and FFI preconditions; the normative rustdoc convention in `docs/prompts-review/documentation-maintenance.md:28-33` is unmet.
- **Recommendation**: Add `# Errors` to `get_or_create_encryption_key`, `dpapi_protect`, `dpapi_unprotect` (`src/platform/windows.rs:715`), and other `pub(crate)` `Result` APIs; add `# Safety` to `target_keyboard_layout` and the other `unsafe fn`s in `src/platform/windows.rs`.

**D2 — README project tree points to files that do not exist at the documented paths**  
- **Document / section**: `README.md` → "Project Structure".  
- **Evidence** (`README.md:52-53`, `README.md:71`):
  ```text
  ├── WINDOWS_REVIEW.md     # Windows readiness review notes
  ├── WINDOWS_TESTING.md    # Windows manual testing checklist
  ```
  ```text
  │   ├── platform.rs       # OS-level window focus capture, restoration & auto-paste
  ```
  Actual layout from `ls -1 src/platform docs/windows scripts/windows`: `docs/windows/WINDOWS_REVIEW.md`, `docs/windows/WINDOWS_TESTING.md`, and `src/platform/{mod,macos,windows,linux}.rs` (no `src/platform.rs`).
- **Drift**: Two Windows docs are listed at repository root but live under `docs/windows/`; the platform module is listed as a single file but is a four-file directory.
- **Impact**: New contributors follow broken paths; the architecture overview misrepresents the target-gated FFI layout documented in `docs/MULTIPLATFORM_LESSONS.md:9`.
- **Recommendation**: Change the tree entries to `docs/windows/WINDOWS_REVIEW.md`, `docs/windows/WINDOWS_TESTING.md`, and `src/platform/` with `mod.rs`, `macos.rs`, `windows.rs`, `linux.rs`.

**D3 — README "Known Limitations" names an outdated `egui` version**  
- **Document / section**: `README.md` → "Known Limitations", monochrome emoji note.  
- **Evidence**: `README.md:159` begins:
  ```text
  * **Monochrome Emoji & Symbol Rendering:** `egui` (`0.36`) rasterizes font glyphs via `ab_glyph` using monochrome vector outlines and does not support color emoji tables (`sbix`, `COLR`/`CPAL`, or `CBDT`).
  ```
  `Cargo.toml:53` uses `eframe = { version = "0.37", features = ["wgpu"] }`; `Cargo.lock:1134-1135` resolves `name = "egui"` / `version = "0.37.0"`.
- **Drift**: Documented version is one minor behind the manifest and lockfile.
- **Impact**: Users and contributors may misattribute a rendering limitation to the wrong dependency version or doubt whether the note was reviewed after the 0.37 upgrade.
- **Recommendation**: Replace `` `egui` (`0.36`) `` with `` `egui` (`0.37`) `` or with a version-neutral phrase such as "current `egui`/`ab_glyph` renderer" and re-verify the limitation on the pinned version.

**D4 — README lacks actionable guidance for a missing or locked encryption key**  
- **Document / section**: `README.md` → "Running the Application" / "Known Limitations".  
- **Evidence**: `README.md:133-136` describes only first-launch setup:
  ```text
  On first launch:
  1. It automatically generates a secure 256-bit encryption key and stores it in your **OS Keyring** (under service `cipherclipboard`), falling back to a restricted local `.encryption_key` file if the keyring is unavailable.
  ```
  `grep -niE 'missing key|lost key|locked key|keyring (missing|unavailable|locked)|recover' README.md || true` → `(no output)`. The code does emit a detailed recovery message at `src/crypto.rs:157-160`:
  ```rust
  format!(
      "An existing encrypted database was found in '{dir}', but its encryption key could not be found.\n\
       Generating a new random key would fail to decrypt the existing database.\n\
       If you want to start fresh with a new database, delete the directory and restart:\n\
       rm -rf '{dir}'"
  )
  ```
- **Drift**: The fail-closed behavior required by `docs/SECURITY_LESSONS.md:9` exists in code, but the user-facing README provides no pre-emptive explanation of key loss or locked-keyring recovery.
- **Impact**: Users may not understand that a missing key means unrecoverable encrypted history until startup fails; the only guidance appears in a runtime error.
- **Recommendation**: Add a short "Key loss and recovery" subsection that explains the OS keyring entry `cipherclipboard`, the restricted local fallback, and that deleting the data directory destroys history — mirroring the runtime message without presenting deletion as the first step.

### Low

**D5 — Historical bug analysis still cites removed `src/platform.rs` / `src/app.rs` paths without a historical banner**  
- **Document / section**: `docs/BUG_ANALYSIS.md` → header and "Files to Audit Further".  
- **Evidence**: `docs/BUG_ANALYSIS.md:3` reads `**Generated**: 2026-11-14`; there is no "historical" marker. Stale references:
  ```text
  docs/BUG_ANALYSIS.md:116: **File**: `src/platform.rs:260–310` (`setup_reopen_observer`)
  docs/BUG_ANALYSIS.md:196: **File**: `src/platform.rs:143`
  docs/BUG_ANALYSIS.md:255-256: - `src/platform.rs` — 767 lines, heavy `unsafe`, macOS/Windows FFI
                               - `src/app.rs` — 2,647 lines, complex UI state machine
  ```
  Current code places the observer at `src/platform/macos.rs:126` and `src/platform/mod.rs:171`; the UI is `src/app/` (see `ls -1 src/platform` output above and `src/app/mod.rs:1-3`).
- **Drift**: A dated analysis reads as current and points at a monolithic layout that no longer exists.
- **Impact**: Readers may audit or patch nonexistent files, or assume reported line numbers still apply.
- **Recommendation**: Add a top-of-file note such as "> Historical snapshot from 2026-11-14; paths and line numbers predate the `src/platform/` and `src/app/` module split." and correct the path references where the findings were carried forward.

**D6 — README "zero-knowledge" descriptor is imprecise for a local-only product [Inferred]**  
- **Document / section**: `README.md` → opening and "Key Features".  
- **Evidence**: `README.md:8`:
  ```text
  * **Full-Database Encryption at Rest:** Zero-knowledge 256-bit AES encryption powered by **SQLCipher** (`rusqlite` bundled build).
  ```
  `README.md:5` describes running quietly and storing data in an AES-256 encrypted database; `src/app/settings_view.rs:446-448` tells users "Settings and history are strictly stored on your local machine."
- **Drift**: [Inferred] "Zero-knowledge" conventionally describes a remote service that cannot decrypt client data; this application has no server component, so the term adds no verifiable property and can imply a threat model not implemented.
- **Impact**: Overclaiming can erode trust during security review or support conversations.
- **Recommendation**: Replace with "Full-database encryption at rest with a locally stored key (SQLCipher AES-256-CBC + HMAC-SHA512)" and reserve "zero-knowledge" for a documented remote-storage design.

## Quick Wins
1. Fix the three stale README tree entries (`WINDOWS_REVIEW.md`, `WINDOWS_TESTING.md`, `platform.rs`) and the `egui` 0.36 → 0.37 reference — about 15 minutes [Inferred].
2. Add a historical banner to `docs/BUG_ANALYSIS.md` and correct its `src/platform.rs` references — about 15 minutes [Inferred].
3. Add `# Errors` to `get_or_create_encryption_key` and both DPAPI functions, plus `# Safety` to `target_keyboard_layout` — about 1 hour [Inferred].
4. Add a short "Key loss and recovery" subsection to README — about 30 minutes [Inferred].

## Recommended Actions
1. **D2 + D3 + D6 (Medium/Low, ~30–45 min [Inferred])**: correct README paths, dependency version, and product wording in one documentation PR; re-read the rendered tree after editing.
2. **D4 (Medium, ~30 min [Inferred])**: document key-loss behavior and link to the runtime recovery message; do not instruct users to delete data before explaining irreversible loss.
3. **D1 (Medium, ~1–2 h [Inferred])**: add rustdoc sections to the `Result`/`unsafe` APIs cited above; treat this as the prerequisite for any future `cargo doc` warnings-as-errors policy.
4. **D5 (Low, ~15 min [Inferred])**: mark the dated analysis as historical so it is not treated as a current audit.
5. Re-run the two grep commands above after changes; leave `cargo doc`/`cargo test --doc` to the parent's static-analysis pass and link that report here if it produces documentation warnings.

## Coverage
- **Rustdoc quality (`# Errors`, `# Panics`, `# Safety`, examples)**: Finding D1. No examples or doc-test claims made.
- **README and quickstart (metadata, build prerequisites, platform limitations, keyring recovery)**: Findings D2, D3, D4, D6. Build prerequisite commands were checked against existing `scripts/windows/setup-openssl-ci.ps1`; no finding.
- **Project lessons sync (`SECURITY_LESSONS.md`, `MULTIPLATFORM_LESSONS.md`, `CONCURRENCY_AND_PERF_LESSONS.md`, `AGENTS.md`)**: No findings in this category.
- **Prompt and report paths (orchestrator references, output filenames, `DOCUMENTATION_INDEX.md`)**: No findings in this category. All 17 referenced prompt files exist; `docs/DOCUMENTATION_INDEX.md` is absent, and both orchestrator and prompt make its update conditional ("if the index exists"), so absence is not reported as a defect.
- **Release and operational docs (version/tag, packaging, log location/secret redaction)**: No findings in this category. `Cargo.toml:3` and `Cargo.toml:20` both declare `0.1.7`; log permissions/rotation/redaction are documented at `docs/SECURITY_LESSONS.md:47`.
- **Stale/negative legacy mentions in the adaptation map and "do not assume" instructions**: No findings in this category; these are intentional per `docs/prompts-review/RUST_PROMPT_ADAPTATION_MAP.md:40-64` and `docs/prompts-review/master-orchestrator.md:23`.
