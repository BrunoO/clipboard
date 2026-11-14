# CipherClipboard — Bug Analysis & Recommendations

**Generated**: 2026-11-14  
**Project**: `/Users/brunoorsier/dev/clipboard`  
**Index**: 656 nodes, 2,595 edges (Rust, Bash, YAML, TOML)

---

## Executive Summary

| Severity | Count | Description |
|----------|-------|-------------|
| 🔴 Critical | 1 | Mutex poisoning can crash entire application |
| 🟠 High | 3 | Silent error suppression in DB operations & initialization |
| 🟡 Medium | 7 | Race conditions, resource leaks, unsafe unwraps |
| 🟢 Low-Medium | 5 | Fragile timing, stale state, graceful shutdown gaps |

---

## 🔴 Critical Issues

### 1. Mutex Poisoning — Application-Wide Crash

**Files**: `src/db.rs` (all methods), `src/hotkey.rs:42`, `src/hotkey.rs:86`

```rust
// Pattern used everywhere:
let conn = self.conn.lock().unwrap();           // src/db.rs
let _ = self.target_ids.read().unwrap();        // src/hotkey.rs:42
for hk in &self.hotkeys { ... }                 // src/hotkey.rs:86 (Drop)
```

**Impact**: If any thread panics while holding a `Mutex`/`RwLock`, **all other threads panic on next lock acquisition**, crashing the entire app.

**Affected Locks**:
- `Database.conn: Arc<Mutex<Connection>>` — all DB operations
- `Database.transient_text: Arc<Mutex<Option<String>>>` — skip/take logic
- `HotkeyService.target_ids: Arc<RwLock<Vec<u32>>>` — hotkey event loop

**Fix**:
```rust
// Recovery pattern (preferred)
let conn = self.conn.lock().unwrap_or_else(|poisoned| poisoned.into_inner());

// Or explicit handling
match self.conn.lock() {
    Ok(guard) => guard,
    Err(poisoned) => {
        eprintln!("⚠️ Mutex recovered from poison");
        poisoned.into_inner()
    }
}
```

---

## 🟠 High Severity

### 2. Silent Error Suppression — Database Pruning & Migrations

**File**: `src/db.rs`

| Method | Lines | Ignored Operations |
|--------|-------|-------------------|
| `open()` | 163–175 | 4× `ALTER TABLE` migrations, `VACUUM` |
| `insert()` | 313 | History pruning `DELETE` |
| `insert_image()` | 418 | History pruning `DELETE` |
| `insert_secret()` | 357 | History pruning `DELETE` |
| `clear()` | 545 | `VACUUM` |

**Impact**: 
- Max history limits may not be enforced
- Schema migrations can fail silently on corrupted DB
- Disk space not reclaimed after `clear()`

**Fix**: Return `Result` or at minimum log:
```rust
if let Err(e) = conn.execute(...) {
    eprintln!("⚠️ Pruning failed: {e}");
}
```

### 3. Database Errors Hidden at Startup

**File**: `src/app.rs:169`

```rust
let initial_items = db.get_recent(settings.max_history, None).unwrap_or_default();
```

**Impact**: Corrupted/locked database → app starts with empty history, no user feedback.

**Fix**:
```rust
let initial_items = match db.get_recent(settings.max_history, None) {
    Ok(items) => items,
    Err(e) => {
        eprintln!("⚠️ Failed to load history: {e}");
        Vec::new()
    }
};
```

---

## 🟡 Medium Severity

### 4. Hotkey Debounce Race Condition

**File**: `src/app.rs:399–401`

```rust
if self.last_toggle_instant.is_some_and(|t| t.elapsed() < Duration::from_millis(200)) {
    return;
}
self.last_toggle_instant = Some(Instant::now());  // NOT atomic with check
```

**Impact**: Multiple rapid hotkey events (from OS key repeat) can all pass the check before the first updates the timestamp.

**Fix**: Use atomic compare-exchange or reverse order:
```rust
let now = Instant::now();
if let Some(last) = self.last_toggle_instant {
    if now.duration_since(last) < Duration::from_millis(200) { return; }
}
self.last_toggle_instant = Some(now);
```

### 5. macOS Notification Observer Never Removed

**File**: `src/platform.rs:260–310` (`setup_reopen_observer`)

```rust
// Adds observer but NO corresponding removeObserver
let _: () = msg_send![center, addObserver: observer ...];
```

**Impact**:
- Crash if callback fires after observer deallocation
- Memory leak (observer retained by notification center)
- Duplicate observers if `setup_reopen_observer` called twice

**Fix**: Store observer in static, implement cleanup:
```rust
static REOPEN_OBSERVER: OnceLock<*mut Object> = OnceLock::new();

// In setup:
let observer = ...;
REOPEN_OBSERVER.set(observer).ok();

// On shutdown (or Drop):
if let Some(&observer) = REOPEN_OBSERVER.get() {
    unsafe { msg_send![center, removeObserver: observer] };
}
```

### 6. Transient Text Race Condition

**File**: `src/db.rs:212–228` (`skip_next_capture`, `take_if_skipped`)

```rust
pub fn skip_next_capture(&self, text: &str) {
    if let Ok(mut guard) = self.transient_text.lock() {  // Poisoning → silent fail
        *guard = Some(text.to_string());
    }
}

pub fn take_if_skipped(&self, text: &str) -> bool {
    if let Ok(mut guard) = self.transient_text.lock() {
        if guard.as_deref() == Some(text) {
            *guard = None;
            return true;
        }
    }
    false
}
```

**Issues**:
1. Mutex poisoning → silent failure (returns `false`)
2. Check-then-act not atomic — another thread can modify between check and clear

**Fix**: Use `Mutex::lock().unwrap_or_else(...)` + atomic compare-exchange pattern.

### 7. Unwrap/Expect in Production Paths

| Location | Code | Risk |
|----------|------|------|
| `src/db.rs:270` | `i64::try_from(max_limit).unwrap_or(i64::MAX)` | Panic on overflow if `max_limit > i64::MAX` |
| `src/db.rs:438` | Same pattern | Same |
| `src/monitor.rs:376–377` | `u32::try_from(width).unwrap_or(0)` | Loses large image dimensions silently |
| `src/password.rs:474` | `String::from_utf8(chars).unwrap_or_default()` | Returns empty password on non-UTF8 |

**Fix**: Use `saturating_cast`, `try_from` with proper error handling, or `String::from_utf8_lossy`.

---

## 🟢 Low-Medium Severity

### 8. Single-Instance Lock Stale PID

**File**: `src/instance.rs:25–50`

```rust
let mut file = OpenOptions::new().create(true).open(lock_path)?;
// File CREATED here, then try_lock()
match file.try_lock() { ... }
```

**Impact**: If process crashes after `create(true)` but before `try_lock()`, stale PID remains in lock file.

**Fix**: Write PID only after successful lock acquisition (already done in `Ok` branch), but use `create_new` or verify lock before writing.

### 9. Background Threads Lack Graceful Shutdown

| Thread | File | Issue |
|--------|------|-------|
| Hotkey listener | `src/hotkey.rs:38–47` | Eternal `while let Ok(event) = receiver.recv()` — no stop signal |
| Clipboard monitor | `src/monitor.rs:54–144` | Eternal `loop { thread::sleep(...) }` — no stop signal |

**Impact**: Threads killed abruptly on exit; hotkeys unregistered only in `Drop` (may not run), observers never cleaned up.

**Fix**: Add `shutdown_tx/rx` channels, implement `Drop` for `HotkeyService` that signals shutdown.

### 10. Clipboard Writer Failure Silent

**File**: `src/app.rs:163`

```rust
let clipboard_writer = Clipboard::new().ok();  // None on failure
```

**Impact**: All copy operations silently fail if clipboard unavailable. User sees no feedback.

**Fix**:
```rust
let clipboard_writer = match Clipboard::new() {
    Ok(cb) => Some(cb),
    Err(e) => {
        eprintln!("⚠️ Clipboard unavailable: {e}");
        None
    }
};
// Later: show toast "Clipboard unavailable" if None
```

### 11. Auto-Paste Timing Fragility

**File**: `src/platform.rs:143`

```rust
std::thread::sleep(Duration::from_millis(120));  // Hardcoded
```

**Impact**: 120ms may be insufficient on slow systems / under load → paste goes to wrong window.

**Fix**: Make configurable, or poll for focus confirmation before pasting.

### 12. VACUUM Not Properly Awaited

**File**: `src/db.rs:545`

```rust
let _ = conn.execute_batch("VACUUM;");
```

**Impact**: `execute_batch` may not wait for VACUUM completion; errors ignored.

**Fix**: Use `conn.execute("VACUUM", [])?` for proper error handling.

---

## Recommended Fix Priority

| Priority | Issue | Effort | Risk if Unfixed |
|----------|-------|--------|-----------------|
| 1 | Mutex poisoning recovery | Low | App crash on any thread panic |
| 2 | Error propagation in DB ops | Low | Silent data loss, unbounded history |
| 3 | Startup DB error handling | Low | Silent empty history |
| 4 | macOS observer cleanup | Medium | Crash on reopen/Dock click |
| 5 | Hotkey debounce atomicity | Low | Double-toggle on key repeat |
| 6 | Transient text atomicity | Low | Skip logic unreliable |
| 7 | Production unwrap fixes | Low | Panics on edge cases |
| 8 | Graceful thread shutdown | Medium | Resource leaks, hotkey conflicts |
| 9 | Clipboard writer feedback | Low | Confusing UX |
| 10 | Auto-paste timing | Low | Paste to wrong window |

---

## Quick Wins (Can Fix Today)

```bash
# 1. Fix all mutex locks in db.rs
sed -i 's/\.lock()\.unwrap()/.lock().unwrap_or_else(|e| e.into_inner())/g' src/db.rs

# 2. Add error logging for pruning
# Edit insert/insert_image/insert_secret: log pruning failures

# 3. Fix unwrap_or_default in app.rs
# Edit line 169: match db.get_recent(...) { Ok(v) => v, Err(e) => { log; Vec::new() } }

# 4. Fix password.rs unwrap
# String::from_utf8(chars).unwrap_or_else(|e| { eprintln!("..."); String::new() })
```

---

## Files to Audit Further

- `src/platform.rs` — 767 lines, heavy `unsafe`, macOS/Windows FFI
- `src/app.rs` — 2,647 lines, complex UI state machine
- `src/hotkey.rs` — Global hotkey manager lifecycle
- `src/crypto.rs` — Key management, permissions hardening

---

*Analysis based on codebase-memory-mcp graph queries, `search_code` scans, and `get_code_snippet` reviews. No runtime testing performed.*