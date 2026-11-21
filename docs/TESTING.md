# Testing Guide

## What runs today

- `cargo fmt -- --check`
- `cargo clippy --all-targets --all-features --locked -- -D warnings`
- `cargo test --all-targets --locked` — 98 unit tests (99 across platforms) across 17 source files as of 2026-11-21
- `cargo doc --no-deps --locked`
- CI runs the same gates on Linux, macOS, and Windows, plus a debug-only macOS quit smoke job. SonarCloud generates `lcov.info`, but coverage is not measured locally.

There are no `tests/`, `benches/`, or `fuzz/` directories; all tests are unit tests colocated with their modules.

## Tooling intentionally skipped

These are **Skipped: tool not configured**, not failing gates. They are documented here so the absence is deliberate and reviewable.

| Tooling | Status | Why it is skipped | When to adopt |
|---|---|---|---|
| Property-based testing (`proptest`, `quickcheck`) | Not configured | `sanitize_browser_url` and the PNG header parser currently have example-based tests only. | Add arbitrary URL strings and crafted PNG headers once `proptest` is a dev-dependency. |
| Fuzzing (`cargo-fuzz`) | Not configured; no `fuzz/` target | No fuzz harness exists for clipboard byte payloads or image headers. | Add a `fuzz/` crate for clipboard payloads and PNG headers. |
| Miri | Not configured | The platform backends call real OS FFI (`AppKit`, Win32, X11), which Miri cannot execute. | Use Miri only for pure/safe modules after FFI boundaries are isolated; keep OS backends excluded. |
| Criterion / `iai-callgrind` | Not configured | No benchmark suite is declared. | Add benchmarks before optimizing hot paths such as history filtering or PNG conversion. |
| `cargo-nextest` | Not configured | The default test runner is sufficient for the current suite. | Adopt when test sharding or per-test output becomes necessary. |
| Local coverage | Not measured | CI emits `lcov.info` for SonarCloud; no local artifact is produced by the standard pre-flight. | Run `cargo llvm-cov --all-targets --locked --lcov --output-path lcov.info` locally when investigating coverage. |

## Fixture hygiene

Secret-shaped values in unit tests are generated at runtime (for example, `format!("{seed:016x}").repeat(4)` for SQLCipher keys and generated token strings for classifier tests). Do not add static credential-like literals to source: they train secret scanners to ignore real leaks. See `docs/SECURITY_LESSONS.md` §6.

## Local pre-flight

```bash
./scripts/preflight.sh
```

This runs formatting, Clippy, tests, and the repository's other guardrails with locked dependency resolution.
