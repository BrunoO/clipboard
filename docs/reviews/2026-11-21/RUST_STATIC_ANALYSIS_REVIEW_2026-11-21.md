# Rust Static Analysis Review - 2026-11-21

## Executive Summary
- **Health Score**: 9/10
- **Critical Issues**: 0
- **High Issues**: 0
- **Total Findings**: 2
- **Estimated Remediation Effort**: 1–2 hours
- **Evidence Commands Run**:
  - `rustc --version` → `rustc 1.100.0 (0ef1bc386 2026-11-09) (Homebrew)`
  - `cargo --version` → `cargo 1.100.0 (3acc79c0c 2026-11-05) (Homebrew)`
  - `cargo fmt -- --check` → `FMT_EXIT=0`
  - `cargo check --all-targets --all-features --locked` → `Finished \`dev\` profile [unoptimized + debuginfo] target(s) in 12.08s`, `CHECK_EXIT=0`
  - `cargo clippy --all-targets --all-features --locked -- -D warnings` → `Finished \`dev\` profile [unoptimized + debuginfo] target(s) in 1.55s`, `CLIPPY_EXIT=0`
  - `cargo test --all-targets --locked` → `test result: ok. 74 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 1.15s`, `TEST_EXIT=0`
  - `cargo doc --no-deps --all-features --locked` → `Finished \`dev\` profile [unoptimized + debuginfo] target(s) in 1.96s`, `DOC_EXIT=0`
  - `rustup target list --installed` → `aarch64-apple-darwin`, `x86_64-pc-windows-gnu`
  - `grep -nE 'rust-toolchain|channel|msrv|rust-version' Cargo.toml README.md AGENTS.md .github/workflows/*.yml 2>/dev/null || true` → no MSRV/toolchain declaration matches

All locally runnable gates passed. The two findings are configuration hygiene items, not code defects.

## Findings

### Critical
No findings in this category.

### High
No findings in this category.

### Medium

#### 1. No declared MSRV or pinned Rust toolchain
- **Evidence**: command `grep -nE 'rust-toolchain|channel|msrv|rust-version' Cargo.toml README.md AGENTS.md .github/workflows/*.yml 2>/dev/null || true` produced no matches. `Cargo.toml:1-5` reads:
  ```toml
  [package]
  name = "cipherclipboard"
  version = "0.1.7"
  edition = "2021"
  description = "A simple, encrypted cross-platform clipboard manager in Rust"
  ```
- **Cause**: The project declares an edition but no `rust-version`, `rust-toolchain.toml`, or documented MSRV.
- **Impact**: Contributors and CI cannot tell which minimum compiler is supported; a future dependency upgrade can silently raise the real MSRV.
- **Recommendation**: Add `rust-version = "<chosen version>"` to `[package]` and either add a `rust-toolchain.toml` pin or document the supported toolchain policy in `README.md`/`AGENTS.md`. Verify with `cargo check --all-targets --all-features --locked` under the chosen minimum toolchain.
- **Status**: Fail (configuration gap), not a compiler failure.

### Low

#### 2. Broad crate-level lint suppression in the safe platform facade
- **Evidence**: `src/platform/mod.rs:1`:
  ```rust
  #![allow(clippy::missing_const_for_fn)]
  ```
  The rest of the file is intended to be a 100% safe facade, but this crate-wide `allow` applies to every function in the module.
- **Cause**: A broad suppression is used instead of a function-scoped one with a recorded reason.
- **Impact**: Future non-const-safe helpers in the facade will not be flagged, weakening the module's architectural lint signal.
- **Recommendation**: Replace the crate-level `allow` with narrowly scoped `#[allow(clippy::missing_const_for_fn)]` on the specific functions that cannot be `const`, each with a short reason comment.
- **Status**: Fail (lint hygiene), not a compiler failure.

No findings in this category beyond the item above.

## Quick Wins
1. Add `rust-version` to `Cargo.toml` and document the toolchain policy.
2. Replace `#![allow(clippy::missing_const_for_fn)]` in `src/platform/mod.rs` with function-scoped allows plus reasons.
3. Keep the already-green gate commands in CI unchanged; no code fix is required for fmt/check/clippy/test/doc.

## Recommended Actions
1. Choose the supported MSRV based on the dependency graph, add it to `Cargo.toml`, and CI-check that minimum version.
2. Narrow the `src/platform/mod.rs` allow and re-run `cargo clippy --all-targets --all-features --locked -- -D warnings`.
3. Add the three target-check commands to a documented cross-target matrix when the required toolchains are installed; local review had only `aarch64-apple-darwin` and `x86_64-pc-windows-gnu`, so `x86_64-apple-darwin`, `x86_64-unknown-linux-gnu`, and `x86_64-pc-windows-msvc` checks were skipped.

## Coverage

- **Lint configuration reviewed**: findings above; `[lints.rust] unsafe_code = "deny"` and clippy `all/pedantic/nursery` are present and green.
- **Formatting gate**: Pass. No findings in this category.
- **Compiler gate**: Pass. No findings in this category.
- **Clippy gate**: Pass. No findings in this category.
- **Tests**: Pass (74 tests). No findings in this category.
- **Doc build**: Pass. No findings in this category.
- **Edition/MSRV**: Medium finding recorded; no 2024 migration issues observed because the crate is edition 2021.
- **Target coverage**: Skipped cross-target checks because required targets are not installed; CI workflow coverage was reviewed by other prompt reports. No local target-check findings.
- **Sonar**: Reviewed separately in `./SONAR_REVIEW_2026-11-21.md`; not duplicated here.
- **Unsafe inventory**: Reviewed by `./UNSAFE_FFI_REVIEW_2026-11-21.md`; not duplicated here.
