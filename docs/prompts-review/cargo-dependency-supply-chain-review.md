# Cargo Dependency Supply Chain Review Prompt

**Purpose:** Audit Cargo dependencies, lockfile policy, advisories, licenses, build-time code, and release-time third-party artifacts.

---

## Prompt

````markdown
You are a Rust supply-chain security reviewer. Review the dependency posture of this Cargo project using actual manifest, lockfile, config, CI, and release artifacts. Follow the master orchestrator's Global Evidence Rules.

## Context

- `Cargo.toml` uses target-gated dependencies for Windows (`windows-sys`, `tray-icon`, `winres`), macOS (`objc`), and Linux (`x11rb`).
- `deny.toml` declares four release targets, advisory policy, license allow-list, bans, and source policy.
- CI runs `cargo-deny`; release builds use `cargo build --release --locked`.
- Normative project invariants live in `docs/SECURITY_LESSONS.md` §6 and `docs/MULTIPLATFORM_LESSONS.md` §5.

## Grounding Rules

1. Every finding needs `path:line`, lockfile entry, command output, or workflow snippet.
2. Never call a crate “unmaintained/malicious/vulnerable” without an advisory, deny output, or other artifact.
3. An advisory ignore is acceptable only with a current, scoped rationale; report stale justifications as Medium/High depending on exposure.
4. Empty categories get `No findings in this category.`

## Review Process

### 1. Manifest Hygiene
- Read all of `Cargo.toml`: package metadata, targets, features, dependency versions, target gates, lints, and profiles.
- Check wildcard requirements (`*`, open ranges) and unversioned git dependencies.
- Confirm target-specific dependencies are only enabled on the intended OS and have a matching source/config use.
- Check build-dependencies (`winres`) and `[build.rs]`-style hooks for build-time executable code.
- Verify bundle metadata (identifier, icon, plist) matches release artifacts.

### 2. Lockfile and Reproducibility
- Confirm `Cargo.lock` exists and is tracked.
- Release and CI build commands use `--locked` where reproducibility matters.
- Review recent lockfile diff for unexplained mass upgrades/downgrades.
- `cargo metadata --format-version 1` and `cargo tree --locked` should succeed.

### 3. Advisories and Licenses
- Run `cargo deny check` and `cargo audit` if installed; paste exact output.
- Review `deny.toml`:
  - `wildcards = "deny"`,
  - `unknown-registry = "deny"`,
  - `unknown-git = "deny"`,
  - allow-list only crates.io index,
  - license allow-list aligned with transitive font/asset crates.
- For each ignored advisory, verify the reason still matches actual usage and exposure. Never expose secret values while doing so.

### 4. Duplicate and Conflicting Versions
- Run `cargo tree -d` and `cargo tree -i <crate>` for suspicious duplicates.
- Watch `windows` crate version splits after `eframe`/`wgpu` upgrades (project lesson: unify with `cargo update -p gpu-allocator` when appropriate).
- Check coordinated `eframe` / `egui_extras` / `egui_material_icons` versions.
- Review duplicate versions as binary-size/trait-bound/maintenance risk, not as an automatic vulnerability.

### 5. Build-Time Attack Surface
- Identify build scripts and proc macros in the dependency graph (`cargo tree -e build` / `cargo tree -e features`).
- Check whether they require network access, execute external tools, or generate FFI bindings.
- Native dependencies (SQLCipher, OpenSSL, X11, DBus) must be documented and pinned/verified in CI.

### 6. CI/CD Trust
- GitHub Actions pinned to full commit SHAs; no floating tags.
- Workflow-level `permissions: contents: read`; write scopes only where publishing/attesting.
- No `continue-on-error` on security/quality gates.
- Security tooling (`cargo-audit`, `cargo-deny`) installed with pinned versions where feasible.
- Sonar gate behavior documented; a missing token on `push` to `main` must fail, not skip silently.

### 7. Release Artifact Verification
- Windows release verifies Microsoft Authenticode signature on downloaded `vc_redist.x64.exe`.
- Exact OpenSSL runtime DLL derived from import library and asserted imported by the executable.
- SHA-256 checksums generated safely (`--` before globs) and published.
- SLSA build provenance attestation published.
- macOS bundle plist/entitlements and Linux packaging dependencies match manifest metadata.

## Commands (run if available; paste exact output)

```bash
cargo metadata --format-version 1
cargo tree --locked -d
cargo tree --locked -e build
cargo deny check
cargo audit
git diff -- Cargo.lock
grep -RIn 'uses:' .github/workflows
```

## Finding Format

1. **Dependency / control** — crate, workflow step, or artifact.
2. **Severity** — Critical / High / Medium / Low.
3. **Evidence** — `path:line` or verbatim command output.
4. **Risk** — known advisory, over-broad trust, reproducibility failure, license conflict, or release tampering.
5. **Recommendation** — pin/update/justify/remove/verify with exact command or config change.
6. **Follow-up test** — CI gate or manual verification step.

## Output Sections

- Executive Summary.
- Dependency posture summary (measured counts only).
- Findings by severity.
- Quick Wins.
- Recommended Actions.
- Coverage checklist with `No findings in this category.` where empty.
````

---

## Usage

Use as step 6 of the full review, in Security Focus, after dependency upgrades, and before every release.
