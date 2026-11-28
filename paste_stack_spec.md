# Specification: Paste Stack ("Paste Next" Hotkey)

> Status: **Draft v3.** Updated with LIFO / FIFO stack modes and the id-based `NEXT` marker (§7), matching the current module layout (`src/app/*`, `src/platform/*`) and addressing hotkey delivery, paste dispatch, image skip, secret handling, and modifier handling.
>
> Required reading before implementing: [`AGENTS.md`](AGENTS.md), [`docs/SECURITY_LESSONS.md`](docs/SECURITY_LESSONS.md), [`docs/MULTIPLATFORM_LESSONS.md`](docs/MULTIPLATFORM_LESSONS.md), [`docs/CONCURRENCY_AND_PERF_LESSONS.md`](docs/CONCURRENCY_AND_PERF_LESSONS.md).

---

## 1. Overview

**Paste Stack** is always on out of the box via a dedicated global **Paste Next** hotkey (and can be turned off at any time by clearing the hotkey in Settings). The user copies several entries in one application, switches to another application, and presses **Paste Next** repeatedly. By default each press pastes the next entry in **LIFO** order (most recent first) into the focused application; placing the `NEXT` marker on an older entry switches to **FIFO** order (oldest-from-marker to newest) until the newest entry is pasted (§7). The `CipherClipboard` window is never shown.

### 1.1 Design decisions

| Topic | Decision |
|---|---|
| Trigger | A dedicated global hotkey, separate from the Toggle Window hotkey. Plain `Cmd+V` / `Ctrl+V` is never intercepted. |
| Order | **LIFO** by default (newest → older). Moving the `NEXT` marker to an older entry switches to **FIFO** (older → newer), automatically returning to LIFO once the newest entry is pasted (§7). |
| Mode | Non-destructive id-based marker (`next_marker: Option<i64>`) + `mode: StackMode { Lifo, Fifo }`. History is never modified by Paste Next. |
| Lifecycle | The marker is always armed. Any new captured copy resets to LIFO with the marker on that new entry (no Reset button). In LIFO, pressing past the oldest entry does nothing (no wrap-around). |
| Activation | Always on out of the box — no separate enable toggle. The hotkey binding itself is the on/off switch: clearing `paste_next_hotkey` (`""`) unregisters the hotkey and hides the Paste Next UI. |
| Default hotkey | `Cmd+Option+V` (macOS) / `Ctrl+Alt+V` (Windows/Linux). Can be re-recorded or cleared. |
| Secrets | `ItemKind::Secret` entries are included, with the same protections as the existing secret-copy flow (§6). |
| Payloads | Text (all text kinds: `Text`, `Url`, `Code`, `Color`, `Secret`) and `Image`. |
| App lock | Not applicable. The codebase has **no app-lock feature** (`src/password.rs` is a password *generator*). See §11. |

### 1.2 Typical workflow

1. In App A the user copies `A`, then `B`, then `C`. History is `[C, B, A, …]`, mode is `LIFO ↓`, and `NEXT` sits on `C`.
2. In App B the user presses Paste Next three times. `C`, `B`, `A` are pasted in that order.
3. A 4th press after the oldest entry has been pasted does nothing.
4. Copying `D` anywhere resets to `LIFO ↓` with `NEXT` on `D`.
5. Alternatively, placing `NEXT` on `A` via "Paste next from here" switches to `FIFO ↑`; pressing three times pastes `A`, `B`, `C` and automatically returns to `LIFO ↓` with `NEXT` on `C`.

---

## 2. Module touchpoints

| Area | File(s) | Change |
|---|---|---|
| Settings model | [`src/settings.rs`](src/settings.rs) | New `paste_next_hotkey` field with serde default, default hotkey helper, conflict helper (§8). |
| Settings UI | [`src/app/settings_view.rs`](src/app/settings_view.rs) | Paste Next group: recorder (Record / Clear / Reset to default), conflict warnings, registration status (§8). |
| Hotkey routing | [`src/hotkey.rs`](src/hotkey.rs) (including `linux_x11_fallback`) | Tag events with `HotkeyAction`, add a lossless Paste Next channel (§3). |
| Wiring | [`src/main.rs`](src/main.rs) | Create the Paste Next channel and pass it to `HotkeyService::new` / `App`. |
| App state and handlers | [`src/app/mod.rs`](src/app/mod.rs) | `next_marker`, `mode: StackMode`, reset rules, Paste Next handler, job submission and result handling (§4, §7). |
| History UI | [`src/app/history_view.rs`](src/app/history_view.rs) | `NEXT` badge, `LIFO ↓` / `FIFO ↑` mode chip, and "Paste next from here" row action + `Cmd/Ctrl+Shift+M` shortcut (§7.6). |
| Paste dispatch | [`src/platform/mod.rs`](src/platform/mod.rs) (safe) | New ordered `paste-next-worker` with an OS-agnostic API (§4). |
| OS keystrokes | [`src/platform/macos.rs`](src/platform/macos.rs), [`src/platform/windows.rs`](src/platform/windows.rs), [`src/platform/linux.rs`](src/platform/linux.rs) | Modifier-safe paste simulation (§5). `unsafe` only in `macos.rs` / `windows.rs`. |
| Image fingerprint | [`src/media.rs`](src/media.rs) (moved from [`src/monitor.rs`](src/monitor.rs)) | Shared `pub(crate) fn image_fingerprint(...)` (§6.1). |
| Skip markers | [`src/db.rs`](src/db.rs), [`src/monitor.rs`](src/monitor.rs) | Unified `SkipCaptureTarget` (`Text` / `Image`) single-slot skip API. Monitor honours it before insert and notify (§6.1). |

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
- In `App::logic()` (background wake handlers, per AGENTS), drain the Paste Next receiver **one event at a time**. Each event becomes one call to `handle_paste_next()` (§7.3), never `while try_recv() { requested = true }`.
- Handle `Disconnected` on both channels. Shutdown must still join the listener thread as it does today.

### 3.3 Registration
- `HotkeyService::new` / `update_hotkeys` take the toggle hotkeys and an `Option<HotKey>` for Paste Next (`None` when `paste_next_hotkey` is empty/cleared or unparseable). Registration results are reported per action so the Settings UI can show status separately.
- When `paste_next_hotkey` is cleared (`""`), the Paste Next hotkey is **not registered** at all, so no system-wide grab happens.

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
The **clipboard write and the simulated keystroke run inside the same job, strictly in submission order**. This prevents press N+1 from overwriting the clipboard before press N's paste has run. For each job, in order:

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

### 6.1 Unified `SkipCaptureTarget` (text, images, and future types)
- Replace the text-only `transient_text` slot in [`src/db.rs`](src/db.rs) with a **single unified slot** holding:
  ```rust
  pub(crate) enum SkipCaptureTarget {
      Text(zeroize::Zeroizing<String>),
      Image(u64), // image_fingerprint(width, height, rgba)
  }
  ```
  exposed via `db.skip_next_capture(target)`, `db.take_if_skipped(candidate: &SkipCaptureTarget) -> bool`, and `db.clear_skip_capture()`. Using one enum guarantees that setting a new skip target replaces any previous one, `clear_skip_capture()` clears all types in one call, and adding future clipboard payload types is compiler-checked via exhaustive `match`.
- **Set inside the worker, not at submit time:** because jobs are serialized and there is one slot, the worker calls `db.skip_next_capture(...)` **inside the job immediately before writing to the clipboard** (via a `Database` handle or callback). This prevents queued jobs from overwriting each other's markers.
- **Text (all text kinds):** when the monitor sees a text write matching `SkipCaptureTarget::Text`, `take_if_skipped` swallows it: it is not re-inserted, does not move to the top, does not notify, and does **not reset the marker or mode**.
- **Images:** move `compute_image_fingerprint` from [`src/monitor.rs`](src/monitor.rs) into [`src/media.rs`](src/media.rs) as `pub(crate) fn image_fingerprint(width, height, rgba) -> u64` so monitor and writer share one implementation. In [`src/monitor.rs`](src/monitor.rs), check `take_if_skipped(&SkipCaptureTarget::Image(fp))` **before** `rgba_to_png` / `insert_image`. When it matches: update `last_copied_image_hash` and `change_token` (so the fast path stays consistent), clear `last_copied_text`, and **do not insert, promote, or notify**.

### 6.2 Secrets (`ItemKind::Secret`)
A secret pasted by Paste Next gets the **same protections** as `copy_secret_to_clipboard` in [`src/app/mod.rs`](src/app/mod.rs):
1. Skip capture (§6.1).
2. Mark the clipboard as concealed (`platform::mark_clipboard_concealed()`), done in the worker right after the write (`conceal: true`).
3. Schedule auto-clear according to `auto_clear_secret_secs`, via the existing `schedule_secret_clipboard_clear`, triggered on `Pasted` outcome (never on `Failed`). Each secret paste **restarts** the pending clear, so an older timer cannot wipe the clipboard in the middle of a sequence. A non-secret `Pasted` outcome cancels any pending secret clear, because the clipboard no longer holds that secret.
4. The secret is held in `zeroize::Zeroizing<String>` from DB read until job completion.
5. No window show/hide, no focus change. Toasts are recorded only if the window is visible (see §7.4).
6. Never log secret or clipboard contents in release builds.

---

## 7. Stack modes (LIFO / FIFO), NEXT marker, and execution

### 7.1 Rules
1. **LIFO is the default.** The `NEXT` marker sits on the newest entry. Each press pastes the marker's entry, then moves the marker to the next *older* entry. When the oldest entry has been pasted, `next_marker` becomes `None` and further presses do nothing.
2. **Moving the marker to an older entry switches to FIFO.** Each press pastes the marker's entry, then moves the marker to the next *newer* entry.
3. **FIFO switches back to LIFO automatically** once the newest entry has been pasted, placing `next_marker` back on the newest entry. (Stepping *onto* the newest entry during a FIFO run stays in `FIFO ↑` until that newest entry is actually pasted.)
4. **Any new captured copy resets to LIFO**, with the marker on that new entry. There is no Reset button.
   - Paste Next's own clipboard writes are skipped by the monitor and never reset.
   - Concealed copies that the monitor ignores never reset.
   - Non-clipboard DB inserts (e.g. `save_generated_secret`) do not reset; the marker stays on its entry by id.
   - Search, filter and other UI refreshes never reset.
5. **Manually moving the marker onto the newest entry** (via "Paste next from here") sets mode to **LIFO**.
6. **Out of scope:** LIFO starting from an older entry. Moving the marker to any older entry always means FIFO.

### 7.2 State
- `next_marker: Option<i64 /* history id */>` and `mode: StackMode { Lifo, Fifo }` in `App` ([`src/app/mod.rs`](src/app/mod.rs)), initialised to `StackMode::Lifo` with `next_marker` pointing to the newest entry (or `None` when history is empty).
- The marker is tracked by id, so deletions above it or non-clipboard inserts cannot shift it. At each press, resolve the id's position from a fresh read of the full, unfiltered history.
- `paste_next_seq: u64`, plus `in_flight: VecDeque<(u64 /*seq*/, i64 /*submitted_id*/, StackMode)>` used to step queued presses from the last *submitted* position and reconcile outcomes.

### 7.3 Handling a Paste Next press (`handle_paste_next`)
1. If `settings.paste_next_hotkey()` is `None` (cleared), ignore the press. (The hotkey shouldn't be registered anyway; this is defence in depth.)
2. **Read from the full, unfiltered history at press time.** Never index the UI's filtered `items` list, which may be stale while hidden or narrowed by search.
3. Resolve the target entry in the current step direction (`Lifo` → older, `Fifo` → newer):
   - If `in_flight` is empty, the target is `next_marker`.
   - If `in_flight` is non-empty, step one entry in the current direction from the last submitted entry in `in_flight`.
   - If there is no next entry in the current direction (past the oldest entry in LIFO, past the newest entry in a FIFO in-flight burst, or empty history), the press does nothing. (An in-flight burst in FIFO stops at the newest entry rather than wrapping immediately into LIFO, preventing accidental double-pastes; once the newest entry completes and switches to `LIFO ↓`, a subsequent press pastes it in LIFO.)
4. Enforce existing size limits (text ≤ monitor `MAX_TEXT_LENGTH`, images ≤ `MAX_IMAGE_PIXELS`, decoded via `media::png_to_rgba`). Skip oversized entries with a logged warning and treat them as `Failed`.
5. Build the `PasteNextJob` (decode the PNG to RGBA for images), push `(seq, target_id, mode)` to `in_flight`, and call `submit_paste_next`.

### 7.4 Outcome handling (in `App::logic()`)
- `Pasted { seq }`: pop the matching `in_flight` entry and advance `next_marker` relative to the **completed job's `submitted_id`** (not the prior `next_marker`, so an earlier failed job cannot cause a double-paste):
  - In `Lifo`: move `next_marker` to the next older entry after `submitted_id` (or `None` if `submitted_id` was the oldest).
  - In `Fifo`: if `submitted_id` was the newest entry, switch to `StackMode::Lifo` with `next_marker` on the newest entry (Rule 3); otherwise move `next_marker` to the next newer entry (remaining in `Fifo`).
  - Also trigger `schedule_secret_clipboard_clear` for secrets or cancel any pending secret clear for non-secrets (§6.2).
- `Failed { seq, .. }`: pop the entry and **leave `next_marker` where it is** (do not advance). Clear any pending skip marker (`clear_skip_capture`), log the reason without content, and record a toast if the window is visible. Jobs already queued behind it keep their own submitted targets, so a failure means that entry is not pasted (no automatic retry). The user can press again.
- `Failed` with reason "Accessibility permission missing" (macOS) shows the existing auto-paste permission hint the next time the window is shown.

### 7.5 Reset and validity rules

| Event | Marker & mode effect |
|---|---|
| New external capture (monitor notify with a new or promoted id) | Reset to `StackMode::Lifo`, `next_marker = Some(new_id)`, clear `in_flight` reconciliation state |
| **In-app re-copy** of a history item (`copy_text_to_clipboard`, `copy_image_to_clipboard`, generator copies not skipped) | Treated like an external copy: the monitor promotes the item to the top, notifies, and resets to `StackMode::Lifo` with `next_marker` on that entry |
| Programmatic Paste Next write (skipped per §6) | No reset |
| Ignored concealed copy (ignored by monitor) | No reset |
| Non-clipboard DB insert (e.g. `save_generated_secret`) | No reset; `next_marker` stays on its entry by id |
| History clear (`db.clear()` in the clear-confirm modal, which preserves pinned items and refreshes without a notify) | Explicit reset to `StackMode::Lifo`, `next_marker` set to the newest remaining entry (e.g. a surviving pinned item) or `None` if empty |
| Single delete / prune | If `next_marker`'s entry is deleted, move `next_marker` to the next entry in the current direction (in FIFO, landing on the newest entry stays `Fifo` until pasted). If there is no next entry in the current direction, apply Rule 1 (`next_marker = None` in LIFO) or Rule 3 (switch to `StackMode::Lifo` on the newest remaining entry, or `None` if empty). Deleting any other entry leaves `next_marker` on its id. |
| `refresh_items()` for search/filter or other UI reloads | **No change** |
| Paste Next hotkey cleared → set | Reset to `StackMode::Lifo`, `next_marker` on the newest entry (or `None` if empty) |
| While hotkey is cleared | Reset rules still apply (cheap), so setting a hotkey starts from a consistent state |

### 7.6 UI (window visible)
- **Directional `NEXT` pill on the marked card** ([`src/app/history_view.rs`](src/app/history_view.rs)): render a pill badge (matching `render_pill_tag`) on the card whose history id equals `next_marker` (mapped by **id**, never by visible row index):
  - `↓ NEXT` in accent color when in `StackMode::Lifo` (newest → older).
  - `↑ NEXT` in a distinct secondary accent color (e.g. amber/emerald) when in `StackMode::Fifo` (older → newer), so the mode flip is immediately visible.
  - Hover tooltip explains the active Paste Next hotkey and direction (e.g. *"Next to paste with ⌘⌥V — pasting downward (newest → oldest)"* / *"pasting upward (oldest → newest, then back to LIFO)"*).
- **Header mode chip:** show a compact `LIFO ↓` or `FIFO ↑` chip in the header (with a "hidden by filter" cue when a search filter hides the marked card). Clicking the chip scrolls to the marked card if visible. There is **no Reset button** (Rule 4).
- **Moving the marker (Options A + D — hover button + context menu + keyboard shortcut):**
  - **1-click hover button (Option A):** hovering (or selecting) any non-marked card reveals a subtle icon button in the card header alongside Pin / Edit / Delete. It previews the direction that clicking will trigger (`↓` on the newest entry → LIFO; `↑` on any older entry → FIFO) with tooltip *"Paste next from here (`Cmd/Ctrl+Shift+M`)"*. Clicking it moves `next_marker` there immediately.
  - **Context menu & keyboard shortcut (Option D):** right-click menu entry **"Paste next from here"** on every card, plus `Cmd/Ctrl+Shift+M` for the currently selected card.
  - Placing the marker on a visible card while a search filter is active is allowed; the run still walks the full, unfiltered history (including rows hidden by the filter).
- **Hidden when cleared:** render no `NEXT` pill, header chip, hover button, or context menu item when `paste_next_hotkey` is cleared (`None`).

---

## 8. Settings, recorder and hotkey coherence

### 8.1 New `AppSettings` field ([`src/settings.rs`](src/settings.rs))
```rust
#[serde(default = "default_paste_next_hotkey")]   // "Cmd+Option+V" (macOS) / "Ctrl+Alt+V" (others)
pub(crate) paste_next_hotkey: String,
```
There is no separate `paste_next_enabled` boolean: an empty string `""` means cleared/disabled, and any valid binding is active. Existing persisted settings (stored through the DB app-settings row) must load with the platform default hotkey. Add a round-trip test with an old JSON payload.

### 8.2 Recorder UI ([`src/app/settings_view.rs`](src/app/settings_view.rs))
- Inside the **Global Shortcuts** card (which already uses a single keycap-pill recorder row for **Toggle Window** with **Clear / Reset / Type**, no preset radio list), add an identical second row for **Paste Next**: the current binding (or "Disabled — click to record…" when cleared), **Clear / Reset / Type**, a conflict warning, and registration status.
- Reuse the `ParsedHotkey` parser, `render_hotkey_recorder_box_contents`, and the key-capture logic from `handle_toggle_hotkey_recording`, with **separate state**: `recording_paste_next_hotkey: bool`, `manual_paste_next_hotkey_edit: bool`, and `pending_paste_next_hotkey: String`. Only one recorder can be active at a time; starting one cancels the other.
- Changes take effect through `update_hotkeys_from_settings()`, which re-registers both actions.

### 8.3 Shortcut inventory

| Scope | Action | Bindings | Notes |
|---|---|---|---|
| Global | Toggle Window | `Cmd+Shift+V` / `Ctrl+Shift+V` (default), recorded, or cleared (`""`) | Coalesced; registered whenever non-empty |
| Global | Paste Next | `Cmd+Option+V` / `Ctrl+Alt+V` (default), recorded, or cleared (`""`) | Lossless; registered whenever non-empty |
| OS | Paste | `Cmd+V` / `Ctrl+V` | Must never be grabbed |
| In-app (focused window only) | Navigation and actions | `Esc`, `Enter`, `Shift+Enter`, `↑/↓`, `Tab/Shift+Tab`, `Ctrl+N/P/J/K`, `Delete`, `Cmd+Backspace`, `Ctrl+Backspace`, `Ctrl+D`, `Cmd/Ctrl+1…9`, `Cmd/Ctrl+L/E/N/Q`, `F2`, `Cmd/Ctrl+Shift+M` ("Paste next from here") | A global grab would shadow these system-wide |

### 8.4 Conflict rules
- Add `fn toggle_hotkey_parsed(&self) -> Option<HotKey>` (or reuse `effective_hotkeys()`) and `fn paste_next_hotkey(&self) -> Option<HotKey>`.
- **Hard conflict (reject):** if `paste_next_hotkey().id()` equals `toggle_hotkey_parsed().id()`, the change is refused in whichever recorder caused it. Show an inline error and keep the previous value. Checked on Paste Next record / manual save / reset-to-default, and on Toggle Window record / manual save / reset-to-default. On startup/upgrade, if a user's existing `toggle_hotkey` equals the default `paste_next_hotkey`, Toggle Window wins and Paste Next stays unregistered with the conflict warning shown in Settings.
- **Reject:** a bare key without modifiers, and exactly `Cmd+V` / `Ctrl+V` (the OS paste).
- **Soft conflict (warn, allow):** bindings that shadow in-app shortcuts (`Cmd/Ctrl+Q`, `Cmd/Ctrl+1…9`, `Ctrl+N/P/J/K/D`, `Cmd/Ctrl+Shift+M`) or common OS/app combos (`Cmd+Space`, `Cmd+Shift+V` "paste and match style"). Explain that global hotkeys are grabbed system-wide.
- Registration failure from the OS (already taken by another app) is shown per action with the existing registration-status widget.

---

## 9. Platform notes
- **macOS:** the simulated paste requires **Accessibility** permission (same as `auto_paste`). The modifier-state check uses `CGEventSourceFlagsState`. Follow the AGENTS invariants: guard `NSApplication` calls with `is_main_thread()`, wrap ObjC calls in `autoreleasepool`, and do no window mutation (Paste Next touches no windows).
- **Windows:** `SendInput` fails into elevated (Administrator) targets. Reuse the existing detection and report `Failed { reason: "target elevated" }`.
- **Linux:** X11 via XTest. On Wayland without X11 the simulated paste is unavailable: report `Failed` and show a one-time note in Settings.

---

## 10. Acceptance criteria

**Activation and defaults**
- [ ] AC-1: By default, `paste_next_hotkey` is registered (`Cmd+Option+V` / `Ctrl+Alt+V`) and Paste Next works out of the box. Clearing `paste_next_hotkey` unregisters it (no system-wide grab; the keystroke reaches the focused app unchanged) and hides the Paste Next UI.
- [ ] AC-2: Old settings JSON without `paste_next_hotkey` loads with the platform default hotkey.

**Ordering and delivery**
- [ ] AC-3: After copying `A`, `B`, `C`, three presses paste `C`, `B`, `A` into the focused app, in order, with the window never shown.
- [ ] AC-4: Ten presses in under one second each run, in order (no drops, no merging). Verified with a unit test on the worker queue using a mock paste backend.
- [ ] AC-5: Toggle Window behaviour (coalescing, focus capture) is unchanged.
- [ ] AC-6: A Paste Next press while an auto-paste from the window is in progress never interleaves keystrokes (shared critical section).

**Correctness of pasted content**
- [ ] AC-7: On Windows and Linux, holding `Ctrl+Alt` from the hotkey never results in `Ctrl+Alt+V` reaching the target, and never re-triggers Paste Next.
- [ ] AC-8: Paste Next writes are not re-inserted into history, do not promote duplicates, and do not reset the marker or mode (text and image).
- [ ] AC-9: Image entries paste correctly, and oversized entries (above monitor limits) are skipped with a logged warning.

**Secrets**
- [ ] AC-10: Pasting a `Secret` sets the concealed marker, is skipped by the monitor, and schedules auto-clear per `auto_clear_secret_secs`. Consecutive pastes restart the timer, and a following non-secret paste cancels it.
- [ ] AC-11: No clipboard or secret content appears in release-build logs.

**Marker and mode rules**
- [ ] AC-12: A new external copy, an in-app re-copy, or a history clear resets to LIFO with the marker on the newest entry (or `None` when empty). Search/filter refreshes do not.
- [ ] AC-13: In LIFO, after the oldest entry has been pasted, further presses do nothing and produce no keystroke.
- [ ] AC-14: A failed write or simulate does not advance the marker and clears pending skip markers.
- [ ] AC-15: Deleting the marked entry moves the marker to the next entry in the current direction (or applies §7.1 rule 1 / rule 3 if none), and resolution always uses full, current history (fresh read at press time), even while hidden.
- [ ] AC-16: The `NEXT` badge follows the entry by id. When the entry is filtered out, the header indicates it.

**Settings and conflicts**
- [ ] AC-17: Recording (or manually entering / resetting) a Paste Next binding equal to the active Toggle Window hotkey id is rejected with an inline error. The reverse (changing Toggle Window to collide with Paste Next) is rejected too.
- [ ] AC-18: `Cmd+V` / `Ctrl+V` and modifier-less keys are rejected. Shadowing in-app shortcuts produces a warning.
- [ ] AC-19: The Paste Next and Toggle Window recorders keep independent state, and starting one cancels the other.

**Engineering invariants**
- [ ] AC-20: `unsafe` code exists only in `src/platform/macos.rs` / `src/platform/windows.rs`. `platform/mod.rs` and `platform/linux.rs` remain safe.
- [ ] AC-21: All new threads are named, handle `Disconnected`, and are joined on shutdown. No hidden-window repaint timers are added.
- [ ] AC-22: `cargo fmt -- --check`, `cargo clippy --all-targets --all-features -- -D warnings` (Linux, macOS, Windows CI) and `cargo test --all-targets` pass.

**Stack modes (LIFO / FIFO) and NEXT marker**
- [ ] AC-23: Copy A, B, C, place the marker on A, and press 3×: A, B, C are pasted. Then the mode chip shows `LIFO ↓` and the marker is back on the newest entry.
- [ ] AC-24: A new copy during a FIFO run resets to LIFO, with the marker on the new entry.
- [ ] AC-25: Paste Next's own writes and ignored concealed copies never reset the mode or the marker.
- [ ] AC-26: The marker follows its entry by id across deletions above it and non-clipboard inserts (e.g. saved generated secrets). Deleting the marked entry moves the marker to the next entry in the current direction.
- [ ] AC-27: Placing the marker from a filtered view works, and the run walks the full history.

---

## 11. Out of scope / future
- **App lock:** if a lock or re-authentication feature is added later, Paste Next must do nothing while locked. That requirement depends on the lock feature and is not part of this work.
- LIFO starting from an older entry, wrap-around, destructive "pop" mode, and passive `Cmd+V` interception (event taps / keyboard hooks).
