# Agent Instructions

## Security & Platform Hardening

Before modifying crypto, database, clipboard monitoring, FFI (`src/platform.rs`), packaging, or CI workflows, read [`docs/SECURITY_LESSONS.md`](docs/SECURITY_LESSONS.md) and preserve all invariants documented there:

- Keep `unsafe_code = "deny"` crate-wide; confine all FFI `unsafe` code to `src/platform.rs`.
- Never log clipboard contents or secrets in release builds; wrap in-memory secrets in `zeroize::Zeroizing`.
- Validate URLs (`http`/`https` with host) before `webbrowser::open` and bound PNG dimensions/allocations before decoding.
- On macOS, guard `NSApplication` calls with `is_main_thread()`, wrap ObjC calls in `autoreleasepool`, filter `[ns_app windows]` with `canBecomeMainWindow`, and never mutate windows re-entrantly inside `NSNotificationCenter` callbacks.
- Pin all GitHub Actions to full commit SHAs with least-privilege job-level `permissions:`.

## Concurrency & Performance

Before modifying threads, channels, locks, database access, platform polling, or the `eframe` event loop, read [`docs/CONCURRENCY_AND_PERF_LESSONS.md`](docs/CONCURRENCY_AND_PERF_LESSONS.md) and preserve all invariants documented there:

- Always handle channel `Disconnected` on background loops, name all threads via `thread::Builder::name`, and join owned threads on shutdown.
- Keep `Mutex` critical sections free of CPU-bound work; wrap multi-statement SQLite writes in a single transaction; never split coupled cross-thread state across separate atomics.
- Prefer event-driven blocking (`crossbeam_channel::select!`, blocking `recv()` when idle, `sync_channel(1)` for coalesced wakeups) and OS change counters (`clipboard_change_token`, thread-local `X11Session`) over busy polling.
- In `eframe` 0.36+, keep hidden-window/background wake handlers (hotkey, tray, reopen, monitor, auto-clear, `CancelClose`) in `App::logic()`, keep keyboard/pointer input in `App::ui()`, and avoid unconditional hidden-window repaint timers.

## Verification

Before committing or opening a PR, run:

```bash
cargo fmt -- --check
cargo clippy --all-targets --all-features -- -D warnings
cargo test --all-targets
```

Remember that `CI` runs `clippy -D warnings` on Linux, macOS, and Windows — avoid `items_after_statements`, `borrow_as_ptr`, unchecked integer casts, and manual nul-terminated byte strings in `#[cfg(target_os = "...")]` blocks.
