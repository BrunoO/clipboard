# Compilation Speed Review - 2026-11-21

## Executive Summary

- **Health Score**: N/A — **No timing data available.** A numeric X/10 would be invented: no `cargo build --timings` artifact, wall-time measurement, unit count, or incremental rebuild measurement exists in the evidence, and `cargo` commands are being gathered by the parent for the separate static-analysis report.
- **Critical Issues**: 0
- **High Issues**: 0
- **Total Findings**: 0
- **Estimated Remediation Effort**: Not estimated — no measured bottleneck or finding to remediate; all suggestions below are explicitly `[Inferred]`.
- **Evidence Commands Run**:
  - `ls -la target/cargo-timings 2>/dev/null || echo "no target/cargo-timings directory"` → `no target/cargo-timings directory`
  - `grep -RIn 'cache\|sccache\|--locked\|all-features' .github/workflows` → `release.yml:70` and `:218` `cargo build --release --locked`, `release.yml:195` `cache: false`, `sonarcloud.yml:41`/`:43` Sonar-only cache path, `ci.yml:38`/`:69` `--all-targets --all-features`; no `sccache` or `target/` cargo cache step was printed.
  - Direct reads of `Cargo.toml`, `build.rs`, `.github/workflows/ci.yml`, `.github/workflows/release.yml`, `.github/workflows/sonarcloud.yml`, and `Cargo.lock` ranges for the configuration observations below.
  - **Not run**: `cargo clean`, `cargo build --timings`, `cargo build --release --timings`, `cargo tree`, or any other command that could yield a time, unit count, or dependency-count metric.

## Findings

**No timing data available.** Consequently no bottleneck is asserted at any severity, and no wall time, unit count, or slowest-crate ranking is reported.

### Critical

No findings in this category.

### High

No findings in this category.

### Medium

No findings in this category.

### Low

No findings in this category.

## Quick Wins

All items below are `[Inferred]` process improvements, not measured findings; none claims a speedup.

1. **[Inferred] Capture a baseline before changing anything**: in the separate evidence pass, run `cargo clean`, then `cargo build --all-targets --all-features --locked --timings` and `cargo build --release --locked --timings`, and retain `target/cargo-timings/`. No times are predicted here.
2. **[Inferred] Use `cargo check` for local iteration** on the large GUI dependency stack (`Cargo.toml:53-55`), reserving `--all-targets --all-features` for CI (`ci.yml:38`, `:69`).
3. **[Inferred] Add CI caching for `~/.cargo/registry`, `~/.cargo/git`, and `target/` keyed by `Cargo.lock` and the toolchain** — the current grep output shows only `sonarcloud.yml:41-45` caching `~/.sonar/cache`, and `release.yml:195` sets `cache: false`. Cache effectiveness is unmeasured.
4. **[Inferred] Keep `--locked` on release builds** (`release.yml:70`, `:218`) and extend it to CI/coverage commands so cache keys stay stable; this is also the subject of `./CARGO_BUILD_HEALTH_REVIEW_2026-11-21.md` M-1 and `./SONAR_REVIEW_2026-11-21.md` H-1.

## Recommended Actions

Re-measurement plan — every step is `[Inferred]` and must be validated with a timing artifact before any change is judged beneficial:

1. **Step 1 — baseline (no changes)**: record total wall time, slowest crates from `target/cargo-timings/`, the number of compiled units, and an incremental rebuild time after one small source edit. Current state: `no target/cargo-timings directory`.
2. **Step 2 — profile review**: the release profile at `Cargo.toml:101-107` sets `opt-level = 3`, `lto = "thin"`, `codegen-units = 1`, `panic = "abort"`, and `strip = true`. [Inferred] thin LTO plus a single codegen unit can increase link/codegen cost; there is no [profile.dev] block in `Cargo.toml`, so [Inferred] Cargo defaults apply to dev/test. Do not change these settings without a before/after timing pair.
3. **Step 3 — dependency graph**: [Inferred] candidate cost centers are the `wgpu`-enabled GUI stack (`Cargo.toml:53-55`), bundled SQLCipher (`Cargo.toml:64`), the Linux `x11rb` target dependency (`Cargo.toml:49-50`), and the Windows-only `winres` build dependency (`Cargo.toml:28-29`); only target-gated crates should be built on a given host. `Cargo.lock:5414`, `:5423`, `:5432` show three `windows-sys` versions, which [Inferred] could add redundant compilation; `cargo tree -d` was not run for this report.
4. **Step 4 — native/build-script costs**: [Inferred] `build.rs:2-22` runs Windows resource compilation only when `CARGO_CFG_TARGET_OS == "windows"`, and SQLCipher/X11/OpenSSL prerequisites are installed in CI (`ci.yml:24-27`, `:51-60`, `release.yml:187-190`). No build-script duration was measured, so no claim is made that these dominate.
5. **Step 5 — developer loop**: [Inferred] consider staged gates (`cargo check` on save, full `cargo clippy --all-targets --all-features` in CI) and an `sccache`/`mold` evaluation only after a baseline exists. No IDE/rust-analyzer or `.cargo/config.toml` measurement was taken; no `.cargo` directory was observed in the workspace listing.
6. **Step 6 — re-measure and archive**: repeat the Step 1 commands after any adjustment and keep both timing directories; report deltas only from those artifacts, never from estimation.

## Coverage

**Timing summary table**:

| Metric | Value |
|---|---|
| Total wall time (clean build) | No timing data available. |
| Slowest crates | No timing data available. |
| Compiled unit count | No timing data available. |
| Incremental rebuild time | No timing data available. |
| `target/cargo-timings/` artifacts | `no target/cargo-timings directory` (verbatim command output above) |
| Build/clippy/test results | Not run for this report; no result claimed |

**Prompt category checklist**:

1. **Baseline Measurement**: No timing data available. (`cargo clean` and both timed builds were deliberately not run here.)
2. **Profile Settings**: No findings in this category. Configuration observed at `Cargo.toml:101-107`; no measured cost attributed to it.
3. **Dependency Graph**: No findings in this category. `cargo tree --locked -e features` and `cargo tree --locked -d` not run; `Cargo.lock` entries were read for target-gating context only.
4. **Native and Build Script Costs**: No findings in this category. `build.rs:1-24` and CI prerequisite steps read; no duration measured.
5. **Developer Loop**: No findings in this category. `[Inferred]` suggestions are in Recommended Actions.
6. **CI Caching**: No findings in this category. The grep output shows no `~/.cargo`/`target/` cache step, but without timing data this is recorded as an `[Inferred]` opportunity, not a measured bottleneck.
