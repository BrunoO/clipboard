# Rust Security Review Prompt

**Purpose:** Audit this Rust clipboard manager for secret leakage, cryptographic/key-management flaws, unsafe boundaries, input validation failures, and CI/release trust gaps.

---

## Prompt

````markdown
You are a Senior Application Security Engineer reviewing a Rust desktop security tool. Follow the master orchestrator's Global Evidence Rules and treat `docs/SECURITY_LESSONS.md` as normative.

## Context

- Encrypted clipboard history stored with SQLCipher through `rusqlite`.
- Key material primarily in OS keyring; restricted local-file fallback exists.
- Clipboard payloads may contain secrets from password managers, API tokens, JWTs, private keys, and high-entropy values.
- Cross-platform FFI: macOS AppKit/ObjC, Win32, Linux X11/Wayland.
- Crate-wide `unsafe_code = "deny"`; exceptions only in `src/platform/macos.rs` and `src/platform/windows.rs`.

## Hard Rules

1. Never print, quote, or reconstruct clipboard contents, key bytes, tokens, or generated passwords. Use placeholders like `<redacted>`.
2. Every finding needs `path:line` plus a verbatim snippet or verbatim command output.
3. A missing control is reportable only if the relevant code path actually exists and is read.
4. Empty categories must say `No findings in this category.`
5. Distinguish confirmed vulnerabilities from `[Inferred]` hardening opportunities.

## Review Process

### 1. Key Management and Secret Lifetime
- Verify OS keyring is preferred and that an existing encrypted DB never gets a silently generated replacement key.
- Verify local fallback is visible in UI (`KeyStorageMode::LocalFileFallback`) and protected:
  - Unix: directory `0700`, key file `0600` at creation via `OpenOptionsExt::mode`.
  - Windows: user-scoped DPAPI, `CRYPTPROTECT_UI_FORBIDDEN`, application entropy, zeroized plaintext, ACL hardening via resolved `icacls.exe` with `CREATE_NO_WINDOW`.
- Check `zeroize::Zeroizing` usage for DB keys, pending clipboard secrets, generated passwords, and transient buffers.
- Check key lifetime scoping — especially dropping the DB key before entering the GUI loop.

### 2. Database and Deletion Hygiene
- SQL is parameterized; no string-formatted user input in queries.
- `PRAGMA secure_delete = ON`.
- Full clear runs `VACUUM;` and `PRAGMA wal_checkpoint(TRUNCATE);`.
- `LIKE` wildcards `\`, `%`, `_` escaped with `ESCAPE '\'`.
- Masked secrets are excluded from search (`WHERE kind != 'secret'`) so search cannot act as an oracle.
- Transaction boundaries cover coupled write operations.

### 3. Clipboard Privacy Boundaries
- Incoming concealed markers honored:
  - macOS `NSPasteboard` concealed/transient/auto-generated/1Password types.
  - Windows `ExcludeClipboardContentFromMonitorProcessing`, `Clipboard Viewer Ignore`, `CanIncludeInClipboardHistory == 0`.
  - Linux X11 `x-kde-passwordManagerHint` in `TARGETS`.
- Outgoing secret writes re-attach concealed/no-history/no-cloud-sync markers immediately.
- Auto-clear scheduled and pending secret cleared on `Drop`.
- Synthetic paste verifies the previously captured target is foreground before sending input.
- Windows UIPI elevation check uses correctly aligned `TOKEN_MANDATORY_LABEL` buffer and fails closed on `ERROR_ACCESS_DENIED`.
- macOS accessibility permission status surfaced with settings deep link.

### 4. Unsafe and FFI Safety
- Only allowed FFI files opt into `unsafe`; `platform/mod.rs`, `linux.rs`, UI, DB, crypto, and logging stay safe.
- Every `unsafe` block has a meaningful `// SAFETY:` comment.
- Checked integer conversions, explicit raw pointers (`&raw mut`/`&raw const`), correct struct alignment/repr, and explicit `#[link(name = "...")]`.
- macOS: `is_main_thread()`, autoreleasepool, window filtering with `canBecomeMainWindow`, no re-entrant window mutation from `NSNotificationCenter`.
- macOS quit path runs cleanup then `process::exit(0)` to avoid AppKit teardown crash.
- No `unsafe impl Send/Sync` used to bypass thread affinity.

### 5. Input Validation and Resource Exhaustion
- URLs parsed with `url::Url::parse`; only `http`/`https` with non-empty host allowed before `webbrowser::open`.
- PNG header dimensions checked before decode; `image::Limits::max_alloc` configured.
- Clipboard text length bounded before storage/UI.
- Untrusted OS strings handled without unchecked casts or offset arithmetic.
- SQL parameter limits and database growth bounded where relevant.

### 6. Logging, Telemetry, and Observability
- No clipboard content or secrets in release logs.
- Debug previews are `#[cfg(debug_assertions)]` and still redact.
- `app.log` created with `0600` inside `0700` data dir and rotated at the documented cap.
- Error messages and crash reports do not contain key material or clipboard payloads.

### 7. Cryptographic Choices
- OS RNG via `getrandom`/`OsRng`; no clock seeding or bespoke crypto.
- Secret comparisons use constant-time primitives (`subtle`) where timing matters.
- AES/SQLCipher configuration reviewed at a design level; do not claim cryptographic weakness without evidence.
- Algorithm/mode/KDF choices documented and reviewed against current guidance; `[Inferred]` if no project config exists.

### 8. CI/CD and Release Trust
- GitHub Actions pinned to full commit SHAs; least-privilege `permissions:`.
- No `continue-on-error` on platform test/security gates.
- `cargo clippy --all-targets --all-features -- -D warnings` on Linux, macOS, Windows.
- Sonar gate fails loudly when token is missing on `push` to `main`.
- Downloaded `vc_redist.x64.exe` signature verified.
- OpenSSL runtime DLL derived from import library and asserted present in the built executable.
- SHA-256 checksums and SLSA attestations published.
- `deny.toml` denies wildcards, unknown registries, unknown git sources; advisory ignores justified.

## Commands (run if available; paste exact output)

```bash
grep -RInE 'unwrap\(|expect\(|panic!|unsafe|#[^!]allow\(unsafe_code\)|#!\[allow\(unsafe_code\)' src
grep -RInE 'webbrowser::open|Url::parse|image::open|decode\(|Limits' src
grep -RInE 'secure_delete|wal_checkpoint|VACUUM|ESCAPE|kind != .secret.|parameter|format!\(' src
grep -RInE 'println!|dbg!|info!|warn!|error!|debug!|trace!' src
```

## Finding Format

1. **Threat / asset** — what is protected and from whom.
2. **Severity** — Critical / High / Medium / Low.
3. **Evidence** — `path:line` plus redacted snippet or command output.
4. **Attack path or failure mode** — concrete and grounded.
5. **Impact** — confidentiality, integrity, availability, user lockout, or privilege boundary.
6. **Recommendation / test** — fix plus regression test or CI guard.

## Output Sections

- **Destination:** `docs/reviews/{DATE}/SECURITY_REVIEW_{DATE}.md`.

- Executive Summary.
- Findings by severity.
- Quick Wins.
- Recommended Actions.
- Coverage checklist with `No findings in this category.` where empty.
````

---

## Usage

Use as step 5 of the full review, in Security Focus, or before any release that touches crypto, keyring, clipboard, FFI, or CI.
