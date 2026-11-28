# Spec: Transform Workbench

## Status

**Draft / proposal spec.** This document explores and specifies a Workbench for copied entries where users can apply successive transformations, inspect results, perform Undo/Redo, and copy a final result. It is grounded in the current codebase and includes recommendations for the key product decisions.

## 1. Overview

### Problem

Today, transforms are one-shot copy actions from history cards. Users can apply a built-in `TextTransform` or a sandboxed Rhai script, preview it on hover, and copy the transformed output, but they cannot:

- chain multiple transforms,
- inspect intermediate results as a working buffer,
- undo/redo transform steps,
- manually tweak the result before copying,
- force-apply transforms when applicability pre-selection is wrong.

`docs/FEATURE_TEXT_TRANSFORMATIONS.md` identifies a related Phase 4 idea: a quick transform palette with live before/after preview. The Workbench supersedes that concept as a richer destination, while preserving a fast quick-launch path.

### Proposal

Add a dedicated **Transform Workbench** view where a user loads content (from a history item, clipboard snapshot, or OCR result), applies successive transforms/scripts, optionally edits text, undoes/redoes committed steps, and explicitly finalizes the result by copying or saving it.

## 2. Current baseline

| Area | Current behavior | Key files/symbols |
|---|---|---|
| History model | Immutable-ish clipboard records in SQLCipher: `ClipboardItem { id, kind, content, width, height, created_at, embedded_colors, label, notes, is_pinned }` | `src/db.rs` |
| Views | `ActiveView::{History, Generator, Settings}`; `ui()` routes to panel renderers | `src/app/mod.rs` |
| Transform engine | Pure, deterministic text transforms: `TextTransform::{applies_to, apply, apply_with_summary, category, label, tooltip, toast_*}`; `TransformOutput { text, summary }`; `TransformError` | `src/transform.rs` |
| Custom scripts | Sandboxed Rhai scripts loaded by `ScriptRegistry`; execution via `ScriptRegistry::execute`; applicability via `ScriptRegistry::applies_to` | `src/script.rs` |
| Card transform UI | Card `⋯` menu / context menu renders applicable built-ins and scripts; hover preview for small revealed text; click dispatches action | `src/app/history_view.rs` |
| Action dispatch | `ItemAction::CopyTransformed { content, toast, is_secret }` -> `ClipboardApp::copy_transformed_to_clipboard`; secret path uses `copy_secret_to_clipboard`; normal path uses `copy_text_to_clipboard` | `src/app/history_view.rs`, `src/app/mod.rs` |
| Secret handling | Masked by default; reveal toggle; secret copies skip capture, mark concealed, zeroize, schedule auto-clear | `src/app/mod.rs`, `docs/SECURITY_LESSONS.md` |
| Image OCR | Async background worker; result delivered via `services.ocr_rx`; UI action `ExtractTextFromImage` | `src/app/mod.rs`, `src/app/history_view.rs`, platform OCR helpers |
| Existing edit widgets | `egui::TextEdit` already used for search, annotations, secret input; egui 0.37 / `egui_extras` syntect available | `Cargo.toml`, `src/app/mod.rs`, `src/app/settings_view.rs` |

## 3. Goals

- **Successive transformations:** apply multiple built-in transforms and custom scripts to the same working buffer.
- **Inspectable results:** show current output, transform summary, errors, and source/step context.
- **Undo/Redo:** reversible step history for transforms and committed manual edits.
- **Non-destructive default:** do not mutate original history records unless the user explicitly saves/replaces (replace deferred).
- **Secret parity:** preserve existing secret masking, copy, concealment, auto-clear, and no-logging invariants.
- **Fast entry:** support both deep workbench use and quick-launch one-shot flow.
- **Escape hatch:** allow forcing all transforms/scripts when applicability detection is wrong.

## 4. Non-goals (MVP)

- Not a full IDE/editor or file editor.
- Not image editing; image support is OCR-to-text ingestion only.
- Not automatic capture-time transform pipelines.
- Not destructive in-place replacement of original history entries.
- Not collaborative/shared workbench state.
- Not persistent saved pipelines/macros (future).
- Not a separate Phase 4 quick transform palette UI; Workbench replaces it.

## 5. User experience

### 5.1 Placement and navigation

Add `ActiveView::Workbench` alongside `History`, `Generator`, and `Settings`.

**Entry points:**

- Top-bar button: `Workbench`.
- Card `⋯` menu / right-click context menu: `Open in Workbench`.
- Selected item shortcut: open selected item in Workbench (final key after shortcut audit; avoid conflicts).
- Quick launch: `Cmd/Ctrl+T` opens Workbench preloaded with the selected item and focuses transform search. This supersedes the Phase 4 palette concept.
- Optional: open empty/ad-hoc workbench from top bar/menu.

**Exit:**

- `Esc` returns to History when no modal/confirmation is active.
- `Back` button in Workbench header.

### 5.2 Layout

```text
┌────────────────────────────────────────────────────────────────────┐
│ Workbench  [Back]                [Undo] [Redo] [Reset] [Copy ▾]   │
│ Source: #123 · Text · "invoice..."  [Reload original] [Reveal]    │
├───────────────────────────────┬────────────────────────────────────┤
│ Transform catalog             │ Output                             │
│ [Search...] [Show all ☐]      │ [Wrap] [Monospace] [Preview info]  │
│ AI & Prompt                   │ ┌────────────────────────────────┐ │
│  Prepare Code for AI          │ │ current buffer                 │ │
│ Cleanup                       │ │ editable TextEdit (MVP simple) │ │
│  Strip Formatting             │ │                                │ │
│ ...                           │ └────────────────────────────────┘ │
│ Custom Scripts                │ Summary / warning / inline error    │
│  Filter Errors                │                                    │
├───────────────────────────────┴────────────────────────────────────┤
│ Steps                                                              │
│ ● 0 Source snapshot                                                │
│   1 Strip Formatting            "removed HTML/ANSI..."   [Go]      │
│   2 Manual edit                 "adjusted prompt"        [Go]      │
│ ○ 3 Prepare Log for AI          current                   [Go]      │
└────────────────────────────────────────────────────────────────────┘
```

### 5.3 Core flow

1. User opens Workbench from a history item or quick launch.
2. Workbench loads a detached snapshot as step 0.
3. User searches/selects a transform or script.
4. Workbench previews when safe/allowed; user applies.
5. Successful apply pushes a new step and moves cursor to it.
6. User may manually edit the buffer; edit becomes a committed `Manual edit` step by the commit rule.
7. Undo/Redo moves across committed steps.
8. User finalizes with `Copy`, `Copy & Close`, or `Save as new entry`.

### 5.4 Image/OCR flow

- If source is `ItemKind::Image`, initial state shows image metadata/preview and an `Extract Text (OCR)` action.
- OCR runs through the existing background OCR path; result creates a text step/buffer.
- After OCR, normal text transform flow applies.
- If OCR unsupported/fails, show inline error; no text step created.

## 6. Data and state model

### 6.1 App state

Add to `ClipboardApp`:

```rust
workbench: WorkbenchState,
```

Or `Option<WorkbenchState>` if lazy initialization is preferred; a default empty state is simpler.

### 6.2 Workbench state

```rust
pub(crate) struct WorkbenchState {
    source: WorkbenchSource,
    source_item_id: Option<i64>,
    source_detached_reason: Option<String>,
    is_secret: bool,
    reveal_secret: bool,
    show_all_transforms: bool,
    wrap_output: bool,
    search_query: String,
    steps: Vec<WorkbenchStep>,
    cursor: usize,
    pending_edit: Option<PendingEdit>,
    preview: Option<WorkbenchPreview>,
    status: WorkbenchStatus,
}

pub(crate) enum WorkbenchSource {
    HistoryItem { id: i64, kind: ItemKind },
    ClipboardSnapshot,
    AdHoc,
    ImageOcr { id: i64 },
}

pub(crate) struct WorkbenchStep {
    label: String,
    origin: StepOrigin,
    output: String, // Zeroizing<String> or equivalent strategy for secret sessions
    summary: Option<String>,
}

pub(crate) enum StepOrigin {
    Source,
    BuiltIn(TextTransform),
    CustomScript { filename: String, name: String },
    ManualEdit,
    Ocr,
}

pub(crate) struct PendingEdit {
    baseline_cursor: usize,
    last_changed_at: Instant,
}

pub(crate) enum WorkbenchStatus {
    Idle,
    OcrRunning,
    Error(String),
}
```

**Notes:**

- `steps[0]` is always the source snapshot.
- `cursor` points to the active step; output shown is `steps[cursor].output`.
- Secret sessions should avoid unnecessary cloning and should use zeroizing storage where practical.
- Large outputs remain in memory only as needed; UI preview may truncate.

### 6.3 Undo/Redo semantics

- **Apply transform/script:** commit pending edit first (if any), then if result differs from current buffer, `truncate` redo tail, push step, `cursor = steps.len() - 1`.
- **Manual edit commit:** if buffer differs from current committed step, push `ManualEdit` step.
- **Undo:** if pending uncommitted edit exists, commit it as a step (or discard if identical); then `cursor = cursor.saturating_sub(1)`.
- **Redo:** if `cursor + 1 < steps.len()`, `cursor += 1`.
- **Reset:** confirm if steps > 1 / pending edits; restore to source snapshot step or reload original snapshot.
- **Jump to step:** moving cursor to an earlier step is allowed; applying a new transform after jump truncates redo tail.

**Why cursor model:** simple, deterministic, and matches user expectations for linear undo/redo.

## 7. Transform and edit behavior

### 7.1 Transform applicability

- Default: show transforms/scripts where `applies_to` returns true for the **current buffer context**.
- `Show all transforms` toggle bypasses applicability filtering only.
- Forced transforms still run normal validation and may fail with inline `TransformError` / script error; failures do not create steps.

### 7.2 Manual edit

- Use built-in `egui::TextEdit::multiline`.
- Keep editing simple in MVP: multiline text, wrap toggle, optional monospace.
- No new editor crate in MVP; `egui_extras::syntect` is already available for future preview enhancements.
- Manual edit commits become `Manual edit` steps.

### 7.3 Custom scripts

- Reuse `ScriptRegistry::applies_to` and `execute`.
- Script preview in Workbench is explicit by default (button/action), not automatic hover execution.
- Script errors are inline and non-destructive.

## 8. Locked decisions, recommendations, and WHYs

### 8.1 Finalize actions

**Recommendation: MVP = `Copy`, `Copy & Close`, `Save as new entry`. Defer `Replace original`.**

| Action | Behavior |
|---|---|
| `Copy` | Copy current buffer to clipboard, show toast, keep Workbench open. |
| `Copy & Close` | Copy current buffer, then dismiss/return using normal close behavior. |
| `Save as new entry` | Explicitly insert a new history item from current buffer. No source mutation. |
| `Replace original` | Deferred. |

**Why:**

- Preserves the current non-destructive model; transforms today do not overwrite source items.
- Avoids accidental history pollution during experimentation.
- `Replace original` has tricky semantics for pinned, annotated, secret, image-derived, or deleted sources and needs stronger confirmation/design.

### 8.2 Copy behavior

**Recommendation: `Copy` stays open; `Copy & Close` may hide/auto-paste; secret copies keep existing secret path exactly.**

| Case | Recommendation |
|---|---|
| Normal text, `Copy` | Write clipboard, toast, remain in Workbench. Do not hide/auto-paste. |
| Normal text, `Copy & Close` | Write clipboard, toast, then follow existing `finish_copy` dismiss/auto-paste settings. |
| Secret-derived buffer | Always use `copy_secret_to_clipboard` semantics: skip capture, mark concealed, zeroize, schedule auto-clear, secret toast. |
| Persistence | No implicit history insert on copy. Only `Save as new entry` persists. |

**Why:**

- Workbench is iterative; copying intermediate/final candidates should not force window dismissal.
- `Copy & Close` still supports the fast “done, paste it now” workflow.
- Secret handling is security-sensitive and already well-defined; reusing it avoids regressions.

### 8.3 Manual edit commit rule

**Recommendation: built-in `TextEdit` + committed Workbench steps; do not make every keystroke a step.**

| Rule | Recommendation |
|---|---|
| Editor | `egui::TextEdit::multiline` for MVP. |
| Commit triggers | Explicit Apply/Commit action; blur/focus loss before transform/navigation/finalize; debounce idle commit (750–1000 ms) when content changed. |
| Undo/Redo | Workbench Undo/Redo operates on committed steps. Pending edit is committed or resolved by a clear rule before cross-step undo/redo. |
| TextEdit internal undo | May exist during active typing, but is not the authoritative Workbench history. |

**Why:**

- Keystroke-level steps create noisy undo history and unnecessary memory churn.
- Step-based commits align manual edits with transform/script steps.
- A simple, explicit rule prevents confusing dual-undo behavior.

### 8.4 Source lifecycle

**Recommendation: detached snapshot by default; original remains unchanged; clear detached status.**

| Scenario | Recommendation |
|---|---|
| Open item | Snapshot content and relevant metadata at open time. |
| Source changed/deleted later | Keep Workbench snapshot; show `Detached snapshot` / source unavailable status. |
| Reload original | Only when source still exists; confirm if current work would be lost. |
| Save as new entry | Create a new DB item; do not mutate original. |

**Why:**

- Snapshot isolation makes step history deterministic.
- Avoids surprising users if history changes underneath an active Workbench session.
- Keeps non-destructive principle clear.

### 8.5 Secret policy

**Recommendation: secret-derived sessions stay secret; masked by default; no downgrade in MVP.**

| Rule | Recommendation |
|---|---|
| Secret source | `is_secret = true` for the session. |
| Visibility | Mask preview by default; reveal is explicit and local to Workbench. |
| `Show all transforms` | Does not reveal masked content and does not bypass secret copy protections. |
| Copy/save | Copy uses secret path; Save as new entry creates a Secret item. |
| Downgrade | No “save/copy as plain text” in MVP. |

**Why:**

- Minimizes secret leakage risk and matches existing app behavior.
- Secret downgrade is a separate security/product decision requiring explicit UX and audit.

### 8.6 Preview policy

**Recommendation: safe auto-preview for cheap built-ins; explicit preview for scripts/forced/all-transforms cases.**

| Case | Recommendation |
|---|---|
| Built-in transform, small revealed buffer | Auto-preview allowed (`apply_with_summary`), with existing size guard. |
| Masked secret | No preview unless explicitly revealed. |
| Large buffer | Show “Preview omitted for large content”; allow explicit Apply. |
| Custom scripts | No auto-run on hover by default; explicit Preview/Apply. |
| `Show all transforms` enabled | Disable auto-preview by default; click to preview/apply. |
| Failure | Inline error/warning; no step pushed; buffer unchanged. |

**Why:**

- Prevents unnecessary CPU work on large buffers and avoids surprise script execution.
- Preserves current secret preview protections.
- Keeps “show all” safe and intentional.

### 8.7 Entry points / shortcuts

**Recommendation: visible Workbench destination + fast quick-launch; keep existing one-shot transforms.**

| Entry | Recommendation |
|---|---|
| Top bar | Add `Workbench` button/view entry. |
| Card menu/context menu | `Open in Workbench`. |
| Quick launch | `Cmd/Ctrl+T` opens Workbench with selected item preloaded and transform search focused. |
| Existing card transforms | Keep them for one-shot quick actions. |
| In-workbench shortcuts | `Cmd/Ctrl+Z` undo, `Shift+Cmd/Ctrl+Z` redo, `Cmd/Ctrl+Enter` copy, `Shift+Cmd/Ctrl+Enter` copy & close (final audit required), `Esc` back/close modal. |

**Why:**

- Replaces Phase 4 palette without losing speed.
- Keeps simple tasks fast while making complex multi-step tasks possible.
- Avoids a fragmented transform UX.

## 9. Security, concurrency, and platform invariants

These are mandatory per `AGENTS.md` and docs in repo:

- **No `unsafe` outside platform modules.** Workbench code should be safe Rust in app/transform/script/UI layers.
- **No clipboard/secret logging in release builds.** Do not log buffer contents, previews, OCR text, script output, or errors containing sensitive payloads.
- **Secret handling parity.** Secret-derived output uses `copy_secret_to_clipboard`, concealed marking, skip capture, zeroized values, and auto-clear scheduling.
- **No heavy work on UI thread or DB mutex.** Transforms are usually fast; scripts/previews should be guarded. OCR remains background-worker based. DB writes/reads should be short; no transform execution inside DB critical sections.
- **Platform boundary discipline.** No scattered `#[cfg(target_os = ...)]` outside `src/platform/*`; use existing platform capability helpers.
- **eframe 0.36+ split awareness.** Background/wakeup logic in `App::logic()`; keyboard/pointer UI handling in `App::ui()`. Avoid hidden-window repaint regressions.
- **Bounded input/preview.** Respect existing size limits and preview guards; avoid OOM/lag on large entries.

## 10. Edge cases and failure handling

| Case | Expected behavior |
|---|---|
| Transform fails (`InvalidJson`, `InvalidBase64`, etc.) | Inline error, no new step, buffer unchanged. |
| Script fails | Inline script error, no new step. |
| Script returns very large output | Accept only within safe limits; otherwise error/warn and do not corrupt state. |
| OCR unsupported/fails | Show inline status/error; keep image source loaded. |
| Source deleted while Workbench open | Continue on detached snapshot; disable Reload original. |
| Masked secret preview | Hidden unless revealed; preview omitted message. |
| `Show all transforms` + incompatible transform | Transform remains visible but execution may fail safely. |
| Empty/ad-hoc buffer | Only applicable transforms enabled; others hidden unless `Show all transforms`. |
| Copy while clipboard unavailable | Existing warning toast path; no false success. |
| Pending manual edit + Undo/Apply/Close | Resolve by commit rule before action. |

## 11. Accessibility and UX quality

- Keyboard-first flow: search, select, apply, undo/redo, copy, close.
- Visible focus state and disabled-button reasons.
- Clear labels for destructive/risky actions (`Reset`, future `Replace original`).
- Secret reveal state obvious and local.
- Step list readable: origin, summary, current marker.
- Toasts/errors actionable and non-sensitive.

## 12. Testing and verification

### Unit tests

- `WorkbenchState` step stack: apply, undo, redo, redo-tail truncation, jump+apply, reset.
- Manual edit commit rules: debounce/blur/explicit commit behavior (as pure state transitions where possible).
- Applicability override: `Show all transforms` shows hidden transforms but does not bypass execution validation.
- Secret policy: secret session cannot produce plain-copy path; save-as-new creates Secret.
- Source detach behavior: missing source item -> detached status, Reload original disabled.

### Integration/UI tests (where feasible)

- Open history item in Workbench via action.
- Apply two transforms, undo, redo, copy.
- Forced incompatible transform shows inline error and preserves buffer.
- Script error does not mutate steps.
- OCR result becomes text step.
- `Cmd/Ctrl+T` quick-launch focuses search with selected item loaded.

### Manual verification

- Dark/light themes, small/large buffers, masked secrets, image items, custom scripts, keyboard-only flow.
- Confirm no hidden-window repaint regressions and no heavy UI-thread work.

### Required checks before PR

```bash
cargo fmt -- --check
cargo clippy --all-targets --all-features -- -D warnings
cargo test --all-targets
```

## 13. Rollout plan

### Phase W1 — MVP Workbench

- `ActiveView::Workbench`, basic panel/layout.
- Open from selected history text/secret item.
- Transform catalog search + built-in apply.
- Cursor-based Undo/Redo.
- `Copy`, `Copy & Close`, `Reset`.
- `Show all transforms` toggle.
- Inline errors and safe preview guards.

### Phase W2 — Manual edit + scripts

- Simple `TextEdit` buffer editing.
- Committed `Manual edit` steps.
- Custom script apply/preview with explicit execution policy.
- Step summaries and jump-to-step.

### Phase W3 — OCR + persistence

- Image source -> OCR step.
- `Save as new entry`.
- Source detach/reload handling.
- Quick-launch polish (`Cmd/Ctrl+T`).

### Phase W4 — Advanced (optional)

- Diff view between steps.
- Named pipelines/favorites.
- Optional `Replace original` with strong confirmation.
- Richer code editing only if justified; evaluate crate cost first.

## 14. Recommendation summary

| Decision | Recommendation | Why |
|---|---|---|
| Finalize actions | `Copy`, `Copy & Close`, `Save as new entry`; defer `Replace original` | Non-destructive, low-risk, covers main workflows |
| Copy behavior | `Copy` stays open; `Copy & Close` may dismiss/auto-paste; secrets use existing secret path | Supports iteration without losing fast finish flow; preserves security |
| Manual edit commit rule | `TextEdit` + committed step-based edits; no per-keystroke steps | Simple, predictable Undo/Redo; avoids noisy history |
| Source lifecycle | Detached snapshot; original unchanged; clear detached status | Deterministic session; no surprise cross-view mutations |
| Secret policy | Secret stays secret; masked by default; no downgrade in MVP | Preserves current security invariants |
| Preview policy | Safe auto-preview for cheap built-ins; explicit preview for scripts/forced/large/masked cases | Avoids perf/surprise execution issues; keeps previews safe |
| Entry points/shortcuts | Workbench view + card action + `Cmd/Ctrl+T` quick launch; keep one-shot card transforms | Replaces Phase 4 palette while preserving speed and discoverability |
