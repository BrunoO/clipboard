# Feature Exploration Review - 2026-11-21

## Executive Summary
- **Health Score**: 7/10 ([Inferred] weighting of 2 high, 4 medium and 1 low evidence-backed findings; the proposal is unusually well grounded in current code, but it is still an unshipped draft with cross-platform/secret-handling prerequisites)
- **Critical Issues**: 0
- **High Issues**: 2
- **Total Findings**: 7
- **Estimated Remediation Effort**: 24–40 hours [Inferred] to implement the proposed opt-in feature with its documented verification; not a measured artifact
- **Evidence Commands Run**:
  - `grep -RInE 'paste_stack_spec|paste_next|Paste Next' README.md AGENTS.md docs src Cargo.toml .github || echo '(no matches)'` → `(no matches)`
  - `grep -nE 'paste_next|PasteNext|paste-next' src/settings.rs src/app/settings_view.rs src/hotkey.rs src/main.rs src/app/mod.rs || echo '(no matches)'` → `(no matches)`
  - `grep -nE 'paste_worker|focus-paste-worker|compute_image_fingerprint|skip_next_capture|take_if_skipped|clear_skip_capture|MAX_IMAGE_PIXELS|simulate_paste' src/platform/mod.rs src/monitor.rs src/db.rs` → relevant output:
    ```text
    src/platform/mod.rs:212:fn simulate_paste_if_foreground() {
    src/platform/mod.rs:250:fn paste_worker_loop(rx: &std::sync::mpsc::Receiver<PasteRequest>) {
    src/platform/mod.rs:296:            .name("focus-paste-worker".to_string())
    src/platform/mod.rs:315:    let _ = paste_worker_tx().send(PasteRequest {
    src/monitor.rs:13:const MAX_IMAGE_PIXELS: usize = 40_000_000; // Limit to ~40 MP to guard memory
    src/monitor.rs:15:fn compute_image_fingerprint(width: usize, height: usize, rgba: &[u8]) -> u64 {
    src/db.rs:239:    pub(crate) fn skip_next_capture(&self, text: &str) {
    src/db.rs:245:    pub(crate) fn clear_skip_capture(&self) {
    src/db.rs:252:    pub(crate) fn take_if_skipped(&self, text: &str) -> bool {
    ```
  - **Explicit proposal status**: an explicit proposal exists — `paste_stack_spec.md` ("Specification: Paste Stack (\"Paste Next\" Hotkey)", `paste_stack_spec.md:1`), so the "no explicit proposal" branch does not apply.

## Findings

### Critical
No findings in this category.

### High

**F1 — Lossless, action-tagged hotkey delivery is a blocking prerequisite, not a detail**  
- **Problem framing / target user**: A power user copies several items, switches applications, and presses a dedicated Paste Next hotkey repeatedly; every press must paste exactly one next item in LIFO order. [Inferred] product judgment.
- **Evidence**: The spec identifies the current coalescing path correctly at `paste_stack_spec.md:57-72`:
  ```text
  Today the listener in [`src/hotkey.rs`](src/hotkey.rs) sends `()` over a `sync_channel(1)` (created in [`src/main.rs`](src/main.rs)) with `try_send`. The app drains it with `while self.hotkey_rx.try_recv().is_ok()` in [`src/app/mod.rs`](src/app/mod.rs). Coalescing is correct for Toggle Window but would **drop** Paste Next presses.
  ```
  Current code confirms it: `src/main.rs:146-148`:
  ```rust
  // 7. Setup inter-thread channels (bounded capacity 1 for coalesced wakeup signals)
  let (notify_tx, notify_rx) = std::sync::mpsc::sync_channel(1);
  let (hotkey_tx, hotkey_rx) = std::sync::mpsc::sync_channel(1);
  ```
  `src/hotkey.rs:122-125`:
  ```rust
  if matches && event.state == HotKeyState::Pressed {
      crate::platform::capture_active_window();
      let _ = hotkey_tx.try_send(());
      ctx.request_repaint();
  }
  ```
  `src/app/mod.rs:551-555`:
  ```rust
  fn handle_hotkey_toggle(&mut self, ctx: &egui::Context) {
      let mut toggle_requested = false;
      while self.hotkey_rx.try_recv().is_ok() {
          toggle_requested = true;
      }
  ```
- **User-visible problem**: If Paste Next reuses this path, rapid presses are merged into one toggle/one paste, breaking LIFO order and the core product promise.
- **Risk / mitigation**: Treat spec §3.2 as mandatory: add `HotkeyAction::{ToggleWindow, PasteNext}`, keep Toggle Window coalesced, give Paste Next a separate lossless channel, do not call `capture_active_window()` for Paste Next, drain one event per press in `App::logic()`, and handle `Disconnected` on both channels (`paste_stack_spec.md:60-72`).
- **Recommendation**: Reject any implementation that routes Paste Next through `hotkey_tx`; add the AC-3/AC-4 ordering tests (`paste_stack_spec.md:258-259`) before UI work.

**F2 — The existing auto-paste worker is latest-wins and must not be reused**  
- **Evidence**: Spec `paste_stack_spec.md:82-89`:
  ```text
  `paste_worker_loop` in [`src/platform/mod.rs`](src/platform/mod.rs) is designed so that **the latest auto-paste wins**:
  - it coalesces bursts to the newest request,
  - it cancels a pending paste if a newer request arrives during `auto_paste_delay_ms`,
  - it restores focus to the captured window,
  - it calls `simulate_paste_if_foreground()`, which checks against the **captured** target window.
  ```
  Current implementation `src/platform/mod.rs:261-264` and `src/platform/mod.rs:276-285`:
  ```rust
  // Coalesce any burst of queued requests, keeping only the latest.
  while let Ok(newer) = rx.try_recv() {
      req = newer;
  }
  ```
  ```rust
  match rx.recv_timeout(Duration::from_millis(req.auto_paste_delay_ms)) {
      Ok(newer) => {
          pending = Some(newer);
          continue;
      }
      Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => break,
      Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {}
  }

  simulate_paste_if_foreground();
  ```
  The foreground check is target-captured, not current-focused: `src/platform/mod.rs:215-222`:
  ```rust
  if !macos::is_target_foreground() {
      log_message(
          "WARN",
          "Auto-paste skipped: captured target application is not frontmost",
      );
      return;
  }
  ```
- **User-visible problem**: Queueing Paste Next jobs through `focus-paste-worker` would drop middle entries, restore focus to a captured window that Paste Next never captured, and fail foreground checks after the app stays hidden.
- **Risk / mitigation**: Add the separate named `paste-next-worker` from spec §4 (`paste_stack_spec.md:92-126`) with FIFO jobs, no coalescing, foreground (not captured-target) simulation, shared `Mutex<()>` around the write→simulate section, and clean `Disconnected`/shutdown handling.
- **Recommendation**: Keep `restore_focus_and_paste()` (`src/platform/mod.rs:305-320`) untouched; AC-5 and AC-6 (`paste_stack_spec.md:260-261`) are regression gates for the existing auto-paste path.

### Medium

**F3 — The proposal is an orphaned Draft v2 with no implementation or discovery path**  
- **Evidence**: `paste_stack_spec.md:3`:
  ```text
  > Status: **Draft v2.** Rewritten after code review to match the current module layout (`src/app/*`, `src/platform/*`) and to address the hotkey delivery, paste dispatch, image skip, secret handling, modifier and cursor issues raised in review.
  ```
  All acceptance boxes remain unchecked, e.g. `paste_stack_spec.md:254-259`:
  ```text
  - [ ] AC-1: With `paste_next_enabled = false` (default), the Paste Next hotkey is not registered, and pressing it reaches the focused app unchanged.
  - [ ] AC-2: Old settings JSON without the new fields loads with `paste_next_enabled = false` and the platform default hotkey.

  **Ordering and delivery**
  - [ ] AC-3: After copying `A`, `B`, `C`, three presses paste `C`, `B`, `A` into the focused app, in order, with the window never shown.
  - [ ] AC-4: Ten presses in under one second each run, in order (no drops, no merging). Verified with a unit test on the worker queue using a mock paste backend.
  ```
  `grep -RInE 'paste_stack_spec|paste_next|Paste Next' README.md AGENTS.md docs src Cargo.toml .github || echo '(no matches)'` → `(no matches)`; `grep -nE 'paste_next|PasteNext|paste-next' src/settings.rs src/app/settings_view.rs src/hotkey.rs src/main.rs src/app/mod.rs || echo '(no matches)'` → `(no matches)`.
- **Product fit**: [Inferred] The spec is high quality but undiscoverable; neither users nor contributors can tell whether it is planned, deferred, or abandoned.
- **Recommendation**: Link the spec from README "Key Features"/"Known Limitations" or `docs/`, record a decision (Build/Defer/Reject) in the repository, and check off acceptance criteria as PRs land.

**F4 — Secret-handling parity must be reused, not reimplemented, in the new worker**  
- **Evidence**: Existing secret copy path `src/app/mod.rs:449-475`:
  ```rust
  fn copy_secret_to_clipboard(&mut self, secret: &str, ctx: &egui::Context) {
      self.db.skip_next_capture(secret);
      let Some(cb) = self.ensure_clipboard_writer() else {
          self.db.clear_skip_capture();
          self.show_toast(&format!("{} Clipboard unavailable", icons::WARNING));
          return;
      };
      ...
          crate::platform::mark_clipboard_concealed();
          self.schedule_secret_clipboard_clear(secret);
  ```
  Spec §6.3 requires the same protections at `paste_stack_spec.md:161-167`:
  ```text
  A secret pasted by Paste Next gets the **same protections** as `copy_secret_to_clipboard` in [`src/app/mod.rs`](src/app/mod.rs):
  1. Skip capture (§6.1).
  2. Mark the clipboard as concealed (`platform::mark_clipboard_concealed()`), done in the worker right after the write (`conceal: true`).
  3. Schedule auto-clear according to `auto_clear_secret_secs`, via the existing `schedule_secret_clipboard_clear`.
  ```
- **Risk / mitigation**: A new worker writing secrets without the single-slot skip marker, OS concealed marker, zeroized buffer, and restarted auto-clear would violate `docs/SECURITY_LESSONS.md:28` and `docs/SECURITY_LESSONS.md:12`. Extract shared helpers from `copy_secret_to_clipboard` rather than duplicating the sequence; keep skip markers set inside the serialized job (`paste_stack_spec.md:152`).
- **Recommendation**: Gate the secret path on AC-8/AC-10/AC-11 (`paste_stack_spec.md:265-270`) and add a test that no secret or clipboard content reaches release logs.

**F5 — Settings schema and migration are not yet present**  
- **Evidence**: `src/settings.rs:445-478` declares `AppSettings` with no Paste Next fields:
  ```rust
  pub(crate) struct AppSettings {
      pub(crate) hotkey: HotkeyPreset,
      ...
      #[serde(default = "default_keyring_explanation_shown")]
      pub(crate) keyring_explanation_shown: bool,
  }
  ```
  `src/settings.rs:527-547` (`impl Default`) likewise has no Paste Next entries. Spec §8.1 requires `#[serde(default)] paste_next_enabled` and `#[serde(default = "default_paste_next_hotkey")] paste_next_hotkey` plus an old-JSON round-trip test (`paste_stack_spec.md:212-219`).
- **Risk / mitigation**: Adding fields without serde defaults can break existing `app_config` rows stored in SQLCipher; a conflicting default hotkey can grab `Cmd+V`/`Ctrl+V`. Add the defaults, a conflict check against `active_toggle_hotkeys()` (`paste_stack_spec.md:235-240`), and AC-2/AC-17/AC-18 tests.
- **Recommendation**: Keep `paste_next_enabled = false` by default and land settings persistence/migration as the first isolated PR.

**F6 — Cross-platform parity gaps require explicit degradation, not silent failure**  
- **Evidence**: Spec §9 (`paste_stack_spec.md:245-247`):
  ```text
  - **macOS:** the simulated paste requires **Accessibility** permission (same as `auto_paste`).
  - **Windows:** `SendInput` fails into elevated (Administrator) targets. Reuse the existing detection and report `Failed { reason: "target elevated" }`.
  - **Linux:** X11 via XTest. On Wayland without X11 the simulated paste is unavailable: report `Failed` and show a one-time note in Settings.
  ```
  Matching invariants: `docs/MULTIPLATFORM_LESSONS.md:22` (live tray/minimize fallback), `docs/MULTIPLATFORM_LESSONS.md:41` (macOS accessibility status/deep link), `docs/MULTIPLATFORM_LESSONS.md:49` (pure Wayland fallback), and `docs/SECURITY_LESSONS.md:30` (Windows UIPI fail closed).
- **Risk / mitigation**: [Inferred] Without per-platform failure reasons and a visible Settings disclosure, users on Wayland or elevated targets will see nothing happen and may assume data loss. Reuse `platform::windows::target_is_elevated()` / UIPI toast pattern (`src/app/mod.rs:359-369`) and the existing macOS accessibility notice (`src/app/settings_view.rs:384-413`).
- **Recommendation**: Make "unavailable on this platform/session" a first-class `PasteNextOutcome::Failed` reason surfaced when the window is next shown; do not silently register a grab that cannot deliver.

### Low

**F7 — Product-promise opportunity: fast recall currently means one item at a time [Inferred]**  
- **Evidence**: `README.md:14` promises:
  ```text
  * **Global Hotkey & Fast Recall:** Configurable global summon shortcut (`Ctrl+Shift+V` / `Cmd+Shift+V`) with single-instance window restoration and optional auto-paste.
  ```
  The Settings UI exposes one global shortcut group (`src/app/settings_view.rs:95-121`) and optional auto-paste (`src/app/settings_view.rs:362-372`); no bulk/stack recall exists (grep outputs above).
- **Product fit**: [Inferred] Paste Next reinforces the existing encrypted, local, masking-preserving promise — it creates no new secret class and no network access — but it is an opt-in power-user workflow, not a fix for an observed defect.
- **Recommendation**: Position it as "optional multi-item recall" with no telemetry; measure success locally through the spec's acceptance tests rather than adoption metrics.

## Quick Wins
1. Link `paste_stack_spec.md` from the docs/README and record a Build/Defer decision so the draft is discoverable (F3) — about 15 minutes [Inferred].
2. Add the `AppSettings` fields with serde defaults and the old-JSON round-trip test before any UI work (F5) — about 2 hours [Inferred].
3. Write the `HotkeyAction` + lossless-channel unit test that proves ten rapid presses are neither dropped nor merged (F1) — about 3 hours [Inferred].
4. Extract the existing secret-copy sequence into a reusable helper so Paste Next cannot drift from masking/conceal/auto-clear (F4) — about 2 hours [Inferred].

## Recommended Actions

**Problem statement and target user** — [Inferred] A power user who copies several items from one application and pastes them into another wants repeated, invisible, ordered recall without reopening the window. The problem is observed in this repository as a written proposal (`paste_stack_spec.md:1-11`), not as a user-reported defect.

**Repository evidence** — `paste_stack_spec.md:1-293` is the explicit proposal; `src/hotkey.rs:83-125`, `src/main.rs:146-148`, `src/app/mod.rs:551-555`, and `src/platform/mod.rs:250-320` establish the current coalescing/latest-wins behavior; `src/app/mod.rs:449-475` and `src/settings.rs:445-547` establish the secret and settings baselines.

**Options comparison** ([Inferred] cost estimates)

| Option | Value | Cost / risk | Verdict |
|---|---|---|---|
| A. Do nothing; keep the draft | No new code or FFI surface | No new value; opportunity cost remains | Defer only if platform capacity is unavailable |
| B. Implement full spec v2 including images | Highest feature completeness | 24–40 h [Inferred]; image skip markers, new worker, and 3-OS paste verification all at once | Reject as a single PR |
| C. Staged opt-in text-only MVP, then images | Delivers core LIFO recall with existing secret protections | 16–24 h [Inferred] for MVP; risk bounded by PR gates | **Recommend** |

**Risks and mitigations** — F1/F2 are hard blockers (lossless delivery, dedicated worker); F4 is a security invariant; F5 is a migration hazard; F6 is the platform-degradation requirement. No new network access or secret class is introduced, so the principal risks are concurrency, synthetic-input permissions, and settings compatibility.

**MVP scope** — Opt-in `paste_next_enabled = false` default; one re-recordable global Paste Next hotkey with conflict rejection; text kinds only (`Text`, `Url`, `Code`, `Color`, `Secret`); LIFO cursor with no wrap; reused skip/marker/auto-clear helpers; per-platform failure reasons; Settings disclosure; AC-1–AC-5, AC-8, AC-10–AC-12, AC-17–AC-22 from `paste_stack_spec.md:253-287`. Defer image payloads and badge polish to a follow-up.

**Recommendation: Build, staged (Defer image payloads from the first release).** PR breakdown:
1. Pure logic + tests: settings fields/defaults, old-JSON round trip, conflict helper, action enum, queue-order unit test.
2. Platform facade + stubs: `paste-next-worker`, named thread, `Disconnected`/shutdown, safe stubs on unsupported sessions, shared keystroke mutex.
3. Secret/DB plumbing: reuse skip marker and conceal/auto-clear helpers; image fingerprint relocation only if images are pulled forward.
4. UI wiring: Settings toggle/recorder/status, cursor badge, visible failure notice on next show; all handlers in `logic()` per `docs/CONCURRENCY_AND_PERF_LESSONS.md:51-53`.
5. CI/docs/release: update README, settings help, lessons docs if invariants change, changelog, and 3-OS smoke checklist; verify `cargo fmt -- --check`, `cargo clippy --all-targets --all-features -- -D warnings`, and `cargo test --all-targets` in the parent/static-analysis pass (not claimed here).

**Open questions**
- Should image payloads stay in v2 or move to a follow-up PR? (`paste_stack_spec.md:24`, `paste_stack_spec.md:154-158`)
- Is the 500 ms modifier-release timeout acceptable on all three OSes? (`paste_stack_spec.md:116`, `paste_stack_spec.md:138`)
- Where should failures be shown when the window is hidden — deferred toast only, or tray/log? (`paste_stack_spec.md:166`, `paste_stack_spec.md:187`)
- Which bounds and backpressure policy should the "lossless" channel use on a stuck consumer? (`paste_stack_spec.md:68`)
- Who owns the single text skip marker across interleaved Toggle-window copies and Paste Next jobs? (`paste_stack_spec.md:152`, `src/db.rs:239-252`)

## Coverage
- **Problem framing (user, observed vs [Inferred], smallest version, local success measure)**: Finding F3 and the Recommended Actions problem statement; no adoption or retention metric claimed.
- **Product fit (privacy positioning, new secret classes, permissions, network)**: Findings F4, F6, F7. No new secret class or network access proposed; required secret mitigations are explicit.
- **Rust implementation options (modules, dependencies, target APIs, errors, threads, UI state, DB migration)**: Findings F1, F2, F4, F5, F6; options table above. No new Cargo dependency is required by the spec as written [Inferred].
- **Cost and risk (parity, supply chain, FFI/unsafe, concurrency, performance, testing)**: Findings F1, F2, F4, F6; risk table above. `unsafe` remains confined to `src/platform/macos.rs` and `src/platform/windows.rs` in the proposed design (`paste_stack_spec.md:144`, `paste_stack_spec.md:285`).
- **Rollout plan (feature gate, migration, docs, release checklist)**: Findings F3, F5 and PR breakdown steps 1–5.
- **Recommendation (Build / Defer / Reject and PR decomposition)**: Build, staged; image payloads deferred; see Recommended Actions.
- **Open questions**: listed above; no answer invented.
