# Rust Static Analysis Review - 2026-11-21-b

## Executive Summary
- **Health Score**: 9/10
- **Critical Issues**: 0
- **High Issues**: 0
- **Total Findings**: 1 (0 Critical, 0 High, 0 Medium, 1 Low)
- **Estimated Remediation Effort**: 0.5 hours
- **Evidence Commands Run**:
  - `rustc --version` → `rustc 1.100.0 (0ef1bc386 2026-11-09)`
  - `cargo --version` → `cargo 1.100.0 (3acc79c0c 2026-11-05)`
  - `rustup target list --installed` → `x86_64-unknown-linux-gnu`
  - `cargo fmt -- --check` → `FMT_EXIT=0`
  - `cargo check --all-targets --all-features --locked` → `Finished \`dev\` profile [unoptimized + debuginfo] target(s) in 0.44s`, `CHECK_EXIT=0`
  - `cargo clippy --all-targets --all-features --locked -- -D warnings` → `Finished \`dev\` profile [unoptimized + debuginfo] target(s) in 0.48s`, `CLIPPY_EXIT=0`
  - `cargo test --all-targets --locked` → `test result: ok. 97 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 6.76s`, `TEST_EXIT=0`
  - `cargo doc --no-deps --all-features --locked` → `Finished \`dev\` profile [unoptimized + debuginfo] target(s) in 0.36s`, `DOC_EXIT=0`
  - `grep -RInE '#!?\[allow\(|#!?\[expect\(|\bunsafe\b' src Cargo.toml` → all allows, expects, and unsafe sites inventoried

Note: `clang-tidy` is not applicable to this Rust/Cargo repository (`docs/prompts-review/rust-compiler-clippy-review.md:10,19`). All locally runnable compiler, formatter, Clippy, documentation, and test gates passed with zero warnings or errors. Compared to the `2026-11-21` review, the MSRV configuration gap has been resolved (`Cargo.toml:5` now declares `rust-version = "1.95"` and `README.md:93` documents the policy). Only one Low lint-hygiene finding remains.

### Gate Results Table

| Command | Status | Output Reference |
|---|---|---|
| `cargo fmt -- --check` | **Pass** | Exit `0` (`FMT_EXIT=0`, 0 formatting diffs) |
| `cargo check --all-targets --all-features --locked` | **Pass** | Exit `0` (`Finished \`dev\` profile [unoptimized + debuginfo] target(s) in 0.44s`) |
| `cargo clippy --all-targets --all-features --locked -- -D warnings` | **Pass** | Exit `0` (`Finished \`dev\` profile [unoptimized + debuginfo] target(s) in 0.48s`, 0 warnings) |
| `cargo test --all-targets --locked` | **Pass** | Exit `0` (`97 passed; 0 failed; 0 ignored; finished in 6.76s`) |
| `cargo doc --no-deps --all-features --locked` | **Pass** | Exit `0` (`Generated .../target/doc/CipherClipboard/index.html`) |
| `cargo check --target x86_64-unknown-linux-gnu --all-targets --all-features --locked` | **Pass** | Host target (`x86_64-unknown-linux-gnu`) checked |
| `cargo check --target x86_64-apple-darwin --all-targets --all-features --locked` | **Skipped** | Target `x86_64-apple-darwin` not installed locally (`rustup target list --installed`); covered in CI by `.github/workflows/ci.yml:45,67-69` |
| `cargo check --target x86_64-pc-windows-msvc --all-targets --all-features --locked` | **Skipped** | Target `x86_64-pc-windows-msvc` not installed locally (`rustup target list --installed`); covered in CI by `.github/workflows/ci.yml:45,67-69` |

## Findings

### Critical
No findings in this category.

### High
No findings in this category.

### Medium
No findings in this category.

### Low

#### 1. Module-wide `#![allow(clippy::missing_const_for_fn)]` in `src/platform/mod.rs` makes function-level `expect` attributes redundant
- **Gate / lint**: `clippy::missing_const_for_fn`
- **Severity**: Low
- **Evidence**: `src/platform/mod.rs:8`, `src/platform/mod.rs:97-104`, and `src/platform/mod.rs:149-156`:
  ```rust
  // src/platform/mod.rs:8
  #![allow(clippy::missing_const_for_fn)]

  // src/platform/mod.rs:97-104
  #[cfg_attr(
      not(target_os = "windows"),
      expect(
          clippy::missing_const_for_fn,
          reason = "non-Windows stub keeps the cross-platform facade uniform"
      )
  )]
  pub(crate) fn quote_key_is_ugrave() -> bool {

  // src/platform/mod.rs:149-156
  #[cfg_attr(
      not(target_os = "windows"),
      expect(
          clippy::missing_const_for_fn,
          reason = "non-Windows stub keeps the cross-platform facade uniform"
      )
  )]
  pub(crate) fn set_title_bar_dark_mode(is_dark: bool) {
  ```
- **Cause**: `src/platform/mod.rs` already has narrowly scoped `#[cfg_attr(..., expect(clippy::missing_const_for_fn, reason = "..."))]` attributes on `quote_key_is_ugrave()` and `set_title_bar_dark_mode()`, yet retains the top-level `#![allow(clippy::missing_const_for_fn)]` inner attribute at line 8 across the 766-line facade module.
- **Recommendation**: Remove `#![allow(clippy::missing_const_for_fn)]` at `src/platform/mod.rs:8` and place narrow `#[cfg_attr(..., expect(clippy::missing_const_for_fn, reason = "..."))]` attributes only on the remaining non-`const` OS-dispatch stubs (such as `auto_paste_permission_missing`, `prompt_auto_paste_permission`, `open_auto_paste_permission_settings`, `clipboard_change_token`, `is_clipboard_concealed`, `mark_clipboard_concealed`, `hide_application`, `auto_paste_blocked_warning`, `order_front_main_window`, `take_reopen_requested`, `setup_reopen_observer`, `teardown_reopen_observer`, `capture_active_window`, `restore_focus_platform`, and `simulate_paste_if_foreground`), or mark pure helpers `const fn` where possible (`clipboard_clear_is_due` in `src/app/mod.rs:1511`).
- **Status**: Fail (lint hygiene), not a compiler failure.

## Quick Wins
1. Remove `#![allow(clippy::missing_const_for_fn)]` from `src/platform/mod.rs:8` and use targeted `#[cfg_attr(..., expect(clippy::missing_const_for_fn, reason = "..."))]` on the specific platform-facade functions whose bodies are no-ops on non-target OSes.
2. [Inferred] Add a `reason = "..."` argument to the remaining local `#[allow(...)]` attributes (such as `src/db.rs:609`, `src/settings.rs:163,309`, matching the pattern already used at `src/app/mod.rs:1722-1725`).

## Recommended Actions
1. **Lint Configuration Hygiene**: Replace the module-wide `#![allow(clippy::missing_const_for_fn)]` in `src/platform/mod.rs:8` with function-level `#[expect(..., reason = "...")]` attributes and verify with `cargo clippy --all-targets --all-features --locked -- -D warnings`.
2. **Maintain MSRV Verification**: Keep `rust-version = "1.95"` in `Cargo.toml:5` aligned with dependency upgrades and `README.md:93`.
3. **Preserve Multi-OS Clippy Gates in CI**: Keep `.github/workflows/ci.yml:38` (Linux) and `.github/workflows/ci.yml:69` (macOS and Windows) running `cargo clippy --all-targets --all-features --locked -- -D warnings` on all three operating systems.

## Coverage

- **1. Lint Configuration**:
  - Reviewed `Cargo.toml:90-103`:
    ```toml
    [lints.rust]
    unsafe_code = "deny"
    missing_debug_implementations = "warn"
    unreachable_pub = "warn"

    [lints.clippy]
    all = { level = "warn", priority = -1 }
    pedantic = { level = "warn", priority = -1 }
    nursery = { level = "warn", priority = -1 }
    cast_possible_wrap = "allow"
    module_name_repetitions = "allow"
    significant_drop_tightening = "allow"
    redundant_pub_crate = "allow"
    ```
  - Lint priority is `-1` on the lint groups so individual allows override cleanly, and CI passes `-D warnings` (`.github/workflows/ci.yml:38,69`) to promote all warnings to errors.
  - `unsafe_code = "deny"` is enforced crate-wide and overridden only in `src/platform/macos.rs:1` and `src/platform/windows.rs:1` (`#![allow(unsafe_code)]`), preserving the `AGENTS.md` and `docs/SECURITY_LESSONS.md` invariant.
  - The four crate-level Clippy allows (`cast_possible_wrap`, `module_name_repetitions`, `significant_drop_tightening`, `redundant_pub_crate`) are deliberate project conventions for a single-binary `pub(crate)` crate using `egui`/`rusqlite`.
  - Local `#[allow(...)]` inventory across `src/`:
    - `src/main.rs:2` (`#![allow(unexpected_cfgs)]`), `src/main.rs:74` (`#[allow(clippy::too_many_lines)]` on `run()`)
    - `src/platform/macos.rs:1-2` (`#![allow(unsafe_code)]`, `#![allow(unexpected_cfgs)]` for `objc` macros)
    - `src/platform/windows.rs:1` (`#![allow(unsafe_code)]`)
    - `src/platform/mod.rs:8` (`#![allow(clippy::missing_const_for_fn)]` — reported as Low Finding 1), `src/platform/mod.rs:36` (`QuoteHotkeyFallback` stub), `src/platform/mod.rs:77` (`unnecessary_wraps` on `is_app_active_state`), `src/platform/mod.rs:99,151` (`expect(clippy::missing_const_for_fn)`), `src/platform/mod.rs:385` (`dead_code` on Windows for `hide_application`)
    - `src/db.rs:609` (`too_many_lines` on `get_recent`)
    - `src/secret.rs:374` (`cast_precision_loss` on `shannon_entropy`)
    - `src/color.rs:66,268,283,295,299,305,340` (bounded float-to-u8 color channel casts after `.round().clamp(...)` and `many_single_char_names` for `r,g,b,h,s,l`)
    - `src/theme.rs:405` (bounded float-to-u8 casts in `ensure_contrast`), `src/theme.rs:723` (`too_many_lines, similar_names` on `test_wcag_contrast_audit`)
    - `src/settings.rs:163,309` (`too_many_lines` on keycode match tables), `src/settings.rs:429` (`struct_excessive_bools` on `AppSettings`)
    - `src/password.rs:72,110` (`struct_excessive_bools` on `PasswordConfig` and `PasswordGeneratorState`), `src/password.rs:272` (`cast_precision_loss` on `estimate_entropy_bits`)
    - `src/app/mod.rs:173` (`struct_excessive_bools` on `ClipboardApp`), `src/app/mod.rs:189` (`dead_code` on non-Windows `tray_service`), `src/app/mod.rs:219` (`too_many_arguments` on `ClipboardApp::new`), `src/app/mod.rs:1722` (`too_many_lines` with explicit `reason` on `egui_key_to_hotkey_name`)
    - `src/app/generator_view.rs:139` (`cast_possible_truncation` on clamped entropy bar ratio)
    - `src/app/history_view.rs:20` (`struct_excessive_bools` on `ItemRenderState`)
    - `src/tray.rs:5,36,155,176` (`dead_code` / `unused_variables` / `missing_const_for_fn` on non-Windows stubs and test-covered `load_tray_icon_rgba`)
- **2. Formatting**: Pass. `cargo fmt -- --check` exited `0`. `.rustfmt.toml:1` (`edition = "2021"`) matches `Cargo.toml:4` (`edition = "2021"`). No findings in this category.
- **3. Compiler and Clippy Gates**: Pass. `cargo check --all-targets --all-features --locked`, `cargo clippy --all-targets --all-features --locked -- -D warnings`, `cargo test --all-targets --locked` (97 passed), and `cargo doc --no-deps --all-features --locked` all exited `0` with zero warnings. No findings in this category.
- **4. Target Coverage**:
  - `deny.toml:4-9` declares four target triples: `x86_64-unknown-linux-gnu`, `x86_64-apple-darwin`, `aarch64-apple-darwin`, and `x86_64-pc-windows-msvc`.
  - Locally, only `x86_64-unknown-linux-gnu` is installed (`rustup target list --installed`), so cross-target `cargo check` for `x86_64-apple-darwin` and `x86_64-pc-windows-msvc` was marked **Skipped**.
  - CI runs `cargo clippy --all-targets --all-features --locked -- -D warnings` and `cargo test --all-targets --locked --verbose` across `ubuntu-latest`, `macos-latest`, and `windows-latest` (`.github/workflows/ci.yml:38,45,68-72`). No findings in this category.
- **5. Edition and MSRV Policy**:
  - `Cargo.toml:4-5` declares `edition = "2021"` and `rust-version = "1.95"`.
  - `README.md:93` documents the MSRV policy (`1.95`) and `--locked` CI resolution. No Edition 2024 migration issues apply because the crate is Edition 2021. No findings in this category.
- **6. Static Analysis Complement**:
  - SonarQube is reviewed separately in `./SONAR_REVIEW_2026-11-21-b.md` and not duplicated here. No findings in this category.
