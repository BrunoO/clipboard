# Agent Instructions

## Security & Platform Hardening

Before modifying crypto, database, clipboard monitoring, FFI (`src/platform.rs`), packaging, or CI workflows, read [`docs/SECURITY_LESSONS.md`](docs/SECURITY_LESSONS.md) and preserve all invariants documented there:

- Keep `unsafe_code = "deny"` crate-wide; confine all FFI `unsafe` code to `src/platform.rs`.
- Never log clipboard contents or secrets in release builds; wrap in-memory secrets in `zeroize::Zeroizing`.
- Validate URLs (`http`/`https` with host) before `webbrowser::open` and bound PNG dimensions/allocations before decoding.
- On macOS, guard `NSApplication` calls with `is_main_thread()`, wrap ObjC calls in `autoreleasepool`, filter `[ns_app windows]` with `canBecomeMainWindow`, and never mutate windows re-entrantly inside `NSNotificationCenter` callbacks.
- Pin all GitHub Actions to full commit SHAs with least-privilege job-level `permissions:`.

## Verification

Before committing or opening a PR, run:

```bash
cargo fmt -- --check
cargo clippy --all-targets --all-features -- -D warnings
cargo test --all-targets
```

Remember that `CI` runs `clippy -D warnings` on Linux, macOS, and Windows — avoid `items_after_statements`, `borrow_as_ptr`, unchecked integer casts, and manual nul-terminated byte strings in `#[cfg(target_os = "...")]` blocks.
