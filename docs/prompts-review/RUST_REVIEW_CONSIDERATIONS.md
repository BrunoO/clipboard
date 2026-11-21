# Professional Rust Review Checklist (2026 Reference Frame)

**Purpose:** The checklist that the Rust-adapted `docs/prompts-review/` prompts are based on. Use it as a quick pre-review reminder, not as a substitute for the evidence-bound prompt file for a given review area.

**Project anchor:** CipherClipboard — a cross-platform Rust desktop app using Cargo, `eframe`/`egui`, `rusqlite` + SQLCipher, `keyring`, OS clipboard/hotkey APIs, background threads, and target-gated FFI backends.

---

## 0. Review Rules That Override Everything

1. **Read before reporting.** A file name, symbol name, or crate name is not evidence. Open the file and cite `path:line` plus a verbatim snippet.
2. **Run before measuring.** Never claim a command is clean unless you ran it in this review. Paste the exact command and relevant output.
3. **No silent skips.** Every checklist category must end with findings or `No findings in this category.`
4. **No invented metrics.** Coverage, timings, binary sizes, issue counts, and dependency counts must come from a measured artifact.
5. **Label reasoning.** General Rust knowledge not grounded in this repository is prefixed with `[Inferred]`.
6. **Respect project invariants.** `AGENTS.md`, `docs/SECURITY_LESSONS.md`, `docs/MULTIPLATFORM_LESSONS.md`, and `docs/CONCURRENCY_AND_PERF_LESSONS.md` are normative for this codebase.
7. **Do not report tool style as a defect.** rustfmt/clippy-enforced items are only findings when the configured gate fails or an `#[allow]` hides a real problem.

---

## 1. Toolchain, Edition, Cargo Manifest

- Confirm the actual edition (`Cargo.toml` currently uses `edition = "2021"`), MSRV policy, and whether a `rust-toolchain.toml` exists. Do not assume 2024 migration has happened.
- If 2024 migration is under review, check the breaking edges: RPIT lifetime capture rules, `if let`/tail-expression temporary scopes, match-ergonomics reservations, required `unsafe extern` blocks, `unsafe` attributes (`no_mangle`, `export_name`, `link_section`), and `unsafe_op_in_unsafe_fn`.
- Review `[lints.rust]` and `[lints.clippy]` in `Cargo.toml`. Every crate-wide `allow` needs a reason; every local `#[allow(...)]` needs a narrow scope and a `// SAFETY:`-style justification where appropriate.
- Check target-specific `[dependencies]`, `[build-dependencies]`, and `[target.'cfg(...)'.dependencies]` for accidental host leakage.
- Check `[features]` for additive, composable defaults. `default = ["..."]` should not silently enable heavyweight or unsafe platform integrations.
- Check `[[bin]]`, `default-run`, `package.metadata.bundle`, icons, plist/version resources, and release profile coherence.
- Check that release builds use `--locked` in CI/release workflows so `Cargo.lock` is authoritative.

**Commands:** `cargo metadata --format-version 1`, `cargo tree -e features`, `rustc --version`, `cargo --version`.

---

## 2. Ownership, Types, and API Design

- Prefer strong types over strings and booleans: newtypes for validated values, enums for states, `Result` for recoverable failure.
- Public API should avoid `&String`, `&Vec<T>`, `&Box<T>`, and `&Option<T>` where slices, iterators, or `Option<&T>` express the contract better.
- Use `#[must_use]`, `#[non_exhaustive]` where future enum/struct growth is plausible, and meaningful `Default`/`is_empty`/`From`/`TryFrom` implementations.
- Review `clone()` calls in hot or synchronization-heavy paths. A clone used only to appease the borrow checker may indicate poor ownership boundaries.
- `Rc`, `Arc`, `RefCell`, `Mutex`, `RwLock`, and atomics need a recorded reason when they leak implementation details into the API. Prefer returning a guard over an opaque `with_foo` closure only when the guard does not expose locking semantics.
- Keep non-leaf modules thin (`mod` declarations and re-exports); name leaf modules after their primary item.
- Avoid new prelude modules; scope trait imports narrowly.
- Check `Send`/`Sync` bounds. Never “fix” a thread-affinity problem with `unsafe impl Send`/`Sync`.

---

## 3. Error Handling

- Library/module code: typed errors, typically `thiserror`, with stable variants that callers can match.
- Application boundary: `anyhow::Result` with `.context()`/`.with_context()` is acceptable when the top level only reports and exits.
- Every `Result` consumer must either propagate, handle, or explicitly justify ignoring the error. `let _ = ...` needs a reason.
- `unwrap()`, `expect()`, `panic!()`, indexing, and arithmetic assumptions are acceptable only in tests or proven invariants; document the invariant otherwise.
- Distinguish user-actionable errors (“Keyring is locked; unlock it and retry”) from internal defects.
- Check error source chains (`#[source]`, `#[from]`), display messages, and that no secret/clipboard content appears in `Display`, `Debug`, logs, or telemetry.
- FFI status codes must be converted into Rust `Result` at the boundary; check return values before dependent calls (e.g. `AttachThreadInput` attach before detach).
- Poisoned mutex recovery must be deterministic, including in test mocks.

---

## 4. Unsafe Code and FFI

- Crate-wide `unsafe_code = "deny"` is the baseline. In this repo only `src/platform/macos.rs` and `src/platform/windows.rs` may opt in.
- Every `unsafe fn` documents its safety contract; every `unsafe` block has a nearby `// SAFETY:` comment that explains *why* each precondition holds.
- Valid reasons for unsafe: FFI/OS calls, novel abstractions, or measured performance. Ad-hoc `transmute`, unchecked casts, and bypassing `Send`/`Sync` are not valid shortcuts.
- Raw pointers: check null, alignment, provenance, aliasing, lifetime, and initialization. Prefer `&raw mut`/`&raw const` and `ptr::addr_of_mut!` over `&mut` references to uninitialized memory.
- Use checked integer conversions (`try_from`) at FFI boundaries; avoid unchecked `as` casts in target-gated code.
- `#[repr(C)]` layouts for Win32/OS structs must include explicit alignment/padding where required; verify byte counts against Microsoft/Apple docs.
- macOS: guard AppKit calls with `is_main_thread()`, wrap ObjC calls in `objc::rc::autoreleasepool`, filter `[NSApplication windows]` with `canBecomeMainWindow`, and never mutate windows re-entrantly inside `NSNotificationCenter` callbacks.
- Windows: explicit `#[link(name = "...")]`, `c"..."` literals, `CREATE_NO_WINDOW`, DPAPI zeroization, UIPI fail-closed behavior, and correct `TOKEN_MANDATORY_LABEL` buffer alignment.
- Miri is useful for unsafe code but cannot model most real OS calls; absence of Miri findings is not proof of FFI soundness. `cargo-geiger` is a signal, not a verdict.

---

## 5. Concurrency, Async, and Event Loops

- Name every thread via `thread::Builder::name`, store `JoinHandle`s, and join owned workers on shutdown.
- Background loops must handle channel `Disconnected` as well as explicit shutdown. Treat `Ok(Shutdown) | Err(Disconnected)` as exit-and-cleanup.
- Prefer event-driven blocking (`recv`, `crossbeam_channel::select!`) over `recv_timeout` polling. Use `sync_channel(1)`/`try_send` for coalesced notifications.
- Keep coupled state in one synchronization primitive. Do not split related HWND/PID or X11 window data across independent atomics.
- Keep `Mutex` critical sections free of CPU-bound work and multi-statement SQLite writes inside one transaction.
- Use `thread_local!` for main-thread-only or connection-affine handles. Do not wrap them in `Arc<Mutex<_>>` merely to satisfy a compiler error.
- Choose atomic orderings deliberately: `Relaxed` for standalone IDs, `Release`/`AcqRel` for cross-thread signal flags.
- For `eframe` 0.36+, hidden-window/background handling belongs in `App::logic()`; keyboard/pointer input belongs in `App::ui()`. Do not add unconditional hidden-window repaint timers.
- Async code: blocking work belongs in `std::thread`, `rayon`, or `spawn_blocking`, never directly on an async worker. Check cancellation safety and `Send` futures.

---

## 6. Performance

- Measure release builds (`cargo run --release`, `cargo bench`); debug timings are meaningless for performance claims.
- Profile before optimizing: `cargo flamegraph`, `samply`, `perf`, or platform equivalents. Use `criterion` for statistical microbenchmarks, `iai-callgrind` for noisy CI, and `hyperfine` for end-to-end commands.
- Use `black_box` and realistic input sizes; benchmark several sizes to expose accidental O(n²) behavior.
- Inspect allocation and clone sites, repeated parsing, repeated `intern_atom`, repeated keyring lookups, and database round-trips.
- Clipboard polling: gate payload reads on OS change counters (`NSPasteboard.changeCount`, `GetClipboardSequenceNumber`) before copying text/images.
- Image decoding: bound dimensions and allocations before decode; security limits and performance work belong together.
- Release profile: `opt-level = 3`, thin LTO, `codegen-units = 1`, `panic = "abort"`, `strip = true` are project choices — verify they still match binary size/startup/crash-reporting requirements rather than copying blindly.

**Commands:** `cargo build --release --timings`, `cargo bench`, `cargo flamegraph --bin CipherClipboard`, `hyperfine`.

---

## 7. Testing, Fuzzing, Coverage

- Run `cargo test --all-targets`; prefer `cargo nextest run` locally/CI for parallelism and failure isolation when available.
- Unit tests for pure logic; integration tests for cross-module contracts; doc tests for documented examples; platform smoke tests for OS lifecycle behavior.
- Tests must be deterministic, host/layout-agnostic, and free of hardcoded secret literals. Use `tempfile` for filesystem isolation.
- Property tests (`proptest`/`quickcheck`) for parsers, format conversion, URL sanitization, and state machines.
- Fuzz (`cargo-fuzz`) for untrusted input paths: clipboard payloads, PNG headers, URLs, DB rows, OS-provided strings.
- Miri for unsafe abstractions where the code does not depend on real OS calls.
- Coverage (`cargo llvm-cov`, `cargo tarpaulin`) is a guide, not a target by itself. Report only measured numbers.
- Keep CI matrix on Linux, macOS, Windows; Linux Clippy does not validate `#[cfg(target_os = "macos")]`/`"windows"` blocks.
- Preserve platform-specific smoke tests such as the macOS quit test; they guard invariants that unit tests cannot.

---

## 8. Security and Secret Hygiene

- Never log clipboard content or secrets in release builds. Gate previews with `#[cfg(debug_assertions)]` and still redact detected secrets.
- Keys, tokens, generated passwords, and transient skip buffers use `zeroize::Zeroizing`; scope lifetimes tightly.
- Fail closed if an encrypted DB exists but the OS keyring is unavailable. Never silently generate a new key and lock the user out.
- Surface degraded key-storage modes in the UI.
- Unix fallback key files: `OpenOptionsExt::mode(0o600)` at creation inside a `0o700` data directory. Windows fallback keys: user-scoped DPAPI + ACL hardening.
- SQLCipher: parameterized SQL, `secure_delete = ON`, WAL checkpoint/truncate on full clear, escaped `LIKE` wildcards, and no search oracle for masked secrets.
- Honor and emit concealed/transient clipboard markers on macOS, Windows, and Linux X11.
- Before synthetic paste, verify the captured target is foreground; respect Windows UIPI and macOS accessibility permissions.
- URL handling: parse with `url::Url`, allow only `http`/`https` with a non-empty host before `webbrowser::open`.
- Crypto: OS RNG (`getrandom`/`OsRng`), constant-time comparison for secrets (`subtle`), no bespoke ciphers, no clock seeding.
- GitHub Actions: pin to full commit SHAs, least-privilege `permissions:`, no `continue-on-error` on security/quality gates.

---

## 9. Supply Chain and Dependencies

- Treat `Cargo.lock` as a security artifact. Use `--locked` in release builds and review lockfile diffs in dependency PRs.
- Run `cargo-deny check` (advisories, licenses, bans, sources) and `cargo audit`. Pin the security tools themselves.
- Every advisory ignore needs a scoped, current justification (see `deny.toml`’s `RUSTSEC-2026-0543` entry).
- Deny wildcard requirements, unknown registries, and unknown git sources. Keep license allow-list aligned with transitive fonts/assets.
- Watch duplicate major versions (`cargo tree -d`) — especially `windows` crate version splits after GUI stack upgrades.
- Review build scripts and proc macros as executable code at build time; prefer crates with minimal build-time surface.
- Check target-specific dependencies for native toolchain requirements (X11, DBus, OpenSSL, SQLCipher) and document installation steps.
- Release packaging: verify downloaded binaries, stage exact runtime DLLs, publish checksums and provenance attestations.

---

## 10. Cross-Platform Organization

- OS code lives in `src/platform/{mod,macos,windows,linux}.rs`; `mod.rs` stays a thin safe facade.
- Use `#[cfg(target_os = "...")]` consistently; reserve `#[cfg(unix)]` for POSIX APIs only.
- Cross-platform capabilities get OS-agnostic names and safe no-op stubs. Single-OS APIs are exposed through their target-gated submodule.
- Collapse repeated platform branches with `const fn platform_select`-style helpers where possible.
- Gate pure platform helpers with `#[cfg(any(target_os = "...", test))]` so logic tests run on every CI host.
- Do not hardcode keyboard layouts, path separators, line endings, locale, or administrator/elevation assumptions.
- Verify target triples in `deny.toml` match the actual CI/release matrix.

---

## 11. Documentation and Rustdoc

- Public items: rustdoc that states invariants, `# Errors`, `# Panics`, and `# Safety` where relevant; compile doc examples.
- Keep README, `AGENTS.md`, and `docs/*LESSONS.md` synchronized with code changes. A hardening invariant that is no longer true must be updated, not silently ignored.
- Document feature flags, MSRV, OS prerequisites, keyring fallback behavior, and recovery steps for lost/unlocked keyring states.
- Report files generated by the orchestrator belong in `docs/reviews/{DATE}/` with the documented filename pattern; update `docs/DOCUMENTATION_INDEX.md` when it exists.

---

## 12. CI/CD and Release

- `cargo fmt -- --check`, `cargo clippy --all-targets --all-features -- -D warnings`, `cargo test --all-targets`, `cargo deny check`, and Sonar gates must fail loudly.
- Run clippy/test on all target OSes; target-gated code is invisible on Linux.
- Sonar review requires `SONARQUBE_TOKEN`; if absent, report skipped — never invent issues.
- Release workflows use `--locked`, verify embedded resources, signature/checksums, and publish attestations.
- Review post-review actions: index updates, issue creation, historical report archiving, and next review date.

---

## Sources Consulted (2026-11-21)

- Rust 1.85 / Edition 2024 announcement: <https://blog.rust-lang.org/2025/02/20/Rust-1.85.0>
- The Cargo Book — SemVer compatibility: <https://doc.rust-lang.org/cargo/reference/semver.html>
- Cargo manifest lints: <https://doc.rust-lang.org/cargo/reference/manifest.html#the-lints-section>
- Clippy documentation: <https://doc.rust-lang.org/clippy/>
- Rust API Guidelines: <https://rust-lang.github.io/api-guidelines/>
- Microsoft Pragmatic Rust Guidelines: <https://microsoft.github.io/rust-guidelines/>
- Google Rust Style Guide / human review guidance: <https://google.github.io/styleguide/RustStyleGuide.html>
- cargo-deny: <https://embarkstudios.github.io/cargo-deny/>
- RustSec Advisory Database: <https://github.com/RustSec/advisory-db>
- Rust crate security (cargo audit/cargo vet): <https://safeguard.sh/resources/blog/rust-crate-security-cargo-audit-cargo-vet-and-beyond>
- Rust testing/fuzzing/benchmarking 2026 survey: <http://codeloopback.dev/articles/rust-property-testing-fuzzing-benchmarking>
- Rust performance 2026 (criterion/flamegraph/hyperfine): <https://blog.rajpoot.dev/posts/rust/rust-performance-2026>
