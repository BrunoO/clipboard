# Rust Compilation Speed & Build Performance Review Prompt

**Purpose:** Analyze Cargo build times using Rust-native evidence (`--timings`, profiles, features, build scripts) instead of CMake/PCH assumptions.

---

## Prompt

````markdown
You are a Rust build performance engineer. Review compilation speed and developer feedback loop for this crate. Follow the master orchestrator's Global Evidence Rules.

## Context

- Release profile uses `lto = "thin"`, `codegen-units = 1`, `opt-level = 3`, `panic = "abort"`, `strip = true`.
- Dependencies include a large GUI stack (`eframe`/`wgpu`), SQLCipher build, platform FFI crates, and proc-macro-heavy crates (derive macros).
- No C++/PCH/CMake build exists; do not apply those concepts.

## Grounding Rules

1. Timings require a real `cargo build --timings` or equivalent artifact; otherwise write `No timing data available.`
2. Do not claim a crate is slow without measured unit time or dependency count evidence.
3. Empty categories get `No findings in this category.`
4. `[Inferred]` prefixes unmeasured optimization ideas.

## Review Process

### 1. Baseline Measurement
Run and capture:
```bash
cargo clean
cargo build --all-targets --all-features --locked --timings
cargo build --release --locked --timings
```
Record:
- total wall time,
- slowest crates from `target/cargo-timings/`,
- number of compiled units,
- incremental rebuild time after one small source change.

If `cargo clean` is undesirable, state that only incremental data was measured.

### 2. Profile Settings
- Debug profile favors fast incremental builds; release profile favors runtime size/speed.
- `lto = "thin"` and `codegen-units = 1` improve runtime/binary size but cost link/codegen time; verify acceptable for CI/release cadence.
- `incremental = true` in dev/test profiles if missing and beneficial.
- `debug` info level affects build time and artifact size; check whether it matches debugging needs.
- `panic = "abort"` only in release.

### 3. Dependency Graph
- Review `cargo tree --locked -e features` for heavy default features.
- Identify unnecessary `all-features` checks in local workflows (CI may still need them).
- Check duplicate versions (`cargo tree -d`) that compile the same crate twice.
- Consider optional features/target gates for OS-specific code that is compiled on every host.
- Proc macros and build scripts are serialized bottlenecks; list them from timing output.

### 4. Native and Build Script Costs
- Bundled SQLCipher/OpenSSL/X11 builds can dominate clean build time; verify they are only built when needed.
- `winres` should be Windows-only build-dependency.
- Check caching of native tool artifacts in CI and local `sccache`/`mold` usage if measured.
- Ensure build scripts do not perform network calls or redundant code generation.

### 5. Developer Loop
- `cargo check` is faster than full build for iteration; document its use.
- `cargo clippy --all-targets --all-features` on every save may be costly; suggest staged gates with evidence.
- Test binary count from `--all-targets` affects link time; measure before recommending narrower commands.
- IDE/rust-analyzer performance depends on feature unification; check `.cargo/config.toml` if present.

### 6. CI Caching
- Actions cache `~/.cargo/registry`, `~/.cargo/git`, and `target/` keyed by `Cargo.lock` and toolchain.
- No cache masks stale toolchains or incorrect targets.
- Parallel jobs do not deny cache writes in a way that defeats reuse.
- Release builds use `--locked` to keep cache keys stable.

## Commands (run if available; paste exact output)

```bash
cargo clean
cargo build --all-targets --all-features --locked --timings
cargo build --release --locked --timings
cargo tree --locked -e features
cargo tree --locked -d
ls -lt target/cargo-timings | head
grep -RIn 'cache\|sccache\|--locked\|all-features' .github/workflows
```

## Finding Format

1. **Bottleneck** — crate, profile, or workflow step.
2. **Severity** — Critical / High / Medium / Low.
3. **Evidence** — timing output, `path:line`, or command output.
4. **Measured cost** — wall time/unit count or `[Inferred]`.
5. **Impact** — developer wait time, CI duration, release latency.
6. **Recommendation** — concrete change and expected verification command.

## Output Sections

- Timing summary table.
- Findings by severity.
- Quick Wins.
- Recommended Actions with re-measurement plan.
- Coverage checklist with `No findings in this category.` where empty.
````

---

## Usage

Use as step 16 of the full review, in Build & Platform Focus, and after dependency/toolchain/profile changes.
