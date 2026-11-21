# Rust Performance Review Prompt

**Purpose:** Find measurable performance problems in this Rust desktop app using release-build evidence and Rust-native tooling — not guesses from code shape alone.

---

## Prompt

````markdown
You are a Rust performance engineer. Review performance and responsiveness of this clipboard manager using actual code, measured artifacts, and the project's concurrency lessons. Follow the master orchestrator's Global Evidence Rules.

## Context

- Release profile: `opt-level = 3`, `lto = "thin"`, `codegen-units = 1`, `panic = "abort"`, `strip = true`.
- Hot paths: clipboard polling, text classification, image decode, SQLCipher DB writes/queries, keyring access, UI frame work, hidden-window background wakeups.
- Normative constraints: `docs/CONCURRENCY_AND_PERF_LESSONS.md`.

## Grounding Rules

1. Debug timings are not evidence of release performance. Mark them as invalid if used.
2. Do not claim an allocation/clone is expensive without showing the call site and expected frequency.
3. Benchmark numbers require the exact command, tool, environment summary, and output.
4. Empty categories get `No findings in this category.`
5. `[Inferred]` prefixes optimization ideas not backed by a measurement.

## Review Process

### 1. Measurement Baseline
- Ask for or run release builds:
```bash
cargo build --release --locked
cargo test --release --all-targets --locked
```
- For profiling, use `cargo flamegraph --bin CipherClipboard`, `samply`, `perf`, or platform equivalents.
- For microbenchmarks, use `criterion` (`cargo bench`), with `black_box`, realistic inputs, and several input sizes.
- For noisy CI, prefer instruction-count benchmarks (`iai-callgrind`) over wall-clock.
- For end-to-end CLI-like checks, use `hyperfine`.
- If no measurement exists, write `No performance measurements available.` and continue with code-level `[Inferred]` observations.

### 2. Clipboard and Image Path
- OS change counters (`NSPasteboard.changeCount`, `GetClipboardSequenceNumber`) gate payload reads.
- No multi-megabyte image decode/copy on unchanged text paths.
- PNG header dimensions and `image::Limits::max_alloc` checked before decode.
- Text length bounded before classification/storage/UI.
- Windows transient clipboard failures retried rather than permanently disabling.

### 3. Database and Keyring
- Classification happens outside the SQLite `Mutex`.
- Duplicate check → delete stale → insert → prune in one transaction.
- Prepared statements reused where practical; no query in a frame loop without need.
- Keyring lookups cached/lifetime-scoped rather than repeated per frame.
- `VACUUM`/WAL checkpoint only on explicit full clear, not hot paths.

### 4. Concurrency and Wakeups
- No idle polling where blocking `recv`/`select!` is possible.
- `recv_timeout` used only when a bounded wait is required (paste delay, active X11 grab).
- Bounded `sync_channel(1)`/`try_send` for coalesced notifications.
- `Disconnected` handled as shutdown; no busy spins.
- Coupled state in one lock; critical sections free of CPU work.
- Named threads and joined handles; no detached per-action threads racing on focus/paste.

### 5. UI Frame Work (`eframe`/`egui`)
- `App::logic()` handles hidden-window background events; `App::ui()` handles keyboard/pointer input.
- No unconditional repaint timers while hidden; use `request_repaint()` or exact-deadline `request_repaint_after`.
- Aggregates (filter counts, color detection) computed on refresh, not every frame.
- Expensive widgets and large image textures cached/reused appropriately.

### 6. Allocation, Clone, and Type Choices
- `clone()` in hot paths has a reason; prefer borrowing, `Cow`, iterators, or smallVec-like strategies where justified.
- `format!`/`to_string()` inside loops reviewed.
- Public APIs avoid unnecessary boxing/indirection.
- `Arc<Mutex<_>>` acquired once per operation, not per field.

### 7. Build/Release Tradeoffs
- LTO/codegen-units/panic/strip settings measured against startup time, binary size, and crash reporting.
- Debug assertions and logging gated correctly; release logging does not serialize secrets.

## Commands (run if available; paste exact output)

```bash
cargo build --release --locked --timings
cargo bench
cargo flamegraph --bin CipherClipboard
grep -RInE 'clone\(\)|format!\(|to_string\(\)|recv_timeout|try_recv|request_repaint|changeCount|GetClipboardSequenceNumber|transaction\(' src
```

## Finding Format

1. **Hot path** — operation and trigger frequency.
2. **Severity** — Critical / High / Medium / Low.
3. **Evidence** — `path:line` snippet plus measurement output if available.
4. **Observed/expected cost** — measured number or `[Inferred]` reason.
5. **Impact** — UI jank, CPU, memory, battery, startup, DB latency, or user delay.
6. **Recommendation** — concrete change and how to verify improvement.

## Output Sections

- Measurement summary (or explicit absence).
- Findings by severity.
- Quick Wins.
- Recommended Actions with verification plan.
- Coverage checklist with `No findings in this category.` where empty.
````

---

## Usage

Use as step 7 of the full review, in Pre-Release Review, or after changes to polling, DB, UI loop, or FFI caching.
