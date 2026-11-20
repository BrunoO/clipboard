# Specification: Paste Stack ("Paste Next" Hotkey)

## 1. Overview
An optional **Paste Stack** feature for `CipherClipboard` that lets the user copy multiple entries sequentially in one application and paste them in **LIFO** (most-recent-first) order into another application using a dedicated global **"Paste Next"** hotkey, without opening the `CipherClipboard` window between pastes.

### Core Design Decisions
- **Trigger**: Dedicated global hotkey (separate from the main open-window shortcut).
- **Order**: **LIFO** (newest entry first, walking down history from index `0`).
- **Mode**: **Non-destructive cursor** (`stack_cursor: usize`)—entries remain in history.
- **Lifecycle**: Always-ready cursor that automatically resets to `0` on any new copy, and stops cleanly at the end of history.
- **Customization**: Enabled via an optional setting; defaults to `Cmd+Option+V` (macOS) / `Ctrl+Alt+V` (Windows/Linux), with a built-in key recorder so the user can record any custom shortcut.

---

## 2. User Experience & Behavior

### 2.1 Typical Workflow
1. User enables **Paste Next** in Settings (or uses it with the default/recorded shortcut).
2. In App A, user copies `A`, then `B`, then `C`. History (LIFO) is `[C, B, A, ...]`, and `stack_cursor` is `0`.
3. User switches to App B and presses the **Paste Next** hotkey:
   - **1st press**: Pastes `C` directly into App B; `stack_cursor` becomes `1`.
   - **2nd press**: Pastes `B` directly into App B; `stack_cursor` becomes `2`.
   - **3rd press**: Pastes `A` directly into App B; `stack_cursor` becomes `3`.
4. Pressing **Paste Next** when `stack_cursor >= items.len()` (end of history) is a **no-op** (does not wrap around).
5. Switching back to App A and copying `D` automatically resets `stack_cursor` to `0` (pointing at `D`).

### 2.2 Headless Execution
- Pressing **Paste Next** must **not** show or focus the `CipherClipboard` window.
- It writes the entry at `stack_cursor` to the OS clipboard and simulates `Cmd+V` (macOS) or `Ctrl+V` (Windows/Linux) directly into the currently focused application.

### 2.3 Security, Secrets & Lock State
- If `CipherClipboard` is currently **password-locked**, pressing **Paste Next** is a **no-op** so locked entries are never pasted while locked.
- When unlocked, **`ItemKind::Secret` entries are included** in the cursor walk just like any other entry (no automatic skipping).

### 2.4 Visual Feedback (When Window Is Open)
- Display a subtle **`NEXT`** badge on the entry at `stack_cursor` so the user can see what the next **Paste Next** press will paste.
- Provide a **"Reset cursor"** button/action in the header or status area when `stack_cursor > 0`.

---

## 3. Settings & Key Recording

### 3.1 New Settings in [`AppSettings`](src/settings.rs)
- `paste_next_enabled: bool` — **defaults to `false`** (opt-in, so no extra global shortcut is claimed until enabled).
- `paste_next_hotkey: String` — defaults to `"Cmd+Option+V"` on macOS / `"Ctrl+Alt+V"` on Windows/Linux.

### 3.2 Hotkey Recorder UI
- Add a **"Paste Next (Stack Mode)"** subsection under **Settings → Shortcuts** in [`src/app.rs`](src/app.rs).
- Reuse the existing [`ParsedHotkey`](src/settings.rs#L150-L155) parser and key-recording flow ([`handle_custom_hotkey_recording`](src/app.rs#L491)) so the user can click **Record** and press any modifier + key combination (or clear/reset to default).

### 3.3 Hotkey Coherence & Overlap Prevention
All global and in-app shortcuts must remain non-overlapping and coherent:

| Scope | Action | Default / Existing Bindings | Notes |
|---|---|---|---|
| Global | **Toggle Window** | `Cmd+Shift+V` / `Ctrl+Shift+V` (or preset: `Cmd+ù`, `Cmd+Shift+C`, `Option+Space`, `Cmd+Space`, or `Custom`) | Opens/hides `CipherClipboard` |
| Global | **Paste Next** | `Cmd+Option+V` / `Ctrl+Alt+V` (or user-recorded `Custom`) | Distinct from `Shift+V` (Toggle) and plain `Cmd+V` (OS paste) |
| In-App | Navigation & Actions | `Esc`, `Enter`, `Up`/`Down`, `Tab`/`Shift+Tab`, `Ctrl+N/P/J/K`, `Delete`/`Cmd+Backspace`/`Ctrl+D`, `Cmd+1..9`, `Cmd+Q` | Active only when window is focused |

- **Global-to-Global Conflict Check**: When enabling **Paste Next**, changing its recorded hotkey, or changing the **Toggle Window** preset/custom hotkey, compare the normalized `HotKey::id()` values of both shortcuts. If they match, reject/prevent the conflicting registration and display an inline warning in Settings.
- **In-App / OS Sanity Check**: Warn in Settings if a user records a Paste Next shortcut that shadows an in-app shortcut (e.g., `Cmd+Q`, `Cmd+1`–`9`) or OS paste (`Cmd+V` / `Ctrl+V`).

---

## 4. Technical Architecture & Touchpoints

### 4.1 Global Hotkey Routing ([`src/hotkey.rs`](src/hotkey.rs))
- Replace the `Sender<()>` channel with `Sender<HotkeyAction>`, where:
  ```rust
  pub(crate) enum HotkeyAction {
      ToggleWindow,
      PasteNext,
  }
  ```
- Map each registered `HotKey::id()` to its `HotkeyAction`.
- **Important**: On `HotkeyAction::PasteNext`, do **not** call `crate::platform::capture_active_window()`, since we are not switching focus to `CipherClipboard`.

### 4.2 Direct Paste Simulation ([`src/platform.rs`](src/platform.rs))
- Expose a helper (e.g., `simulate_paste_direct(delay_ms: u64)`) that waits briefly for the clipboard write to settle and invokes `macos::simulate_paste()` / `windows::simulate_paste()` without altering window focus.

### 4.3 Preventing Monitor Feedback Loops ([`src/db.rs`](src/db.rs) & [`src/monitor.rs`](src/monitor.rs))
- **Text**: Before writing text to `arboard::Clipboard`, call [`Database::skip_next_capture`](src/db.rs#L235) so [`start_clipboard_monitor`](src/monitor.rs#L38-L150) ignores the programmatic write and does not re-insert it or falsely reset `stack_cursor`.
- **Images**: Add an equivalent `skip_next_image_capture(fingerprint: u64)` / `take_if_image_skipped(fingerprint: u64)` in [`src/db.rs`](src/db.rs) using [`compute_image_fingerprint`](src/monitor.rs#L15-L29), so pasting image entries in the stack also avoids re-capture.

### 4.4 Cursor State & Event Handling ([`src/app.rs`](src/app.rs))
- Add `stack_cursor: usize` (initialized to `0`) to the app state.
- **Reset condition**: When `monitor_rx` receives a notification of a newly captured clipboard item (or when history is cleared), set `stack_cursor = 0`.
- **On `HotkeyAction::PasteNext`**:
  1. Return early if `paste_next_enabled` is `false` or the app is locked.
  2. Look up `items.get(self.stack_cursor)`. If `None` (end of stack), return early.
  3. Write the item's text or image to `arboard::Clipboard` and mark it skipped in `Database`.
  4. Call `platform::simulate_paste_direct(...)`.
  5. Increment `self.stack_cursor += 1`.

---

## 5. Edge Cases & Platform Notes
- **macOS Permissions**: Just like `auto_paste`, simulated `Cmd+V` requires **Accessibility** permission in *System Settings → Privacy & Security → Accessibility*.
- **Rapid Presses**: Because `arboard::Clipboard` write + `simulate_paste` happen sequentially, keep a small settle delay (or reuse `auto_paste_delay_ms`) so the target app reads the updated clipboard rather than the previous value.
- **History Deletion / Pruning**: If items are deleted or pruned while `stack_cursor > 0`, clamp `stack_cursor` safely (`stack_cursor = stack_cursor.min(items.len())`).
