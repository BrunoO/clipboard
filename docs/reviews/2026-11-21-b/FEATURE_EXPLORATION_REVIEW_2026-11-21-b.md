# Feature Exploration Review - 2026-11-21-b

## Executive Summary
- **Health Score**: 7/10 ([Inferred] weighting of 0 critical, 2 high, 4 medium and 1 low evidence-backed findings; `paste_stack_spec.md` is a detailed Draft v2 specification aligned with the repository's module structure, but it remains unimplemented and must adapt to the latest `src/platform/` encapsulation rules and Linux `ConcealedMarkerSupport::DetectOnly` semantics landed in commits `7ce5fef`–`cb829ff`)
- **Critical Issues**: 0
- **High Issues**: 2
- **Total Findings**: 7
- **Estimated Remediation Effort**: 24–40 hours [Inferred] to implement and verify the full staged feature across macOS, Windows, and Linux; not a measured artifact
- **Evidence Commands Run**:
  - `grep -RInE 'paste_stack_spec|paste_next|Paste Next' README.md AGENTS.md docs src Cargo.toml .github || echo '(no matches)'` → only matches in prior review files (`docs/reviews/2026-11-21/*`); zero matches in `src/`, `README.md`, `AGENTS.md`, `Cargo.toml`, or `.github/`.
  - `grep -nE 'paste_next|PasteNext|paste-next' src/settings.rs src/app/settings_view.rs src/hotkey.rs src/main.rs src/app/mod.rs || echo '(no matches)'` → `(no matches)`
  - `grep -nE 'paste_worker|focus-paste-worker|compute_image_fingerprint|skip_next_capture|take_if_skipped|clear_skip_capture|MAX_IMAGE_PIXELS|simulate_paste' src/platform/mod.rs src/monitor.rs src/db.rs` →
    ```text
    src/platform/mod.rs:493:fn simulate_paste_if_foreground() {
    src/platform/mod.rs:512:fn paste_worker_loop(rx: &std::sync::mpsc::Receiver<PasteRequest>) {
    src/platform/mod.rs:616:            .name("focus-paste-worker".to_string())
    src/monitor.rs:13:const MAX_IMAGE_PIXELS: usize = 40_000_000; // Limit to ~40 MP to guard memory
    src/monitor.rs:15:fn compute_image_fingerprint(width: usize, height: usize, rgba: &[u8]) -> u64 {
    src/db.rs:322:    pub(crate) fn skip_next_capture(&self, text: &str) {
    src/db.rs:328:    pub(crate) fn clear_skip_capture(&self) {
    src/db.rs:335:    pub(crate) fn take_if_skipped(&self, text: &str) -> bool {
    ```

## Findings

### Critical
No findings in this category.

### High

**F1 — Coalesced Toggle-Window hotkey channel (`sync_channel(1)`) cannot carry ordered Paste Next presses**  
- **Problem framing / target user**: A user copies multiple snippets (`A`, `B`, `C`), focuses a target application, and presses a global "Paste Next" shortcut (`Cmd+Option+V` / `Ctrl+Alt+V`) repeatedly to paste `C`, `B`, `A` in LIFO order without opening the CipherClipboard window (`paste_stack_spec.md:11-26`).  
- **Severity**: High  
- **Evidence**:
  - `paste_stack_spec.md:57-72` identifies the coalescing bottleneck.
  - Current code in `src/main.rs:150-151`:
    ```rust
    let (notify_tx, notify_rx) = std::sync::mpsc::sync_channel(1);
    let (hotkey_tx, hotkey_rx) = std::sync::mpsc::sync_channel(1);
    ```
  - `src/hotkey.rs:112-117`:
    ```rust
    if matches && event.state == HotKeyState::Pressed {
        crate::platform::capture_active_window();
        // Best-effort coalesced wakeup; a full or closed channel needs no action.
        let _ = hotkey_tx.try_send(());
        ctx.request_repaint();
    }
    ```
  - `src/app/mod.rs:641-645`:
    ```rust
    fn handle_hotkey_toggle(&mut self, ctx: &egui::Context) {
        let mut toggle_requested = false;
        while self.hotkey_rx.try_recv().is_ok() {
            toggle_requested = true;
        }
    ```
  - And in the extracted Linux X11 Quote-key fallback (`src/platform/linux.rs:381`):
    ```rust
    pub(crate) fn new(hotkey_tx: SyncSender<()>, ctx: egui::Context) -> Self {
    ```
- **Risk / mitigation**: Reusing `hotkey_tx` (`SyncSender<()>`) or `handle_hotkey_toggle`'s drain-and-debounce loop (`200ms` debounce at `src/app/mod.rs:651-657`) would drop rapid Paste Next keystrokes and overwrite `capture_active_window()`. Both `HotkeyService` (`src/hotkey.rs`) and `platform::QuoteHotkeyFallback` (`src/platform/linux.rs:375-411`, `src/platform/mod.rs:31-54`) must distinguish `HotkeyAction::ToggleWindow` (coalesced `sync_channel(1)` + `capture_active_window()`) from `HotkeyAction::PasteNext` (separate bounded lossless channel, capacity >= 64, drained one event per press in `App::logic()` without calling `capture_active_window()`).
- **Recommendation**: Implement `HotkeyAction` routing and the dedicated Paste Next channel first, and gate on unit test AC-4 (`paste_stack_spec.md:259`: 10 rapid presses executed in FIFO order with zero drops).

**F2 — Existing `focus-paste-worker` is latest-wins and tied to `is_target_foreground()`**  
- **Problem framing / target user**: Each Paste Next press must write its clipboard payload and inject `Cmd+V` / `Ctrl+V` into the *currently focused* window in strict FIFO order, without racing against previous presses (`paste_stack_spec.md:82-126`).  
- **Severity**: High  
- **Evidence**:
  - `src/platform/mod.rs:512-566` (`paste_worker_loop`) coalesces bursts and supersedes pending requests during `auto_paste_delay_ms`:
    ```rust
    loop {
        match rx.try_recv() {
            Ok(newer) => req = newer,
            Err(std::sync::mpsc::TryRecvError::Empty) => break,
            Err(std::sync::mpsc::TryRecvError::Disconnected) => return,
        }
    }
    ...
    match rx.recv_timeout(Duration::from_millis(req.auto_paste_delay_ms)) {
        Ok(newer) => {
            pending = Some(newer);
            continue;
        }
        Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => break,
        Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {}
    }
    ```
  - `src/platform/mod.rs:493-508` (`simulate_paste_if_foreground`) requires `backend::is_target_foreground()`, which checks against the window captured before CipherClipboard took focus (`PREVIOUS_APP_PID` on macOS, `PREVIOUS_WINDOW` on Windows, `PREVIOUS_X11_WINDOW` on Linux):
    ```rust
    fn simulate_paste_if_foreground() {
        #[cfg(any(target_os = "macos", target_os = "windows", target_os = "linux"))]
        {
            if !backend::is_target_foreground() {
                log_message(
                    "WARN",
                    "Auto-paste skipped: captured target window/application is not in the foreground",
                );
                return;
            }
    ```
- **Risk / mitigation**: Because Paste Next never summons the CipherClipboard window and never calls `capture_active_window()`, `backend::is_target_foreground()` will either be uninitialized (`false`) or point to a stale window from an earlier Toggle summon. Furthermore, coalescing would discard intermediate items when the user presses Paste Next rapidly.
- **Recommendation**: Add a separate `paste-next-worker` owned by `PasteNextWorker` (mirroring `PasteWorker` in `src/platform/mod.rs:571-608`, joined via `platform::shutdown_workers()` at `src/app/mod.rs:1609`) that executes jobs in strict FIFO order, waits for hotkey modifier release (`paste_stack_spec.md:138-143`), writes the payload, waits `settle_ms`, and invokes foreground paste simulation serialized via a shared `Mutex<()>` with `focus-paste-worker`.

### Medium

**F3 — Recent `src/platform/` refactoring (`6128b9a`–`cb829ff`) requires spec §2 & §8.1 adjustments to preserve architectural invariants**  
- **Problem framing / target user**: Contributors implementing `paste_stack_spec.md` against current `HEAD` (`20db1a5`).  
- **Severity**: Medium  
- **Evidence**:
  - `AGENTS.md:17` (added in commit `cb829ff`) enforces:
    ```markdown
    - **Confine platform `#[cfg(target_os = ...)]` / `#[cfg(unix)]` attributes to `src/platform/` (`mod.rs`, `fs_security.rs`, `linux.rs`, `macos.rs`, `windows.rs`).** Do not scatter OS-conditional `#[cfg]` blocks across application, UI, settings, hotkey, theme, or crypto modules; instead, expose OS-agnostic capability helpers, constants, or no-op fallback types from `src/platform/`.
    ```
  - Commit `6128b9a` moved the X11 Quote fallback out of `src/hotkey.rs` into `src/platform/linux.rs:357-411` (`QuoteHotkeyFallback`), whereas `paste_stack_spec.md:43` and `65` still refer to `linux_x11_fallback` inside `src/hotkey.rs`:
    ```markdown
    | Hotkey routing | [`src/hotkey.rs`](src/hotkey.rs) (including `linux_x11_fallback`) | Tag events with `HotkeyAction`, add a lossless Paste Next channel (§3). |
    ```
  - `paste_stack_spec.md:216` proposes `default_paste_next_hotkey` in `src/settings.rs` returning `"Cmd+Option+V"` on macOS and `"Ctrl+Alt+V"` on other platforms.
- **Risk / mitigation**: Implementing `default_paste_next_hotkey()` with `#[cfg(target_os = "macos")]` inside `src/settings.rs` would violate `AGENTS.md:17`. Instead, `default_paste_next_hotkey()` in `src/settings.rs` should use `crate::platform::PRIMARY_MODIFIER_NAME` (or a new constant `crate::platform::DEFAULT_PASTE_NEXT_HOTKEY` defined in `src/platform/mod.rs`), and any `QuoteHotkeyFallback` action-tagging changes must be made in `src/platform/linux.rs` and `src/platform/mod.rs:31-54`.
- **Recommendation**: Update `paste_stack_spec.md` §2, §3.2, and §8.1 to reference `src/platform/linux.rs` (`QuoteHotkeyFallback`) and `src/platform/mod.rs` (`DEFAULT_PASTE_NEXT_HOTKEY`) so no `#[cfg]` is introduced outside `src/platform/`.

**F4 — Secret-handling and Linux `ConcealedMarkerSupport::DetectOnly` interaction must be explicit**  
- **Problem framing / target user**: Security-conscious users whose Paste Stack includes `ItemKind::Secret` entries (`paste_stack_spec.md:23,160-167`) or who enable `preserve_secrets_on_prune` (added in commit `a57aa64`).  
- **Severity**: Medium  
- **Evidence**:
  - `src/app/mod.rs:539-567` (`copy_secret_to_clipboard`) sets `self.db.skip_next_capture(secret)`, writes to clipboard, calls `crate::platform::mark_clipboard_concealed()`, and schedules `self.schedule_secret_clipboard_clear(secret)`.
  - Commit `7ce5fef` introduced `ConcealedMarkerSupport::DetectOnly` on Linux (`src/platform/mod.rs:303-329`) because `arboard` owns the X11 `CLIPBOARD` selection and Linux cannot emit `x-kde-passwordManagerHint` on outgoing writes (`src/platform/linux.rs:234-243`).
  - Commit `a57aa64` added `preserve_secrets_on_prune` (`src/settings.rs:461-462`, `src/db.rs:308-319`), which keeps `ItemKind::Secret` entries in history when older non-secret entries are pruned.
- **Risk / mitigation**:
  1. On Linux, `mark_clipboard_concealed()` is a documented no-op (`ConcealedMarkerSupport::DetectOnly`), so `self.db.skip_next_capture(&secret)` inside `paste-next-worker` is the **sole** barrier preventing the background monitor from re-capturing a Paste Next secret and resetting `stack_cursor` to `0` on Linux.
  2. When `preserve_secrets_on_prune = true`, older preserved secrets remain at higher indices in `db.get_recent(...)`; accidental extra presses of Paste Next after recent non-secret items could paste a preserved secret into an untrusted window if the user does not realize secrets are in the stack.
- **Recommendation**: Ensure the single-slot `skip_next_capture` marker is set inside `paste-next-worker` immediately before `cb.set_text(...)` on all platforms, and consider an optional toggle or disclosure in Paste Next settings noting that `ItemKind::Secret` items are included in the stack (or offering an "Exclude secrets from Paste Next" checkbox).

**F5 — Settings persistence and conflict detection against multi-hotkey presets (`AzertyQuote`)**  
- **Problem framing / target user**: Existing users upgrading to a version with Paste Next, and users on French/Belgian AZERTY layouts where `HotkeyPreset::AzertyQuote` registers two hotkeys (`Cmd/Ctrl+ù` and `Cmd/Ctrl+Shift+ù`).  
- **Severity**: Medium  
- **Evidence**:
  - `src/settings.rs:433-463` (`AppSettings`) and `src/settings.rs:509-531` (`impl Default for AppSettings`) currently have 17 fields ending with `preserve_secrets_on_prune: bool` and no `paste_next_enabled` or `paste_next_hotkey`.
  - `src/settings.rs:558-568` (`effective_hotkeys`) expands a single `HotkeyPreset` into `Vec<HotKey>` (2 hotkeys for `AzertyQuote`).
  - `paste_stack_spec.md:212-219,235-240` specifies `#[serde(default)]` fields and bidirectional conflict rejection against all active toggle hotkey IDs (`HotKey::id()`).
- **Risk / mitigation**: Without `#[serde(default)]` and backward-compatible JSON deserialization tests, upgrading could reset user preferences in `app_settings`. Without checking every `HotKey::id()` returned by `effective_hotkeys()`, a user could bind Paste Next to one of the two `AzertyQuote` variants or to `Cmd+V`/`Ctrl+V`.
- **Recommendation**: Land `AppSettings` fields, `#[serde(default)]` attributes, `Cmd+V`/`Ctrl+V` rejection, and `active_toggle_hotkeys()` conflict checks with unit tests in `src/settings.rs` as PR 1 before any worker or UI changes.

**F6 — Cross-platform synthetic paste limitations (macOS Accessibility, Windows UIPI on foreground window, Linux pure Wayland)**  
- **Problem framing / target user**: Users invoking Paste Next when macOS Accessibility is missing, when the currently focused Windows app runs elevated (Administrator), or on Linux pure Wayland without X11 (`paste_stack_spec.md:244-248`).  
- **Severity**: Medium  
- **Evidence**:
  - On Windows, current UIPI detection checks `PREVIOUS_WINDOW` (`src/platform/windows.rs:600` `unsafe fn captured_target_pid()`, called by `target_is_elevated()` at `src/platform/mod.rs:427`). For Paste Next, `capture_active_window()` is *not* called (`paste_stack_spec.md:69`), so `PREVIOUS_WINDOW` does not hold the target window! Checking `target_is_elevated()` as-is would inspect the wrong PID.
  - On Linux, `src/platform/linux.rs:212-232` (`simulate_paste`) uses XTest inside `with_x11(...)` and returns `()` without reporting whether X11 was unavailable (`X11State::Unavailable`).
  - On macOS, `src/platform/macos.rs` checks `is_trusted()` (`src/platform/mod.rs:125-134`).
- **Risk / mitigation**: This is a key technical finding from reading `src/platform/windows.rs:600`: `paste_stack_spec.md:246` says *"Windows: `SendInput` fails into elevated (Administrator) targets. Reuse the existing detection and report `Failed { reason: "target elevated" }`."* However, the existing `target_is_elevated()` inspects `captured_target_pid()` (`PREVIOUS_WINDOW`), whereas Paste Next pastes into the *current* foreground window (`GetForegroundWindow()`). Reusing `target_is_elevated()` unmodified would check a stale or zero PID (`uipi_should_skip_paste(0, ...)` returns `false` at `src/platform/mod.rs:364-366`), failing open into an elevated window!
- **Recommendation**: Add `foreground_target_is_elevated()` in `src/platform/windows.rs` (querying the PID of `GetForegroundWindow()` directly and feeding it into `integrity_level(pid)` and `uipi_should_skip_paste`), return `bool` from `linux::simulate_paste()` so pure-Wayland unavailability is reported as `PasteNextOutcome::Failed`, and check `platform::auto_paste_permission_missing()` on macOS before simulating.

### Low

**F7 — `paste_stack_spec.md` is untracked in `README.md` and `docs/` navigation [Inferred]**  
- **Problem framing / target user**: Contributors and product owners reviewing planned features.  
- **Severity**: Low  
- **Evidence**: `paste_stack_spec.md` sits at the repository root (`/home/antigravity/BrunoO/clipboard/paste_stack_spec.md`, lines 1–294) with status `Draft v2` and all 22 acceptance criteria (`AC-1`..`AC-22`) unchecked (`paste_stack_spec.md:254-287`). `grep -RInE 'paste_stack_spec' README.md AGENTS.md docs src` returns zero matches outside `docs/reviews/`.
- **Risk / mitigation**: [Inferred] Keeping a 294-line design spec at the repo root without linking it from `README.md` or `docs/` makes it easy for the spec to drift as architecture refactors land (as seen in F3 and F6).
- **Recommendation**: Move or link `paste_stack_spec.md` under `docs/` and update it with findings F3 and F6 before starting implementation.

## Quick Wins
1. Update `paste_stack_spec.md` §2, §8.1, and §9 to reflect `src/platform/linux.rs` (`QuoteHotkeyFallback`), the `AGENTS.md` ban on `#[cfg]` outside `src/platform/`, and the `GetForegroundWindow()` PID requirement for Windows UIPI checks (F3, F6, F7) — ~30 minutes [Inferred].
2. Implement PR 1 (`src/settings.rs` + `src/platform/mod.rs`): `paste_next_enabled`, `paste_next_hotkey`, `DEFAULT_PASTE_NEXT_HOTKEY`, backward-compatible JSON round-trip test, and conflict detection against `effective_hotkeys()` and `Cmd/Ctrl+V` (F3, F5) — ~2 hours [Inferred].
3. Add `HotkeyAction::{ToggleWindow, PasteNext}` and a dedicated lossless Paste Next channel with a 10-press burst unit test (F1) — ~3 hours [Inferred].
4. Extract a shared helper for secret clipboard writes (`skip_next_capture` + `set_text` + `mark_clipboard_concealed` + `schedule_secret_clipboard_clear`) so `copy_secret_to_clipboard` and Paste Next cannot diverge (F4) — ~1.5 hours [Inferred].

## Recommended Actions

### Problem Statement and Target User
- **Target user**: Power users and developers who copy multiple fields, tokens, or snippets from one source window and want to paste them sequentially in LIFO order into a destination window without summoning or interacting with the CipherClipboard popup (`paste_stack_spec.md:11-34`).
- **Observed vs. Inferred**: Observed as an explicit repository specification (`paste_stack_spec.md:1-294`); user demand and workflow frequency are `[Inferred]`.
- **Smallest valuable version (MVP)**: Opt-in (`paste_next_enabled = false`), text-based items (`Text`, `Url`, `Code`, `Color`, `Secret`) with full secret-masking/skip/conceal/auto-clear parity, lossless hotkey channel, dedicated `paste-next-worker`, and Settings toggle/recorder/conflict UI. Defer image payloads (`ItemKind::Image`) to a follow-up PR.
- **Local success measurement (no telemetry)**: Deterministic verification via `AC-1` through `AC-22` in `paste_stack_spec.md:253-287` plus the standard pre-flight gates (`cargo fmt -- --check`, `cargo clippy --all-targets --all-features -- -D warnings`, `cargo test --all-targets`).

### Evidence from Repository
- **Specification**: `paste_stack_spec.md:1-294`.
- **Hotkey & coalescing baseline**: `src/main.rs:150-152`, `src/hotkey.rs:17-168`, `src/platform/linux.rs:357-411`, `src/app/mod.rs:641-670`.
- **Paste worker & platform guards**: `src/platform/mod.rs:303-371,420-625`, `src/platform/windows.rs:597-670`, `src/platform/linux.rs:212-243`.
- **Database skip markers & image fingerprinting**: `src/db.rs:253-261,321-343`, `src/monitor.rs:12-29,74-98`.
- **Settings & secret copy baseline**: `src/settings.rs:433-568`, `src/app/mod.rs:457-487,539-567`, `src/app/settings_view.rs:98-325`.

### Options Comparison Table

| Option | Scope & Modules Touched | New Dependencies / `unsafe` | Effort / Risk [Inferred] | Verdict |
|---|---|---|---|---|
| **Option A: Defer / Keep Spec Only** | Update `paste_stack_spec.md` for F3/F6 and link from `docs/` | 0 crates; 0 `unsafe` | ~30 min; zero regression risk, delivers no user feature | Defer only if prioritizing release stabilization |
| **Option B: Big-Bang Full Spec v2 (Text + Images)** | `src/settings.rs`, `src/hotkey.rs`, `src/main.rs`, `src/platform/{mod,macos,windows,linux}.rs`, `src/db.rs`, `src/monitor.rs`, `src/media.rs`, `src/app/{mod,history_view,settings_view}.rs` | 0 crates; small `unsafe` addition in `macos.rs` (`CGEventSourceFlagsState`) and `windows.rs` (`GetAsyncKeyState`, foreground PID UIPI check, modifier key-up `SendInput`) | 24–40 hours [Inferred]; high review complexity if landed in one PR | **Reject as single PR** |
| **Option C: Staged Opt-In Build (Text MVP first, Images second)** | PRs 1–4 cover settings, lossless hotkey channel, `paste-next-worker` + text/secret path, and UI; PR 5 adds image fingerprint skip markers | 0 crates; `unsafe` strictly confined to `src/platform/macos.rs` and `src/platform/windows.rs` | 16–24 hours [Inferred] for Text MVP + 6–10 hours [Inferred] for Images; low per-PR risk | **Recommended (Build, Staged)** |

### Risks and Mitigations
- **Concurrency & shutdown determinism (High — F1, F2)**: `paste-next-worker` must be owned by a `PasteNextWorker` struct in `src/platform/mod.rs` and joined inside `platform::shutdown_workers()` (`src/app/mod.rs:1609`), handling `Disconnected` on its channel and sharing a keystroke `Mutex<()>` with `focus-paste-worker`.
- **Windows UIPI false-negative on foreground window (Medium — F6)**: Do not call `target_is_elevated()` (which reads `PREVIOUS_WINDOW`); inspect the current `GetForegroundWindow()` PID via `uipi_should_skip_paste`.
- **Platform `#[cfg]` confinement (Medium — F3)**: Keep all OS-specific constants (`DEFAULT_PASTE_NEXT_HOTKEY`), modifier checks, and `QuoteHotkeyFallback` routing inside `src/platform/`.
- **Secret leakage & Linux `DetectOnly` (Medium — F4)**: Set `db.skip_next_capture` inside the worker immediately before writing to the clipboard; wrap secret payloads in `zeroize::Zeroizing<String>`; restart `schedule_secret_clipboard_clear` on each secret paste and cancel it on a subsequent non-secret paste.

### MVP Scope
- `paste_next_enabled: bool` (default `false`) and `paste_next_hotkey: String` (default `Cmd+Option+V` / `Ctrl+Alt+V` via `src/platform/mod.rs`).
- Bidirectional conflict detection against `effective_hotkeys()` (including both `AzertyQuote` hotkeys) and rejection of `Cmd+V` / `Ctrl+V`.
- `HotkeyAction::{ToggleWindow, PasteNext}` with lossless Paste Next delivery.
- `paste-next-worker` for all text item kinds (`Text`, `Url`, `Code`, `Color`, `Secret`) with modifier-release wait, foreground UIPI/Accessibility/Wayland checks, and secret conceal/auto-clear parity.
- `stack_cursor` LIFO state in `ClipboardApp`, `NEXT` badge / header chip in `src/app/history_view.rs`, and Settings recorder/status UI in `src/app/settings_view.rs`.

### Recommendation and PR Breakdown
**Recommendation: Build (Staged — Option C).** Decompose into 5 small, reviewable PRs:
1. **PR 1 — Pure settings logic & conflict rules**: Add `paste_next_enabled` and `paste_next_hotkey` to `src/settings.rs` using a platform constant from `src/platform/mod.rs`, add conflict validation against `effective_hotkeys()` and `Cmd/Ctrl+V`, and add legacy JSON round-trip unit tests (`AC-1`, `AC-2`, `AC-17`, `AC-18`).
2. **PR 2 — Platform facade, foreground UIPI/modifier helpers, and `paste-next-worker`**: Add modifier-release and foreground-paste helpers in `src/platform/{mod,macos,windows,linux}.rs` (including `foreground_target_is_elevated()` on Windows and boolean return on Linux X11), spawn/join `paste-next-worker` in `src/platform/mod.rs`, and add mock-backend queue-ordering unit tests (`AC-4`, `AC-6`, `AC-7`, `AC-20`, `AC-21`).
3. **PR 3 — Lossless hotkey action routing & DB skip markers**: Extend `HotkeyService` (`src/hotkey.rs`) and `QuoteHotkeyFallback` (`src/platform/linux.rs`, `src/platform/mod.rs`) with `HotkeyAction`, wire the Paste Next channel in `src/main.rs`, and (if image support is included) move `image_fingerprint` to `src/media.rs` and add image skip markers to `src/db.rs` and `src/monitor.rs` (`AC-3`, `AC-5`, `AC-8`, `AC-9`).
4. **PR 4 — `App::logic()` cursor handler, secret auto-clear parity & UI wiring**: Wire `handle_paste_next` and outcome reconciliation in `src/app/mod.rs` (`App::logic()`), add the `NEXT` badge and reset action in `src/app/history_view.rs`, and add the Paste Next recorder group in `src/app/settings_view.rs` (`AC-10`–`AC-16`, `AC-19`).
5. **PR 5 — Documentation & CI verification**: Update `README.md`, `paste_stack_spec.md`, and `docs/TESTING.md`, and verify `cargo fmt -- --check`, `cargo clippy --all-targets --all-features -- -D warnings`, and `cargo test --all-targets` (`AC-22`).

### Open Questions
1. Should `ItemKind::Secret` entries be included in Paste Next by default (as written in `paste_stack_spec.md:23`), or should an "Include secrets in Paste Next" toggle be provided given `preserve_secrets_on_prune` (`src/settings.rs:462`) and Linux's `ConcealedMarkerSupport::DetectOnly` limitation (`src/platform/mod.rs:312-329`)?
2. Should image payloads (`ItemKind::Image`, `paste_stack_spec.md:154-158`) ship in the initial milestone or be deferred to a follow-up PR after text/secret Paste Next stabilizes?
3. When Paste Next fails while the window is hidden (e.g., Windows UIPI elevation block, macOS missing Accessibility, or Linux pure Wayland), should the failure only queue a `pending_toast` for the next `show_window()` (`src/app/mod.rs:422-424`), or should it also advance/preserve the cursor with an audible/visual cue?

## Coverage
- **1. Problem Framing (target user, observed vs `[Inferred]`, smallest valuable version, local success measurement)**: Covered in Findings F1, F7 and Recommended Actions → Problem Statement and Target User.
- **2. Product Fit (encryption/privacy positioning, secret handling, keyring/permissions, network access)**: Covered in Findings F4, F6. No network access or new secret class is introduced.
- **3. Rust Implementation Options (modules touched, Cargo dependencies, target APIs, error types, threads/channels, `logic()` vs `ui()`, DB schema/SQLCipher)**: Covered in Findings F1, F2, F3, F5, F6 and Options Comparison Table. No new Cargo dependencies or SQLite table schema migrations are required (`app_settings` stores JSON).
- **4. Cost and Risk (cross-platform parity, supply chain, FFI/`unsafe` confinement, concurrency/shutdown, performance, testing)**: Covered in Findings F1, F2, F3, F4, F6 and Risks and Mitigations.
- **5. Rollout Plan (feature gating/opt-in default, settings migration, documentation updates, release checklist)**: Covered in Finding F5, MVP Scope, and PR Breakdown (PRs 1–5).
- **6. Recommendation (Build / Defer / Reject with evidence-tied reasons and PR decomposition)**: Covered in Recommendation and PR Breakdown (Build, Staged — Option C).
