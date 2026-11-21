# Review Prompt Adaptation Map: C++17 → Rust (2026)

**Purpose:** Record how the inherited C++17/Windows/ImGui/CMake review prompts were translated into a Rust reference frame, which reports changed, and which Rust-specific prompts were added.

**Status:** The prompts in `docs/prompts-review/` are now the Rust-adapted set. `master-orchestrator.md` is the entry point.

---

## 1. High-Level Translation Rules

| Inherited assumption | Rust replacement |
|---|---|
| C++17, MSVC/Clang/GCC, CMake, PCH, submodules | Cargo, `rustc` edition 2021/2024 policy, Cargo features/target triples, lockfile, build scripts |
| Raw pointers, `unique_ptr`/`shared_ptr`, RAII, `std::string_view`, `[[nodiscard]]`, `std::optional` | Ownership/borrowing, `Box`/`Rc`/`Arc`, slices, `#[must_use]`, `Option`/`Result`, typed errors |
| Clang-tidy static-analysis phase | `cargo fmt --check`, `cargo check`, `cargo clippy -D warnings`, compiler/lint configuration review |
| Header/source and class-layout mismatch scripts | Module/facade boundary review, `#[cfg]` consistency, target-triple checks |
| `cpp:Sxxxx` Sonar rules | Rust rules as emitted by Sonar (`rust:Sxxxx`); do not reuse C++ rule examples |
| Preprocessor `#ifdef` | `#[cfg(...)]`, Cargo features, target-gated dependencies |
| Windows USN Journal / file indexer context | Cross-platform encrypted clipboard manager: clipboard, keyring, SQLCipher, `eframe`, hotkeys, FFI |
| ImGui/DirectX UI review | `egui`/`eframe` immediate-mode review, `App::logic()` vs `App::ui()`, hidden-window lifecycle |
| Doxygen | rustdoc (`# Errors`, `# Panics`, `# Safety`) plus project docs |
| Build speed via PCH/CMake | Cargo incremental compilation, `--timings`, codegen units, LTO, feature graph, sccache |
| `find_class_struct_mismatches.py` and `git submodule status` requirements | `cargo metadata`, `cargo tree`, target checks, lockfile review; no scripts/submodules exist |

---

## 2. Prompt-by-Prompt Map

| # | Old prompt / phase | New prompt | New phase | Output file | Main changes |
|---|---|---|---|---|---|
| 1 | `tech-debt.md` / Code Quality | `tech-debt.md` | Code Quality | `docs/reviews/{DATE}/TECH_DEBT_REVIEW_{DATE}.md` | Rust debt: `allow` attributes, `clone` workarounds, `unwrap`, module cohesion, Cargo feature debt, target-gated duplication |
| 2 | `architecture-review.md` / Code Quality | `architecture-review.md` | Code Quality | `docs/reviews/{DATE}/ARCHITECTURE_REVIEW_{DATE}.md` | Crate/module boundaries, safe platform facade, `Arc<Mutex<_>>` boundaries, Cargo targets/features as architecture, Rust API guidelines |
| 3 | `security-review.md` / Reliability & Security | `security-review.md` | Security & Supply Chain | `docs/reviews/{DATE}/SECURITY_REVIEW_{DATE}.md` | Replaced C++ memory-safety framing with `unsafe`/FFI, zeroize, SQLCipher, keyring fallback, clipboard markers, URL/image validation, CI secrets |
| 4 | `error-handling-review.md` / Reliability & Security | `error-handling-review.md` | Rust Language Safety | `docs/reviews/{DATE}/ERROR_HANDLING_REVIEW_{DATE}.md` | `Result`/`thiserror`/`anyhow`, context chains, no panic-driven flow, `panic = "abort"`, poisoned locks, FFI status conversion |
| 5 | `performance-review.md` / Performance & Testing | `performance-review.md` | Performance & Testing | `docs/reviews/{DATE}/PERFORMANCE_REVIEW_{DATE}.md` | Release-build profiling, criterion/iai-callgrind/flamegraph/clones/allocations, clipboard polling, `eframe` frame work |
| 6 | `test-strategy-audit.md` / Performance & Testing | `test-strategy-audit.md` | Performance & Testing | `docs/reviews/{DATE}/TEST_STRATEGY_REVIEW_{DATE}.md` | `cargo test`, nextest, doc tests, proptest, cargo-fuzz, Miri, llvm-cov, platform CI/smoke tests |
| 7 | `documentation-maintenance.md` / Documentation | `documentation-maintenance.md` | Documentation & UX | `docs/reviews/{DATE}/DOCUMENTATION_REVIEW_{DATE}.md` | rustdoc sections, doc tests, Cargo/README/AGENTS/lessons synchronization, correct `docs/reviews/{DATE}/` paths |
| 8 | `ux-review.md` / User Experience | `ux-review.md` | Documentation & UX | `docs/reviews/{DATE}/UX_REVIEW_{DATE}.md` | `egui`/`eframe` immediate-mode UX, hidden-window `logic()`, tray/hotkey feedback, keyring fallback visibility, accessibility |
| 9 | `feature-exploration-product-owner.md` / Product Strategy | `feature-exploration-product-owner.md` | Product Strategy | `docs/reviews/{DATE}/FEATURE_EXPLORATION_REVIEW_{DATE}.md` | Same product lens, but feasibility/risk framed around Rust dependency cost, cross-platform FFI, secret handling, and binary/packaging impact |
| 10 | **Missing** `2026-02-14_AGENT_INSTRUCTIONS_UPDATE_CLANG_TIDY_STATUS.md` / Static Analysis | **New** `rust-compiler-clippy-review.md` | Static Analysis | `docs/reviews/{DATE}/RUST_STATIC_ANALYSIS_REVIEW_{DATE}.md` | Replaces nonexistent clang-tidy prompt; audits rustfmt/rustc/clippy invocation, lint configuration, edition/MSRV, and `#[allow]` justifications |
| 11 | `sonar-check.md` / Static Analysis | `sonar-check.md` | Static Analysis | `docs/reviews/{DATE}/SONAR_REVIEW_{DATE}.md` | Rust project context, `rust:Sxxxx` rules, Rust/clippy coverage gaps, FFI/secret/eframe project rules; token behavior preserved |
| 12 | `platform-compat-review.md` / Platform Compatibility | `platform-compat-review.md` | Platform Compatibility | `docs/reviews/{DATE}/PLATFORM_COMPAT_REVIEW_{DATE}.md` | Replaced `#ifdef`/Windows USN checks with `#[cfg]`, target triples, safe facade, macOS/Windows/Linux invariants, layout/locale/path assumptions |
| 13 | `cmake-build-health.md` / Build System Health | **Renamed logically to** `cargo-build-health.md` | Build & Toolchain | `docs/reviews/{DATE}/CARGO_BUILD_HEALTH_REVIEW_{DATE}.md` | Cargo manifest, lockfile, features, profiles, build-dependencies, bundle metadata, native deps; removes CMake/submodule/PCH checks |
| 14 | `compilation-speed-review.md` / Compilation Speed | `compilation-speed-review.md` | Build & Toolchain | `docs/reviews/{DATE}/COMPILATION_SPEED_REVIEW_{DATE}.md` | Cargo `--timings`, incremental, LTO/codegen-units, feature graph, proc macros/build scripts, sccache; removes PCH/CMake framing |
| 15 | **New** dependency supply-chain prompt | `cargo-dependency-supply-chain-review.md` | Security & Supply Chain | `docs/reviews/{DATE}/CARGO_SUPPLY_CHAIN_REVIEW_{DATE}.md` | `Cargo.lock`, cargo-deny/audit, duplicate versions, licenses, wildcards/git sources, build scripts, release dependency verification |
| 16 | **New** unsafe/FFI prompt | `unsafe-ffi-review.md` | Rust Language Safety | `docs/reviews/{DATE}/UNSAFE_FFI_REVIEW_{DATE}.md` | Crate-wide deny, SAFETY comments, raw pointers, checked casts, repr/align, AppKit/Win32/X11 invariants, Miri limits |

`cmake-build-health.md` remains only as a deprecation stub pointing to `cargo-build-health.md`. `TaskmasterPrompt.md` is a generic task-decomposition template and `rx_syntax_ai_prompt.md` documents the unrelated FindHelper `rx:` dialect. Neither is referenced by the orchestrator and neither was rewritten.

---

## 3. Phase and Report Changes

- Old orchestrator: **14 steps across Phases 1–11**.
- New orchestrator: **16 steps across Phases 1–9** (phases were regrouped so related Rust concerns sit together).
- All reports and the comprehensive summary are written to `docs/reviews/{DATE}/` instead of the flat `docs/` root.
- New report rows in `COMPREHENSIVE_REVIEW_SUMMARY_{DATE}.md`:
  - Unsafe & FFI
  - Cargo Supply Chain
  - Rust Static Analysis (replaces Clang-Tidy)
  - Cargo Build Health (replaces CMake Build Health)
- Fixed legacy post-review requirements removed:
  - `find_class_struct_mismatches.py` output in platform review.
  - `git submodule status` output in build health review.
  - Any reference to the missing clang-tidy status file.

---

## 4. New Supporting Documents

- `RUST_REVIEW_CONSIDERATIONS.md` — the 2026 Rust professional-review checklist and source list.
- `RUST_PROMPT_ADAPTATION_MAP.md` — this file.

---

## 5. Usage

To run the adapted full review:

```text
Run a comprehensive review using docs/prompts-review/master-orchestrator.md.
```

To run only Rust-specific areas:

```text
Run the Rust Language Safety, Security & Supply Chain, and Static Analysis phases from docs/prompts-review/master-orchestrator.md.
```
