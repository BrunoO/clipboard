# Cargo Build Health Review Prompt

**Purpose:** Replace the CMake build-health phase with a Cargo-native audit of manifest, lockfile, features, profiles, native dependencies, packaging, and reproducible builds.

---

## Prompt

````markdown
You are a Rust build systems engineer. Review Cargo build health and packaging for this crate. Follow the master orchestrator's Global Evidence Rules. Do not search for CMakeLists.txt, submodules, PCH, or clang-tidy files except to note they are not applicable.

## Context

- Package: `cipherclipboard`, binary `CipherClipboard`, edition 2021.
- Bundle metadata for macOS; Windows `winres` build-dependency; release workflow packages on Windows.
- Native dependencies: bundled SQLCipher via `rusqlite`, X11/DBus on Linux, OpenSSL on Windows CI.
- `deny.toml` lists four release target triples.

## Grounding Rules

1. Every finding needs `path:line`, manifest/lockfile snippet, or verbatim command output.
2. Do not claim a build passes unless you ran it; otherwise write `Skipped`.
3. Empty categories get `No findings in this category.`
4. Do not report absence of CMake/submodules as a defect.

## Review Process

### 1. Manifest correctness
- Package name/version/description/license/repository/readme/default-run match README and workflows.
- `[[bin]]` path exists and binary name matches release artifacts.
- Version in `package.metadata.bundle` matches `[package].version`.
- macOS plist keys match actual keyring usage description.
- Icons/resources referenced by bundle metadata exist.
- Lint configuration parses and is not contradicted by local source attributes.

### 2. Dependency Graph and Features
- `[dependencies]`, `[dev-dependencies]`, `[build-dependencies]`, and target-specific sections are correctly scoped.
- No runtime dependency declared as build-dependency or vice versa.
- Features are additive and do not accidentally enable heavyweight/native components by default.
- `cargo tree -e features` output reviewed for surprising default features.

### 3. Lockfile and Reproducibility
- `Cargo.lock` exists and is tracked.
- Release workflow uses `cargo build --release --locked`.
- `cargo check --locked` / `cargo build --locked` succeed locally.
- Lockfile diff reviewed for unintended changes if a dependency PR is in scope.

### 4. Build Scripts and Native Toolchains
- Identify `build.rs` files and build-dependencies (`winres`).
- Document native prerequisites: SQLCipher bundled build, Linux X11/XTest/DBus, Windows OpenSSL.
- Verify CI installs prerequisites on each OS.
- Check that generated resources (Windows icon/version info, macOS plist) are present in built artifacts.

### 5. Profiles and Artifacts
- Review `[profile.release]`: `opt-level`, `lto`, `codegen-units`, `panic`, `strip`.
- Check debug profile and debug assertions used only where intended.
- Verify release artifact names/paths in workflows match binary target.
- Check bundle identifier, icon, version, category, and short description.

### 6. Cross-Target Build Matrix
- Target triples in `deny.toml`, CI workflows, and release workflows are consistent.
- Run target checks if toolchains are installed; otherwise cite workflow coverage.
- Confirm no target-gated dependency is missing from CI install steps.

### 7. Quality Gates in CI
- `cargo fmt -- --check`,
- `cargo clippy --all-targets --all-features -- -D warnings`,
- `cargo test --all-targets`,
- `cargo deny check`,
- Sonar gate behavior.
- GitHub Actions pinned to full SHAs and least-privilege permissions.

## Commands (run if available; paste exact output)

```bash
cargo metadata --no-deps --format-version 1
cargo tree --locked -e features
cargo check --all-targets --all-features --locked
cargo build --release --locked
cargo deny check
find . -name build.rs -not -path './target/*' -print
grep -RIn 'cargo build\|cargo clippy\|cargo test\|cargo deny' .github/workflows
```

## Finding Format

1. **Build control** — manifest field, workflow step, or artifact.
2. **Severity** — Critical / High / Medium / Low.
3. **Evidence** — `path:line` or verbatim command output.
4. **Failure mode** — non-reproducible build, wrong artifact, missing native dep, metadata drift.
5. **Impact** — CI break, release failure, wrong bundle metadata, or unreproducible binary.
6. **Recommendation** — exact config/workflow change and verification command.

## Output Sections

- **Destination:** `docs/reviews/{DATE}/CARGO_BUILD_HEALTH_REVIEW_{DATE}.md`.

- Build configuration summary.
- Command results table.
- Findings by severity.
- Quick Wins.
- Recommended Actions.
- Coverage checklist with `No findings in this category.` where empty.
````

---

## Usage

Use as step 15 of the full review, in Build & Platform Focus, and before release/toolchain upgrades.
