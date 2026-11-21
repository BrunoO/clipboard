# Platform Compatibility Review - 2026-11-21

## Executive Summary

- **Health Score**: 7/10 — no Critical or High issues; three OS backends are target-gated and CI exercises all three hosts, but one normative `#[cfg]` inconsistency and one Linux feature gap are evidenced, plus inferred risks. Not 9–10 because these gaps remain; not lower because no compile/runtime failure is demonstrated.
- **Critical Issues**: 0
- **High Issues**: 0
- **Total Findings**: 5 (2 Medium, 3 Low)
- **Estimated Remediation Effort**: [Inferred] ~4 hours total (2 Medium + 3 Low); no measured effort artifact exists.
- **Evidence Commands Run**:
  - `grep -RIn 'QuoteFallback' src` → matches in `src/hotkey.rs:24,30,91,226,231`
  - `grep -RIn 'icacls\|CREATE_NO_WINDOW' src` → `src/crypto.rs:371,378,387,416,418,420,...`
  - `grep -n '^name = "windows-sys"' Cargo.lock` → `5414:`, `5423:`, `5432:`
  - `git ls-files Cargo.lock .github/workflows/ci.yml .github/workflows/release.yml .github/workflows/sonarcloud.yml scripts/windows/setup-openssl-ci.ps1 assets/icon.icns assets/icon.ico assets/icon.png` → all listed as tracked
  - Direct reads of `Cargo.toml`, `build.rs`, `deny.toml`, `.github/workflows/*.yml`, `docs/MULTIPLATFORM_LESSONS.md`, and all of `src/platform/{mod,macos,windows,linux}.rs` plus the relevant `src/hotkey.rs`/`src/crypto.rs` ranges.
  - **No** `cargo check --target ...`, `cargo tree -d`, `cargo build`, `cargo clippy`, or `cargo test` command was run for this report; cross-target compile status is not claimed. CI coverage is cited from workflow lines instead.

## Findings

### Critical

No findings in this category.

### High

No findings in this category.

### Medium

**M-1 — Configuration: mixed `cfg(windows)` and `cfg(target_os = "windows")` predicates in `Cargo.toml`**

- **Platform / boundary**: all platforms; manifest target gates (`Cargo.toml`).
- **Evidence** (verbatim):

```toml
Cargo.toml:28:[target.'cfg(windows)'.build-dependencies]
Cargo.toml:31:[target.'cfg(target_os = "windows")'.dependencies]
Cargo.toml:46:[target.'cfg(target_os = "macos")'.dependencies]
Cargo.toml:49:[target.'cfg(target_os = "linux")'.dependencies]
```

  Source side uses the long form: `src/platform/mod.rs:16: #[cfg(target_os = "windows")]`. Normative rule `docs/MULTIPLATFORM_LESSONS.md:13`: "Never mix `#[cfg(windows)]` and `#[cfg(target_os = "windows")]` in the same file."
- **Compatibility issue**: normative predicate-style violation in a single manifest; `cfg(windows)` is the short alias while every other gate uses `cfg(target_os = "...")`.
- **Impact**: [Inferred] maintenance/review confusion and drift; no current compile failure is evidenced.
- **Recommendation**: change `Cargo.toml:28` to `[target.'cfg(target_os = "windows")'.build-dependencies]`; keep `Cargo.toml:31-50` unchanged. Verify by re-reading the manifest (no build claim made here).

**M-2 — Linux: `mark_clipboard_concealed()` has no Linux branch**

- **Platform / boundary**: Linux; `platform::mark_clipboard_concealed` facade (`src/platform/mod.rs`) vs `src/platform/linux.rs`.
- **Evidence** (verbatim, complete function body):

```rust
src/platform/mod.rs:133:pub(crate) fn mark_clipboard_concealed() {
src/platform/mod.rs:134:    #[cfg(target_os = "macos")]
src/platform/mod.rs:135:    macos::mark_clipboard_concealed();
src/platform/mod.rs:136:
src/platform/mod.rs:137:    #[cfg(target_os = "windows")]
src/platform/mod.rs:138:    windows::mark_clipboard_concealed();
src/platform/mod.rs:139:}
```

  `src/platform/linux.rs` (read in full, 327 lines) defines `pub(super) fn is_clipboard_concealed() -> bool` at line 237 but has no `mark_clipboard_concealed` function. Review criterion `docs/prompts-review/platform-compat-review.md:67`: "Clipboard concealed markers honored/emitted on every platform." The facade's sibling capability does have a Linux stub: `src/platform/mod.rs:102-105: #[cfg(not(any(target_os = "macos", target_os = "windows")))] { None }`.
- **Compatibility issue**: on Linux the emit side is a silent no-op while the detect side exists (`src/platform/linux.rs:237-295`), so behavior is asymmetric across OSes.
- **Impact**: [Inferred] secrets copied by the app are not advertised as concealed to other Linux clipboard consumers; no clipboard content is quoted or logged.
- **Recommendation**: add a `#[cfg(target_os = "linux")]` branch in `src/platform/mod.rs:133-139` calling a Linux backend implementation, or document the intentional gap with an explicit no-op stub and a UI explanation. Verify with a Linux CI test that asserts the marker path runs (test not run here).

### Low

**L-1 — Build script: host-side `#[cfg(windows)]` can diverge from target-gated `winres` dependency (cross-compile risk)**

- **Platform / boundary**: Windows; `build.rs` vs `Cargo.toml` target dependency.
- **Evidence** (verbatim):

```rust
build.rs:2:    #[cfg(windows)]
build.rs:4:        if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
build.rs:5:            let mut res = winres::WindowsResource::new();
build.rs:16:            res.compile().unwrap_or_else(|e| {
```

```toml
Cargo.toml:28:[target.'cfg(windows)'.build-dependencies]
Cargo.toml:29:winres = "0.1"
```

- **Compatibility issue**: `#[cfg(windows)]` in `build.rs` is evaluated when compiling the build script itself, whereas `winres` is declared under a target gate in the manifest; the two conditions are not necessarily the same when cross-compiling. Current release CI builds natively on Windows (`.github/workflows/release.yml:22-24`), so no current release failure is claimed.
- **Impact**: [Inferred] a non-Windows-hosted Windows cross-build could skip icon/version-resource generation; `build.rs:16-21` already fails loudly when `res.compile()` runs and the resource toolchain is missing.
- **Recommendation**: normalize the manifest predicate per M-1 and consider gating resource compilation solely on `CARGO_CFG_TARGET_OS == "windows"` (removing the host `#[cfg(windows)]` wrapper if the build-dependency is always available). Verify with a cross-target check in CI if a toolchain is installed; not run here.

**L-2 — macOS: unchecked `std::process::id() as i32` cast in target-gated code**

- **Platform / boundary**: macOS; `src/platform/macos.rs::capture_frontmost`.
- **Evidence** (verbatim):

```rust
src/platform/macos.rs:270:        let pid: i32 = msg_send![frontmost, processIdentifier];
src/platform/macos.rs:271:        let our_pid = std::process::id() as i32;
```

  Normative rule `docs/MULTIPLATFORM_LESSONS.md:58`: "Inside target-gated code, avoid `items_after_statements`, `borrow_as_ptr` (prefer `&raw mut` / `&raw const` / `ptr::addr_of_mut!`), unchecked `as` casts (prefer `u32::try_from`), and manual nul-terminated byte strings (prefer `c"..."`)." A global allow exists at `Cargo.toml:96: cast_possible_wrap = "allow"`, so lint output would not surface it.
- **Compatibility issue**: unchecked `u32 -> i32` conversion in OS-gated code; the file already uses C-string literals elsewhere (`src/platform/macos.rs:154`, `:169`, `:239`), so the doc's other guidance is followed.
- **Impact**: [Inferred] theoretical PID sign wrap at the extreme of the `u32` range; no observed failure.
- **Recommendation**: replace with `i32::try_from(std::process::id())` and fall back to `0` on error, preserving the `pid > 0` logic at `src/platform/macos.rs:273`.

**L-3 — No macOS release-packaging job, despite shipped macOS bundle metadata**

- **Platform / boundary**: macOS; release workflow vs `Cargo.toml` bundle metadata.
- **Evidence** (verbatim):

```yaml
.github/workflows/release.yml:22:  release-windows:
.github/workflows/release.yml:176:  release-linux:
```

  Workflow read in full (268 lines): no `release-macos` job. Bundle metadata exists at `Cargo.toml:16-26`, e.g. `Cargo.toml:19: icon = ["assets/icon.icns"]` and `Cargo.toml:25: [package.metadata.bundle.macos]`. All three icons are tracked (`git ls-files` output: `assets/icon.icns`, `assets/icon.ico`, `assets/icon.png`).
- **Compatibility issue**: platform support is declared and smoke-tested (`.github/workflows/ci.yml:74-114`) but no automated macOS artifact is produced by this workflow.
- **Impact**: [Inferred] macOS distribution may be manual or handled outside this repository; no claim that macOS is broken.
- **Recommendation**: either add a macOS release job using the existing bundle metadata, or record in docs that macOS artifacts are produced externally so reviewers do not treat the missing job as a regression.

## Quick Wins

1. Normalize `Cargo.toml:28` to `cfg(target_os = "windows")` (M-1; one-line change).
2. Replace `src/platform/macos.rs:271` `as i32` with checked conversion (L-2; small localized change).
3. Add an explicit Linux no-op/documented branch at `src/platform/mod.rs:133-139` so the conceal API is honest on all three OSes (partial M-2).
4. Add a short note on macOS packaging status to close L-3 without changing release automation.

## Recommended Actions

1. **First**: resolve M-2 (Linux concealed-marker emit path) because it is the only functional cross-platform gap with a privacy-relevant impact; keep the marker names consistent with the implementations at `src/platform/macos.rs:411-438` and `src/platform/windows.rs:490-525`.
2. **Second**: normalize target predicates (M-1) and re-check the build-script condition (L-1) together; verify by reading `Cargo.toml:28-50`, `build.rs:1-24`, and `src/platform/mod.rs:12-17` after the edit.
3. **Third**: apply the checked PID conversion (L-2) and clarify macOS release scope (L-3).
4. **Verification when build evidence is available in the separate report**: [Inferred] cross-target `cargo check` for `x86_64-unknown-linux-gnu`, `x86_64-apple-darwin`, and `x86_64-pc-windows-msvc` with `--all-targets --all-features --locked`, plus the existing macOS quit smoke test at `.github/workflows/ci.yml:74-114`. No result is claimed in this report.

## Coverage

**Observed target matrix** (from `deny.toml:4-9` and workflow hosts only; not a compile-result matrix):

| Target triple | Declared in `deny.toml` | CI host coverage observed | Release job observed |
|---|---|---|---|
| `x86_64-unknown-linux-gnu` | `deny.toml:5` | `ubuntu-latest` (`.github/workflows/ci.yml:45`) | `release-linux` (`.github/workflows/release.yml:176`) |
| `x86_64-apple-darwin` | `deny.toml:6` | `macos-latest` (`.github/workflows/ci.yml:45`) | No macOS job (L-3) |
| `aarch64-apple-darwin` | `deny.toml:7` | Not observed in workflows | No macOS job (L-3) |
| `x86_64-pc-windows-msvc` | `deny.toml:8` | `windows-latest` (`.github/workflows/ci.yml:45`) | `release-windows` (`.github/workflows/release.yml:22`) |

**Configuration consistency (prompt §1)**: M-1 and L-1. Coordinated GUI versions observed and clean: `Cargo.toml:53-55` requests `eframe = "0.37"`, `egui_extras = "0.37"`, `egui_material_icons = "0.9"`; `Cargo.lock:1097-1098`, `:1134-1135`, `:1199-1200` resolve `eframe 0.37.0`, `egui 0.37.0`, `egui_extras 0.37.0`, and `Cargo.lock:1229-1234` resolves `egui_material_icons 0.9.0` depending on a single `egui`. No finding there.

**Safe facade discipline (prompt §2)**: No findings in this category. `src/platform/mod.rs` contains no `unsafe` block; the unsafe inventory lists `#![allow(unsafe_code)]` only at `src/platform/macos.rs:1` and `src/platform/windows.rs:1`. OS-agnostic stubs are present, e.g. `src/platform/mod.rs:55-58` (`None` for unsupported OSes) and `:69-72`/`:81-84` (no-op defaults).

**Windows (prompt §3)**: No findings in this category. Evidence reviewed: captured-target UIPI logic `src/platform/windows.rs:560-577` and fail-closed check `:638` (`integrity_level(target_pid).is_none_or(|level| level > self_level)`); `IsIconic` restore at `:157-161` and `:335-339`; layout detection at `:75-108` and `:529-557`; DPAPI and hardened `icacls` path at `src/crypto.rs:371`, `:387`, `:418-420`; Win32 is accessed via feature-gated `windows-sys` (`Cargo.toml:33-44`) rather than raw `extern` blocks needing `#[link]`.

**macOS (prompt §4)**: L-2 only. Evidence reviewed for the rest: main-thread guard `src/platform/macos.rs:36-41` used at `:44`, `:59`, `:72`, `:128`, `:188`; autoreleasepool use at `:75`, `:132`, `:193`, `:236`, `:261`, `:283`, `:307`, `:363`, `:374`, `:412`; `canBecomeMainWindow` filter at `:101-103`; deferred reopen at `:110-115`; C-string literals at `:154`, `:169`, `:239`; accessibility deep link at `:254-258`.

**Linux (prompt §5)**: M-2 only. Evidence reviewed for the rest: 5s reconnect backoff `src/platform/linux.rs:14`, `:82-84`; `thread_local!` persistent session `src/platform/linux.rs:66-68`; pure-Wayland degradation `src/platform/linux.rs:130-133` (`active_window_and_pid()?` → `pid.map(...)`); no X11 change counter `src/platform/mod.rs:102-105`; `QuoteFallback` blocking `recv()` at `src/hotkey.rs:324`, named thread at `:234-236`, grab release at `:314-317`, `:390`, `:394`, `:440`, `:448`.

**Cross-cutting data and UI (prompt §6)**: No findings in this category beyond M-2. Evidence reviewed: concealed-format handling on Windows `src/platform/windows.rs:422-436`, `:462-484`, `:490-525` and macOS `src/platform/macos.rs:373-409`, `:411-438`.

**CI verification (prompt §7)**: No findings in this category. Evidence: OS matrix `.github/workflows/ci.yml:45`; clippy on Linux `:38` and on macOS/Windows `:67-69`; tests on all hosts `:71-72`; no `continue-on-error` on these jobs; macOS quit smoke test `:74-114`; workflow-level least-privilege `permissions: contents: read` at `:9-10` with full-SHA-pinned actions (e.g. `:22`, `:30`). Cross-target `cargo check` was not run for this report.
