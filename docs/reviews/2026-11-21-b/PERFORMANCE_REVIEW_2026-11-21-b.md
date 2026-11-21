# Performance Review - 2026-11-21-b

## Executive Summary
- **Health Score**: 9/10 — all normative concurrency and performance invariants in `docs/CONCURRENCY_AND_PERF_LESSONS.md` are implemented in code (lazy on-demand `get_image_data` with `ImageTextureCache` LRU + `failed_ids` instead of loading image BLOBs in `get_recent`, classification outside the SQLite `Mutex`, single-transaction insert+dedup+prune, OS clipboard change counters on macOS/Windows, `PollOutcome::Handled` guard on unchanged/oversized text, thread-local `X11Session` with pipelined atom interning and 5s Wayland backoff, event-driven `crossbeam_channel::select!` hotkeys, coalesced `sync_channel(1)` wakeups, and `App::logic()` hidden-window handling with zero unconditional repaint timers); only three minor `[Inferred]` code-level optimization opportunities remain.
- **Critical Issues**: 0
- **High Issues**: 0
- **Total Findings**: 3 (0 Critical, 0 High, 1 Medium, 2 Low) — all marked `[Inferred]`
- **Estimated Remediation Effort**: 4 hours (including benchmark/profiling setup)
- **Evidence Commands Run**:
  - `grep -RInE 'clone\(\)|format!\(|to_string\(\)|recv_timeout|try_recv|request_repaint|changeCount|GetClipboardSequenceNumber|transaction\(' src` (313 matches)
  - `grep -RIniE 'proptest|fuzz_target|quickcheck|criterion|iai_callgrind|nextest' src Cargo.toml .github 2>/dev/null; echo "test_tools_exit=$?"` → `test_tools_exit=1` (no `criterion` or `iai-callgrind` benchmarks configured)
  - Direct file reads of `Cargo.toml`, `docs/CONCURRENCY_AND_PERF_LESSONS.md`, `src/monitor.rs`, `src/db.rs`, `src/media.rs`, `src/hotkey.rs`, `src/main.rs`, `src/code.rs`, `src/color.rs`, `src/secret.rs`, `src/app/{mod.rs,history_view.rs}`, and `src/platform/{mod.rs,macos.rs,windows.rs,linux.rs}`.
  - **No performance measurements available.** No `cargo bench`, `cargo flamegraph`, `samply`, `perf`, or `hyperfine` runtime measurements were captured; all findings below are code-level `[Inferred]` observations.

## Findings

### Critical
No findings in this category.

### High
No findings in this category.

### Medium

**M1. `[Inferred]` Linux clipboard polling lacks an OS change-counter gate and polls `arboard::get_text()` every 200ms while idle**
- **Hot path**: Background `clipboard-monitor` loop (`src/monitor.rs:56-98`), running every `POLL_INTERVAL = 200ms` (`5 Hz`).
- **Severity**: Medium.
- **Evidence** — `src/monitor.rs:11` verbatim:
  ```rust
  const POLL_INTERVAL: Duration = Duration::from_millis(200);
  ```
  `src/monitor.rs:66-78` verbatim:
  ```rust
                  // Fast path: when the OS exposes a monotonic clipboard change counter
                  // (macOS NSPasteboard.changeCount, Windows GetClipboardSequenceNumber),
                  // skip all clipboard reads/allocations/hashing when nothing has changed.
                  let change_token = crate::platform::clipboard_change_token();
                  if change_token.is_some() && state.change_token == change_token {
                      continue;
                  }

                  // Skip processing if marked as concealed/transient by password managers
                  if db.ignore_concealed() && crate::platform::is_clipboard_concealed() {
                      state.change_token = change_token;
                      continue;
                  }
  ```
  `src/platform/mod.rs:276-285` verbatim:
  ```rust
  pub(crate) fn clipboard_change_token() -> Option<u64> {
      #[cfg(any(target_os = "macos", target_os = "windows"))]
      {
          backend::clipboard_change_token()
      }
      #[cfg(not(any(target_os = "macos", target_os = "windows")))]
      {
          None
      }
  }
  ```
  Mitigation already present at `src/monitor.rs:154-156` verbatim:
  ```rust
      if text.len() > MAX_TEXT_LENGTH || state.last_copied_text.as_deref() == Some(text.as_str()) {
          return PollOutcome::Handled;
      }
  ```
- **Observed/expected cost**: `[Inferred]` On Linux (X11/Wayland), `clipboard_change_token()` returns `None`, so every 200ms tick runs `is_clipboard_concealed()` (an X11 `ConvertSelection(TARGETS)` round-trip on a temporary `INPUT_ONLY` window, `src/platform/linux.rs:261-318`) and `clipboard.get_text()` (`src/monitor.rs:146`). When text is present and unchanged, `PollOutcome::Handled` prevents falling through to `get_image()`, and `with_x11` reuses a thread-local `X11Session` (`src/platform/linux.rs:73-97`), keeping per-tick overhead low, but idle X11 traffic still occurs 5 times per second.
- **Impact**: Minor idle CPU/wakeups on Linux X11/Wayland compared to 0-read idle polling on macOS and Windows.
- **Recommendation**: `[Inferred]` Consider listening for X11 `XFIXES` `SelectionNotify` (`XFixesSelectSelectionInput` on `CLIPBOARD`) to synthesize a monotonic per-process change counter on X11, or apply adaptive poll-interval backoff when idle. Verify with `perf stat` / `cargo flamegraph` over a 60-second idle window on Linux.

### Low

**L1. `[Inferred]` `ClipboardItem::embedded_colors()` re-scans text on every visible UI frame for `Text` and `Code` cards**
- **Hot path**: `ClipboardApp::render_clipboard_item` and `ClipboardApp::render_text_badge` (`src/app/history_view.rs:269-279,290-296`), called on every visible UI frame for each rendered `ItemKind::Text` or `ItemKind::Code` item.
- **Severity**: Low.
- **Evidence** — `src/app/history_view.rs:269-275` verbatim:
  ```rust
                  if item.kind == ItemKind::Text || item.kind == ItemKind::Code {
                      let embedded = item.embedded_colors();
                      if !embedded.is_empty() {
                          ui.add_space(4.0);
                          let color_action =
                              Self::render_embedded_colors(ui, &embedded, state.is_dark);
  ```
  and `src/app/history_view.rs:290-292` verbatim:
  ```rust
      fn render_text_badge(ui: &mut egui::Ui, item: &ClipboardItem, is_dark: bool) {
          let embedded = item.embedded_colors();
  ```
  where `item.embedded_colors()` calls `crate::color::extract_colors(&self.content)` (`src/db.rs:89-100`), which allocates `let chars: Vec<char> = text.chars().collect();` (`src/color.rs:533`).
- **Observed/expected cost**: `[Inferred]` While `FilterCounts::from_items` (`src/app/mod.rs:138-157`) precomputes tab counts once per `refresh_items()`, rendering visible `ItemKind::Text` cards calls `item.embedded_colors()` twice per frame (once in `render_text_badge` via `render_item_header` and once in `render_clipboard_item`), allocating a `Vec<char>` over `item.content` (up to `MAX_TEXT_LENGTH = 500_000` bytes).
- **Impact**: Unnecessary per-frame heap allocation when viewing history containing large multi-kilobyte text or code entries.
- **Recommendation**: `[Inferred]` Cache `embedded_colors: Vec<ParsedColor>` on `ClipboardItem` during `Database::get_recent` (outside the SQLite lock at `src/db.rs:694-724`), eliminating both the duplicate per-frame scan and the `FilterCounts` scan. Verify by comparing frame allocations or `criterion` microbenchmarks on a 100 KB text entry.

**L2. `[Inferred]` PNG encoding in `media::rgba_to_png` clones the raw RGBA buffer via `rgba.to_vec()`**
- **Hot path**: `poll_clipboard_image` → `media::rgba_to_png` (`src/monitor.rs:210`, `src/media.rs:11-28`), triggered once when a new image is copied to the OS clipboard.
- **Severity**: Low.
- **Evidence** — `src/media.rs:22-27` verbatim:
  ```rust
      let img: RgbaImage = ImageBuffer::from_raw(width_u32, height_u32, rgba.to_vec())
          .ok_or("Invalid image dimensions or byte length")?;

      let mut png_bytes = Vec::new();
      img.write_to(&mut Cursor::new(&mut png_bytes), image::ImageFormat::Png)?;
      Ok(png_bytes)
  ```
- **Observed/expected cost**: `[Inferred]` `ImageBuffer::from_raw` requires an owned container, so `rgba.to_vec()` copies the incoming slice (up to 160 MB at the `MAX_IMAGE_PIXELS = 40_000_000` cap) even though `PngEncoder::new(&mut png_bytes).write_image(rgba, width_u32, height_u32, ExtendedColorType::Rgba8)` can encode directly from `&[u8]` without copying.
- **Impact**: One extra transient buffer allocation during large screenshot capture.
- **Recommendation**: `[Inferred]` Encode directly from `&[u8]` using `image::codecs::png::PngEncoder` and `ImageEncoder::write_image` (after validating `rgba.len() == width * height * 4`).

## Quick Wins
1. Cache `embedded_colors` on `ClipboardItem` when constructing items in `Database::get_recent` so `FilterCounts`, `render_text_badge`, and `render_clipboard_item` never re-run `extract_colors` during `ui()` frames (L1; ~30 min).
2. Encode PNGs directly from `&[u8]` via `image::codecs::png::PngEncoder` in `src/media.rs:22-27` to avoid `rgba.to_vec()` (L2; ~20 min).

## Recommended Actions
1. **Establish a measurement baseline (~1.5h)**: Add a `criterion` benchmark target for `secret::is_likely_secret`, `color::extract_colors`, `code::detect_code_language`, and `db::get_recent`, and capture a release flamegraph (`cargo flamegraph --bin CipherClipboard`).
2. **L1 (Low, ~0.5h)**: Store precomputed `embedded_colors` on `ClipboardItem` during `Database::get_recent`.
3. **L2 (Low, ~0.5h)**: Replace `ImageBuffer::from_raw(..., rgba.to_vec())` in `src/media.rs:22` with `PngEncoder::write_image` over the borrowed slice.
4. **M1 (Medium, ~1.5h)**: Evaluate `XFixesSelectSelectionInput` on Linux X11 to provide a lightweight `clipboard_change_token()` on X11 sessions.

## Coverage
- **1. Measurement Baseline**:
  - **No performance measurements available.** No `criterion`, `iai-callgrind`, `flamegraph`, `samply`, or `hyperfine` harnesses exist in the repository (`grep` for benchmark tools returned `exit=1`). All findings are labeled `[Inferred]`.
- **2. Clipboard and Image Path**:
  - OS change counters (`NSPasteboard.changeCount` in `src/platform/macos.rs:382-392`, `GetClipboardSequenceNumber` in `src/platform/windows.rs:474-478`) gate all clipboard reads (`src/monitor.rs:69-72`), and `state.change_token` is only committed when `arboard` does not return `ClipboardOccupied` (`src/monitor.rs:80-97`).
  - Unchanged or oversized (`> 500_000` bytes) text returns `PollOutcome::Handled` so the monitor does not fall through to `poll_clipboard_image` (`src/monitor.rs:154-156`).
  - PNG decoding validates header dimensions (`<= 40_000_000` pixels) via `into_dimensions()` and sets `image::Limits::max_alloc` (`160 MB`) before `.decode()` (`src/media.rs:39-58`).
  - Transient clipboard unavailability is retried in both `init_clipboard_with_retry` (`src/monitor.rs:121-137`) and `ensure_clipboard_writer` (`src/app/mod.rs:489-499`).
  - Findings M1, L2.
- **3. Database and Keyring**:
  - Text classification (`classify_text_kind`) runs *before* `self.lock_conn()` in `Database::insert` (`src/db.rs:450-455`), and legacy row classification runs *after* dropping `conn` in `Database::get_recent` (`src/db.rs:635-724`).
  - `Database::get_recent` now selects only metadata (`SELECT id, kind, content, width, height, created_at`, `src/db.rs:651,664,680`), resolving the previous review's M1 finding; image PNG bytes are fetched lazily on demand via `Database::get_image_data(id)` (`src/db.rs:734-745`) and cached in `ImageTextureCache` (capped at `MAX_CACHED_TEXTURES = 24` with `failed_ids` tracking, `src/app/mod.rs:20-104`).
  - Duplicate check, stale copy deletion, insertion, and `prune_in_tx` execute in a single `conn.transaction()?` (`src/db.rs:456-495,510-539,563-601`).
  - Keyring is accessed once at startup in `open_encrypted_database` (`src/main.rs:107,258-266`).
  - `VACUUM;` and `PRAGMA wal_checkpoint(TRUNCATE);` run only on explicit `Database::clear()` (`src/db.rs:802-833`).
  - No findings in this category.
- **4. Concurrency and Wakeups**:
  - `HotkeyService` blocks with zero idle wakeups via `crossbeam_channel::select!` over `GlobalHotKeyEvent::receiver()` and a bounded shutdown channel (`src/hotkey.rs:96-120`).
  - Linux `QuoteHotkeyFallback::run_loop` blocks on `rx.recv()` when `grabs.is_empty()` and uses `rx.recv_timeout(50ms)` only while an X11 fallback grab is active (`src/platform/linux.rs:574-579`).
  - `notify_tx` and `hotkey_tx` use `std::sync::mpsc::sync_channel(1)` (`src/main.rs:150-151`) with `try_send(())` (`src/monitor.rs:169,227`, `src/hotkey.rs:115`).
  - All background loops exit cleanly on `Disconnected` (`src/monitor.rs:59,128`, `src/hotkey.rs:103-105`, `src/platform/linux.rs:576,594,602`, `src/platform/mod.rs:518,530,549,560`).
  - Coupled Windows window state (`CapturedWindow { hwnd, pid }`) is stored in a single `Mutex<Option<CapturedWindow>>` (`src/platform/windows.rs:44-70`).
  - Every spawned thread is named (`clipboard-monitor`, `hotkey-listener`, `x11-quote-hotkey`, `focus-paste-worker`, `smoke-quit`) and joined on shutdown (`src/app/mod.rs:1593-1609`, `src/platform/mod.rs:589-607`).
  - No findings in this category.
- **5. UI Frame Work (`eframe`/`egui`)**:
  - `eframe::App::logic()` handles all hidden-window and background events (`handle_window_close_request`, `handle_tray_events`, `handle_hotkey_toggle`, `handle_app_activation`, `handle_monitor_updates`, `handle_clipboard_auto_clear`, `src/app/mod.rs:1528-1547`), while `App::ui()` handles keyboard shortcuts and widget rendering (`src/app/mod.rs:1549-1584`).
  - No unconditional hidden-window repaint timer exists: `logic()` schedules a one-shot exact-deadline `ctx.request_repaint_after(deadline.saturating_duration_since(Instant::now()))` when `pending_clipboard_clear` is set (`src/app/mod.rs:1536-1538`) and a short 100ms tick after hiding only until `was_inactive_since_hidden` latches (`src/app/mod.rs:1544-1546`).
  - `FilterCounts` is precomputed on `refresh_items()` (`src/app/mod.rs:326`), and `ImageTextureCache` caches up to 24 GPU textures (`src/app/mod.rs:20-104`).
  - Finding L1 (`embedded_colors()` called during `render_clipboard_item` / `render_text_badge`).
- **6. Allocation, Clone, and Type Choices**:
  - `Database` wraps a single `Arc<DatabaseInner>` (`src/db.rs:151-162`).
  - `arboard::ImageData` borrows decoded RGBA bytes via `Cow::Borrowed(&rgba)` on copy (`src/app/mod.rs:615-619`).
  - Findings L1, L2.
- **7. Build/Release Tradeoffs**:
  - `Cargo.toml:105-110` configures `opt-level = 3`, `lto = "thin"`, `codegen-units = 1`, `panic = "abort"`, and `strip = true`.
  - Release builds compile out debug clipboard logging (`src/monitor.rs:252-253`) and the smoke-quit hook (`src/main.rs:252-253`).
  - No findings in this category.
