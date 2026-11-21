# Cargo Build Health Review - 2026-11-21

## Executive Summary

- **Health Score**: 8/10 — manifest, lockfile tracking, bundle metadata, build script, native prerequisites, profiles, and pinned CI gates are evidenced and consistent; one reproducibility gap and one warn-only duplicate-version condition are found. Not lower because no broken build/artifact is demonstrated; not 9–10 because CI dependency resolution is not locked.
- **Critical Issues**: 0
- **High Issues**: 0
- **Total Findings**: 2 (1 Medium, 1 Low)
- **Estimated Remediation Effort**: [Inferred] ~1 hour for the `--locked` alignment (the Sonar issue for the same `ci.yml:87` command carries `effort: "1h"`); duplicate-version triage is not estimated.
- **Evidence Commands Run**:
  - `git ls-files Cargo.lock .github/workflows/ci.yml .github/workflows/release.yml .github/workflows/sonarcloud.yml scripts/windows/setup-openssl-ci.ps1 assets/icon.icns assets/icon.ico assets/icon.png` → all eight paths printed, `git status --short Cargo.lock` → empty.
  - `grep -n '^name = "cipherclipboard"\|^version = "0.1.7"\|^name = "windows"\|^name = "gpu-allocator"\|^name = "eframe"\|^name = "egui"\|^name = "egui_extras"\|^name = "egui_material_icons"\|^name = "winres"' Cargo.lock` → `782:name = "cipherclipboard"`, `783:version = "0.1.7"`, `1097:eframe`, `1134:egui`, `1199:egui_extras`, `1229:egui_material_icons`, `1946:gpu-allocator`, `5300:windows`, `5584:winres`.
  - `grep -n '^name = "windows-sys"' Cargo.lock` → `5414:`, `5423:`, `5432:` (versions `0.52.0`, `0.59.0`, `0.61.2` on the following lines).
  - `sed -n '780,820p' Cargo.lock` → `cipherclipboard 0.1.7` package block with target-gated deps (`objc`, `tray-icon`, `winres`, `windows-sys 0.61.2`, `x11rb 0.14.0`).
  - `grep -RIn 'cache\|sccache\|--locked\|all-features' .github/workflows` → output quoted in Findings and Coverage.
  - `ls -la assets` → `icon.icns`, `icon.ico`, `icon.png`, `wordlist.txt` present.
  - Direct reads of `Cargo.toml`, `Cargo.lock`, `build.rs`, `deny.toml`, `.rustfmt.toml`, and all three workflow files.
  - **Not run**: `cargo metadata`, `cargo tree`, `cargo check`, `cargo build`, `cargo deny check`, or any timing command. No build, lint, test, or timing result is claimed in this report; those are being gathered separately.

## Findings

### Critical

No findings in this category.

### High

No findings in this category.

### Medium

**M-1 — CI quality gates resolve dependencies without `--locked` while release builds lock them**

- **Build control**: workflow cargo invocations (`.github/workflows/ci.yml`, `.github/workflows/sonarcloud.yml`) vs release builds.
- **Evidence** (verbatim lines, from file reads and `grep -RIn 'cache\|sccache\|--locked\|all-features' .github/workflows`):

```text
.github/workflows/ci.yml:38:        run: cargo clippy --all-targets --all-features -- -D warnings
.github/workflows/ci.yml:69:        run: cargo clippy --all-targets --all-features -- -D warnings
.github/workflows/ci.yml:72:        run: cargo test --all-targets --verbose
.github/workflows/ci.yml:87:        run: cargo build
.github/workflows/sonarcloud.yml:38:        run: cargo llvm-cov --all-targets --lcov --output-path lcov.info
.github/workflows/release.yml:70:        run: cargo build --release --locked
.github/workflows/release.yml:218:        run: cargo build --release --locked
```

  Lockfile existence/tracking evidence: `git ls-files Cargo.lock` → `Cargo.lock`; `git status --short Cargo.lock` → empty.
- **Failure mode**: non-reproducible dependency graph in CI/coverage relative to `Cargo.lock` and to the release builds. The `ci.yml:87` instance is also the single open Sonar issue (`githubactions:S8549`; see `./SONAR_REVIEW_2026-11-21.md`).
- **Impact**: [Inferred] lint/test/smoke results may not reflect the exact locked graph shipped by `--release --locked` builds. No CI failure or build result is claimed.
- **Recommendation**: append `--locked` to `.github/workflows/ci.yml:38`, `:69`, `:72`, `:87` and `.github/workflows/sonarcloud.yml:38`. Verify by re-reading those lines and by re-running the CI checks in the separate evidence pass; do not report a pass here.

### Low

**L-1 — Three `windows-sys` versions are accepted because duplicate versions are warn-only**

- **Build control**: `deny.toml` bans policy and resolved lockfile.
- **Evidence** (verbatim):

```toml
deny.toml:45:multiple-versions = "warn"
deny.toml:46:wildcards = "deny"
```

```text
Cargo.lock:5414:name = "windows-sys"
Cargo.lock:5415:version = "0.52.0"
Cargo.lock:5423:name = "windows-sys"
Cargo.lock:5424:version = "0.59.0"
Cargo.lock:5432:name = "windows-sys"
Cargo.lock:5433:version = "0.61.2"
```

  Direct dependency pins `windows-sys = { version = "0.61", features = [...] }` at `Cargo.toml:33-44`; `Cargo.lock:798` shows the crate's resolved dependency `"windows-sys 0.61.2"`. By contrast, the `windows` crate specifically called out in `docs/MULTIPLATFORM_LESSONS.md:30` has a single entry: `Cargo.lock:5300:name = "windows"`, `Cargo.lock:5301:version = "0.62.2"`.
- **Failure mode**: the configured quality gate cannot fail on duplicate versions; the currently observed duplicates are `windows-sys`, not `windows`. No duplicate `windows` crate is claimed.
- **Impact**: [Inferred] extra compilation of similar crates and slower future upgrades; no build failure is evidenced.
- **Recommendation**: [Inferred] triage whether `0.52.0`/`0.59.0` can be unified with `0.61.2` via a targeted `cargo update -p windows-sys@...` in a change that can be verified with `cargo tree -d`; do not assume semver compatibility without running that check. Alternatively, record an explicit allow/ignore in `deny.toml` with a reason if the split is required by transitive constraints. Neither command was run here.

## Quick Wins

1. Append `--locked` to the five workflow commands in M-1; the one-line `ci.yml:87` change matches the Sonar-reported 1h effort.
2. Keep the existing `cargo-deny` CI gate (`.github/workflows/ci.yml:123-126`) unchanged; it already runs `command: check` with a full-SHA-pinned action (`ci.yml:124`).
3. [Inferred] Add a short comment in `deny.toml` beside `multiple-versions = "warn"` documenting why duplicates are tolerated, pending the `windows-sys` triage.
4. [Inferred] Record the current `windows-sys` version split (`Cargo.lock:5415`, `:5424`, `:5433`) as the baseline so a future `cargo tree -d` result can be compared.

## Recommended Actions

1. **Immediate**: implement M-1 (`--locked` on all CI/coverage cargo commands), then re-run the workflow checks and the Sonar fetch in the separate evidence pass. This also addresses the only Sonar issue from `./SONAR_REVIEW_2026-11-21.md`.
2. **Short-term**: triage L-1 with `cargo tree -d` (not run here) and either unify versions or document the exception; keep `deny.toml:46 wildcards = "deny"` and the source-policy at `deny.toml:50-52` unchanged.
3. **Long-term**: [Inferred] add a workflow check that rejects cargo commands without `--locked` so the M-1 pattern cannot reappear; no such check was found in `scripts/` (directory listing read).
4. **Verification plan (commands to be run in the separate build-evidence pass, not here)**: `cargo metadata --no-deps --format-version 1`, `cargo tree --locked -e features`, `cargo tree --locked -d`, `cargo check --all-targets --all-features --locked`, `cargo build --release --locked`, `cargo deny check`. No result is predicted.

## Coverage

**Build configuration summary** (evidence-backed, no build claims):

| Area | Evidence | Status |
|---|---|---|
| Package / binary | `Cargo.toml:1-14` — `cipherclipboard 0.1.7`, edition 2021, `[[bin]] CipherClipboard` at `src/main.rs` | Consistent; `src/main.rs` present |
| Lockfile | `Cargo.lock:782-783` `cipherclipboard 0.1.7`; `git ls-files Cargo.lock` tracked, clean | Consistent |
| Bundle metadata | `Cargo.toml:16-26` — identifier `com.cipherclipboard.app`, version `0.1.7`, `icon = ["assets/icon.icns"]`, macOS plist at `:25-26` | Matches package version; icons tracked |
| Target deps | `Cargo.toml:28-50` — `winres` (Windows build), `tray-icon`/`windows-sys`, `objc` (macOS), `x11rb` (Linux) | Scoped as intended |
| Build script | `build.rs:2-22` — Windows-only `winres` resource compile with fail-loud panic at `:16-21`; `assets/icon.ico` tracked | Evidence present; cross-compile note in `./PLATFORM_COMPAT_REVIEW_2026-11-21.md` (L-1 there) |
| Profiles | `Cargo.toml:101-107` — `opt-level = 3`, `lto = "thin"`, `codegen-units = 1`, `panic = "abort"`, `strip = true` | Release-only settings |
| Native prerequisites | `ci.yml:24-27`, `:51-55` Linux X11/XCB/DBus; `ci.yml:57-60` and `release.yml:57-59` Windows OpenSSL script; `release.yml:187-190` Linux adds `libssl-dev pkg-config` | Install steps present |
| Artifacts | `release.yml:75` `target\release\CipherClipboard.exe`; `release.yml:222` `target/release/CipherClipboard`; SHA-256 steps `:139-154` and `:241-248` | Names match `[[bin]]` |
| Lints / gates | `Cargo.toml:87-99`; `ci.yml:35` fmt, `:38`/`:69` clippy, `:72` tests, `:124-126` cargo-deny; `sonarcloud.yml:47-51` Sonar scan | Gates present; `--locked` gap = M-1 |
| Action pinning / permissions | `ci.yml:9-10`, `:22`, `:30`; `release.yml:14-15`, `:25-28`; full SHAs with version comments | Least-privilege defaults and job-scoped write permissions observed |

**Command results table**:

| Command from prompt | Run in this report? | Result |
|---|---|---|
| `cargo metadata --no-deps --format-version 1` | No | Not run (parent gathering build evidence) |
| `cargo tree --locked -e features` | No | Not run |
| `cargo check --all-targets --all-features --locked` | No | Not run |
| `cargo build --release --locked` | No | Not run |
| `cargo deny check` | No | Not run locally; CI action exists at `ci.yml:123-126` |
| `find . -name build.rs -not -path './target/*' -print` | No | `build.rs` read directly at repo root |
| `grep -RIn 'cargo build\|cargo clippy\|cargo test\|cargo deny' .github/workflows` | Yes (broader pattern quoted above) | Lines `:38`, `:69`, `:70`, `:72`, `:87`, `:195`, `:218` observed |

**Prompt category checklist**:

1. **Manifest correctness**: No findings in this category. (Version, bundle metadata, icon paths, and lint config verified above.)
2. **Dependency Graph and Features**: No findings in this category. (`Cargo.toml:28-85` scoping reviewed; `Cargo.lock:782-806` package deps reviewed; `cargo tree -e features` not run.)
3. **Lockfile and Reproducibility**: M-1.
4. **Build Scripts and Native Toolchains**: No findings in this category. (`build.rs:1-24`, `winres` at `Cargo.toml:28-29`, CI native-dependency steps cited above; resource verification at `release.yml:79-82`.)
5. **Profiles and Artifacts**: No findings in this category.
6. **Cross-Target Build Matrix**: No findings in this category. (`deny.toml:4-9` four triples; workflow hosts at `ci.yml:45`, `release.yml:24`, `:178`; macOS release-job gap covered in `./PLATFORM_COMPAT_REVIEW_2026-11-21.md` L-3.)
7. **Quality Gates in CI**: M-1 (missing `--locked`). Action pinning and permissions: No findings in this category.
