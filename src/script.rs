//! Sandboxed Rhai scripting engine for user-defined clipboard text transformations.
//!
//! Scripts live as `*.rhai` files inside `<app_data>/transforms/` (owner-only permissions).
//! Each script defines `fn transform(text)` and optionally `fn applies_to(text)`, plus
//! optional header comments:
//!
//! ```rhai
//! // @name: Filter Errors (±2 lines)
//! // @category: AI & Prompt
//! // @tooltip: Keep only ERROR/WARN/PANIC lines with 2 lines of surrounding context
//! ```
//!
//! Execution is strictly sandboxed: no filesystem, network, or environment access,
//! bounded operation count, bounded call/expression depth, and bounded string/collection sizes.

use crate::db::{ClipboardItem, ItemKind};
use crate::icons;
use crate::transform::TransformCategory;
use rhai::{Dynamic, Engine, EvalAltResult, OptimizationLevel, Scope, AST};
use std::fs;
use std::path::{Path, PathBuf};

/// Subdirectory under the app data directory where user `.rhai` scripts are stored.
pub(crate) const TRANSFORMS_SUBDIR: &str = "transforms";

/// Filename of the bundled demo script seeded on first run.
pub(crate) const DEMO_SCRIPT_FILENAME: &str = "filter_errors.rhai";

/// Maximum byte size of a single `.rhai` script file loaded from disk.
const MAX_SCRIPT_FILE_BYTES: u64 = 64 * 1024;

/// Maximum number of `.rhai` scripts loaded from the transforms directory.
const MAX_LOADED_SCRIPTS: usize = 64;

/// Maximum input bytes passed to `applies_to` to keep menu opening instantaneous.
const MAX_APPLIES_TO_INPUT_BYTES: usize = 32 * 1024;

/// Hard Rhai execution caps to prevent accidental infinite loops or memory exhaustion.
const MAX_RHAI_OPERATIONS: u64 = 50_000;
const MAX_RHAI_STRING_BYTES: usize = 1_048_576;
const MAX_RHAI_COLLECTION_LEN: usize = 10_000;
const MAX_RHAI_CALL_LEVELS: usize = 32;

/// Bundled demo Rhai script: extracts ERROR / WARN / PANIC / EXCEPTION lines with ±2 lines of context.
pub(crate) const DEMO_FILTER_ERRORS_SCRIPT: &str = r#"// @name: Filter Errors (±2 lines)
// @category: AI & Prompt
// @tooltip: Keep only ERROR/WARN/PANIC lines with 2 lines of surrounding context

fn applies_to(text) {
    if !text.contains("\n") {
        return false;
    }
    let u = text.to_upper();
    u.contains("ERROR") || u.contains("WARN") || u.contains("PANIC") || u.contains("EXCEPTION")
}

fn transform(text) {
    let lines = text.strip_ansi().split("\n");
    let total = lines.len();
    let keep = #{};

    for i in 0..total {
        let u = lines[i].to_upper();
        if u.contains("ERROR") || u.contains("WARN") || u.contains("PANIC") || u.contains("EXCEPTION") {
            let start = if i >= 2 { i - 2 } else { 0 };
            let end = if i + 2 < total { i + 2 } else { total - 1 };
            for j in start..=end {
                keep[j.to_string()] = true;
            }
        }
    }

    let out = "";
    let prev_idx = -2;
    for i in 0..total {
        if i.to_string() in keep {
            if prev_idx >= 0 && i > prev_idx + 1 {
                out += "...\n";
            }
            out += `${i + 1}: ${lines[i]}` + "\n";
            prev_idx = i;
        }
    }

    if out.is_empty() {
        text
    } else {
        out.trim();
        out
    }
}
"#;

/// A compiled user-defined Rhai transformation script.
pub(crate) struct CustomScript {
    pub(crate) filename: String,
    pub(crate) name: String,
    pub(crate) category: TransformCategory,
    pub(crate) tooltip: String,
    has_applies_to: bool,
    ast: AST,
}

impl std::fmt::Debug for CustomScript {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CustomScript")
            .field("filename", &self.filename)
            .field("name", &self.name)
            .field("category", &self.category)
            .field("has_applies_to", &self.has_applies_to)
            .finish_non_exhaustive()
    }
}

impl CustomScript {
    /// Confirmation toast shown after copying this script's output.
    #[must_use]
    pub(crate) fn toast_success(&self) -> String {
        format!(
            "{} Copied '{}' result to clipboard!",
            icons::CHECK,
            self.name
        )
    }
}

/// Non-fatal script loading or compilation diagnostic shown in Settings.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ScriptLoadError {
    pub(crate) filename: String,
    pub(crate) message: String,
}

/// Registry holding the sandboxed Rhai engine and all compiled user transformation scripts.
pub(crate) struct ScriptRegistry {
    engine: Engine,
    scripts_dir: PathBuf,
    scripts: Vec<CustomScript>,
    errors: Vec<ScriptLoadError>,
}

impl std::fmt::Debug for ScriptRegistry {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ScriptRegistry")
            .field("scripts_dir", &self.scripts_dir)
            .field("scripts_len", &self.scripts.len())
            .field("errors_len", &self.errors.len())
            .finish_non_exhaustive()
    }
}

impl ScriptRegistry {
    /// Initializes the registry from `<data_dir>/transforms/`.
    ///
    /// If the directory does not exist yet, creates it with owner-only permissions and seeds
    /// the bundled demo script (`filter_errors.rhai`).
    #[must_use]
    pub(crate) fn load_from_data_dir(data_dir: &Path) -> Self {
        let scripts_dir = data_dir.join(TRANSFORMS_SUBDIR);
        let engine = build_sandboxed_engine();
        let mut registry = Self {
            engine,
            scripts_dir,
            scripts: Vec::new(),
            errors: Vec::new(),
        };
        registry.ensure_dir_and_seed_demo();
        registry.reload();
        registry
    }

    /// Initializes an empty in-memory registry for unit tests without touching disk.
    #[cfg(test)]
    #[must_use]
    pub(crate) fn empty_for_tests() -> Self {
        Self {
            engine: build_sandboxed_engine(),
            scripts_dir: PathBuf::new(),
            scripts: Vec::new(),
            errors: Vec::new(),
        }
    }

    /// Directory path where user `.rhai` scripts live.
    #[must_use]
    pub(crate) fn scripts_dir(&self) -> &Path {
        &self.scripts_dir
    }

    /// All successfully compiled custom scripts, sorted by name.
    #[must_use]
    pub(crate) fn scripts(&self) -> &[CustomScript] {
        &self.scripts
    }

    /// Non-fatal errors encountered while loading or compiling scripts.
    #[must_use]
    pub(crate) fn errors(&self) -> &[ScriptLoadError] {
        &self.errors
    }

    /// Writes (or restores) the bundled demo script `filter_errors.rhai` and reloads scripts.
    pub(crate) fn restore_demo_script(&mut self) -> Result<PathBuf, std::io::Error> {
        fs::create_dir_all(&self.scripts_dir)?;
        crate::platform::fs_security::set_restricted_dir_permissions(&self.scripts_dir);
        let demo_path = self.scripts_dir.join(DEMO_SCRIPT_FILENAME);
        fs::write(&demo_path, DEMO_FILTER_ERRORS_SCRIPT)?;
        crate::platform::fs_security::set_restricted_permissions(&demo_path);
        self.reload();
        Ok(demo_path)
    }

    /// Re-scans `<data_dir>/transforms/*.rhai` and recompiles all scripts.
    pub(crate) fn reload(&mut self) {
        self.scripts.clear();
        self.errors.clear();

        let entries = match fs::read_dir(&self.scripts_dir) {
            Ok(rd) => rd,
            Err(e) => {
                if e.kind() != std::io::ErrorKind::NotFound {
                    self.errors.push(ScriptLoadError {
                        filename: TRANSFORMS_SUBDIR.to_string(),
                        message: format!("Failed to read scripts directory: {e}"),
                    });
                }
                return;
            }
        };

        let mut rhai_paths: Vec<PathBuf> = entries
            .filter_map(Result::ok)
            .map(|entry| entry.path())
            .filter(|path| {
                path.is_file()
                    && path
                        .extension()
                        .and_then(|ext| ext.to_str())
                        .is_some_and(|ext| ext.eq_ignore_ascii_case("rhai"))
            })
            .collect();
        rhai_paths.sort();

        for path in rhai_paths.into_iter().take(MAX_LOADED_SCRIPTS) {
            let filename = path
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or("script.rhai")
                .to_string();

            match self.load_single_script(&path, &filename) {
                Ok(script) => self.scripts.push(script),
                Err(message) => self.errors.push(ScriptLoadError { filename, message }),
            }
        }

        self.scripts.sort_by_key(|a| a.name.to_lowercase());
    }

    /// Evaluates whether `script` applies to `item` (returns `false` for images or empty text).
    #[must_use]
    pub(crate) fn applies_to(&self, script: &CustomScript, item: &ClipboardItem) -> bool {
        if item.kind == ItemKind::Image || item.content.trim().is_empty() {
            return false;
        }
        if !script.has_applies_to {
            return true;
        }
        let sample = truncate_utf8(&item.content, MAX_APPLIES_TO_INPUT_BYTES);
        let mut scope = Scope::new();
        self.engine
            .call_fn::<bool>(&mut scope, &script.ast, "applies_to", (sample.to_string(),))
            .unwrap_or(false)
    }

    /// Executes `script`'s `transform(text)` function inside the sandbox.
    pub(crate) fn execute(&self, script: &CustomScript, input: &str) -> Result<String, String> {
        let mut scope = Scope::new();
        self.engine
            .call_fn::<String>(&mut scope, &script.ast, "transform", (input.to_string(),))
            .map_err(|e| format_rhai_error(&e))
    }

    fn ensure_dir_and_seed_demo(&self) {
        if self.scripts_dir.exists() {
            return;
        }
        if fs::create_dir_all(&self.scripts_dir).is_ok() {
            crate::platform::fs_security::set_restricted_dir_permissions(&self.scripts_dir);
            let demo_path = self.scripts_dir.join(DEMO_SCRIPT_FILENAME);
            if fs::write(&demo_path, DEMO_FILTER_ERRORS_SCRIPT).is_ok() {
                crate::platform::fs_security::set_restricted_permissions(&demo_path);
            }
        }
    }

    fn load_single_script(&self, path: &Path, filename: &str) -> Result<CustomScript, String> {
        let meta = fs::metadata(path).map_err(|e| format!("Cannot read metadata: {e}"))?;
        if meta.len() > MAX_SCRIPT_FILE_BYTES {
            return Err(format!(
                "Script exceeds maximum size ({MAX_SCRIPT_FILE_BYTES} bytes)"
            ));
        }

        let source = fs::read_to_string(path).map_err(|e| format!("Cannot read file: {e}"))?;
        let ast = self
            .engine
            .compile(&source)
            .map_err(|e| format!("Compile error: {e}"))?;

        let has_transform = ast
            .iter_functions()
            .any(|f| f.name == "transform" && f.params.len() == 1);
        if !has_transform {
            return Err("Missing required `fn transform(text)` function".to_string());
        }

        let has_applies_to = ast
            .iter_functions()
            .any(|f| f.name == "applies_to" && f.params.len() == 1);

        let headers = parse_script_headers(&source, filename);
        Ok(CustomScript {
            filename: filename.to_string(),
            name: headers.name,
            category: headers.category,
            tooltip: headers.tooltip,
            has_applies_to,
            ast,
        })
    }
}

/// Builds a strictly sandboxed Rhai engine with `CipherClipboard` text helpers registered.
fn build_sandboxed_engine() -> Engine {
    let mut engine = Engine::new();
    engine.set_max_operations(MAX_RHAI_OPERATIONS);
    engine.set_max_string_size(MAX_RHAI_STRING_BYTES);
    engine.set_max_array_size(MAX_RHAI_COLLECTION_LEN);
    engine.set_max_map_size(MAX_RHAI_COLLECTION_LEN);
    engine.set_max_call_levels(MAX_RHAI_CALL_LEVELS);
    engine.set_max_expr_depths(64, 32);
    engine.set_optimization_level(OptimizationLevel::Simple);

    // Silence stdout/stderr hooks so user scripts cannot leak clipboard data to console logs.
    engine.on_print(|_| {});
    engine.on_debug(|_, _, _| {});

    // Register CipherClipboard's built-in text helpers (callable both as `f(text)` and `text.f()`).
    engine.register_fn("strip_ansi", crate::transform::strip_ansi);
    engine.register_fn(
        "strip_formatting",
        crate::transform::strip_source_formatting,
    );
    engine.register_fn("dedent", crate::transform::dedent);
    engine.register_fn("strip_local_paths", crate::transform::strip_local_paths);
    engine.register_fn("redact_secrets", |s: &str| {
        crate::transform::redact_secrets(s, false)
    });
    engine.register_fn("redact_secrets_and_ips", |s: &str| {
        crate::transform::redact_secrets(s, true)
    });
    engine.register_fn("add_line_numbers", crate::transform::add_line_numbers);
    engine.register_fn("unwrap_lines", crate::transform::unwrap_lines);
    engine.register_fn("collapse_whitespace", crate::transform::collapse_whitespace);

    engine.register_fn(
        "parse_json",
        |text: &str| -> Result<Dynamic, Box<EvalAltResult>> {
            let val: serde_json::Value = serde_json::from_str(text)
                .map_err(|e| Box::new(EvalAltResult::from(format!("Invalid JSON: {e}"))))?;
            rhai::serde::to_dynamic(&val)
                .map_err(|e| Box::new(EvalAltResult::from(format!("JSON conversion failed: {e}"))))
        },
    );
    engine.register_fn(
        "to_json_pretty",
        |val: Dynamic| -> Result<String, Box<EvalAltResult>> {
            let json_val: serde_json::Value = rhai::serde::from_dynamic(&val).map_err(|e| {
                Box::new(EvalAltResult::from(format!("Cannot serialize value: {e}")))
            })?;
            serde_json::to_string_pretty(&json_val)
                .map_err(|e| Box::new(EvalAltResult::from(format!("JSON format failed: {e}"))))
        },
    );
    engine.register_fn(
        "to_json_compact",
        |val: Dynamic| -> Result<String, Box<EvalAltResult>> {
            let json_val: serde_json::Value = rhai::serde::from_dynamic(&val).map_err(|e| {
                Box::new(EvalAltResult::from(format!("Cannot serialize value: {e}")))
            })?;
            serde_json::to_string(&json_val)
                .map_err(|e| Box::new(EvalAltResult::from(format!("JSON format failed: {e}"))))
        },
    );

    engine
}

struct ParsedHeaders {
    name: String,
    category: TransformCategory,
    tooltip: String,
}

fn parse_script_headers(source: &str, filename: &str) -> ParsedHeaders {
    let mut name: Option<String> = None;
    let mut category: Option<TransformCategory> = None;
    let mut tooltip: Option<String> = None;

    for line in source.lines().take(20) {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        let Some(comment) = trimmed.strip_prefix("//") else {
            break;
        };
        let comment = comment.trim();
        if let Some(val) = comment.strip_prefix("@name:") {
            let v = val.trim();
            if !v.is_empty() {
                name = Some(v.to_string());
            }
        } else if let Some(val) = comment.strip_prefix("@category:") {
            category = Some(parse_category_name(val.trim()));
        } else if let Some(val) = comment.strip_prefix("@tooltip:") {
            let v = val.trim();
            if !v.is_empty() {
                tooltip = Some(v.to_string());
            }
        }
    }

    let default_name = filename
        .strip_suffix(".rhai")
        .unwrap_or(filename)
        .replace(['_', '-'], " ");

    ParsedHeaders {
        name: name.unwrap_or(default_name),
        category: category.unwrap_or(TransformCategory::Custom),
        tooltip: tooltip.unwrap_or_else(|| format!("Run custom Rhai script ({filename})")),
    }
}

fn parse_category_name(raw: &str) -> TransformCategory {
    match raw.to_ascii_lowercase().as_str() {
        "ai" | "ai & prompt" | "prompt" => TransformCategory::AiPrompt,
        "cleanup" => TransformCategory::Cleanup,
        "case" => TransformCategory::Case,
        "lines" => TransformCategory::Lines,
        "developer" | "developer & encode" | "dev" => TransformCategory::Developer,
        "url" | "url & web" => TransformCategory::Url,
        _ => TransformCategory::Custom,
    }
}

fn truncate_utf8(s: &str, max_bytes: usize) -> &str {
    if s.len() <= max_bytes {
        return s;
    }
    let mut end = max_bytes;
    while end > 0 && !s.is_char_boundary(end) {
        end -= 1;
    }
    &s[..end]
}

fn format_rhai_error(err: &EvalAltResult) -> String {
    match err {
        EvalAltResult::ErrorTooManyOperations(_) => {
            "Script exceeded operation limit (possible infinite loop)".to_string()
        }
        EvalAltResult::ErrorDataTooLarge(kind, _) => {
            format!("Script exceeded maximum {kind} size limit")
        }
        other => other.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_text_item(content: &str) -> ClipboardItem {
        ClipboardItem {
            id: 1,
            kind: ItemKind::Text,
            content: content.to_string(),
            width: None,
            height: None,
            created_at: "2026-01-01".to_string(),
            embedded_colors: vec![],
            label: None,
            notes: None,
            is_pinned: false,
        }
    }

    #[test]
    fn test_demo_filter_errors_script_seeds_and_executes() {
        let tmp = tempfile::tempdir().unwrap();
        let registry = ScriptRegistry::load_from_data_dir(tmp.path());

        assert!(registry.errors().is_empty(), "{:?}", registry.errors());
        assert_eq!(registry.scripts().len(), 1);
        let demo = &registry.scripts()[0];
        assert_eq!(demo.name, "Filter Errors (±2 lines)");
        assert_eq!(demo.category, TransformCategory::AiPrompt);

        let log = "\
INFO boot 1
INFO boot 2
INFO boot 3
INFO boot 4
\x1b[31mERROR\x1b[0m database timeout
INFO retry 1
INFO retry 2
INFO quiet 1
INFO quiet 2
INFO quiet 3
WARN disk 90%
INFO done";

        let item = make_text_item(log);
        assert!(registry.applies_to(demo, &item));

        let out = registry.execute(demo, log).unwrap();
        assert_eq!(
            out,
            "\
3: INFO boot 3
4: INFO boot 4
5: ERROR database timeout
6: INFO retry 1
7: INFO retry 2
...
9: INFO quiet 2
10: INFO quiet 3
11: WARN disk 90%
12: INFO done"
        );

        let plain_item = make_text_item("just two normal lines\nnothing wrong here");
        assert!(!registry.applies_to(demo, &plain_item));
    }

    #[test]
    fn test_rhai_builtin_helpers_and_json() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().join(TRANSFORMS_SUBDIR);
        fs::create_dir_all(&dir).unwrap();
        fs::write(
            dir.join("sanitize.rhai"),
            r"
// @name: Sanitize Helper Test
// @category: Custom
fn transform(text) {
    text.dedent().strip_local_paths().redact_secrets().add_line_numbers()
}
",
        )
        .unwrap();
        fs::write(
            dir.join("json_keys.rhai"),
            r"
// @name: JSON Keys
fn transform(text) {
    let obj = parse_json(text);
    to_json_compact(obj)
}
",
        )
        .unwrap();

        let registry = ScriptRegistry::load_from_data_dir(tmp.path());
        assert!(registry.errors().is_empty(), "{:?}", registry.errors());
        let sanitize = registry
            .scripts()
            .iter()
            .find(|s| s.filename == "sanitize.rhai")
            .unwrap();
        let out = registry
            .execute(
                sanitize,
                "    path=/Users/alice/app\n    token=ghp_abcdefghijklmnopqrstuvwxyz123456",
            )
            .unwrap();
        assert_eq!(out, "1: path=<HOME>/app\n2: token=[REDACTED]");

        let json_script = registry
            .scripts()
            .iter()
            .find(|s| s.filename == "json_keys.rhai")
            .unwrap();
        let compact = registry
            .execute(json_script, "{ \"b\": 2, \"a\": 1 }")
            .unwrap();
        assert_eq!(compact, "{\"a\":1,\"b\":2}");
    }

    #[test]
    fn test_rhai_sandbox_aborts_infinite_loop_and_records_bad_scripts() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().join(TRANSFORMS_SUBDIR);
        fs::create_dir_all(&dir).unwrap();
        fs::write(
            dir.join("loop.rhai"),
            "fn transform(text) { loop { } text }",
        )
        .unwrap();
        fs::write(dir.join("broken.rhai"), "fn transform(text { ??? }").unwrap();
        fs::write(dir.join("missing_fn.rhai"), "fn other(x) { x }").unwrap();

        let registry = ScriptRegistry::load_from_data_dir(tmp.path());
        assert_eq!(registry.scripts().len(), 1);
        assert_eq!(registry.errors().len(), 2);

        let err = registry
            .execute(&registry.scripts()[0], "hello")
            .unwrap_err();
        assert!(err.contains("operation limit"), "unexpected error: {err}");
    }
}
