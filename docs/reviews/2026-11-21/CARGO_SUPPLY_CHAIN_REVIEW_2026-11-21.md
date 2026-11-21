# Cargo Supply Chain Review - 2026-11-21

## Executive Summary
- **Health Score**: 8/10 — manifests, lockfile, advisory/license policy, action pinning, and release artifact verification are largely in place; gaps are reproducibility of non-release builds and a stale advisory rationale.
- **Critical Issues**: 0
- **High Issues**: 0
- **Total Findings**: 4 (2 Medium, 2 Low)
- **Estimated Remediation Effort**: 5 hours
- **Dependency posture summary (measured counts only)**: `Cargo.lock` is git-tracked (`git ls-files Cargo.lock deny.toml .github/workflows` output includes `Cargo.lock`), 6008 lines, 574 `[[package]]` entries; `deny.toml:4-9` declares 4 graph target triples; `deny.toml:13-16` has 1 advisory ignore; `Cargo.toml:84-85` has one dev-dependency (`tempfile`).
- **Evidence Commands Run**: `cd /Users/brunoorsier/dev/clipboard && git ls-files Cargo.lock deny.toml .github/workflows && wc -l Cargo.lock && grep -c '^\[\[package\]\]' Cargo.lock` → `.github/workflows/ci.yml`, `.github/workflows/release.yml`, `.github/workflows/sonarcloud.yml`, `Cargo.lock`, `deny.toml`, `6008`, `574`; `cd /Users/brunoorsier/dev/clipboard && grep -RIniE 'proptest|fuzz_target|quickcheck|criterion|iai_callgrind|nextest' src Cargo.toml .github 2>/dev/null; echo "exit=$?"` → `exit=1`; read tool inspections of `Cargo.toml`, `Cargo.lock` metadata via the counted command, `deny.toml`, `build.rs`, `.github/workflows/{ci,release,sonarcloud}.yml`, `src/crypto.rs`, `src/db.rs`, `src/logging.rs`, `docs/SECURITY_LESSONS.md`, `docs/MULTIPLATFORM_LESSONS.md`. **Not run (tools not installed per parent): `cargo deny check`, `cargo audit`, `cargo metadata --format-version 1`, `cargo tree --locked -d`, `cargo tree --locked -e build`, `git diff -- Cargo.lock`.** No cargo/clippy/test commands were run; those belong to the separate static-analysis report.

## Findings

### Critical
No findings in this category.

### High
No findings in this category.

### Medium

**M1. CI test and smoke builds do not use `--locked`, unlike release builds**
- **Dependency / control**: CI reproducibility in `.github/workflows/ci.yml`.
- **Severity**: Medium.
- **Evidence** — `.github/workflows/ci.yml:71-72` verbatim:
  ```yaml
        - name: Run unit and integration tests
          run: cargo test --all-targets --verbose
  ```
  `.github/workflows/ci.yml:86-87` verbatim:
  ```yaml
        - name: Build debug binary
          run: cargo build
  ```
  Contrast `.github/workflows/release.yml:69-70` verbatim:
  ```yaml
      - name: Build release binary
        run: cargo build --release --locked
  ```
  and `:217-218` verbatim:
  ```yaml
      - name: Build release binary
        run: cargo build --release --locked
  ```
- **Risk**: CI can validate a dependency resolution different from `Cargo.lock`, while shipped artifacts use the locked graph; a newly published compatible crate version can change test/smoke behavior without a lockfile diff. (The Sonar issue on the smoke build is tracked separately in the Sonar report; this finding is about the supply-chain reproducibility property.)
- **Recommendation**: Change both CI steps to `cargo test --locked --all-targets --verbose` and `cargo build --locked`.
- **Follow-up test**: Confirm the workflow diff and that both jobs still pass on all three matrix OSes.

**M2. Advisory ignore rationale is narrower than actual `chrono` usage**
- **Dependency / control**: `deny.toml` `RUSTSEC-2026-0543` ignore.
- **Severity**: Medium.
- **Evidence** — `deny.toml:13-16` verbatim:
  ```toml
  ignore = [
      # chrono unmaintained advisory RUSTSEC-2026-0543
      { id = "RUSTSEC-2026-0543", reason = "Used only for local timestamp string formatting in db.rs" },
  ]
  ```
  Actual usage includes `src/db.rs:1` verbatim: `use chrono::Local;` with `:329` verbatim: `        let now = Local::now().format("%Y-%m-%d %H:%M:%S").to_string();` **and** `src/logging.rs:52` verbatim: `        let ts = chrono::Local::now().format("%Y-%m-%d %H:%M:%S");`.
- **Risk**: The word "only ... in db.rs" is factually stale/incomplete; future reviewers may accept the exemption without re-checking the second call site. Per `docs/MULTIPLATFORM_LESSONS.md:60`, major UI/DB upgrades can also resolve old advisories and trigger `advisory-not-detected` warnings, so justifications need current scope.
- **Recommendation**: Update the reason to name both `src/db.rs` and `src/logging.rs` local timestamp formatting, and re-confirm the advisory's affected API is not used; remove the ignore if `cargo deny check` no longer reports it.
- **Follow-up test**: Run `cargo deny check advisories` in CI (already wired via `ci.yml:123-126`) after updating the rationale.

### Low

**L1. No macOS release job despite declared macOS targets and bundle metadata [Inferred]**
- **Dependency / control**: Release artifact coverage.
- **Severity**: Low.
- **Evidence** — `deny.toml:4-9` lists `{ triple = "aarch64-apple-darwin" }` and `{ triple = "x86_64-apple-darwin" }`; `Cargo.toml:25-26` declares macOS bundle metadata (`info-plist`); `release.yml:21-22` verbatim: `  release-windows:
    name: Build & Package Windows (x64)` and `:176-177` verbatim: `  release-linux:
    name: Build & Package Linux (x64)` — the workflow read contains no macOS release job.
- **Risk**: macOS is tested in CI but no signed/notarized or checksummed macOS artifact is published; users of that declared target have no verified supply-chain chain.
- **Recommendation**: Add a `release-macos` job with `cargo build --release --locked`, SHA-256 sums, and `actions/attest-build-provenance`, or document that macOS binaries are intentionally source-built only.
- **Follow-up test**: Verify the published release contains the expected macOS artifacts.

**L2. Native tool installation is unpinned [Inferred]**
- **Dependency / control**: OS toolchain packages used by CI and release jobs.
- **Severity**: Low.
- **Evidence** — `.github/workflows/ci.yml:24-27` verbatim:
  ```yaml
        - name: Install Linux dependencies
          run: |
            sudo apt-get update
            sudo apt-get install -y libx11-dev libxcb1-dev libdbus-1-dev
  ```
  `.github/workflows/release.yml:62-64` verbatim: `      - name: Install Inno Setup
        run: |
          choco install innosetup -y --no-progress` and `:187-190` verbatim: `      - name: Install Linux dependencies
        run: |
          sudo apt-get update
          sudo apt-get install -y libx11-dev libxcb1-dev libdbus-1-dev libssl-dev pkg-config`.
- **Risk**: Native X11/DBus/OpenSSL/Inno Setup components can change between runs without a repository change; at least OpenSSL runtime matching is checked after build (`release.yml:84-101`), but the toolchain itself is floating.
- **Recommendation**: Pin the runner image or package versions for release jobs; keep the existing post-build DLL assertion as defense in depth.
- **Follow-up test**: Review release logs for resolved package versions after pinning.

## Quick Wins
1. Add `--locked` to the two CI build/test commands in `.github/workflows/ci.yml` (M1; ~15 min).
2. Correct the `RUSTSEC-2026-0543` rationale to mention `src/logging.rs` as well as `src/db.rs` (M2; ~15 min).
3. Add a macOS release job or an explicit "source-build only" note (L1; ~2h if added).
4. Pin native package versions in release jobs (L2; ~1h).

## Recommended Actions
1. **M1 (Medium, ~0.25h)**: make CI use the lockfile for tests and smoke build; confirm no behavior change on Linux/macOS/Windows.
2. **M2 (Medium, ~0.25h)**: re-scope or drop the advisory ignore; keep `cargo-deny` CI gate green.
3. **L2 (Low, ~1h)**: pin native toolchain inputs in release.
4. **L1 (Low, ~2h)**: decide and implement macOS artifact publication with checksums and attestation.
5. When the required tools are available locally, run `cargo deny check`, `cargo audit`, `cargo metadata --format-version 1`, `cargo tree --locked -d`, and `git diff -- Cargo.lock`; these were **not run** for this report and no clean result is claimed.

## Coverage
- **Manifest hygiene**: `Cargo.toml:1-107` read; package metadata/bundle identifier (`:16-26`), target-gated deps (`:28-50`), core deps (`:52-82`), lints (`:87-99`), release profile (`:101-107`). No wildcard `*` requirements or unversioned git deps observed. `build.rs:1-21` is a target-gated `winres` build step that calls `res.compile().unwrap_or_else(...panic...)` (`:16-21`), failing loudly rather than shipping a generic exe. No findings in this category.
- **Lockfile and reproducibility**: `Cargo.lock` git-tracked, 6008 lines, 574 packages; release uses `--locked`. Finding M1. `cargo metadata`/`cargo tree` not run — **Not run**, no claim made.
- **Advisories and licenses**: `deny.toml:18-42` license allow-list includes font/asset licenses (`Ubuntu-font-1.0`, `OFL-1.1`, `LicenseRef-UFL-1.0`); `:44-47` `multiple-versions = "warn"`, `wildcards = "deny"`; `:49-52` `unknown-registry = "deny"`, `unknown-git = "deny"`, crates.io index only. Finding M2. `cargo deny check`/`cargo audit` **Not run** (tools not installed).
- **Duplicate and conflicting versions**: **Not run** (`cargo tree -d`, `cargo tree -i <crate>`). No duplicate-version finding is claimed. Project lesson on `gpu-allocator`/`windows` unification is recorded in `docs/MULTIPLATFORM_LESSONS.md:30` and is an actionable check for the next tooled pass. No findings in this category.
- **Build-time attack surface**: Only in-repo build script is `build.rs` (finding above); `Cargo.toml:28-29` gates `winres` to `cfg(windows)` build deps. Proc-macro/build-script enumeration via `cargo tree -e build` **Not run**. No findings in this category.
- **CI/CD trust**: All actions in all three workflows pinned to full SHAs (e.g. `ci.yml:22,30,124`; `release.yml:157,164,251`; `sonarcloud.yml:35,49`); workflow-level `permissions: contents: read` (`ci.yml:9-10`, `release.yml:14-15`, `sonarcloud.yml:9-10`); publish jobs scoped to `contents: write`, `id-token: write`, `attestations: write` (`release.yml:25-28`, `:179-182`); no `continue-on-error` found in the three workflows read. Finding M1 (locked builds).
- **Release artifact verification**: `release.yml:97-101` asserts the linked OpenSSL DLL is imported; `:112-115` verifies Microsoft Authenticode on `vc_redist.x64.exe`; `:139-154` and `:241-248` generate SHA-256 sums with safe globbing; `:156-161` and `:250-253` publish SLSA attestations. Finding L1 (no macOS job).
