# Feature Document: Manual Secret Classification ("Mark as Secret")

> **Status:** Implemented (Baseline) + Proposed UX Enhancements  
> **Date:** 2026-11-21  
> **Related:** [FEATURE_TEXT_TRANSFORMATIONS.md](FEATURE_TEXT_TRANSFORMATIONS.md)

---

## 1. Problem Statement

`CipherClipboard` automatically detects many sensitive entries at capture time via `crate::secret::is_likely_secret` (`src/secret.rs`)—covering PEM/PGP private keys, Bearer tokens, URLs with embedded credentials, known API key prefixes/JWTs, `.env` secret assignments, and high-entropy strings.

However, heuristic detection cannot catch every secret (e.g., short PINs, custom passphrases, internal identifiers, security answers, or domain-specific tokens). Users need a manual way to **promote any captured text entry to a Secret** (and reversibly unmark false positives).

---

## 2. Architectural Boundary: Classification vs. Transformation

Although "convert an entry to a secret" can feel similar to "convert an entry to raw text," **they belong to two distinct architectural layers**:

| Dimension | Content Transformations (`src/transform.rs`) | Secret Classification (`ToggleSecret`) |
|---|---|---|
| **What changes** | The **text payload** (`&str -> Result<String>`) | The **entry's `ItemKind` metadata** in `clipboard_history` (`kind = 'secret'`) |
| **Persistence** | Non-destructive by default (copies transformed text without mutating the DB row) | Persisted immediately in SQLCipher (reversible via Unmark) |
| **Security role** | Must *respect* `ItemKind::Secret` (e.g., routing transformed secret output through `copy_secret_to_clipboard`) | *Establishes* the `ItemKind::Secret` security boundary |

**Design takeaway:** Keep secret classification separate from `src/transform.rs`, while optionally surfacing *"Mark as Secret"* / *"Unmark as Secret"* in shared entry action menus (such as a card context menu or command palette) for discoverability.

---

## 3. Existing Implementation in `CipherClipboard`

Manual secret conversion is **already implemented** in the codebase:

1. **UI Trigger (`src/app/history_view.rs`):**
   - `render_secret_header_buttons` renders a lock icon button (`icons::LOCK` / `icons::LOCK_OPEN`) on every non-image entry (`item.kind != ItemKind::Image`).
   - Tooltip: `"Mark as Secret (mask content)"` when unmarked; `"Unmark as Secret"` when already `ItemKind::Secret`.
   - For `ItemKind::Secret` entries, it also renders an eye toggle (`icons::EYE` / `icons::EYE_SLASH`) to temporarily reveal/mask content in the UI (`revealed_secrets` set, cleared on `hide_window`).

2. **Action Dispatch (`src/app/history_view.rs`):**
   - Clicking the lock button emits `ItemAction::ToggleSecret(item.id, !is_secret)`.
   - `dispatch_item_action` calls `self.db.set_item_secret(id, make_secret)`, clears `revealed_secrets` when unmarking, refreshes the item list, and shows a confirmation toast.

3. **Database Mutation (`src/db.rs`):**
   - `Database::set_item_secret(id, is_secret)` ignores `image` rows, reads `content`, and updates `kind`:
     - When `is_secret == true`: sets `kind = 'secret'`.
     - When `is_secret == false`: re-runs classification (`is_valid_url` → `'url'`, `parse_color` → `'color'`, `detect_code_language` → `'code'`, otherwise `'text'`).

4. **Security Protections Applied to `ItemKind::Secret`:**
   - **UI masking:** `render_secret_content` masks content with bullets (`•`) unless explicitly revealed.
   - **Clipboard copy (`src/app/mod.rs`):** `copy_secret_to_clipboard` sets `db.skip_next_capture(secret)`, calls `platform::mark_clipboard_concealed()`, wraps the value in `zeroize::Zeroizing` for `pending_clipboard_clear`, and schedules `auto_clear_secret_secs`.
   - **Retention:** Protected from automatic history pruning when `preserve_secrets_on_prune` is enabled.

---

## 4. Proposed UX & Discoverability Enhancements

While the core functionality is complete, discoverability and keyboard-first ergonomics can be improved:

1. **Keyboard Shortcut for Selected Entry:**
   - Bind a shortcut in `App::ui()` (e.g., `Cmd/Ctrl+L` or `Shift+S` when the search box is not focused) to toggle secret status on `self.selected_index`.
   - Surface the shortcut in the lock button's hover tooltip.

2. **Card Context Menu / Action Palette Integration:**
   - When a right-click context menu or `Cmd/Ctrl+T` action palette is added for entries (see [FEATURE_TEXT_TRANSFORMATIONS.md](FEATURE_TEXT_TRANSFORMATIONS.md)), include **"Mark as Secret"** / **"Unmark as Secret"** under an *Entry / Security* section so users do not have to rely solely on the small header lock icon.

3. **Revealed-State Audit (Optional):**
   - Ensure `revealed_secrets` remains cleared on window hide (already done in `hide_window` in `src/app/mod.rs`) and consider clearing it on `ToggleSecret` when marking an entry *as* a secret (currently `revealed_secrets.remove(&id)` is only called when `!make_secret`, which is fine since newly marked secrets weren't in the set, though clearing unconditionally is harmless).

4. **Custom Secret Rules / Patterns (Future):**
   - Optional user-defined prefix or regex patterns in Settings for organization-specific tokens that `secret::is_likely_secret` cannot know ahead of time.
