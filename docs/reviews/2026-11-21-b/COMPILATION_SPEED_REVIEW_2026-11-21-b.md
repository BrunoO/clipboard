# Compilation Speed Review - 2026-11-21-b

## Executive Summary
- **Health Score**: 8/10
- **Critical Issues**: 0
- **High Issues**: 0
- **Total Findings**: 2 (0 Critical, 0 High, 0 Medium, 2 Low)
- **Estimated Remediation Effort**: 1 hour
- **Evidence Commands Run**:
  - `cargo check --all-targets --all-features --locked` → `Finished \`dev\` profile [unoptimized + debuginfo] target(s) in 0.44s`
  - `cargo clippy --all-targets --all-features --locked -- -D warnings` → `Finished \`dev\` profile [unoptimized + debuginfo] target(s) in 0.48s`
  - `cargo build --all-targets --all-features --locked --timings` → `Finished \`dev\` profile [unoptimized + debuginfo] target(s) in 19.49s` (`Total units: 460`, `Fresh units: 457`, `Dirty units: 3`, `Max concurrency: 2 (jobs=2 ncpu=2)`)
  - `cargo tree --locked -e features --depth 1` → verified minimal feature activation (`image` with `default-features = false, features = ["png"]`)
  - `cargo tree --locked -d --depth 1` → identified duplicate crates in the 460-unit build graph (`syn 2.0.119` + `3.0.10`, `thiserror 1.0.69` + `2.0.21`, `x11rb 0.13.2` + `0.14.0`, `smithay-client-toolkit 0.19.2` + `0.20.0`, `calloop 0.13.0` + `0.14.5`, `rustix 0.38.44` + `1.1.6`, `hashbrown 0.15.5` + `0.16.1` + `0.17.1`, `miniz_oxide 0.8.9` + `0.9.1`, `quick-xml 0.41.0` + `0.42.0`, `phf 0.11.3` + `0.14.0`)
  - `grep -RIn 'cache\|sccache\|--locked\|all-features' .github/workflows` → confirmed `--locked` on all CI/release cargo commands (`ci.yml:38,69,72,87`, `sonarcloud.yml:38`, `release.yml:70,218`), `actions-rust-lang/setup-rust-toolchain` default caching on `ci.yml` and `sonarcloud.yml`, and `cache: false` on `release.yml:195`

Note: No C++, CMake, or precompiled headers (PCH) exist in this Cargo crate (`docs/prompts-review/compilation-speed-review.md:3,16`). Per prompt §1 (`docs/prompts-review/compilation-speed-review.md:40`), `cargo clean` was intentionally omitted to preserve the warm workspace cache; incremental timing data was measured directly via `cargo build --all-targets --all-features --locked --timings`.

### Timing Summary Table

| Metric | Measured Value | Evidence Source |
|---|---|---|
| Total compiled units (`--all-targets --all-features`) | `460` (`457` fresh, `3` dirty) | `target/cargo-timings/cargo-timing.html:184-190` |
| Host concurrency | `2 (jobs=2 ncpu=2)` | `target/cargo-timings/cargo-timing.html:193` |
| Incremental `cargo check --all-targets --all-features --locked` | `0.44s` | Direct CLI execution |
| Incremental `cargo clippy --all-targets --all-features --locked -- -D warnings` | `0.48s` | Direct CLI execution |
| Incremental `cargo build --all-targets --all-features --locked --timings` (leaf crate + link) | `19.49s` (`19.5s` in HTML report) | `target/cargo-timings/cargo-timing.html:199` |
| Unit 1: `cipherclipboard v0.1.7 CipherClipboard "bin"` | `17.7s` | `target/cargo-timings/cargo-timing.html:246-248` |
| Unit 2: `cipherclipboard v0.1.7 CipherClipboard "bin" (test)` | `7.9s` | `target/cargo-timings/cargo-timing.html:254-256` |
| Unit 3: `cipherclipboard v0.1.7 build-script (run)` | `0.0s` | `target/cargo-timings/cargo-timing.html:262-264` |
| Clean `cargo build --release --locked --timings` | Not run (`cargo clean` omitted per prompt §1) | `No timing data available.` |

## Findings

### Critical
No findings in this category.

### High
No findings in this category.

### Medium
No findings in this category.

### Low

#### L-1 — Duplicate direct vs. transitive `x11rb` versions (`0.13.2` and `0.14.0`) and transitive duplicate crates increase total unit count (`460` units)
- **Bottleneck**: Dependency graph duplicate crates (`Cargo.toml:53` and transitive GUI/Wayland/D-Bus dependencies).
- **Severity**: Low
- **Evidence**: `cargo tree --locked -d --depth 1` output:
  ```text
  x11rb v0.13.2
  ├── arboard v3.7.0
  ├── global-hotkey v0.10.0 (*)
  └── winit v0.30.13 (*)

  x11rb v0.14.0
  └── cipherclipboard v0.1.7 (/home/antigravity/BrunoO/clipboard)

  x11rb-protocol v0.13.2
  └── x11rb v0.13.2 (*)

  x11rb-protocol v0.14.0
  └── x11rb v0.14.0 (*)
  ```
  Alongside transitive duplicates (`syn v2.0.119` + `v3.0.10`, `thiserror v1.0.69` + `v2.0.21`, `smithay-client-toolkit v0.19.2` + `v0.20.0`, `calloop v0.13.0` + `v0.14.5`, `rustix v0.38.44` + `v1.1.6`, `hashbrown v0.15.5` + `v0.16.1` + `v0.17.1`, `miniz_oxide v0.8.9` + `v0.9.1`).
- **Measured cost**: Total graph comprises `460` units (`target/cargo-timings/cargo-timing.html:190`), of which `x11rb` and `x11rb-protocol` account for 2 extra direct-dependency units on Linux (`[Inferred]` minor clean-build overhead; 0s on warm incremental builds).
- **Impact**: Slightly higher clean compile time and dependency unit count on Linux CI runners.
- **Recommendation**: Evaluate aligning `Cargo.toml:53` (`x11rb = { version = "0.14", features = ["xtest"] }`) with `0.13` so `cipherclipboard` shares `x11rb v0.13.2` and `x11rb-protocol v0.13.2` with `arboard`, `global-hotkey`, and `winit`; verify unit count reduction with `cargo build --all-targets --all-features --locked --timings`.

#### L-2 — Dev and test profiles compile full debug symbols for all 457 dependencies and link two separate large binaries (`17.7s` + `7.9s` on a 2-vCPU host)
- **Bottleneck**: `[profile.dev]` / `[profile.test]` debuginfo generation and final linking of the `wgpu` + `syntect` + `bundled-sqlcipher` binary targets (`Cargo.toml:105-110`).
- **Severity**: Low
- **Evidence**: `Cargo.toml:105-110` defines only `[profile.release]`:
  ```toml
  [profile.release]
  opt-level = 3
  lto = "thin"
  codegen-units = 1
  panic = "abort"
  strip = true
  ```
  In `target/cargo-timings/cargo-timing.html:190-264`, with all `457` dependency units already fresh (`0.0s` build script), compiling and linking the two leaf targets (`CipherClipboard "bin"` at `17.7s` and `CipherClipboard "bin" (test)` at `7.9s`) under `--all-targets` took `19.49s` wall time on `2 (jobs=2 ncpu=2)`, whereas `cargo check --all-targets --all-features --locked` completed in `0.44s`.
- **Measured cost**: `19.49s` wall time (`17.7s` bin + `7.9s` test bin in parallel across 2 cores) vs `0.44s` for `cargo check`.
- **Impact**: Local `cargo test --all-targets` or `cargo build --all-targets` spends most of its incremental rebuild time linking full DWARF debuginfo from 457 dependency crates into both the main executable and the test executable.
- **Recommendation**: `[Inferred]` Consider adding `[profile.dev.package."*"] debug = "line-tables-only"` (or `debug = 0` for third-party dependencies while keeping full `debug = 2` on `cipherclipboard` itself) in `Cargo.toml` to shrink dependency `.rlib` object sizes and speed up incremental linking of `CipherClipboard` and its test harness. Measure before and after with `cargo build --all-targets --all-features --locked --timings`.

## Quick Wins
1. Use `cargo check --all-targets --all-features --locked` (`0.44s` measured) and `cargo clippy --all-targets --all-features --locked -- -D warnings` (`0.48s` measured) for rapid local edit-compile-check loops, and run `cargo test --bin CipherClipboard --locked` when iterating on unit tests to avoid linking the non-test binary target simultaneously.
2. `[Inferred]` Add `[profile.dev.package."*"] debug = "line-tables-only"` in `Cargo.toml` and measure the link-time reduction on `CipherClipboard "bin"` (`17.7s` baseline) and `CipherClipboard "bin" (test)` (`7.9s` baseline).
3. `[Inferred]` Unify `x11rb` to `0.13` in `Cargo.toml:53` to drop 2 duplicate units (`x11rb v0.14.0` and `x11rb-protocol v0.14.0`) on Linux.

## Recommended Actions
1. **Baseline & Re-measurement Plan**:
   - Baseline recorded on 2-core Linux host (`x86_64-unknown-linux-gnu`, `rustc 1.100.0`): `460` total units; `cargo check` = `0.44s`; `cargo clippy` = `0.48s`; incremental leaf build + link (`--all-targets`) = `19.49s` (`bin` `17.7s`, `bin (test)` `7.9s`, `build-script (run)` `0.0s`).
   - Before adopting `[profile.dev.package."*"] debug = "line-tables-only"`, touch `src/main.rs`, run `cargo build --all-targets --all-features --locked --timings`, and compare the `CipherClipboard "bin"` and `CipherClipboard "bin" (test)` unit durations in `target/cargo-timings/cargo-timing.html`.
2. **Keep Release Profile Unchanged**:
   - `Cargo.toml:105-110` (`opt-level = 3`, `lto = "thin"`, `codegen-units = 1`, `panic = "abort"`, `strip = true`) applies only to `[profile.release]` and does not slow down `dev` or `test` builds.
3. **Preserve `--locked` Across CI Workflows**:
   - All workflow commands (`.github/workflows/ci.yml:38,69,72,87`, `.github/workflows/sonarcloud.yml:38`, `.github/workflows/release.yml:70,218`) now pass `--locked`, ensuring stable cache keys for `actions-rust-lang/setup-rust-toolchain@a188e68d0fd2bd36ad51dccb2b07ba6e75105f30` (`v2`, which enables `Swatinem/rust-cache` by default in `ci.yml` and `sonarcloud.yml`).

## Coverage

- **1. Baseline Measurement**:
  - Measured incremental `cargo build --all-targets --all-features --locked --timings` (`19.49s` wall time, `460` total units, `457` fresh, `3` dirty, `CipherClipboard "bin"` = `17.7s`, `CipherClipboard "bin" (test)` = `7.9s`, `build-script (run)` = `0.0s`). Clean build (`cargo clean`) intentionally skipped per prompt §1 (`docs/prompts-review/compilation-speed-review.md:40`).
- **2. Profile Settings**:
  - Reviewed `Cargo.toml:105-110`. Release profile uses `lto = "thin"`, `codegen-units = 1`, `opt-level = 3`, `panic = "abort"`, and `strip = true`. Dev/test profiles use Cargo defaults (`incremental = true`, `debug = 2`). Opportunity to reduce dependency debug info for faster linking noted in **L-2**.
- **3. Dependency Graph**:
  - Reviewed `cargo tree --locked -e features --depth 1` and `cargo tree --locked -d --depth 1`.
  - `image` disables default features (`default-features = false, features = ["png"]` at `Cargo.toml:79`).
  - OS-specific crates (`windows-sys`, `tray-icon`, `objc`, `x11rb`) are properly gated under `[target.'cfg(target_os = "...")'.dependencies]` (`Cargo.toml:34-54`).
  - Duplicate `x11rb` versions reported in **L-1**.
- **4. Native and Build Script Costs**:
  - `build.rs:1-23` is a no-op on non-Windows targets (`0.0s` measured in `cargo-timing.html:264`) and makes no network calls or redundant code generation. No findings in this category.
- **5. Developer Loop**:
  - Measured `cargo check --all-targets --all-features --locked` (`0.44s`) and `cargo clippy --all-targets --all-features --locked -- -D warnings` (`0.48s`) vs full `--all-targets` binary linking (`19.49s`). Documented staged iteration in Quick Wins. No findings in this category.
- **6. CI Caching**:
  - `.github/workflows/ci.yml:30,63,84` and `.github/workflows/sonarcloud.yml:30` use `actions-rust-lang/setup-rust-toolchain@a188e68d0fd2bd36ad51dccb2b07ba6e75105f30` (v2, which caches Cargo registry/git/target by default), while `.github/workflows/release.yml:195` explicitly sets `cache: false` for clean release builds. All CI and release `cargo` commands now pass `--locked`, keeping cache keys deterministic. No findings in this category.
