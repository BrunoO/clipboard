# Feature Research: Stripping Source Formatting & Generalized Entry Transformations

> **Status:** Research / Design Study  
> **Date:** 2026-11-21  
> **Scope:** Plain-text extraction ("strip source formatting") and a generalized entry transformation framework for `CipherClipboard`.

---

## 1. Problem Statement

Users frequently copy content from rich or noisy sources—web pages, PDFs, terminal emulators, word processors, or developer tools—and want to paste only the clean, raw text, or apply a quick transformation (changing case, unwrapping PDF line breaks, stripping URL tracking parameters, decoding Base64, minifying JSON) before pasting into a target application.

This document studies:
1. How to implement **"Strip source formatting → raw text"** within `CipherClipboard`'s current architecture.
2. How to **generalize** that design into a cohesive, extensible **entry transformation** system without cluttering the UI or violating the project's security, concurrency, and multiplatform invariants.

---

## 2. Current Architecture Baseline

Understanding what `CipherClipboard` already strips at capture time versus what remains in stored entries is essential before designing any transformation feature.

### 2.1 OS Rich-Text Layer (Already Stripped at Capture)
- **Ingestion (`src/monitor.rs`):** `poll_clipboard_text` reads from the OS clipboard using `arboard::Clipboard::get_text()`, which requests only the plain-text representation (`CF_UNICODETEXT` on Windows, `NSStringPboardType` on macOS, `UTF8_STRING` on Linux).
- **Storage (`src/db.rs`):** `clipboard_history` stores a single `content TEXT NOT NULL` column (plus `image_data BLOB` for images). No HTML, RTF, or styled-text payloads are persisted.
- **Output (`src/app/mod.rs`):** `copy_text_to_clipboard` and `copy_secret_to_clipboard` write plain UTF-8 text back to the OS clipboard via `arboard::Clipboard::set_text(...)`.

**Implication:** Rich-text styling (fonts, colors, bold/italics from browsers or office suites) is already discarded on capture. What *does* survive into `ClipboardItem.content` is **in-band formatting** inside the plain-text payload itself.

> *Future note:* If rich-text (HTML/RTF) capture is ever added to `Database` in the future, "Strip source formatting" would naturally gain a second dimension: choosing `Copy as Plain Text` vs. `Copy Formatted` at paste time, or destructively discarding the rich column.

### 2.2 In-Band Formatting That Survives Today
Even in plain-text payloads, entries commonly contain unwanted source formatting:
- **HTML / XML markup** (e.g., `<p class="x">Hello <b>world</b></p>` copied from DevTools or View Source; already recognized by `CodeLanguage::Html` in `src/code.rs`).
- **Markdown syntax** (`**bold**`, `[label](https://...)`, `# Heading`, `` `code` ``, `> quote`, fenced code blocks).
- **Terminal ANSI escape sequences** (`\x1b[31mError:\x1b[0m ...` from terminal emulators and CI logs).
- **Invisible & typographic Unicode characters** (`\u{200B}` zero-width space, `\u{200D}` ZWJ, `\u{FEFF}` BOM, `\u{FE0E}`/`\u{FE0F}` variation selectors, `\u{00A0}` non-breaking space, `\r\n` line endings, curly quotes). Notably, `ClipboardApp::format_display_preview` (`src/app/history_view.rs`) already strips a subset of these characters *for UI rendering only*, leaving the copied payload untouched.

### 2.3 Existing Transformation Precedents in the Codebase
`CipherClipboard` already implements two ad-hoc "transform before copy" patterns:
1. **JSON Prettify (`src/code.rs` & `src/app/history_view.rs`):** `render_kind_extra_buttons` shows a `Prettify` button when `item.code_language() == Some(CodeLanguage::Json)`, dispatching `ItemAction::CopyFormattedJson(pretty)`.
2. **Color Format Conversion (`src/color.rs` & `src/app/history_view.rs`):** Color items and embedded color chips offer `# HEX`, `RGB`, `HSL`, and `ARGB` buttons that copy the converted representation via `ItemAction::CopyColor(val, name)`.

---

## 3. Implementing "Strip Source Formatting → Raw Text"

### 3.1 Pure Transformation Module (`src/transform.rs`)
Keep all transformation logic pure, deterministic, `unsafe`-free (preserving crate-wide `unsafe_code = "deny"`), and covered by unit tests.

For `strip_source_formatting(input: &str) -> String`, apply a predictable, multi-stage pipeline:

1. **Control & Invisible Character Cleanup:**
   - Strip ANSI CSI (`\x1b[...<final>`) and OSC (`\x1b]...(\x07|\x1b\\)`) escape sequences.
   - Remove zero-width / invisible characters (`\u{200B}`, `\u{200C}`, `\u{200D}`, `\u{FEFF}`, `\u{FE0E}`, `\u{FE0F}`).
   - Normalize `\u{00A0}` (NBSP) → ASCII `' '` and `\r\n` / `\r` → `\n`.
2. **HTML / XML Stripping (when markup is detected):**
   - Drop `<script>...</script>` and `<style>...</style>` blocks completely.
   - Insert `\n` around block-level tags (`<br>`, `</p>`, `</div>`, `</li>`, `</tr>`, `</h1..6>`) to preserve paragraph/list boundaries.
   - Strip remaining `<...>` tags.
   - Decode common HTML entities (`&amp;`, `&lt;`, `&gt;`, `&quot;`, `&#39;`, `&apos;`, `&nbsp;`, and numeric `&#123;` / `&#x1A;` entities).
3. **Markdown Stripping (when Markdown tokens are detected):**
   - Strip ATX heading prefixes (`# `..`###### `), blockquote prefixes (`> `), and list markers (`- `, `* `, `1. `).
   - Unwrap emphasis (`**`, `__`, `*`, `_`, `~~`) and inline/fenced code markers (`` ` ``, ```` ``` ````).
   - Convert links `[label](url)` → `label` and images `![alt](url)` → `alt`.
4. **Whitespace Normalization:**
   - Trim trailing whitespace per line, collapse 3+ consecutive newlines to 2, and trim outer leading/trailing whitespace.

Also provide a fast, allocation-free predicate:
```rust
pub(crate) fn has_strippable_formatting(item: &ClipboardItem) -> bool
```
This checks for ANSI `\x1b`, zero-width/NBSP characters, HTML tags (`CodeLanguage::Html` or `<tag>` heuristics), or Markdown tokens, allowing the UI to surface a contextual action only when relevant.

### 3.2 Action Dispatch & Secret-Handling Parity
Generalize `ItemAction` in `src/app/history_view.rs` by replacing `CopyFormattedJson(String)` with a unified variant:

```rust
pub(super) enum ItemAction {
    // ...
    CopyTransformed {
        content: String,
        toast: String,
        is_secret: bool,
    },
}
```

In `dispatch_item_action`:
- **Secret parity:** If `is_secret` is `true` (e.g., transforming a `ItemKind::Secret` entry), **must** route through `copy_secret_to_clipboard` (which sets `db.skip_next_capture`, calls `platform::mark_clipboard_concealed()`, wraps the value in `zeroize::Zeroizing`, and schedules `auto_clear_secret_secs`) rather than `copy_text_to_clipboard`.
- **Logging:** Never log input or transformed output in release builds (`docs/SECURITY_LESSONS.md`).
- **History capture semantics (non-destructive by default):**
  - Copying the transformed text leaves the original `ClipboardItem` intact in SQLCipher.
  - Decide explicitly whether to call `db.skip_next_capture(&transformed)` for non-secret transforms:
    - *Skipping capture* prevents cluttering history with one-off stripped copies.
    - *Allowing capture* lets the user retain both the original and raw-text versions in history.

### 3.3 UI / UX Integration
Card headers in `render_item_header_actions` (`src/app/history_view.rs`) are compact, so avoid adding permanent buttons to every row:
1. **Contextual inline button:** In `render_kind_extra_buttons`, show a compact `Raw` button when `has_strippable_formatting(item)` is `true` (mirroring `Open` for URLs and `Prettify` for JSON).
2. **Keyboard shortcut:** Bind `Shift+Enter` on the selected entry in `App::ui()` to **"Copy as Raw Text"** (mirroring `Enter` = Copy, matching standard OS `Shift` = paste without formatting muscle memory).
3. **Auto-paste compatibility:** Route through `finish_copy(ctx, &toast, self.settings.hide_on_copy)` so `restore_focus` and `auto_paste` work identically to standard copy.

---

## 4. Generalized Entry Transformation Architecture

Generalizing from *"strip formatting"* to *"transform entry before copying/pasting"* unifies existing ad-hoc features (`Prettify` JSON, Color conversions) and future transforms under one extensible model.

### 4.1 Core Abstraction (`src/transform.rs`)
Define a `TextTransform` enum (or descriptor table) with:
- **Metadata:** `id`, `label`, `category`, `tooltip`
- **Applicability:** `applies_to(&self, item: &ClipboardItem) -> bool` so irrelevant transforms are hidden (e.g., `StripTrackingParams` only for `ItemKind::Url`; `MinifyJson` only for `CodeLanguage::Json`)
- **Execution:** `apply(&self, input: &str) -> Result<String, TransformError>` (returning `Result` allows fallible transforms like `Base64Decode` or `JsonMinify` to surface a clean warning toast instead of failing silently)

### 4.2 Scaling the UI Without Clutter
As the number of transforms grows beyond 2–3 buttons:
1. **Card Context Menu (`response.context_menu(...)`) + `Transform ▾` Menu:** Group applicable transforms by category (*Cleanup*, *Case*, *Lines*, *Encode/Decode*, *Developer*) inside an on-demand popup menu on each card.
2. **Quick Transform Palette (`Cmd/Ctrl+T` or `Cmd/Ctrl+K`, Phase 2+):** A searchable modal/palette for the currently selected entry showing a **live before/after preview** before copying/pasting.
3. **Optional Auto-Transform Rules (Future):** Opt-in settings in `AppSettings` (e.g., *"Automatically strip URL tracking parameters on capture"* or *"Automatically strip ANSI codes on capture"*) applied in `monitor.rs` prior to `db.insert()`.

---

## 5. Generalized Transformation Taxonomy

The table below catalogs high-value transformations for a clipboard manager, noting applicability and whether existing `Cargo.toml` dependencies already support them (zero new supply-chain surface):

| Category | Transformations | Applicability | Existing Crates / Notes |
|---|---|---|---|
| **1. Formatting & Cleanup** | Strip all source formatting (raw text); Strip HTML/XML; Strip Markdown; Strip ANSI codes; Remove zero-width/invisible chars; Smart quotes/dashes → ASCII; Normalize `CRLF` ↔ `LF` | Text, Code, URL | Pure Rust in `src/transform.rs` |
| **2. Whitespace & Line Fixing** | Unwrap hard line breaks (PDF/terminal copy fix); Trim leading/trailing per line; Collapse multiple blank lines; Collapse all whitespace to single line | Multi-line Text | High daily utility for PDF & terminal copies |
| **3. Case & Identifiers** | `UPPERCASE`, `lowercase`, `Title Case`, `Sentence case`, `camelCase`, `PascalCase`, `snake_case`, `SCREAMING_SNAKE_CASE`, `kebab-case` | Text, Code | Pure Rust; high value for writers and developers |
| **4. Line Operations** | Sort lines (A–Z, Z–A, numeric); Deduplicate lines (order-preserving); Remove empty lines; Reverse lines; Join lines with delimiter (`, `, `;`, space); Prefix/quote lines (`> `) | Multi-line Text, Code | Useful for lists, logs, SQL `IN (...)` clauses |
| **5. Encoding & Escaping** | Base64 encode/decode (UTF-8 validated); URL percent-encode/decode; JSON string escape/unescape; HTML entity encode/decode; Hex encode/decode | Text, Code, URL | `url` and `serde_json` already in `Cargo.toml` |
| **6. Structured Data & Code** | JSON Prettify *(exists)*, JSON Minify, JSON Sort Keys; CSV/TSV ↔ Markdown table or JSON; Unix epoch (s/ms) ↔ ISO-8601 | Code, numeric Text | `serde_json` and `chrono` already in `Cargo.toml` |
| **7. URL & Extraction** | Strip tracking params (`utm_*`, `fbclid`, `gclid`, `mc_eid`, `igshid`, `ref`); Extract all URLs / Emails / IPs / Numbers from text | URL, Text, Code | `url::Url` already used in `src/db.rs` |
| **8. Color & Media** | `#HEX` / `RGB` / `HSL` / `ARGB` *(exists in `src/color.rs`)*; **Image → Text (OCR)** *(see Section 6)*; Future: Image → Base64 data URI, resize/compress | Color, Image | Color is the existing precedent |

---

## 6. Media-to-Text Transformation: Image OCR

Extracting text from a copied screenshot or image (`ItemKind::Image -> String`) is an on-demand, non-destructive transformation that fits directly into the `Transform` model while delegating platform-specific OCR to `src/platform/`.

### 6.1 Platform Capabilities & APIs
- **macOS (`src/platform/macos.rs`) — Apple Vision Framework (`VNRecognizeTextRequest`):**
  - Available natively since **macOS 10.15+** (offline, hardware-accelerated, automatic language detection/correction).
  - Pass the stored PNG bytes directly as `NSData` to `VNImageRequestHandler::initWithData:options:`, execute `VNRecognizeTextRequest` (`VNRequestTextRecognitionLevelAccurate`, `usesLanguageCorrection = true`), and collect `topCandidates(1)` from each `VNRecognizedTextObservation`.
  - Uses `objc = "0.2"` already in `Cargo.toml` (wrapped in `autoreleasepool`, with all `unsafe` confined to `src/platform/macos.rs`) — **zero new crate dependencies**.
- **Windows 10/11 (`src/platform/windows.rs`) — WinRT `Windows.Media.Ocr` (`OcrEngine`):**
  - Available natively on **Windows 10/11**, offline, using installed Windows OCR language packs via `OcrEngine::TryCreateFromUserProfileLanguages()`.
  - Decodes PNG bytes into a `SoftwareBitmap` (RGBA/BGRA8), calls `OcrEngine::RecognizeAsync`, and extracts `OcrResult::Lines()` / `Text()`.
  - **Crate note:** `Cargo.toml` currently uses `windows-sys = "0.61"` (C-style Win32 bindings only, no WinRT COM projections). Implementing this cleanly requires adding the official `windows` crate under `[target.'cfg(target_os = "windows")'.dependencies]` with `Media_Ocr` / `Graphics_Imaging` features.
  - **Graceful degradation:** If no Windows OCR language pack is installed (`TryCreateFromUserProfileLanguages()` returns `None`), return `OcrError::NoLanguagePack` and surface a warning toast.
- **Linux (`src/platform/linux.rs`):**
  - Linux has no built-in OS-wide OCR API.
  - Expose `platform::supports_image_ocr() -> bool` (`false` in MVP, or `true` if `tesseract` is detected on `$PATH` as an optional CLI fallback without bundling native C++ Tesseract/Leptonica libraries).

### 6.2 Concurrency & UI Integration
- **Background execution (`docs/CONCURRENCY_AND_PERF_LESSONS.md`):** Because OCR can take 50–500 ms and WinRT/Vision calls block, OCR **must not** run on the `eframe` UI thread or inside `Mutex<Connection>`. Load the PNG bytes from `Database`, run `platform::extract_text_from_png(&png_bytes)` on a named background worker thread, and deliver the result to `App::logic()` via a channel.
- **UI trigger:** Show an **`Extract Text`** button in `render_kind_extra_buttons` (and card context menu) when `item.kind == ItemKind::Image && platform::supports_image_ocr()`.
- **Output:** Copy extracted text to the clipboard (or optionally allow saving as a new `Text` entry) with a toast reporting character/line count or `"No text detected in image."`.

---

## 7. Architecture, Security & Concurrency Invariants

Any implementation must preserve the project's documented invariants:
- **No `unsafe` (`Cargo.toml` / `AGENTS.md`):** `src/transform.rs` and `src/platform/mod.rs` / `linux.rs` must remain 100% safe Rust; any OCR FFI `unsafe` is strictly confined to `src/platform/macos.rs` and `src/platform/windows.rs`.
- **Multiplatform (`docs/MULTIPLATFORM_LESSONS.md`):** Pure text transformations have zero `#[cfg(target_os = ...)]` blocks. Platform-dependent capabilities like OCR and shortcut labels are exposed via OS-agnostic helpers in `src/platform/mod.rs` (e.g., `supports_image_ocr()`, `extract_text_from_png()`).
- **Secret handling (`docs/SECURITY_LESSONS.md`):**
  - Never log clipboard, OCR-extracted, or transformed content in release builds.
  - Wrap secret inputs/outputs in `zeroize::Zeroizing` where retained, and always dispatch `ItemKind::Secret` transforms through `copy_secret_to_clipboard` (`skip_next_capture`, `mark_clipboard_concealed()`, auto-clear timer).
- **Performance & Concurrency (`docs/CONCURRENCY_AND_PERF_LESSONS.md`):**
  - Text transformations run on demand when the user triggers an action (or in bounded preview slices), never inside `Mutex<Connection>` critical sections and never blocking `clipboard-monitor`.
  - Blocking/heavy transforms like Image OCR run on a dedicated named background worker, communicating results back to `App::logic()`.
  - Guard against pathological inputs (note `monitor.rs` already caps `MAX_TEXT_LENGTH` at 500 KB and `MAX_IMAGE_PIXELS` at 40 MP).
- **Static analysis:** Keep `clippy::pedantic` / `clippy::nursery` clean (`cargo fmt -- --check`, `cargo clippy --all-targets --all-features -- -D warnings`, `cargo test --all-targets`).

---

## 8. Recommended Phased Rollout

1. **Phase 1 — Core Module & "Strip Source Formatting" MVP:**
   - Add `src/transform.rs` with `strip_source_formatting`, `has_strippable_formatting`, and comprehensive unit tests (ANSI, HTML entities/tags, Markdown, zero-width chars, CRLF/NBSP).
   - Generalize `ItemAction::CopyFormattedJson` → `ItemAction::CopyTransformed`.
   - Add contextual `Raw` button in `render_kind_extra_buttons` + `Shift+Enter` shortcut for selected item.
2. **Phase 2 — High-Value Generalized Transforms + Context Menu:**
   - Add Whitespace/PDF unwrap, Case/Identifier conversions, Line operations, JSON Minify, and URL tracking-param stripping (all zero new crates).
   - Add `Transform ▾` / right-click context menu on cards grouped by category.
3. **Phase 3 — Image OCR (`ItemKind::Image -> Text`):**
   - Add `platform::supports_image_ocr()` and `platform::extract_text_from_png()` (macOS Vision first with zero new crates, then Windows `windows` crate WinRT `OcrEngine`, plus optional Linux `tesseract` fallback).
   - Wire background OCR worker → `App::logic()` → clipboard copy + toast.
4. **Phase 4 — Quick Transform Palette & Optional Auto-Rules:**
   - Add `Cmd/Ctrl+T` searchable Transform Palette with live before/after preview.
   - Evaluate opt-in capture-time auto-transforms (e.g., URL tracking param stripping) in `AppSettings`.

---

## 9. Open Questions

1. **History capture on transform:** When a user copies a transformed/raw/OCR version of an existing entry, should `db.skip_next_capture` be called (keeping history clean) or omitted (saving the transformed output as a new entry)?
2. **URL tracking parameter list:** Should tracking-parameter stripping use a curated built-in list (`utm_*`, `fbclid`, `gclid`, etc.) with a Settings toggle for auto-strip?
3. **Destructive vs. non-destructive:** Should Phase 1/2 remain strictly copy-only (like `Prettify`), or also offer *"Replace entry in history"*?
4. **Linux OCR fallback:** Should `src/platform/linux.rs` return `Unsupported` in MVP, or shell out to `tesseract` when present on `$PATH`?
