# Implementation Plan: Encrypted Vault Annotations & Simple Password Manager

> **Objective:** Transform `CipherClipboard` into a lightweight, delightful, and highly secure personal secret manager and password vault by adding encrypted annotations, item pinning (immunity from pruning and bulk clearing), direct non-clipboard credential entry, and label-based search.

> **Implementation note (Option 2 — Decoupled Pin vs. Secret):** Retention and sensitivity are strictly orthogonal concepts. **Pin (`is_pinned`)** controls persistence (immunity from pruning and bulk `Clear All`, surfaced in the **Pinned** tab) and works on all item kinds (Text, Links, Code, Colors, Images, Secrets). **Secret (`ItemKind::Secret`)** controls sensitivity (masking, exclusion from plaintext search, concealed copy + clipboard auto-clear) and does not silently pin or unpin items. Manually created secrets (`+ Secret` / Password Forge `Save Secret`) are saved as pinned secrets by default. Legacy databases that used the `in_vault` column are migrated automatically to `is_pinned`.

---

## 1. Architectural Strategy & Security Invariants

### 1.1 Zero Overhead Encryption via SQLCipher
Because the SQLite database is encrypted at the page level using **SQLCipher (AES-256-CBC with HMAC-SHA512)**:
- Any new column added to `clipboard_history` (e.g. `label TEXT`, `notes TEXT`) is **automatically encrypted on disk**.
- No application-level ciphertext management or bespoke key derivation is needed.
- In-memory representations of secret notes and values will be protected and zeroized on drop.

### 1.2 Confidentiality During Search
- **The Invariant:** Plaintext secret values MUST NEVER be exposed to raw substring queries or SQL `LIKE` wildcard scans.
- **The Design:** The search query will match against `label LIKE ?` and `notes LIKE ?`. For `ItemKind::Secret`, the encrypted secret `content` is excluded from search pattern matching, but its **annotation** is searchable. Searching `"github"` or `"vpn"` will instantly locate the card, while the secret itself remains masked (`•••• •••• •••• ••••`) until explicitly copied or revealed.

### 1.3 Direct Non-Clipboard Entry
- **The Threat:** Background clipboard snooping by malicious or untrusted applications running on the user's desktop.
- **The Solution:** A dedicated "New Vault Entry" modal dialog that writes credentials directly to the encrypted database without passing through the OS clipboard buffers (`NSPasteboard` / Windows clipboard / X11 selection).

---

## 2. Phased Implementation Roadmap

```mermaid
flowchart TD
    M1["Phase 1: Database Hardening & Schema Migration"]
    M2["Phase 2: Card Architecture & Inline Annotation UX"]
    M3["Phase 3: Direct Vault Entry & Password Forge Integration"]
    M4["Phase 4: Vault Filter Tab & Search Navigation"]
    M5["Phase 5: Verification, Tests & Security Hardening"]

    M1 --> M2
    M2 --> M3
    M3 --> M4
    M4 --> M5
```

---

### Phase 1: Database Hardening & Schema Migration

**Primary Files:** [`src/db.rs`](file:///home/antigravity/BrunoO/clipboard/src/db.rs)

1. **Schema Migration:**
   - Add new columns to `clipboard_history`:
     - `label TEXT`: Short identifier (e.g., *"GitHub Work PAT"*, *"Home Wi-Fi"*).
     - `notes TEXT`: Optional multi-line context (e.g., username, recovery codes).
     - `is_pinned INTEGER NOT NULL DEFAULT 0`: Boolean flag (1 = pinned / protected from pruning and `Clear All`). Legacy `in_vault` columns are automatically renamed to `is_pinned` on open.
   - Add indexes for fast sorting and filtering:
     - `CREATE INDEX IF NOT EXISTS idx_clipboard_history_pinned ON clipboard_history(is_pinned, id DESC);`
   - Incorporate `ALTER TABLE` statements into the existing graceful migration loop in `Database::open` with duplicate column error suppression.

2. **Database API Additions:**
   - `set_item_annotation(&self, id: i64, label: Option<&str>, notes: Option<&str>) -> Result<(), rusqlite::Error>`
   - `toggle_item_pin(&self, id: i64) -> Result<bool, rusqlite::Error>`
   - `insert_manual_secret(&self, secret: &str, label: Option<&str>, notes: Option<&str>, is_pinned: bool) -> Result<i64, rusqlite::Error>`

3. **Pruning & History Clearing Protection:**
   - **`prune_history`:** Update `DELETE FROM clipboard_history ...` to strictly exempt pinned items:
     ```sql
     WHERE is_pinned = 0 AND id NOT IN (
         SELECT id FROM clipboard_history WHERE is_pinned = 0 ORDER BY id DESC LIMIT ?
     )
     ```
   - **`clear`:** Bulk clearing preserves pinned items by default:
     ```sql
     DELETE FROM clipboard_history WHERE is_pinned = 0;
     ```
     (pinned items can only be deleted individually via their card's delete action).

4. **Search Query Refinement:**
   - Update `search(&self, query: &str)` to evaluate `(label LIKE ? OR notes LIKE ?)` across all entries.
   - For `ItemKind::Secret`, match solely against `label` and `notes`. For other kinds, match against `(label OR notes OR content)`.

---

### Phase 2: Card Architecture & Inline Annotation UX

**Primary Files:** [`src/icons.rs`](file:///home/antigravity/BrunoO/clipboard/src/icons.rs), [`src/app/history_view.rs`](file:///home/antigravity/BrunoO/clipboard/src/app/history_view.rs)

1. **Icon Assets:**
   - Register `PUSH_PIN` (`mi::ICON_PUSH_PIN.codepoint`) and `PENCIL` (`mi::ICON_EDIT.codepoint`) in [`src/icons.rs`](file:///home/antigravity/BrunoO/clipboard/src/icons.rs).

2. **Card Visual Hierarchy:**
   - When an item has a `label`, display it prominently as the card's **headline** in 13.5pt bold text:
     ```
     ┌────────────────────────────────────────────────────────┐
     │ 📌 GitHub Production PAT              [Secret] • 2m ago │
     │ •••• •••• •••• ••••                          [👁] [⎘] [✎]│
     │ user: dev@company.com                                  │
     └────────────────────────────────────────────────────────┘
     ```
   - If pinned, display an amber/gold push-pin icon on the card header.

3. **Action Triggers & Micro-Interactions:**
   - **Pin Button:** Icon button on the card action row to toggle pinned state with instant visual feedback.
   - **Edit Annotation Button (`✎`):** Clicking opens an inline editing card or lightweight popover with:
     - Title / Label input field
     - Multi-line Notes field
     - "Save (Enter)" / "Cancel (Esc)" buttons
   - **Keyboard Shortcut:** Pressing `F2` or `Cmd+E` / `Ctrl+E` on a selected card immediately initiates annotation editing.

---

### Phase 3: Direct Vault Entry ("New Secret") & Password Forge Integration

**Primary Files:** [`src/app/mod.rs`](file:///home/antigravity/BrunoO/clipboard/src/app/mod.rs), [`src/app/generator_view.rs`](file:///home/antigravity/BrunoO/clipboard/src/app/generator_view.rs)

1. **"New Secret" Modal Dialog:**
   - Accessible via a dedicated `+ Secret` button in the top header or `Cmd+N` / `Ctrl+Shift+N`.
   - Fields:
     - **Label / Title** (e.g., *"AWS Root Password"*)
     - **Secret Value** (password field with bullet masking and show/hide eye toggle)
     - **Notes / Username** (optional multi-line text)
     - Saved as a pinned secret by default (`is_pinned = true`, `ItemKind::Secret`)
   - Action: Inserts directly into `Database` and refreshes history; **does not** touch OS clipboard buffers.

2. **Password Forge Integration:**
   - Add a **"Save Secret"** button in [`src/app/generator_view.rs`](file:///home/antigravity/BrunoO/clipboard/src/app/generator_view.rs).
   - Opens the "New Secret" dialog pre-filled with the generated password and prompts for the label.

---

### Phase 4: Pinned Filter Tab & Search Navigation

**Primary Files:** [`src/app/history_view.rs`](file:///home/antigravity/BrunoO/clipboard/src/app/history_view.rs), [`src/app/mod.rs`](file:///home/antigravity/BrunoO/clipboard/src/app/mod.rs)

1. **Dedicated Pinned Tab:**
   - Add a `Pinned` filter tab (`ContentFilter::Pinned`) matching strictly `item.is_pinned`:
     - Displays count of all pinned entries (snippets, links, code, colors, images, and pinned secrets).
     - Allows users to switch immediately from transient clipboard history into their curated saved items.
2. **Search Refinements:**
   - Typing in the spotlight search bar matches labels and notes instantly.
   - For example, typing `"aws"` filters directly to the annotated AWS secret, displaying its label, notes, and bullet-masked capsule.

---

### Phase 5: Verification, Tests & Security Hardening

1. **Database Unit Tests:**
   - Test schema migration with existing, legacy `in_vault`, and new databases.
   - Test that `prune_history` never deletes pinned items even under aggressive item caps.
   - Test that `clear` preserves pinned entries.
   - Test that `set_item_secret` does not mutate `is_pinned`.
   - Test annotation search (verifying that secret values are NOT leaked into query logs or matched via plain content search).
2. **UI & Micro-interaction Verification:**
   - Test inline editing flow and hotkey bindings (`F2`, `Cmd+N`).
   - Verify WCAG contrast standards for the Vault badge and annotation typography.
3. **Multiplatform Checks:**
   - Run `cargo fmt -- --check`.
   - Run `cargo clippy --all-targets --all-features -- -D warnings`.
   - Run `cargo test --all-targets`.

---

## 3. Milestones & Work Breakdown

| Milestone | Deliverables | Estimated Scope |
| :--- | :--- | :--- |
| **Milestone A** | **Database & Storage Core:** Schema migration (`label`, `notes`, `is_pinned`, plus legacy `in_vault` rename), prune immunity, clear protection, and label search queries. | ~150 lines in `src/db.rs` + tests |
| **Milestone B** | **Card UI & Annotations:** Prominent label headlines, pinned amber badge, inline edit popup (`F2`), and independent pin & mask-as-secret actions. | ~200 lines in `src/app/history_view.rs` |
| **Milestone C** | **Direct Entry & Forge Integration:** "New Secret" modal dialog (`Cmd+N` / `Ctrl+Shift+N`) and "Save Secret" action in Password Forge. | ~180 lines in `src/app/` |
| **Milestone D** | **Pinned Filter Tab & Polish:** Dedicated `Pinned` filter tab, count badges, and end-to-end integration tests. | ~100 lines |

---

## 4. Confirmed Design Choices

1. **Decoupled Pin vs. Secret (Option 2):**
   - **Decision:** Keep **Pin** (`is_pinned`, `PUSH_PIN` icon) for retention and **Mask as Secret** (`ItemKind::Secret`, `LOCK` icon, `Cmd/Ctrl+L`) for sensitivity as strictly independent axes.
   - Toggling Secret masking never pins or unpins an entry; pinning works uniformly across Text, Links, Code, Colors, Images, and Secrets.
   - Ephemeral auto-detected secrets age out of history unless pinned.
2. **Pinned Tab Integration:**
   - **Decision:** Use a **`Pinned`** filter tab (`ContentFilter::Pinned`) matching strictly `item.is_pinned`.
   - Preserves the streamlined 7-tab bar geometry (`All`, `Text`, `Links`, `Code`, `Colors`, `Pinned`, `Images`) so tabs stay on a single line without crowding.
3. **Bulk Clear Protection:**
   - **Decision:** Automatically preserve all pinned items during "Clear All History".
   - Shows feedback toast: *"History cleared. Pinned items kept."*
   - Pinned items can only be deleted individually via their card's delete action.
