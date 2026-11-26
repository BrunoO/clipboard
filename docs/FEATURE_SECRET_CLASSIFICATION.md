# Feature Document: Manual Secret Classification ("Mark as Secret")

> **Status:** Implemented (Baseline + Keyboard Shortcut `Cmd/Ctrl+L`)  
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

Manual secret conversion is implemented in the codebase:

1. **UI Trigger (`src/app/history_view.rs`):**
   - `render_secret_header_buttons` renders a lock icon button (`icons::LOCK` / `icons::LOCK_OPEN`) on every non-image entry (`item.kind != ItemKind::Image`).
   - Tooltip: `"Mark as Secret (mask content, {MOD}+L)"` when unmarked; `"Unmark as Secret ({MOD}+L)"` when already `ItemKind::Secret` (interpolating `platform::PRIMARY_MODIFIER_NAME`).
   - For `ItemKind::Secret` entries, it also renders an eye toggle (`icons::EYE` / `icons::EYE_SLASH`) to temporarily reveal/mask content in the UI (`revealed_secrets` set, cleared on `hide_window`).

2. **Action Dispatch (`src/app/history_view.rs`):**
   - Clicking the lock button or pressing `Cmd+L` (macOS) / `Ctrl+L` (Windows/Linux) emits `ItemAction::ToggleSecret(item.id, !is_secret)`.
   - `dispatch_item_action` calls `self.db.set_item_secret(id, make_secret)`, unconditionally removes the item ID from `revealed_secrets`, refreshes the item list, and shows a confirmation toast.

3. **Database Mutation (`src/db.rs`):**
   - `Database::set_item_secret(id, is_secret)` ignores `image` rows, reads `content`, and updates `kind`:
     - When `is_secret == true`: sets `kind = 'secret'`.
     - When `is_secret == false`: re-runs classification (`is_valid_url` → `'url'`, `parse_color` → `'color'`, `detect_code_language` → `'code'`, otherwise `'text'`).

4. **Security Protections Applied to `ItemKind::Secret`:**
   - **UI masking:** `render_secret_content` masks content with bullets (`•`) unless explicitly revealed.
   - **Clipboard copy (`src/app/mod.rs`):** `copy_secret_to_clipboard` sets `db.skip_next_capture(secret)`, calls `platform::mark_clipboard_concealed()`, wraps the value in `zeroize::Zeroizing` for `pending_clipboard_clear`, and schedules `auto_clear_secret_secs`.
   - **Retention:** Controlled independently via **Pinning** (`is_pinned`); unpinned secrets age out with history pruning and `Clear All`, while pinned secrets are preserved.

---

## 4. UX & Discoverability Enhancements

1. **Keyboard Shortcut for Selected Entry (Implemented):**
   - `Cmd+L` (macOS) or `Ctrl+L` (Windows/Linux) in history navigation toggles the secret status on the currently selected entry (`self.selected_index`).
   - Surfaced dynamically in the lock button's hover tooltip via `{PRIMARY_MODIFIER_NAME}+L`.

2. **Defensive Revealed-State Invalidation (Implemented):**
   - In `dispatch_item_action`, `revealed_secrets.remove(&id)` is executed unconditionally whenever secret status toggles, preventing any newly marked secret from inadvertently remaining revealed.
   - All revealed state continues to be wiped on window hide (`hide_window`).

3. **Card Context Menu / Action Palette Integration (Future):**
   - When a right-click context menu or `Cmd/Ctrl+T` action palette is added for entries (see [FEATURE_TEXT_TRANSFORMATIONS.md](FEATURE_TEXT_TRANSFORMATIONS.md)), include **"Mark as Secret"** / **"Unmark as Secret"** under an *Entry / Security* section so users do not have to rely solely on the small header lock icon or shortcut.

4. **Custom Secret Rules / Patterns (Future):**
   - Optional user-defined prefix or regex patterns in Settings for organization-specific tokens that `secret::is_likely_secret` cannot know ahead of time.
