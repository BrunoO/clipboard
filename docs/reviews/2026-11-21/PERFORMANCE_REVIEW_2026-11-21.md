# Performance Review - 2026-11-21

## Executive Summary
- **Health Score**: 8/10 — code-level hot paths align with the project's concurrency lessons (change counters, transactions outside classification, coalesced wakeups, event-driven hotkeys, logic-time background wakes); all optimization ideas below are unmeasured.
- **Critical Issues**: 0
- **High Issues**: 0
- **Total Findings**: 4 (2 Medium, 2 Low) — all marked `[Inferred]`
- **Estimated Remediation Effort**: 6 hours (including measurement setup)
- **Evidence Commands Run**: read/grep tool inspections of `src/monitor.rs`, `src/db.rs`, `src/app/mod.rs`, `src/media.rs`, `src/hotkey.rs`, `src/main.rs`, `src/platform/{mod.rs,windows.rs}`, `Cargo.toml`, `docs/CONCURRENCY_AND_PERF_LESSONS.md`, and the captured `debt-inventory.log`. **No performance measurements available.** No `cargo build --release --locked --timings`, `cargo bench`, `cargo flamegraph`, `samply`, `perf`, or `hyperfine` artifacts were generated; no cargo/clippy/test commands were run (those belong to the separate static-analysis report).

## Findings

### Critical
No findings in this category.

### High
No findings in this category.

### Medium

**M1. `[Inferred]` Full image BLOBs are loaded on every history refresh**
- **Hot path**: `Database::get_recent` → `ClipboardApp::refresh_items`, triggered on new-item notifications and on every search-query change.
- **Severity**: Medium.
- **Evidence** — `src/db.rs:480-488` verbatim:
  ```rust
          struct RawRow {
              id: i64,
              kind_str: String,
              content: String,
              image_data: Option<Vec<u8>>,
              width: Option<u32>,
              height: Option<u32>,
              created_at: String,
          }
  ```
  `src/db.rs:505-513` verbatim (each row copies `image_data` out of SQLite):
  ```rust
                  Ok(RawRow {
                      id: row.get(0)?,
                      kind_str: row.get(1).unwrap_or_else(|_| "text".to_string()),
                      content: row.get(2)?,
                      image_data: row.get(3)?,
                      width: row.get(4)?,
                      height: row.get(5)?,
                      created_at: row.get(6)?,
                  })
  ```
  `src/app/mod.rs:871-872` verbatim:
  ```rust
          if new_items_detected || self.search_query != self.last_query_cached {
              self.refresh_items();
  ```
  `src/app/mod.rs:291-295` verbatim:
  ```rust
          self.items = self
              .db
              .get_recent(self.settings.max_history, filter)
              .unwrap_or_default();
          self.filter_counts = FilterCounts::from_items(&self.items);
  ```
- **Observed/expected cost**: `[Inferred]` no measurement; each refresh materializes all `image_data` blobs for up to `max_history` rows, even when the active view only needs text/metadata, and allocates a fresh `Vec<u8>` per image row.
- **Impact**: UI-thread latency and memory churn while typing in search or after clipboard bursts; worst with large PNG history.
- **Recommendation**: Split the query into a lightweight metadata list plus on-demand blob fetch for the selected/visible image entries, or select `image_data` only for rows whose `kind = 'image'` and the UI actually displays. Verify with a release flamegraph of a search keystroke with a populated image history.

**M2. `[Inferred]` Linux clipboard polling has no OS change-counter gate**
- **Hot path**: Background `clipboard-monitor` loop.
- **Severity**: Medium.
- **Evidence** — `src/monitor.rs:11` verbatim: `const POLL_INTERVAL: Duration = Duration::from_millis(200);`
  `src/monitor.rs:66-72` verbatim:
  ```rust
                  // Fast path: when the OS exposes a monotonic clipboard change counter
                  // (macOS NSPasteboard.changeCount, Windows GetClipboardSequenceNumber),
                  // skip all clipboard reads/allocations/hashing when nothing has changed.
                  let change_token = crate::platform::clipboard_change_token();
                  if change_token.is_some() && state.change_token == change_token {
                      continue;
                  }
  ```
  `src/platform/mod.rs:102-105` verbatim:
  ```rust
      #[cfg(not(any(target_os = "macos", target_os = "windows")))]
      {
          None
      }
  ```
  Mitigation already present — `src/monitor.rs:154-155` verbatim: `    if text.len() > MAX_TEXT_LENGTH || state.last_copied_text.as_deref() == Some(text.as_str()) {
        return PollOutcome::Handled;
    }`
- **Observed/expected cost**: `[Inferred]` on X11/Wayland the `None` token means the `continue` at `:70-71` never fires, so `arboard::get_text()` is called up to five times per second while idle; duplicate detection avoids DB writes and image fallback but not the read itself. This matches `docs/CONCURRENCY_AND_PERF_LESSONS.md:52` (change counters unavailable on X11) and `:43` (return `Handled` to avoid image fallback).
- **Impact**: Idle CPU/battery cost on Linux; no jank claim without a measurement.
- **Recommendation**: `[Inferred]` explore X11 `SelectionNotify`/property-notify driven wakeups or adaptive backoff while idle; keep the `Handled` guard. Verify with `perf` or a release flamegraph comparing idle CPU before/after.

### Low

**L1. `[Inferred]` PNG encode/decode uses full-buffer copies at the size ceiling**
- **Hot path**: Image clipboard write/read path through `src/media.rs`.
- **Severity**: Low.
- **Evidence** — `src/media.rs:18-19` verbatim: `    let img: RgbaImage = ImageBuffer::from_raw(width_u32, height_u32, rgba.to_vec())
        .ok_or("Invalid image dimensions or byte length")?;`
  `src/media.rs:43-52` verbatim:
  ```rust
      limits.max_alloc = Some(
          u64::try_from(MAX_IMAGE_PIXELS)
              .unwrap_or(40_000_000)
              .saturating_mul(4),
      );
      reader.limits(limits);

      let img = reader.decode()?;
      let rgba = img.to_rgba8();
      Ok((width, height, rgba.into_raw()))
  ```
- **Observed/expected cost**: `[Inferred]` at the 40 MP ceiling the alloc cap is 160 MB, and encode copies the incoming RGBA slice via `to_vec()` while decode produces a converted RGBA buffer; the bounds are intentional and no oversized decode is possible on the guarded path (`src/media.rs:31-38`).
- **Impact**: Peak memory on large legitimate images; not an unbounded-growth issue.
- **Recommendation**: `[Inferred]` avoid the extra copy when the source is already `Rgba8` and measure peak RSS with a release build before optimizing further.

**L2. `[Inferred]` Prepared statements are rebuilt per `get_recent` call**
- **Hot path**: Search refresh (`src/app/mod.rs:871-872`) and clipboard-item notifications.
- **Severity**: Low.
- **Evidence** — `src/db.rs:516-523` verbatim:
  ```rust
              if let Some(pattern) = search_pattern {
                  let mut stmt = conn.prepare(
                      "SELECT id, kind, content, image_data, width, height, created_at
                       FROM clipboard_history
                       WHERE kind != 'secret' AND content LIKE ?1 ESCAPE '\\'
                       ORDER BY id DESC LIMIT ?2",
                  )?;
  ```
  `src/db.rs:528-533` verbatim shows the unfiltered statement prepared in the same call: `                let mut stmt = conn.prepare(
                    "SELECT id, kind, content, image_data, width, height, created_at
                     FROM clipboard_history
                     ORDER BY id DESC LIMIT ?1",
                )?;`
- **Observed/expected cost**: `[Inferred]` statement parsing repeats on each refresh; no measurement exists to size it against blob copying (M1).
- **Impact**: Minor additional latency per search keystroke.
- **Recommendation**: `[Inferred]` cache prepared statements on `DatabaseInner` only if a release-profile measurement attributes meaningful time here; otherwise prioritize M1.

## Quick Wins
1. Confirm with a release flamegraph whether image-blob loading (M1) dominates search refresh before refactoring.
2. Keep the existing duplicate/oversized-text `Handled` guard (`src/monitor.rs:154-155`) when addressing Linux polling (M2).
3. Avoid the `rgba.to_vec()` copy in `rgba_to_png` when input is already owned (L1) — only with a measurement showing allocation pressure.
4. Defer statement caching (L2) until after M1/M2 are measured.

## Recommended Actions
1. **Measurement first**: generate `cargo build --release --locked --timings` plus a flamegraph of (a) idle 60 s on Linux, (b) search typing with 100+ image entries, (c) large PNG paste. Without these, all findings remain `[Inferred]`.
2. **M1 (Medium, ~2h + measurement)**: two-phase metadata/blob loading; verify by re-running the same flamegraph and comparing allocations with `samply`/`perf`.
3. **M2 (Medium, ~2h + measurement)**: event-driven X11 clipboard notification or adaptive idle backoff; verify with idle CPU sampling.
4. **L1 (Low, ~1h)** and **L2 (Low, ~1h)**: apply only if the measurements attribute cost to those call sites; otherwise document as accepted.

## Coverage
- **Measurement baseline**: **No performance measurements available.** No timing/profiling/benchmark command was run; all findings are code-level `[Inferred]` observations. No findings in this category.
- **Clipboard and image path**: OS change counters gate reads (`src/monitor.rs:66-72`, `src/platform/mod.rs:93-100`), transient failures retried (`src/monitor.rs:85-88`), header/alloc guards present (`src/media.rs:31-50`), text bounded (`src/monitor.rs:12,154`). Finding M2 (Linux no counter); L1 noted.
- **Database and keyring**: Classification before mutex (`src/db.rs:326-333`), single transaction per insert (`src/db.rs:333-364`), `VACUUM`/WAL truncate only on explicit clear (`src/db.rs:622-630`), keyring read once at startup (`src/main.rs:253-262`). Findings M1, L2.
- **Concurrency and wakeups**: Named threads (`src/monitor.rs:45-46`, `src/hotkey.rs:107-109`), `Disconnected` handled as shutdown (`src/monitor.rs:58-64`), coalesced `try_send` (`src/monitor.rs:168,225`), event-driven `crossbeam_channel::select!` with zero idle wakeups (`src/hotkey.rs:104-129`), bounded `recv_timeout` only for the paste delay (`src/platform/mod.rs:276`). No findings in this category.
- **UI frame work (`eframe`/`egui`)**: Background handlers in `logic()` (`src/app/mod.rs:1384-1407`), keyboard shortcuts in `ui()` (`:1409-1421`), one-shot exact-deadline repaint for auto-clear (`:1396-1397`), short post-hide tick only until inactive latch (`:1400-1406`), `FilterCounts` precomputed on refresh (`:285-295`). No findings in this category.
- **Allocation, clone, and type choices**: `Database` is one `Arc<DatabaseInner>` (`src/db.rs:103-113`); image textures cached (`src/app/mod.rs:1739,297`); `Arc<RwLock>` for hotkey target IDs is read-scoped (`src/hotkey.rs:100-121`). Findings L1, L2.
- **Build/release tradeoffs**: `Cargo.toml:101-107` sets `opt-level = 3`, `lto = "thin"`, `codegen-units = 1`, `panic = "abort"`, `strip = true`; no startup-time or binary-size measurement exists, so no assertion is made about their cost. No findings in this category.
