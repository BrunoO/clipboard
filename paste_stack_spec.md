# Specification: Paste Stack ("Paste Next" Hotkey)

> Status: **Draft v2.** Rewritten after code review to match the current module layout (`src/app/*`, `src/platform/*`) and to address the hotkey delivery, paste dispatch, image skip, secret handling, modifier and cursor issues raised in review.
>
> Required reading before implementing: [`AGENTS.md`](AGENTS.md), [`docs/SECURITY_LESSONS.md`](docs/SECURITY_LESSONS.md), [`docs/MULTIPLATFORM_LESSONS.md`](docs/MULTIPLATFORM_LESSONS.md), [`docs/CONCURRENCY_AND_PERF_LESSONS.md`](docs/CONCURRENCY_AND_PERF_LESSONS.md).

---

## 1. Overview

**Paste Stack** is an optional feature. The user copies several entries in one application, switches to another application, and presses a dedicated global **Paste Next** hotkey repeatedly. Each press pastes the next entry in **LIFO** order (most recent first) into the focused application. The `CipherClipboard` window is never shown.

### 1.1 Design decisions

| Topic | Decision |
|---|---|
| Trigger | A dedicated global hotkey, separate from the Toggle Window hotkey. Plain `Cmd+V` / `Ctrl+V` is never intercepted. |
| Order | LIFO: index `0` (newest) first, then `1`, `2`, … |
| Mode | Non-destructive cursor (`stack_cursor: usize`). History is never modified by Paste Next. |
| Lifecycle | The cursor is always armed. It resets to `0` on every new capture and on history clear. At the end of history Paste Next does nothing (no wrap-around). |
| Default state | `paste_next_enabled = false` (opt-in). |
| Default hotkey | `Cmd+Option+V` (macOS) / `Ctrl+Alt+V` (Windows/Linux). Can be re-recorded. |
| Secrets | `ItemKind::Secret` entries are included, with the same protections as the existing secret-copy flow (§6). |
| Payloads | Text (all text kinds: `Text`, `Url`, `Code`, `Color`, `Secret`) and `Image`. |
| App lock | Not applicable. The codebase has **no app-lock feature** (`src/password.rs` is a password *generator*). See §11. |

### 1.2 Typical workflow

1. The user enables Paste Next in Settings.
2. In App A the user copies `A`, then `B`, then `C`. History is `[C, B, A, …]` and `stack_cursor = 0`.
3. In App B the user presses Paste Next three times. `C`, `B`, `A` are pasted in that order and the cursor ends at `3`.
4. A 4th press with `stack_cursor >= history_len` does nothing.
5. Copying `D` anywhere resets `stack_cursor` to `0`, so `D` is next.

---

## 2. Module touchpoints

| Area | File(s) | Change |
|---|---|---|
| Settings model | [`src/settings.rs`](src/settings.rs) | New fields with serde defaults, default hotkey helper, conflict helper (§8). |
| Settings UI | [`src/app/settings_view.rs`](src/app/settings_view.rs) | Paste Next group: toggle, recorder, conflict warnings, registration status (§8). |
| Hotkey routing | [`src/hotkey.rs`](src/hotkey.rs) (including `linux_x11_fallback`) | Tag events with `HotkeyAction`, add a lossless Paste Next channel (§3). |
| Wiring | [`src/main.rs`](src/main.rs) | Create the Paste Next channel and pass it to `HotkeyService::new` / `App`. |
| App state and handlers | [`src/app/mod.rs`](src/app/mod.rs) | `stack_cursor`, reset rules, Paste Next handler, job submission and result handling (§4, §7). |
| History UI | [`src/app/history_view.rs`](src/app/history_view.rs) | `NEXT` badge and "Reset cursor" action (§7.4). |
| Paste dispatch | [`src/platform/mod.rs`](src/platform/mod.rs) (safe) | New ordered `paste-next-worker` with an OS-agnostic API (§4). |
| OS keystrokes | [`src/platform/macos.rs`](src/platform/macos.rs), [`src/platform/windows.rs`](src/platform/windows.rs), [`src/platform/linux.rs`](src/platform/linux.rs) | Modifier-safe paste simulation (§5). `unsafe` only in `macos.rs` / `windows.rs`. |
| Image fingerprint | [`src/media.rs`](src/media.rs) (moved from [`src/monitor.rs`](src/monitor.rs)) | Shared `pub(crate) fn image_fingerprint(...)` (§6.2). |
| Skip markers | [`src/db.rs`](src/db.rs), [`src/monitor.rs`](src/monitor.rs) | Image skip marker on par with the text marker. Monitor honours it before insert and notify (§6.2). |

---

## 3. Hotkey delivery: lossless and ordered

### 3.1 Problem
Today the listener in [`src/hotkey.rs`](src/hotkey.rs) sends `()` over a `sync_channel(1)` (created in [`src/main.rs`](src/main.rs)) with `try_send`. The app drains it with `while self.hotkey_rx.try_recv().is_ok()` in [`src/app/mod.rs`](src/app/mod.rs). Coalescing is correct for Toggle Window but would **drop** Paste Next presses.

### 3.2 Design
- Introduce:
  ```rust
  #[derive(Debug, Clone, Copy, PartialEq, Eq)]
  pub(crate) enum HotkeyAction { ToggleWindow, PasteNext }
  ```
- `HotkeyService` keeps a `HashMap<u32 /* HotKey::id() */, HotkeyAction>` behind the existing `RwLock`. Both the `global-hotkey` path and the `linux_x11_fallback` path resolve the action from that map.
- **Toggle Window**: behaviour unchanged. Same `sync_channel(1)` + `try_send`, coalesced, and still calls `crate::platform::capture_active_window()`.
- **Paste Next**: a **separate** channel that never drops a press while the app is running:
  - `std::sync::mpsc::channel::<()>()` (unbounded) or a bounded `crossbeam_channel` with a generous capacity (≥ 64). On `Full`, log a warning; never block the listener thread.
  - Does **not** call `capture_active_window()`, because focus is not moving to `CipherClipboard`.
  - Calls `ctx.request_repaint()` so `App::logic()` runs while the window is hidden.
- In `App::logic()` (background wake handlers, per AGENTS), drain the Paste Next receiver **one event at a time**. Each event becomes one call to `handle_paste_next()` (§7.2), never `while try_recv() { requested = true }`.
- Handle `Disconnected` on both channels. Shutdown must still join the listener thread as it does today.

### 3.3 Registration
- `HotkeyService::new` / `update_hotkeys` take the toggle hotkeys and an `Option<HotKey>` for Paste Next (`None` when disabled or unparseable). Registration results are reported per action so the Settings UI can show status separately.
- When `paste_next_enabled == false`, the Paste Next hotkey is **not registered** at all, so no system-wide grab happens.

---

## 4. Paste dispatch: ordered `paste-next-worker`

### 4.1 Why the existing worker is unsuitable
`paste_worker_loop` in [`src/platform/mod.rs`](src/platform/mod.rs) is designed so that **the latest auto-paste wins**:
- it coalesces bursts to the newest request,
- it cancels a pending paste if a newer request arrives during `auto_paste_delay_ms`,
- it restores focus to the captured window,
- it calls `simulate_paste_if_foreground()`, which checks against the **captured** target window.

All four behaviours are wrong for Paste Next. Running the paste on the UI thread would also block the UI.

### 4.2 Design
Add a second named worker thread (`thread::Builder::name("paste-next-worker")`) in [`src/platform/mod.rs`](src/platform/mod.rs). It is lazily spawned like `paste_worker_tx()` and exposed through an OS-agnostic, safe API:

```rust
pub(crate) enum PasteNextPayload {
    Text { text: zeroize::Zeroizing<String>, conceal: bool },
    Image { width: usize, height: usize, rgba: Vec<u8> },
}

pub(crate) struct PasteNextJob {
    pub(crate) seq: u64,                  // monotonically increasing per press
    pub(crate) payload: PasteNextPayload,
    pub(crate) settle_ms: u64,            // from settings.auto_paste_delay_ms (clamped)
    pub(crate) release_modifiers: global_hotkey::hotkey::Modifiers, // modifiers of the Paste Next hotkey
}

pub(crate) enum PasteNextOutcome { Pasted { seq: u64 }, Failed { seq: u64, reason: &'static str } }

pub(crate) fn submit_paste_next(job: PasteNextJob);           // never blocks the caller
pub(crate) fn paste_next_results() -> /* Receiver<PasteNextOutcome> */;
```

### 4.3 Unit of work
The **clipboard write and the simulated keystroke run inside the same job, strictly in FIFO order**. This prevents press N+1 from overwriting the clipboard before press N's paste has run. For each job, in order:

1. **Wait for the hotkey modifiers to be released** (§5.1), with a bounded timeout (≤ 500 ms). If the timeout expires, continue using the explicit-modifier path (§5.2).
2. **Write the payload** to the OS clipboard using the worker's own `arboard::Clipboard`. For concealed text, call `mark_clipboard_concealed()` (§6.1).
3. **Wait `settle_ms`** so the pasteboard/OS clipboard is updated and readable.
4. **Simulate paste into the current foreground window.** No focus restore and no captured-target check. Use a new OS-agnostic `simulate_paste_foreground()` that does **not** depend on `capture_active_window()`.
5. **Report the outcome** (`Pasted` / `Failed`) on the results channel and call `request_repaint()`.

Rules:
- **No coalescing, no superseding.** Every job runs.
- The worker handles `Disconnected` and exits cleanly. It is joined on shutdown, as for other owned threads.
- Payload buffers are dropped (and zeroized for text) as soon as the job finishes.
- The two workers never run keystrokes concurrently: `paste-next-worker` and `focus-paste-worker` share a `Mutex<()>` around the "write → simulate" critical section. No CPU-bound work happens while holding it.

---

## 5. Modifier handling: no corrupted keystrokes

### 5.1 Problem
When the user presses `Ctrl+Alt+V`, Alt (and Ctrl) are typically **still physically held** when the worker sends the synthetic paste.
- **macOS**: safe as implemented. `macos::simulate_paste` calls `CGEventSetFlags(event, MASK_COMMAND)`, which overrides the physical modifiers.
- **Windows** (`SendInput`) and **Linux** (XTest): the target receives `Ctrl+Alt+V`. In Office that opens *Paste Special*, on some layouts it acts as AltGr, and it can **re-trigger our own Paste Next hotkey**, causing a feedback loop.

### 5.2 Requirements
- **Primary:** before simulating, the worker waits until the Paste Next hotkey's modifier keys are released (`GetAsyncKeyState` on Windows; XInput/XQueryKeymap on Linux; `CGEventSourceFlagsState` on macOS), up to the timeout in §4.3.
- **Fallback (timeout expired, user still holding keys):**
  - Windows: inject **key-up** events for each held modifier from `release_modifiers` before `Ctrl↓ V↓ V↑ Ctrl↑`. Do not re-press them afterwards.
  - Linux: XTest key-up for the held modifiers before the `Ctrl+V` sequence.
  - macOS: keep the explicit `CGEventSetFlags(MASK_COMMAND)`.
- **Re-entrancy guard:** while a Paste Next job is in its simulate step, a `PASTE_NEXT_IN_FLIGHT: AtomicBool` (a single flag, not split state) makes the hotkey listener ignore Paste Next events that it generated itself.
- Keep [`src/platform/mod.rs`](src/platform/mod.rs) and [`src/platform/linux.rs`](src/platform/linux.rs) 100% safe Rust. FFI stays in `macos.rs` / `windows.rs`. Mind CI clippy rules: no `borrow_as_ptr`, no unchecked casts, no `items_after_statements` inside `#[cfg(target_os = "...")]` blocks.

---

## 6. Feedback-loop prevention and secret handling

### 6.1 Text (all text kinds)
- Before submitting the job, the app calls `db.skip_next_capture(&text)` ([`src/db.rs`](src/db.rs)) so the monitor's `take_if_skipped` swallows the programmatic write. It is not re-inserted, does not move to the top, does not notify, and does **not reset the cursor**.
- **Single-slot marker:** `transient_text` holds only one value. Because jobs are serialized, the worker sets the marker **inside the job immediately before the clipboard write** (via a `Database` handle or a callback), not at submit time. This prevents queued jobs from overwriting each other's markers.

### 6.2 Images
- Move `compute_image_fingerprint` from [`src/monitor.rs`](src/monitor.rs) into [`src/media.rs`](src/media.rs) as `pub(crate) fn image_fingerprint(width, height, rgba) -> u64`. The monitor and the writer then share one implementation.
- Add to [`src/db.rs`](src/db.rs): `skip_next_image_capture(fp: u64)` / `take_if_image_skipped(fp: u64) -> bool` / `clear_image_skip()`. These mirror the text API, with a single slot set inside the job right before the write.
- In [`src/monitor.rs`](src/monitor.rs), check `take_if_image_skipped(fp)` **before** `rgba_to_png` / `insert_image`. When it matches: update `last_copied_image_hash` and `change_token` (so the fast path stays consistent), clear `last_copied_text`, and **do not insert or notify**.
- Validate against `insert_image` promotion semantics: a skipped write must never promote an older duplicate.

### 6.3 Secrets (`ItemKind::Secret`)
A secret pasted by Paste Next gets the **same protections** as `copy_secret_to_clipboard` in [`src/app/mod.rs`](src/app/mod.rs):
1. Skip capture (§6.1).
2. Mark the clipboard as concealed (`platform::mark_clipboard_concealed()`), done in the worker right after the write (`conceal: true`).
3. Schedule auto-clear according to `auto_clear_secret_secs`, via the existing `schedule_secret_clipboard_clear`. Each secret paste **restarts** the pending clear, so an older timer cannot wipe the clipboard in the middle of a sequence. A non-secret Paste Next cancels any pending secret clear, because the clipboard no longer holds that secret.
4. The secret is held in `zeroize::Zeroizing<String>` from DB read until job completion.
5. No window show/hide, no focus change. Toasts are recorded only if the window is visible (see §7.3).
6. Never log secret or clipboard contents in release builds.

---

## 7. Cursor state and rules

### 7.1 State
- `stack_cursor: usize` in `App` ([`src/app/mod.rs`](src/app/mod.rs)), initialised to `0`.
- `paste_next_seq: u64`, plus an optional `in_flight: VecDeque<(u64 /*seq*/, usize /*index*/)>` used to reconcile outcomes.

### 7.2 Handling a Paste Next press (`handle_paste_next`)
1. If `paste_next_enabled == false`, ignore the press. (The hotkey shouldn't be registered anyway; this is defence in depth.)
2. Resolve the target index as `next = stack_cursor + in_flight.len()` (presses queued but not yet confirmed).
3. **Read from the full, unfiltered history at press time**, e.g. `db.get_recent(limit = next + 1, filter = None)` or a dedicated `db.get_nth(next)`. **Never** index the UI's filtered `items` list, which may be stale while hidden or narrowed by search.
4. If there is no entry at `next`, the press does nothing (end of stack).
5. Enforce existing size limits (text ≤ monitor `MAX_TEXT_LENGTH`, images ≤ `MAX_IMAGE_PIXELS`, decoded via `media::png_to_rgba`). Skip oversized entries with a logged warning and treat them as `Failed`.
6. Build the `PasteNextJob` (decode the PNG to RGBA for images), push `(seq, next)` to `in_flight`, and call `submit_paste_next`.

### 7.3 Outcome handling (in `App::logic()`)
- `Pasted { seq }`: pop the matching `in_flight` entry and set `stack_cursor = index + 1`.
- `Failed { seq, .. }`: pop the entry and **do not advance** the cursor. Clear any pending skip markers (`clear_skip_capture` / `clear_image_skip`), log the reason without content, and record a toast if the window is visible. Jobs already queued behind it keep their own indices, so a failure means that entry is not pasted (no automatic retry). The user can press again.
- `Failed` with reason "Accessibility permission missing" (macOS) shows the existing auto-paste permission hint the next time the window is shown.

### 7.4 Reset and validity rules

| Event | Cursor effect |
|---|---|
| New external capture (monitor notify with a new or promoted id) | `stack_cursor = 0`, clear `in_flight` reconciliation offsets |
| **In-app re-copy** of a history item (`copy_text_to_clipboard`, `copy_image_to_clipboard`, generator copies not skipped) | Treated like an external copy: the monitor promotes the item to the top, notifies, and `stack_cursor = 0` |
| Programmatic Paste Next write (skipped per §6) | No reset |
| History clear (`db.clear()` in the clear-confirm modal, which refreshes without a notify) | Explicit `stack_cursor = 0` in that handler |
| Single delete / prune | `stack_cursor = stack_cursor.min(history_len)`. Deleting an entry above the cursor shifts the "next" item by one; this is accepted and documented, since deletes happen while the window is open and the badge shows the new target |
| `refresh_items()` for search/filter or other UI reloads | **No change** |
| Paste Next disabled → enabled | `stack_cursor = 0` |
| While disabled | Reset rules still apply (cheap), so enabling starts from a consistent state |

### 7.5 Visual feedback (window visible)
- In [`src/app/history_view.rs`](src/app/history_view.rs), show a `NEXT` badge on the row whose history id equals the id at `stack_cursor` in **full** history. Map by **id**, not by visible row index. If a search filter hides that row, show a small header chip instead: "Next: #N (hidden by filter)".
- Show a "Reset cursor" action in the header when `stack_cursor > 0`.
- No badge or chip when `paste_next_enabled == false`.

---

## 8. Settings, recorder and hotkey coherence

### 8.1 New `AppSettings` fields ([`src/settings.rs`](src/settings.rs))
```rust
#[serde(default)]                                  // false
pub(crate) paste_next_enabled: bool,
#[serde(default = "default_paste_next_hotkey")]   // "Cmd+Option+V" (macOS) / "Ctrl+Alt+V" (others)
pub(crate) paste_next_hotkey: String,
```
Existing persisted settings (stored through the DB app-settings row) must load unchanged. Add a round-trip test with an old JSON payload.

### 8.2 Recorder UI ([`src/app/settings_view.rs`](src/app/settings_view.rs))
- New `render_paste_next_settings_group` inside the hotkey section: an enable toggle, the current binding, **Record / Clear / Reset to default**, a conflict warning, and registration status.
- Reuse the `ParsedHotkey` parser and the key-capture logic from `handle_custom_hotkey_recording`, but with **separate state**: `recording_paste_next_hotkey: bool` and `pending_paste_next_hotkey: String`. Only one recorder can be active at a time; starting one cancels the other.
- Changes take effect through `update_hotkeys_from_settings()`, which re-registers both actions.

### 8.3 Shortcut inventory

| Scope | Action | Bindings | Notes |
|---|---|---|---|
| Global | Toggle Window | Preset: `Cmd/Ctrl+Shift+V` (default), `Cmd/Ctrl+ù` **and** `Cmd/Ctrl+Shift+ù` (AzertyQuote registers 2), `Cmd/Ctrl+Shift+C`, `Option/Alt+Space`, `Cmd/Ctrl+Space`, `Disabled`, or `Custom` | Coalesced |
| Global | Paste Next | `Cmd+Option+V` / `Ctrl+Alt+V` (default) or recorded | Lossless; registered only when enabled |
| OS | Paste | `Cmd+V` / `Ctrl+V` | Must never be grabbed |
| In-app (focused window only) | Navigation and actions | `Esc`, `Enter`, `↑/↓`, `Tab/Shift+Tab`, `Ctrl+N/P/J/K`, `Delete`, `Cmd+Backspace`, `Ctrl+Backspace`, `Ctrl+D`, `Cmd/Ctrl+1…9`, `Cmd/Ctrl+Q` | A global grab would shadow these system-wide |

### 8.4 Conflict rules
- Add `fn active_toggle_hotkeys(&self) -> Vec<HotKey>` (expands presets via `to_hotkeys()` or parses `custom_hotkey`) and `fn paste_next_hotkey(&self) -> Option<HotKey>`.
- **Hard conflict (reject):** if `paste_next_hotkey().id()` equals **any** id in `active_toggle_hotkeys()`, the change is refused in whichever recorder or preset selector caused it. Show an inline error and keep the previous value. Checked on Paste Next record or enable, on Toggle preset change, and on Toggle custom record.
- **Reject:** a bare key without modifiers, and exactly `Cmd+V` / `Ctrl+V` (the OS paste).
- **Soft conflict (warn, allow):** bindings that shadow in-app shortcuts (`Cmd/Ctrl+Q`, `Cmd/Ctrl+1…9`, `Ctrl+N/P/J/K/D`) or common OS/app combos (`Cmd+Space`, `Cmd+Shift+V` "paste and match style"). Explain that global hotkeys are grabbed system-wide.
- Registration failure from the OS (already taken by another app) is shown per action with the existing registration-status widget.

---

## 9. Platform notes
- **macOS:** the simulated paste requires **Accessibility** permission (same as `auto_paste`). The modifier-state check uses `CGEventSourceFlagsState`. Follow the AGENTS invariants: guard `NSApplication` calls with `is_main_thread()`, wrap ObjC calls in `autoreleasepool`, and do no window mutation (Paste Next touches no windows).
- **Windows:** `SendInput` fails into elevated (Administrator) targets. Reuse the existing detection and report `Failed { reason: "target elevated" }`.
- **Linux:** X11 via XTest. On Wayland without X11 the simulated paste is unavailable: report `Failed` and show a one-time note in Settings.

---

## 10. Acceptance criteria

**Enablement**
- [ ] AC-1: With `paste_next_enabled = false` (default), the Paste Next hotkey is not registered, and pressing it reaches the focused app unchanged.
- [ ] AC-2: Old settings JSON without the new fields loads with `paste_next_enabled = false` and the platform default hotkey.

**Ordering and delivery**
- [ ] AC-3: After copying `A`, `B`, `C`, three presses paste `C`, `B`, `A` into the focused app, in order, with the window never shown.
- [ ] AC-4: Ten presses in under one second each run, in order (no drops, no merging). Verified with a unit test on the worker queue using a mock paste backend.
- [ ] AC-5: Toggle Window behaviour (coalescing, focus capture) is unchanged.
- [ ] AC-6: A Paste Next press while an auto-paste from the window is in progress never interleaves keystrokes (shared critical section).

**Correctness of pasted content**
- [ ] AC-7: On Windows and Linux, holding `Ctrl+Alt` from the hotkey never results in `Ctrl+Alt+V` reaching the target, and never re-triggers Paste Next.
- [ ] AC-8: Paste Next writes are not re-inserted into history, do not promote duplicates, and do not reset the cursor (text and image).
- [ ] AC-9: Image entries paste correctly, and oversized entries (above monitor limits) are skipped with a logged warning.

**Secrets**
- [ ] AC-10: Pasting a `Secret` sets the concealed marker, is skipped by the monitor, and schedules auto-clear per `auto_clear_secret_secs`. Consecutive pastes restart the timer, and a following non-secret paste cancels it.
- [ ] AC-11: No clipboard or secret content appears in release-build logs.

**Cursor rules**
- [ ] AC-12: A new external copy, an in-app re-copy, or a history clear resets the cursor to `0`. Search/filter refreshes do not.
- [ ] AC-13: At end of history a press does nothing and produces no keystroke.
- [ ] AC-14: A failed write or simulate does not advance the cursor and clears pending skip markers.
- [ ] AC-15: Deletes and prunes clamp the cursor, and indexing always uses full, current history (fresh read at press time), even while hidden.
- [ ] AC-16: The `NEXT` badge follows the entry by id. When the entry is filtered out, the header chip is shown instead.

**Settings and conflicts**
- [ ] AC-17: Recording a Paste Next binding equal to any active Toggle hotkey id (including both AzertyQuote variants and Custom) is rejected with an inline error. The reverse (changing Toggle to collide with Paste Next) is rejected too.
- [ ] AC-18: `Cmd+V` / `Ctrl+V` and modifier-less keys are rejected. Shadowing in-app shortcuts produces a warning.
- [ ] AC-19: The Paste Next and Toggle custom recorders keep independent state, and starting one cancels the other.

**Engineering invariants**
- [ ] AC-20: `unsafe` code exists only in `src/platform/macos.rs` / `src/platform/windows.rs`. `platform/mod.rs` and `platform/linux.rs` remain safe.
- [ ] AC-21: All new threads are named, handle `Disconnected`, and are joined on shutdown. No hidden-window repaint timers are added.
- [ ] AC-22: `cargo fmt -- --check`, `cargo clippy --all-targets --all-features -- -D warnings` (Linux, macOS, Windows CI) and `cargo test --all-targets` pass.

---

## 11. Out of scope / future
- **App lock:** if a lock or re-authentication feature is added later, Paste Next must do nothing while locked. That requirement depends on the lock feature and is not part of this work.
- FIFO mode, wrap-around, destructive "pop" mode, and passive `Cmd+V` interception (event taps / keyboard hooks).
