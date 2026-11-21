# Comprehensive Rust Code Review Summary - 2026-11-21

## Overall Health

| Review Area | Score | Critical | High | Medium | Low |
|-------------|-------|----------|------|--------|-----|
| Tech Debt | 8/10 | 0 | 0 | 4 | 3 |
| Architecture | 8/10 | 0 | 0 | 3 | 1 |
| Unsafe & FFI | 7/10 | 0 | 1 | 4 | 1 |
| Error Handling | 7/10 | 0 | 2 | 4 | 3 |
| Security | 8/10 | 0 | 0 | 2 | 2 |
| Cargo Supply Chain | 8/10 | 0 | 0 | 2 | 2 |
| Performance | 8/10 | 0 | 0 | 2 | 2 |
| Testing | 7/10 | 0 | 0 | 2 | 2 |
| Documentation | 7/10 | 0 | 0 | 4 | 2 |
| User Experience | 7/10 | 0 | 1 | 2 | 1 |
| Product Strategy | 7/10 | 0 | 2 | 4 | 1 |
| Rust Static Analysis | 9/10 | 0 | 0 | 1 | 1 |
| SonarQube | 8/10 | 0 | 1 | 0 | 0 |
| Platform Compatibility | 7/10 | 0 | 0 | 2 | 3 |
| Cargo Build Health | 8/10 | 0 | 0 | 1 | 1 |
| Compilation Speed | N/A | 0 | 0 | 0 | 0 |
| **Overall** | **7.6/10** | **0** | **7** | **37** | **25** |

**Note:** Overall score is the mean of the 15 scored areas; Compilation Speed is intentionally `N/A` because **no timing data was available** and reporting a score would be invented. There are **no Critical issues**. Locally runnable quality gates all passed: `cargo fmt -- --check`, `cargo check --all-targets --all-features --locked`, `cargo clippy --all-targets --all-features --locked -- -D warnings`, `cargo test --all-targets --locked` (74 passed), and `cargo doc --no-deps --all-features --locked`. SonarQube returned exactly one open issue. Cross-target compile checks for `x86_64-apple-darwin`, `x86_64-unknown-linux-gnu`, and `x86_64-pc-windows-msvc` were skipped because those targets are not installed locally; CI coverage is cited instead. Tools not installed (`cargo-deny`, `cargo-audit`, `cargo-nextest`, `cargo-llvm-cov`, `cargo-flamegraph`) are marked skipped, not clean.

## Top Priority Items

1. **H-1 / M-1 / Supply M1 — CI builds resolve dependencies without `--locked`** (`.github/workflows/ci.yml:87`, also `:38`, `:69`, `:72`, `.github/workflows/sonarcloud.yml:38`). This is the single SonarQube MAJOR `VULNERABILITY` (`githubactions:S8549`, 1h effort) and is repeated across supply-chain/build-health. One-line fix closes the only externally reported security issue.
2. **UF-1 / Security M1 — No `// SAFETY:` annotations on FFI unsafe blocks** (`src/platform/macos.rs`, `src/platform/windows.rs`). This violates the normative invariant in `docs/SECURITY_LESSONS.md:59` and weakens auditability of the highest-risk code in the crate.
3. **EH-1 + U1 — Destructive clear/delete can report success while persistence fails** (`src/db.rs` VACUUM/WAL path, `src/app/mod.rs:1286`, `src/app/mod.rs:737`, `src/app/history_view.rs`). This is the highest security/privacy-value error-handling fix: the UI confirms removal that may not have happened.
4. **EH-2 — Settings persistence failures silently discarded at ten UI call sites** (`src/app/settings_view.rs:227,338,347,359,371,477,493,534`; `src/app/mod.rs:1307,1360,1370`). Silent loss of user settings erodes trust and makes failures undiagnosable.
5. **F1 / F2 — Proposed "Paste Next" feature requires lossless hotkey delivery and a non-coalescing paste worker** (`paste_stack_spec.md:57-89`, `src/main.rs:146-148`, `src/hotkey.rs:122-125`, `src/platform/mod.rs:261-285`). Reusing the current `sync_channel(1)` + `try_send` path would drop rapid presses; the product promise fails without this prerequisite.
6. **Security M2 — Windows fallback-key ACL hardening is silently skipped** (`src/crypto.rs:400-431`). When `USERNAME` is empty or `icacls.exe` fails, the degraded ACL state is only logged; it should be visible in `KeyStorageMode`/UI.
7. **UF-2/3/5/6 — FFI correctness hardening** (`src/platform/macos.rs:271`, `:423-424`; `src/platform/windows.rs:610`). Unchecked `as i32` cast, manual nul-terminated byte strings, mutable casts of immutable DPAPI input, and missing `ret_len` bounds checks are localized but material UB/robustness risks.
8. **Security L1 / Test M1 — Clipboard-preview and fixture hygiene** (`src/monitor.rs:236-248`; secret-shaped literals in `src/db.rs`, `src/crypto.rs`, `src/app/mod.rs`). Debug builds still print heuristic-redacted previews; tests embed secret-shaped static values.
9. **D1–D4 — Documentation drift and missing rustdoc contracts** (`src/crypto.rs` DPAPI/key functions, `src/platform/macos.rs` `target_keyboard_layout`, `README.md`, `docs/BUG_ANALYSIS.md`). No `# Errors`/`# Safety` sections were found; README references stale paths and egui 0.36 while `Cargo.lock` pins 0.37.0.
10. **Static M1 / TD-4 / Platform M-1 — Toolchain and lint-configuration hygiene** (`Cargo.toml`, `src/platform/mod.rs:1`, `src/app/mod.rs:1574`). No `rust-version` or toolchain pin is declared; `cfg(windows)` is mixed with `cfg(target_os = "windows")`; broad clippy allows lack recorded reasons.

### Quick Wins (≤2 hours, deduplicated across reports)

1. Add `--locked` to the CI build/test commands, starting with `.github/workflows/ci.yml:87` (`cargo build` → `cargo build --locked`); ~15 minutes and closes the Sonar issue.
2. Change `Cargo.toml:28` from `cfg(windows)` to `cfg(target_os = "windows")`; ~30 minutes, no source impact.
3. Add `rust-version` to `Cargo.toml` and document the toolchain policy; ~30 minutes.
4. Add `// SAFETY:` comments to the existing FFI `unsafe` blocks; ~1 hour of documentation with high audit value.
5. Replace `b"...\0"` literals at `src/platform/macos.rs:423-424` with `c"..."` literals and use `i32::try_from` at `:271`; ~45 minutes.
6. Narrow or annotate module-wide clippy allows at `src/platform/mod.rs:1` and `src/app/mod.rs:1574`; ~1 hour.
7. Replace debug clipboard previews with metadata-only output at `src/monitor.rs:236-248`; ~30 minutes.
8. Fix stale README paths/egui version and add key-loss guidance; ~45 minutes.
9. Add `--locked` to remaining CI/coverage cargo commands and document skipped property/fuzz/Miri tooling; ~30 minutes.
10. Make failure toasts severity-colored and branch the first-run keyring modal on `key_storage_mode`; ~2 hours.

## Generated Reports

- [Tech Debt Review](./TECH_DEBT_REVIEW_2026-11-21.md)
- [Architecture Review](./ARCHITECTURE_REVIEW_2026-11-21.md)
- [Unsafe & FFI Review](./UNSAFE_FFI_REVIEW_2026-11-21.md)
- [Error Handling Review](./ERROR_HANDLING_REVIEW_2026-11-21.md)
- [Security Review](./SECURITY_REVIEW_2026-11-21.md)
- [Cargo Supply Chain Review](./CARGO_SUPPLY_CHAIN_REVIEW_2026-11-21.md)
- [Performance Review](./PERFORMANCE_REVIEW_2026-11-21.md)
- [Test Strategy Review](./TEST_STRATEGY_REVIEW_2026-11-21.md)
- [Documentation Review](./DOCUMENTATION_REVIEW_2026-11-21.md)
- [UX Review](./UX_REVIEW_2026-11-21.md)
- [Feature Exploration Review](./FEATURE_EXPLORATION_REVIEW_2026-11-21.md)
- [Rust Static Analysis Review](./RUST_STATIC_ANALYSIS_REVIEW_2026-11-21.md)
- [SonarQube Review](./SONAR_REVIEW_2026-11-21.md)
- [Platform Compatibility Review](./PLATFORM_COMPAT_REVIEW_2026-11-21.md)
- [Cargo Build Health Review](./CARGO_BUILD_HEALTH_REVIEW_2026-11-21.md)
- [Compilation Speed Review](./COMPILATION_SPEED_REVIEW_2026-11-21.md)

## Next Review Date

Recommended: 2026-12-21
