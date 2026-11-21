# UX Review - 2026-11-21-b

## Executive Summary
- **Health Score**: 8/10 ([Inferred] weighting of 0 critical, 0 high, 2 medium and 2 low evidence-backed findings; all three High/Medium issues from the initial review — database write error handling for secret toggle/delete, severity-colored `ToastSeverity` toasts, and mode-aware first-run keyring modal copy — plus the Linux concealed-marker limitation disclosure and Settings log-path copy widget have been resolved)
- **Critical Issues**: 0
- **High Issues**: 0
- **Total Findings**: 4
- **Estimated Remediation Effort**: 2.5–4 hours [Inferred]
- **Evidence Commands Run**:
  - `grep -RInE 'impl eframe::App|fn logic\(|fn ui\(|request_repaint|Visible\(|CancelClose|toast|hotkey|keyring|masked|secret' src` → 664 matches across `src/`; key lifecycle and UX lines are cited verbatim below (`src/app/mod.rs:1515` `impl eframe::App`, `src/app/mod.rs:1528` `fn logic`, `src/app/mod.rs:1549` `fn ui`).
  - `ls docs/screenshots 2>/dev/null || true` → `(no output)`. **No visual artifacts available.** All findings are grounded in source code.

## Findings

### Critical
No findings in this category.

### High
No findings in this category.

### Medium

**U1 — "Copy & Save as Secret" in Password Generator shows a success toast even when the database insert fails**  
- **Flow / screen**: Password Generator view (`ActiveView::Generator`) → "Copy & Paste" / "Copy & Dismiss" / "Copy" or `Enter` key when "Don't save generated password to clipboard history" is unchecked (`skip_history == false`).  
- **Severity**: Medium  
- **Evidence**: `src/app/mod.rs:590-605`:
  ```rust
            crate::platform::mark_clipboard_concealed();
            self.schedule_secret_clipboard_clear(&secret);
            if !self.password_gen.skip_history {
                if let Err(e) = self.db.insert_secret(&secret) {
                    eprintln!("Failed to save generated password to database: {e}");
                } else {
                    self.refresh_items();
                }
            }
            let msg = if self.password_gen.skip_history {
                format!("{} Copied password (not saved to history)", icons::CHECK)
            } else {
                format!("{} Copied & saved password as Secret!", icons::LOCK)
            };
            self.finish_copy(ctx, &msg, dismiss);
  ```
- **User-visible problem**: While `ItemAction::ToggleSecret`, `ItemAction::Delete`, `delete_filtered_item`, and `Clear All` now check `Result` and surface `report_db_write_failure` (`src/app/history_view.rs:162-165,183-186`, `src/app/mod.rs:840-843,1387-1409`), `copy_generated_password` still only prints to `stderr` when `self.db.insert_secret(&secret)` fails and proceeds to display the green success toast `"Copied & saved password as Secret!"`. If the user relies on the history database having saved that newly generated password (and the clipboard auto-clears 30 seconds later per `auto_clear_secret_secs`), the generated credential can be permanently lost after auto-clear.
- **Recommendation**: Track whether `insert_secret` succeeded. If it returns `Err(e)`, log the failure via `crate::logging::log_message("ERROR", ...)` and show a warning/error toast (or set `pending_toast` when `dismiss == true`) such as `"Copied password to clipboard, but failed to save to history!"` instead of claiming it was saved.
- **Verification**: Simulate a database write failure during `copy_generated_password` with `skip_history = false` and confirm the user receives an error/warning toast rather than `"Copied & saved password as Secret!"`.

**U2 — Theme, hotkey, and history-limit handlers in `settings_view.rs` bypass `persist_settings()` / `report_db_write_failure()`**  
- **Flow / screen**: Settings view (`ActiveView::Settings`) → Theme selector, Global Shortcut preset radio buttons, Custom shortcut apply, and History size limit selector.  
- **Severity**: Medium  
- **Evidence**:
  - `ClipboardApp::persist_settings` exists specifically to log and surface settings-save failures (`src/app/mod.rs:351-359`):
    ```rust
    fn persist_settings(&mut self) {
        if let Err(error) = self.settings.save(&self.db) {
            crate::logging::log_message("ERROR", &format!("Failed to save settings: {error}"));
            self.show_error_toast(&format!(
                "{} Failed to save settings. Changes will be lost on restart.",
                icons::WARNING
            ));
        }
    }
    ```
  - However, three Settings handlers call `self.settings.save(&self.db)` directly with `eprintln!` and then overwrite the status bar with a green success toast:
    - `src/app/settings_view.rs:78-90` (Theme selector):
      ```rust
      self.settings.theme = selected_theme;
      if let Err(e) = self.settings.save(&self.db) {
          eprintln!("Failed to save settings: {e}");
      }
      ...
      self.show_toast(&format!(
          "{} Theme changed to {}",
          icons::CHECK,
          selected_theme.display_name()
      ));
      ```
    - `src/app/settings_view.rs:138-147` (Hotkey preset selector):
      ```rust
      self.settings.hotkey = preset;
      if let Err(e) = self.settings.save(&self.db) {
          eprintln!("Failed to save settings: {e}");
      }
      if self.update_hotkeys_from_settings() {
          self.show_toast(&format!(
              "{} Shortcut set to {}",
              icons::CHECK,
              preset.badge()
          ));
      ```
    - `src/app/settings_view.rs:302-311` (`apply_custom_hotkey`):
      ```rust
      self.settings.custom_hotkey = parsed.format();
      if let Err(e) = self.settings.save(&self.db) {
          eprintln!("Failed to save settings: {e}");
      }
      ```
  - Additionally, `render_max_history_selector` (`src/app/settings_view.rs:428-433`) discards `prune_history` errors silently:
    ```rust
    self.settings.max_history = limit;
    self.db.set_max_history(limit);
    let _ = self.db.prune_history(limit);
    self.persist_settings();
    self.refresh_items();
    ```
- **User-visible problem**: If the database cannot persist settings (or fails to prune when reducing the history limit), the user sees a green success toast (or silent completion) and is not warned that their theme/hotkey change will be lost on restart or that pruning failed.
- **Recommendation**: Route theme, preset hotkey, and custom hotkey saves through a helper that returns `bool` (or calls `self.persist_settings()` after showing the action toast so the error toast takes precedence on failure), and handle `Err(e)` from `self.db.prune_history(limit)` via `self.report_db_write_failure("prune history", &e)`.
- **Verification**: Trigger theme or hotkey change against a read-only/failing database and verify the error toast `"Failed to save settings. Changes will be lost on restart."` is displayed.

### Low

**U3 — Keyboard selection resets to index `0` on background clipboard capture and window summon [Inferred]**  
- **Flow / screen**: History view keyboard navigation (`↑`/`↓`, `Tab`, `Ctrl+N`/`P`/`J`/`K`) when a background copy occurs or the window is hidden and re-summoned.  
- **Severity**: Low  
- **Evidence**:
  - `src/app/mod.rs:407-415` (`show_window`):
    ```rust
    fn show_window(&mut self, ctx: &egui::Context) {
        crate::logging::log_message("INFO", "show_window called");
        self.is_visible = true;
        self.has_been_focused = false;
        self.was_inactive_since_hidden = false;
        self.selected_index = 0;
        self.show_clear_confirm_dialog = false;
        self.scroll_to_selected = true;
    ```
  - `src/app/mod.rs:967-980` (`handle_monitor_updates`):
    ```rust
    fn handle_monitor_updates(&mut self) {
        let mut new_items_detected = false;
        while self.notify_rx.try_recv().is_ok() {
            new_items_detected = true;
        }

        let query_changed = self.search_query != self.last_query_cached;
        if monitor_update_needs_refresh(new_items_detected, query_changed) {
            self.refresh_items();
            self.last_query_cached = self.search_query.clone();
            self.selected_index = 0;
            self.scroll_to_selected = true;
        }
    }
    ```
- **User-visible problem**: [Inferred] While resetting `selected_index = 0` on `show_window` and search-query change is standard launcher behavior, resetting `selected_index = 0` when `new_items_detected` fires while the user is actively browsing older entries jumps their selection and scroll position back to the top of the list.
- **Recommendation**: When `self.is_visible && new_items_detected && !query_changed && self.selected_index > 0`, preserve the currently selected item by its `item.id` across `self.refresh_items()` (falling back to index `0` only if the item was pruned or the user was already at index `0`).
- **Verification**: Open the History view, press `↓` three times to select an older item, copy text in another window, and verify the selection stays anchored on the inspected item instead of jumping to row 0.

**U4 — Pure Wayland sessions on Linux lack an explicit Settings notice explaining global-hotkey and auto-paste limitations**  
- **Flow / screen**: Settings view (`ActiveView::Settings`) → Global Shortcut and General Behavior sections on Linux pure Wayland sessions (where X11/XWayland is unavailable).  
- **Severity**: Low  
- **Evidence**:
  - `src/platform/linux.rs:73-97` (`with_x11`) and `src/platform/mod.rs:71-92` (`is_app_active_state`) gracefully return `None` when `x11rb::connect(None)` fails on pure Wayland.
  - `src/app/settings_view.rs:377-379` renders an accessibility notice on macOS (`render_auto_paste_accessibility_notice`), and `src/app/settings_view.rs:471-478` now renders `crate::platform::concealed_marker_limitation()` on Linux:
    ```rust
    if let Some(limitation) = crate::platform::concealed_marker_limitation() {
        ui.add_space(4.0);
        ui.label(
            egui::RichText::new(limitation)
                .color(crate::theme::text_secondary(is_dark))
                .size(11.0),
        );
    }
    ```
  - However, when `crate::platform::is_app_active_state().is_none()` on Linux (indicating X11 connection is unavailable, such as a pure Wayland session without XWayland), neither `render_hotkey_settings_group` (`src/app/settings_view.rs:98-126`) nor `render_behavior_settings_group` (`src/app/settings_view.rs:326-384`) displays a note explaining that X11-dependent features (synthetic `Ctrl+V` auto-paste via XTest, active-window focus restoration, and the X11 Quote-key fallback) are unavailable.
- **User-visible problem**: On pure Wayland, enabling "Auto-paste selected entry directly into initial window" or "Restore focus to initial window" silently does nothing because `with_x11` returns `None`, with no Settings disclosure comparable to macOS Accessibility or Linux `concealed_marker_limitation()`.
- **Recommendation**: Expose a platform capability helper in `src/platform/mod.rs` (e.g., `pub(crate) fn auto_paste_session_limitation() -> Option<&'static str>`) that returns an explanatory note on Linux when X11 is unavailable, and render it in `render_behavior_settings_group` alongside `render_auto_paste_accessibility_notice`.
- **Verification**: Run on Linux with `DISPLAY` unset (simulating pure Wayland without X11), open Settings → General Behavior, and verify the limitation notice is shown under the auto-paste/restore-focus checkboxes.

## Quick Wins
1. Handle `self.db.insert_secret(&secret)` failure in `copy_generated_password` (`src/app/mod.rs:593-604`) so the toast does not falsely claim the password was saved to history (U1) — ~30 minutes [Inferred].
2. Replace raw `self.settings.save(&self.db)` calls in `src/app/settings_view.rs:79,139,303` with `self.persist_settings()` and surface `prune_history` errors at line 430 (U2) — ~30 minutes [Inferred].
3. Add a pure-Wayland/missing-X11 limitation note in `src/platform/mod.rs` and `render_behavior_settings_group` (U4) — ~45 minutes [Inferred].
4. Preserve selected item by ID when a background monitor update arrives while the user has navigated down the list (`selected_index > 0`) (U3) — ~1 hour [Inferred].

## Recommended Actions
1. **U1 + U2 (Medium, ~1 hour [Inferred])**: Close the remaining database error-feedback gaps in `copy_generated_password` and `settings_view.rs` using the existing `ToastSeverity::Warning`/`Error`, `persist_settings()`, and `report_db_write_failure()` helpers.
2. **U4 (Low, ~45 min [Inferred])**: Surface a pure-Wayland session notice in Settings when X11 is unavailable, matching the pattern established by `platform::concealed_marker_limitation()`.
3. **U3 (Low, ~1 hour [Inferred])**: Anchor keyboard selection by item ID during visible background refreshes when `selected_index > 0`, covered by a unit test on `ClipboardApp`.

## Coverage
- **1. Information Architecture and Privacy (secret masking, search exclusion, key storage visibility, first-run state, error toasts)**: Findings U1, U2. Previously reported U1 (ignored DB write errors on secret toggle/delete), U2 (all toasts styled in green success color), and U3 (first-run keyring modal hardcoded to `OsKeyring`) are all resolved (`src/app/history_view.rs:156-191`, `src/app/mod.rs:160-171,331-405,1195-1205,1433-1471`). Search excludes secrets at `src/db.rs:647` (`WHERE kind != 'secret'`).
- **2. Immediate-Mode UI Correctness (`logic()` vs `ui()` split, hidden-window wakeups, exact-deadline auto-clear, per-frame work, widget state)**: Finding U3. `src/app/mod.rs:1528-1547` keeps all background wake handlers in `App::logic()`, keyboard/pointer shortcuts stay in `App::ui()` (`src/app/mod.rs:1561-1563`), `FilterCounts::from_items` is precomputed on refresh (`src/app/mod.rs:326`), and hidden repaints are strictly bounded to the exact auto-clear deadline and the post-hide deactivation latch.
- **3. Hotkey and Tray UX (registration errors, Windows tray fallback, macOS Dock reopen & accessibility, Linux Quote fallback & Wayland, AZERTY layout re-detect)**: Finding U4. Registration status and failure toasts are shown in `src/app/settings_view.rs:148-154,244-279,312-321`; Windows tray fallback to `Minimized(true)` lives in `src/platform/mod.rs:395-418`; Windows keyboard layout re-detect button is at `src/app/settings_view.rs:116-118,159-174`; macOS Accessibility deep-link is at `src/app/settings_view.rs:386-414`.
- **4. Clipboard and Paste Flow (concealed markers, auto-clear timing, UIPI fail-closed warning, transient retry, log redaction)**: No findings in this category. Outgoing secrets call `mark_clipboard_concealed()` and `schedule_secret_clipboard_clear()` (`src/app/mod.rs:554-555,590-591`); Linux's `DetectOnly` limitation is disclosed in Settings (`src/app/settings_view.rs:471-478`); Windows UIPI defers a warning toast via `pending_toast` (`src/app/mod.rs:422-424,441-446`).
- **5. Platform Conventions (macOS traffic-light padding/window filtering/quit, Windows tray/UIPI/no console flashes, Linux X11/Wayland, consistent terminology)**: No findings in this category. Platform modifier names (`Cmd` vs `Ctrl`/`Win`), navigation hints, and quit tooltips are centralized in `src/platform/mod.rs:167-205`.
- **6. Feedback and Accessibility (loading/disabled/error states, non-color-only indicators, contrast/icons, immediate settings effect)**: Findings U1, U2. Status indicators pair icons (`icons::SHIELD_CHECK`, `icons::WARNING`, `icons::LOCK`) with text and WCAG-checked colors (`src/theme.rs`). Settings now also surface the diagnostic log file path with a "Copy Path" button (`src/app/settings_view.rs:589-659`).
- **7. Manual Verification / Visual Artifacts**: **No visual artifacts available.**
