# Cargo Build Health Review - 2026-11-21-b

## Executive Summary
- **Health Score**: 9/10
- **Critical Issues**: 0
- **High Issues**: 0
- **Total Findings**: 2 (0 Critical, 0 High, 0 Medium, 2 Low)
- **Estimated Remediation Effort**: 1 hour
- **Evidence Commands Run**:
  - `cargo metadata --no-deps --format-version 1` → package `cipherclipboard 0.1.7`, edition `2021`, `rust_version: "1.95"`, binary target `CipherClipboard` (`src/main.rs`) and `custom-build` (`build.rs`)
  - `cargo tree --locked -e features --depth 1` → verified additive features (`image` with `default-features = false, features = ["png"]`, `eframe` with `wgpu`, `egui_extras` with `syntect`, `rusqlite` with `bundled-sqlcipher`, `x11rb` with `xtest`)
  - `cargo tree --locked -d --depth 1` → listed duplicate packages on Linux host (`x11rb`, `smithay-client-toolkit`, `calloop`, `rustix`, `syn`, `thiserror`, `hashbrown`, `miniz_oxide`, etc.)
  - `cargo check --all-targets --all-features --locked` → `Finished \`dev\` profile [unoptimized + debuginfo] target(s) in 0.44s`, `CHECK_EXIT=0`
  - `cargo check --target x86_64-unknown-linux-gnu --all-targets --all-features --locked` → `LINUX_TARGET_EXIT=0`
  - `cargo deny --version` → `error: no such command: \`deny\`` (local `cargo deny check` skipped; covered in CI by `.github/workflows/ci.yml:123-126`)
  - `find . -name build.rs -not -path './target/*' -print` → `./build.rs`
  - `grep -RIn 'cargo build\|cargo clippy\|cargo test\|cargo deny\|cache\|sccache\|--locked\|all-features' .github/workflows`
  - `git ls-files Cargo.lock .github/workflows/ci.yml .github/workflows/release.yml .github/workflows/sonarcloud.yml scripts/windows/setup-openssl-ci.ps1 assets/icon.icns assets/icon.ico assets/icon.png` → all tracked; `git status --short Cargo.lock` → clean

Note: CMake, git submodules, precompiled headers (PCH), and `clang-tidy` are not applicable to this Cargo crate (`docs/prompts-review/cargo-build-health.md:10,24`). Compared to the `2026-11-21` review, the Medium reproducibility gap has been completely resolved: every `cargo` invocation in `.github/workflows/ci.yml:38,69,72,87`, `.github/workflows/sonarcloud.yml:38`, and `.github/workflows/release.yml:70,218` now passes `--locked`, and `deny.toml:45-48` documents the `windows-sys` duplicate-version baseline. Only two Low hygiene findings remain.

### Build Configuration Summary

| Area | Evidence | Status |
|---|---|---|
| Package & Binary Metadata | `Cargo.toml:1-15` — `cipherclipboard 0.1.7`, `edition = "2021"`, `rust-version = "1.95"`, `license = "MIT"`, `default-run = "CipherClipboard"`, `[[bin]] name = "CipherClipboard"`, `path = "src/main.rs"` | Consistent with `README.md` and release artifacts |
| Bundle Metadata | `Cargo.toml:17-27` — `identifier = "com.cipherclipboard.app"`, `version = "0.1.7"`, `icon = ["assets/icon.icns"]`, `NSKeychainUsageDescription` in `[package.metadata.bundle.macos]` | Matches `[package].version`; all icon assets tracked |
| Dependency Scoping | `Cargo.toml:29-89` — `[build-dependencies]` (`winres = "0.1"`), `[target.'cfg(target_os = "windows")'.dependencies]`, `[target.'cfg(target_os = "macos")'.dependencies]`, `[target.'cfg(target_os = "linux")'.dependencies]`, `[dev-dependencies]` (`tempfile = "3.17"`) | Properly scoped |
| Lockfile & Reproducibility | `Cargo.lock` tracked and clean (`git status --short Cargo.lock`); `--locked` used on all CI, coverage, and release cargo commands (`ci.yml:38,69,72,87`, `sonarcloud.yml:38`, `release.yml:70,218`) | Reproducible |
| Build Script & Native Deps | `build.rs:1-23` gates Windows resource compilation on `CARGO_CFG_TARGET_OS == "windows"` and fails loudly on missing `rc.exe`/`windres`; `scripts/windows/setup-openssl-ci.ps1` validates MD `libcrypto.lib` and runtime DLL | Verified in `release.yml:72-102` |
| Release Profile | `Cargo.toml:105-110` — `opt-level = 3`, `lto = "thin"`, `codegen-units = 1`, `panic = "abort"`, `strip = true` | Configured for production binary size/performance |
| CI Quality Gates & Security | `ci.yml:9-10` (`permissions: contents: read`), full-SHA action pins (`ci.yml:22,30,49,63,81,84,121,124`, `release.yml:31,67,157,164,185,193,251,256`, `sonarcloud.yml:20,30,35,41,49`), SLSA provenance attestation (`release.yml:156-161,250-253`) | Hardened |

### Command Results Table

| Command | Status | Result Summary |
|---|---|---|
| `cargo metadata --no-deps --format-version 1` | **Pass** | Validated package `cipherclipboard 0.1.7`, `rust_version: "1.95"`, targets `CipherClipboard` and `build-script-build` |
| `cargo tree --locked -e features --depth 1` | **Pass** | Verified feature set across direct, target, build, and dev dependencies |
| `cargo tree --locked -d --depth 1` | **Pass** | Identified duplicate transitive crates (`x11rb`, `smithay-client-toolkit`, `calloop`, `rustix`, `syn`, `thiserror`, etc.) |
| `cargo check --all-targets --all-features --locked` | **Pass** | `Finished \`dev\` profile [unoptimized + debuginfo] target(s) in 0.44s` (`CHECK_EXIT=0`) |
| `cargo check --target x86_64-unknown-linux-gnu --all-targets --all-features --locked` | **Pass** | `Finished \`dev\` profile [unoptimized + debuginfo] target(s) in 12.11s` (`LINUX_TARGET_EXIT=0`) |
| `cargo build --release --locked` | **Skipped** | Full release LTO compile skipped during static review pass; verified in `.github/workflows/release.yml:70,218` |
| `cargo deny check` | **Skipped** | `cargo-deny` subcommand is not installed in this VM (`error: no such command: \`deny\``); enforced in CI at `.github/workflows/ci.yml:123-126` |
| `find . -name build.rs -not -path './target/*' -print` | **Pass** | `./build.rs` (single root build script) |
| `grep -RIn 'cargo build\|cargo clippy\|cargo test\|cargo deny' .github/workflows` | **Pass** | Confirmed `--locked` on all cargo build/clippy/test/llvm-cov steps |

## Findings

### Critical
No findings in this category.

### High
No findings in this category.

### Medium
No findings in this category.

### Low

#### L-1 — `multiple-versions = "warn"` in `deny.toml` permits duplicate `x11rb` (`0.13.2` and `0.14.0`) and `windows-sys` (`0.52.0`, `0.59.0`, `0.61.2`) versions
- **Build control**: `deny.toml:44-50`, `Cargo.toml:36,53`, and `Cargo.lock`.
- **Severity**: Low
- **Evidence**:
  ```toml
  # deny.toml:44-50
  [bans]
  # Duplicate-version baseline (2026-11-21): windows-sys 0.52.0 (winit), 0.59.0
  # (global-hotkey), and 0.61.2 (direct plus arboard/eframe/etc.) are currently
  # required by transitive constraints. Keep `warn` until an upgrade can unify them.
  multiple-versions = "warn"
  wildcards = "deny"
  highlight = "all"
  ```
  In `Cargo.lock:5414-5433`, three versions of `windows-sys` (`0.52.0`, `0.59.0`, `0.61.2`) are present while `windows` has a single version (`0.62.2` at `Cargo.lock:5300-5301`). Additionally, `cargo tree --locked -d --depth 1` on Linux shows a direct vs. transitive version split for `x11rb`:
  ```text
  x11rb v0.13.2
  ├── arboard v3.7.0
  ├── global-hotkey v0.10.0 (*)
  └── winit v0.30.13 (*)

  x11rb v0.14.0
  └── cipherclipboard v0.1.7 (/home/antigravity/BrunoO/clipboard)
  ```
- **Failure mode**: `multiple-versions = "warn"` does not fail CI when new duplicate crate versions enter `Cargo.lock`, and `x11rb` is compiled twice on Linux (`0.13.2` for `arboard`/`global-hotkey`/`winit` and `0.14.0` for `cipherclipboard`).
- **Impact**: Extra compile units and binary footprint on Linux and Windows; no build failure or type-mismatch bug occurs because `windows` `0.62.2` is unified and `x11rb` connections are crate-private.
- **Recommendation**: Update the baseline comment in `deny.toml:45-48` to include the `x11rb 0.13.2`/`0.14.0` split (or align `Cargo.toml:53` to `x11rb = "0.13"` if `0.14`-specific APIs are not needed).

#### L-2 — Linux CI jobs omit `libssl-dev` and `pkg-config` from `apt-get install`, relying on preinstalled runner packages
- **Build control**: `.github/workflows/ci.yml:24-27,51-55` and `.github/workflows/sonarcloud.yml:24-27` vs `.github/workflows/release.yml:187-190` and `README.md:105`.
- **Severity**: Low
- **Evidence**:
  `.github/workflows/release.yml:187-190` and `README.md:105` install five native packages on Ubuntu for X11, DBus, and bundled SQLCipher (`libsqlite3-sys` links against OpenSSL `libcrypto`):
  ```yaml
  # .github/workflows/release.yml:187-190
        - name: Install Linux dependencies
          run: |
            sudo apt-get update
            sudo apt-get install -y libx11-dev libxcb1-dev libdbus-1-dev libssl-dev pkg-config
  ```
  However, `.github/workflows/ci.yml:24-27,51-55` and `.github/workflows/sonarcloud.yml:24-27` omit `libssl-dev pkg-config`:
  ```yaml
  # .github/workflows/ci.yml:24-27
        - name: Install Linux dependencies
          run: |
            sudo apt-get update
            sudo apt-get install -y libx11-dev libxcb1-dev libdbus-1-dev
  ```
- **Failure mode**: `ci.yml` and `sonarcloud.yml` rely on `ubuntu-latest` having `libssl-dev` and `pkg-config` preinstalled in the base runner image, whereas `release.yml` and `README.md` declare them explicitly.
- **Impact**: If a minimal or self-hosted Ubuntu runner is used (similar to the Windows runner motivation documented in `scripts/windows/setup-openssl-ci.ps1:2-5`), the `libsqlite3-sys` (`bundled-sqlcipher`) build script could fail in `ci.yml` or `sonarcloud.yml`.
- **Recommendation**: Add `libssl-dev pkg-config` to the `apt-get install -y` lines in `.github/workflows/ci.yml:27,55` and `.github/workflows/sonarcloud.yml:27` so all Linux workflows match `.github/workflows/release.yml:190` and `README.md:105`.

## Quick Wins
1. Append `libssl-dev pkg-config` to `.github/workflows/ci.yml:27,55` and `.github/workflows/sonarcloud.yml:27` to match `.github/workflows/release.yml:190`.
2. Record `x11rb 0.13.2` / `0.14.0` in `deny.toml:45-48` (or align `Cargo.toml:53` to `x11rb = "0.13"`).

## Recommended Actions
1. **Standardize Linux Native Prerequisite Lists (L-2)**: Align `.github/workflows/ci.yml` and `.github/workflows/sonarcloud.yml` with `.github/workflows/release.yml` by explicitly installing `libssl-dev pkg-config`.
2. **Maintain Duplicate-Dependency Baseline (L-1)**: Keep `windows` unified on `0.62.2` in `Cargo.lock` and periodically check `cargo tree --locked -d` when upgrading `eframe`, `global-hotkey`, `arboard`, or `x11rb`.

## Coverage

- **1. Manifest correctness**:
  - `Cargo.toml:1-15` declares `name = "cipherclipboard"`, `version = "0.1.7"`, `edition = "2021"`, `rust-version = "1.95"`, `license = "MIT"`, `repository = "https://github.com/BrunoO/clipboard"`, `readme = "README.md"`, `default-run = "CipherClipboard"`, and `[[bin]] name = "CipherClipboard"`, `path = "src/main.rs"`.
  - `[package.metadata.bundle]` (`Cargo.toml:17-27`) version `"0.1.7"` matches `[package].version`, `assets/icon.icns` exists and is tracked, and `NSKeychainUsageDescription` accurately describes macOS Keychain usage. No findings in this category.
- **2. Dependency Graph and Features**:
  - `Cargo.toml:29-89` cleanly separates `[build-dependencies]`, target-specific `[target.'cfg(target_os = "...")'.dependencies]`, `[dependencies]`, and `[dev-dependencies]`.
  - `image` disables default features (`default-features = false, features = ["png"]` at `Cargo.toml:79`) so unused image decoders are not compiled.
  - Duplicate dependency versions reported in **L-1**.
- **3. Lockfile and Reproducibility**:
  - `Cargo.lock` is tracked in git and unmodified.
  - Every `cargo` command across `.github/workflows/ci.yml:38,69,72,87`, `.github/workflows/sonarcloud.yml:38`, and `.github/workflows/release.yml:70,218` uses `--locked`. No findings in this category.
- **4. Build Scripts and Native Toolchains**:
  - `build.rs:1-23` checks `CARGO_CFG_TARGET_OS == "windows"` and panics with an actionable message if `winres::WindowsResource::compile()` fails (`build.rs:16-21`).
  - `.github/workflows/release.yml:79-83` verifies that `CipherClipboard.exe` contains the embedded `VersionInfo` (`ProductName == "CipherClipboard"` and non-empty `FileDescription`) and verifies the staged OpenSSL runtime DLL against the linked import library (`release.yml:84-101`).
  - Minor Linux `apt-get` consistency gap reported in **L-2**.
- **5. Profiles and Artifacts**:
  - `[profile.release]` (`Cargo.toml:105-110`) configures `opt-level = 3`, `lto = "thin"`, `codegen-units = 1`, `panic = "abort"`, and `strip = true`.
  - Release artifact paths in `.github/workflows/release.yml:75` (`target\release\CipherClipboard.exe`) and `.github/workflows/release.yml:222` (`target/release/CipherClipboard`) match `[[bin]] name = "CipherClipboard"`, and SHA-256 checksums plus build provenance attestations are generated for both Windows and Linux releases. No findings in this category.
- **6. Cross-Target Build Matrix**:
  - `deny.toml:4-9` lists `x86_64-unknown-linux-gnu`, `x86_64-apple-darwin`, `aarch64-apple-darwin`, and `x86_64-pc-windows-msvc`.
  - Local `cargo check --target x86_64-unknown-linux-gnu --all-targets --all-features --locked` passed; macOS and Windows targets are verified in CI (`.github/workflows/ci.yml:45`). (The absence of a macOS release packaging job is tracked in `./PLATFORM_COMPAT_REVIEW_2026-11-21-b.md` L-1.) No findings in this category.
- **7. Quality Gates in CI**:
  - `ci.yml` runs `cargo fmt -- --check` (`:35`), `cargo clippy --all-targets --all-features --locked -- -D warnings` (`:38,69`), `cargo test --all-targets --locked --verbose` (`:72`), `macos-quit-smoke` (`:74-114`), and `cargo-deny-action` (`:123-126`).
  - All GitHub Actions across `ci.yml`, `release.yml`, and `sonarcloud.yml` are pinned to full 40-character commit SHAs with least-privilege `permissions: contents: read` at the workflow level. No findings in this category.
