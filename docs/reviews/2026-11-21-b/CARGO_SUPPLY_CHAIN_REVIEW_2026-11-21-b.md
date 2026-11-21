# Cargo Supply Chain Review - 2026-11-21-b

## Executive Summary
- **Health Score**: 9/10 — `Cargo.toml`, `Cargo.lock`, `deny.toml`, and GitHub Actions workflows enforce `--locked` across all CI and release builds, pin every GitHub Action to a full commit SHA, scope the single `chrono` advisory ignore accurately to both `db.rs` and `logging.rs`, unify `windows v0.62.2` across `wgpu-hal` and `gpu-allocator`, and verify Authenticode/OpenSSL/SHA-256/SLSA release provenance; three low-severity maintenance items remain.
- **Critical Issues**: 0
- **High Issues**: 0
- **Total Findings**: 3 (0 Critical, 0 High, 0 Medium, 3 Low)
- **Estimated Remediation Effort**: 3.5 hours
- **Dependency posture summary (measured counts only)**:
  - `Cargo.lock`: git-tracked (`git ls-files Cargo.lock` → `Cargo.lock`), 6,008 lines, 574 `[[package]]` entries (`573` external crates.io packages + `1` workspace root `cipherclipboard@0.1.7`), `git diff -- Cargo.lock` is empty (0 uncommitted lines).
  - `cargo metadata --format-version 1`: 574 total packages, 94 packages with `custom-build` (`build.rs`) targets (including `cipherclipboard@0.1.7` and `libsqlite3-sys@0.40.0`), 36 `proc-macro` packages.
  - Direct dependencies in `Cargo.toml`: 19 `[dependencies]`, 1 `[build-dependencies]` (`winres = "0.1"`), 1 `[dev-dependencies]` (`tempfile = "3.17"`), plus target-gated dependencies for Windows (`tray-icon = "0.26"`, `windows-sys = "0.61"`), macOS (`objc = "0.2"`), and Linux (`x11rb = "0.14"`).
  - `deny.toml`: 4 target triples (`x86_64-unknown-linux-gnu`, `x86_64-apple-darwin`, `aarch64-apple-darwin`, `x86_64-pc-windows-msvc`), 1 advisory ignore (`RUSTSEC-2026-0543`), 18 allowed SPDX license identifiers.
  - Duplicate crate versions (`cargo tree --locked -d`): 19 duplicate crate families on the Linux host target (35 across `--target all`), with `windows v0.62.2` unified to a single version.
- **Evidence Commands Run**:
  - `git ls-files Cargo.lock deny.toml .github/workflows && wc -l Cargo.lock && grep -c '^\[\[package\]\]' Cargo.lock && git diff -- Cargo.lock && grep -RIn 'uses:' .github/workflows`
  - `cargo metadata --format-version 1 > /tmp/meta.json && jq '.packages | length' /tmp/meta.json` → `574`
  - `cargo tree --locked -d` and `cargo tree --locked --target all -d`
  - `cargo tree --locked -e build` → `cipherclipboard v0.1.7` → `[build-dependencies] └── winres v0.1.12`
  - `cargo tree --locked -i x11rb@0.13.2`, `cargo tree --locked -i x11rb@0.14.0`, `cargo tree --locked --target all -i windows-sys@0.52.0`, `cargo tree --locked --target all -i windows-sys@0.59.0`, `cargo tree --locked --target all -i windows-sys@0.61.2`, `cargo tree --locked --target all -i windows`
  - `which cargo-deny cargo-audit` → **Skipped (not installed on local VM)**; `cargo-deny` is enforced in CI via `EmbarkStudios/cargo-deny-action@17aeb3c839e42274e2b8adf721c8c65edfdc74dd` (`.github/workflows/ci.yml:123-126`).

## Findings

### Critical
No findings in this category.

### High
No findings in this category.

### Medium
No findings in this category.

### Low

**L1. Transitive duplicate versions of `x11rb` (`0.13.2` vs `0.14.0`) and `windows-sys` (`0.52.0`, `0.59.0`, `0.61.2`)**
- **Dependency / control**: Duplicate native-binding crates in `Cargo.lock` and `deny.toml` (`multiple-versions = "warn"`).
- **Severity**: Low.
- **Evidence** — `cargo tree --locked -i x11rb@0.13.2` and `cargo tree --locked -i x11rb@0.14.0` verbatim:
  ```text
  x11rb v0.13.2
  ├── arboard v3.7.0
  │   ├── cipherclipboard v0.1.7 (/home/antigravity/BrunoO/clipboard)
  │   └── egui-winit v0.37.0
  │       └── eframe v0.37.0
  │           └── cipherclipboard v0.1.7 (/home/antigravity/BrunoO/clipboard)
  ├── global-hotkey v0.10.0
  │   └── cipherclipboard v0.1.7 (/home/antigravity/BrunoO/clipboard)
  └── winit v0.30.13
  ---
  x11rb v0.14.0
  └── cipherclipboard v0.1.7 (/home/antigravity/BrunoO/clipboard)
  ```
  And `deny.toml:44-48` verbatim:
  ```toml
  [bans]
  # Duplicate-version baseline (2026-11-21): windows-sys 0.52.0 (winit), 0.59.0
  # (global-hotkey), and 0.61.2 (direct plus arboard/eframe/etc.) are currently
  # required by transitive constraints. Keep `warn` until an upgrade can unify them.
  multiple-versions = "warn"
  ```
- **Risk**: Duplicate versions of `x11rb`/`x11rb-protocol` (`0.13.2` and `0.14.0`) and `windows-sys` (`0.52.0`, `0.59.0`, `0.61.2`) increase compile time and binary size slightly, though no types from those crates cross public trait boundaries between dependencies (and `windows v0.62.2` is unified across `wgpu-hal` and `gpu-allocator`).
- **Recommendation**: Update the `deny.toml:45-47` baseline comment to also note `x11rb 0.13.2/0.14.0`, and periodically check whether `arboard`/`winit`/`global-hotkey` upgrades allow unifying `x11rb` and `windows-sys`.
- **Follow-up test**: Run `cargo tree --locked --target all -d` after future `eframe`/`winit`/`global-hotkey` upgrades.

**L2. `[Inferred]` No macOS release packaging job despite declared macOS targets and bundle metadata**
- **Dependency / control**: Release artifact coverage in `.github/workflows/release.yml` vs. `Cargo.toml` and `deny.toml`.
- **Severity**: Low.
- **Evidence** — `Cargo.toml:17-27` verbatim:
  ```toml
  [package.metadata.bundle]
  name = "CipherClipboard"
  identifier = "com.cipherclipboard.app"
  icon = ["assets/icon.icns"]
  version = "0.1.7"
  ...
  [package.metadata.bundle.macos]
  info-plist = { NSKeychainUsageDescription = "CipherClipboard stores your database encryption key securely in macOS Keychain so your clipboard history remains encrypted and accessible only to you." }
  ```
  `deny.toml:4-9` lists `x86_64-apple-darwin` and `aarch64-apple-darwin`, while `.github/workflows/release.yml` defines only `release-windows` (`:22-23`) and `release-linux` (`:176-177`).
- **Risk**: `[Inferred]` macOS builds and quit behavior are tested in CI (`.github/workflows/ci.yml:45,74-114`), but no signed/checksummed/attested macOS release artifact is produced by `release.yml`.
- **Recommendation**: Either add a `release-macos` job to `.github/workflows/release.yml` using `cargo build --release --locked`, SHA-256 checksums, and `actions/attest-build-provenance`, or document in `README.md` that macOS is currently source-built / local-bundle only.
- **Follow-up test**: Trigger `release.yml` via `workflow_dispatch` on a test tag or branch if a `release-macos` job is added.

**L3. `[Inferred]` Native OS package installations in CI and release workflows are unpinned**
- **Dependency / control**: Native build dependencies (`apt-get` and `choco`) in `.github/workflows/ci.yml`, `.github/workflows/release.yml`, and `scripts/windows/setup-openssl-ci.ps1`.
- **Severity**: Low.
- **Evidence** — `.github/workflows/release.yml:61-63` verbatim:
  ```yaml
        - name: Install Inno Setup
          run: |
            choco install innosetup -y --no-progress
  ```
  `.github/workflows/release.yml:187-190` verbatim:
  ```yaml
        - name: Install Linux dependencies
          run: |
            sudo apt-get update
            sudo apt-get install -y libx11-dev libxcb1-dev libdbus-1-dev libssl-dev pkg-config
  ```
  and `scripts/windows/setup-openssl-ci.ps1:13` verbatim:
  ```powershell
      choco install openssl -y --no-progress
  ```
- **Risk**: `[Inferred]` While `setup-openssl-ci.ps1:67-84` and `release.yml:84-101` verify that the staged OpenSSL DLL matches the import library and is imported by `CipherClipboard.exe`, `choco install innosetup` / `choco install openssl` and `apt-get install` pull the latest repository versions at build time rather than pinned package versions.
- **Recommendation**: Pin `--version` on Chocolatey installs in release workflows (or record the resolved versions in release build logs / SBOM) while keeping the existing OpenSSL import-library hash assertion.
- **Follow-up test**: Verify Windows and Linux release jobs succeed with pinned Chocolatey/apt versions or logged package digests.

## Quick Wins
1. Update the comment in `deny.toml:45-47` to document `x11rb 0.13.2/0.14.0` alongside `windows-sys` in the duplicate-version baseline (L1; ~5 min).
2. Pin or log exact Chocolatey package versions (`innosetup`, `openssl`) in `.github/workflows/release.yml` and `scripts/windows/setup-openssl-ci.ps1` (L3; ~30 min).
3. Document macOS release build status or add a `release-macos` job with SHA-256 and SLSA provenance (L2; ~2h if job added).

## Recommended Actions
1. **L1 (Low, ~0.25h)**: Keep `deny.toml` duplicate-version comments synchronized with `cargo tree --locked --target all -d` output.
2. **L3 (Low, ~1h)**: Pin or record native package versions in release workflows.
3. **L2 (Low, ~2h)**: Add a macOS release job or explicit documentation of source/bundle build expectations for macOS.

## Coverage
- **1. Manifest Hygiene**:
  - `Cargo.toml:1-111` reviewed in full: package metadata (`:1-15`), macOS bundle metadata (`:17-27`), `[build-dependencies]` (`winres = "0.1"` with cross-compilation comment at `:29-32`), target-gated OS dependencies (`:34-54`), core dependencies (`:55-86`), `[dev-dependencies]` (`tempfile = "3.17"`, `:87-88`), `[lints.rust]` (`unsafe_code = "deny"`, `:90-94`), `[lints.clippy]` (`:95-103`), and `[profile.release]` (`:105-110`).
  - No wildcard (`*`) dependencies or unversioned git sources exist.
  - `build.rs:1-23` checks `CARGO_CFG_TARGET_OS == "windows"` and panics loudly (`:16-21`) if `winres` resource compilation fails rather than silently producing an unsigned/iconless binary.
  - Coordinated `egui` trio verified in `Cargo.toml:56-58` (`eframe = "0.37"`, `egui_extras = "0.37"`, `egui_material_icons = "0.9"`), all resolving to `egui v0.37.0`.
  - No findings in this category.
- **2. Lockfile and Reproducibility**:
  - `Cargo.lock` is tracked in git (6,008 lines, 574 packages) with zero uncommitted diff (`git diff -- Cargo.lock` empty).
  - Every `cargo` command in `.github/workflows/ci.yml` (`:38,69,72,87`), `.github/workflows/release.yml` (`:70,218`), and `.github/workflows/sonarcloud.yml` (`:38`) now passes `--locked` (resolving previous review finding M1).
  - `cargo metadata --format-version 1` and `cargo tree --locked` both succeeded without error.
  - No findings in this category.
- **3. Advisories and Licenses**:
  - `deny.toml:1-56` sets `wildcards = "deny"`, `unknown-registry = "deny"`, `unknown-git = "deny"`, and `allow-registry = ["https://github.com/rust-lang/crates.io-index"]`.
  - The single ignored advisory `RUSTSEC-2026-0543` in `deny.toml:13-16` now accurately states: `reason = "Used only for local timestamp string formatting in db.rs and logging.rs"` (resolving previous review finding M2; verified against `grep -RIn 'chrono' src/` which matches only `src/db.rs:1` and `src/logging.rs:73`).
  - `cargo-deny` and `cargo-audit` CLI binaries were not preinstalled on this review VM (**Skipped locally**), and `cargo-deny check` is wired as a required CI job in `.github/workflows/ci.yml:116-126`.
  - No findings in this category.
- **4. Duplicate and Conflicting Versions**:
  - `cargo tree --locked --target all -i windows` confirms `windows v0.62.2` is unified to a single version across `accesskit_windows`, `fontique`, `gpu-allocator`, and `wgpu-hal` per `docs/MULTIPLATFORM_LESSONS.md:31`.
  - Finding L1 notes the remaining transitive duplicates (`x11rb 0.13.2/0.14.0` and `windows-sys 0.52.0/0.59.0/0.61.2`).
- **5. Build-Time Attack Surface**:
  - `cargo tree --locked -e build` shows `winres v0.1.12` as the only direct build dependency of `cipherclipboard`.
  - Across the full transitive graph (`cargo metadata`), 94 packages declare `custom-build` scripts (notably `libsqlite3-sys@0.40.0` for bundled SQLCipher) and 36 are `proc-macro` crates.
  - Windows OpenSSL resolution is centralized in `scripts/windows/setup-openssl-ci.ps1:1-96`, selecting the dynamic CRT (`lib\VC\x64\MD\libcrypto.lib`) and deriving `OPENSSL_RUNTIME_DLL` via regex from `libcrypto.lib`.
  - Finding L3.
- **6. CI/CD Trust**:
  - All 19 `uses:` lines across `.github/workflows/ci.yml`, `.github/workflows/release.yml`, and `.github/workflows/sonarcloud.yml` are pinned to full 40-hex commit SHAs.
  - Top-level `permissions: contents: read` is set in all three workflows (`ci.yml:9-10`, `release.yml:14-15`, `sonarcloud.yml:9-10`); write scopes are granted only to `release-windows` (`release.yml:25-28`) and `release-linux` (`release.yml:179-182`).
  - No `continue-on-error` exists in any workflow, and `sonarcloud.yml:48` runs on `push` to `main` even if `SONAR_TOKEN` is empty so missing secrets fail loudly.
  - No findings in this category.
- **7. Release Artifact Verification**:
  - `release.yml:112-115` verifies a valid Microsoft Authenticode signature on downloaded `vc_redist.x64.exe`.
  - `release.yml:84-101` verifies unique SHA-256 across candidate OpenSSL DLLs under `OPENSSL_DIR` and asserts that `CipherClipboard.exe` imports `$env:OPENSSL_RUNTIME_DLL`.
  - `release.yml:139-154` (Windows) and `:241-249` (Linux, using `sha256sum --`) compute SHA-256 checksums, and `:156-161` / `:250-254` publish SLSA build provenance attestations via `actions/attest-build-provenance`.
  - Finding L2.
