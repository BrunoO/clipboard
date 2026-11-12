use eframe::egui;

/// Supported programming and data languages for detection and syntax highlighting.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum CodeLanguage {
    Rust,
    Cpp,
    Python,
    JavaScript,
    TypeScript,
    Json,
    Html,
    Sql,
    Shell,
    Toml,
    Yaml,
}

impl CodeLanguage {
    #[allow(dead_code)]
    pub(crate) const fn name(self) -> &'static str {
        match self {
            Self::Rust => "Rust",
            Self::Cpp => "C++",
            Self::Python => "Python",
            Self::JavaScript => "JavaScript",
            Self::TypeScript => "TypeScript",
            Self::Json => "JSON",
            Self::Html => "HTML",
            Self::Sql => "SQL",
            Self::Shell => "Shell",
            Self::Toml => "TOML",
            Self::Yaml => "YAML",
        }
    }

    pub(crate) const fn icon(self) -> &'static str {
        match self {
            Self::Rust
            | Self::Cpp
            | Self::Python
            | Self::JavaScript
            | Self::TypeScript
            | Self::Html => egui_phosphor::regular::CODE,
            Self::Json => egui_phosphor::regular::BRACKETS_CURLY,
            Self::Sql => egui_phosphor::regular::DATABASE,
            Self::Shell => egui_phosphor::regular::TERMINAL,
            Self::Toml | Self::Yaml => egui_phosphor::regular::FILE_TEXT,
        }
    }

    #[allow(dead_code)]
    pub(crate) fn badge(self) -> String {
        format!("{} {}", self.icon(), self.name())
    }

    #[must_use]
    pub(crate) const fn syntect_name(self) -> &'static str {
        match self {
            Self::Rust => "rs",
            Self::Cpp => "cpp",
            Self::Python => "py",
            Self::JavaScript => "js",
            Self::TypeScript => "ts",
            Self::Json => "json",
            Self::Html => "html",
            Self::Sql => "sql",
            Self::Shell => "sh",
            Self::Toml => "toml",
            Self::Yaml => "yaml",
        }
    }
}

/// Detects if the given text represents a recognized code snippet or structured data format.
#[allow(clippy::too_many_lines)]
pub(crate) fn detect_code_language(text: &str) -> Option<CodeLanguage> {
    let trimmed = text.trim();
    if trimmed.is_empty() || trimmed.len() < 4 {
        return None;
    }

    // 1. JSON detection: valid JSON object or non-trivial array
    let is_json_bracket = (trimmed.starts_with('{') && trimmed.ends_with('}'))
        || (trimmed.starts_with('[') && trimmed.ends_with(']'));
    if is_json_bracket && serde_json::from_str::<serde_json::Value>(trimmed).is_ok() {
        return Some(CodeLanguage::Json);
    }

    // 2. HTML / XML detection
    if trimmed.starts_with('<')
        && (trimmed.contains("</") || trimmed.ends_with("/>") || trimmed.contains("<!DOCTYPE"))
    {
        return Some(CodeLanguage::Html);
    }

    // 3. Shell scripts / command lines
    if is_shell_command(trimmed) {
        return Some(CodeLanguage::Shell);
    }

    // 4. SQL queries
    let upper = trimmed.to_uppercase();
    if upper.starts_with("SELECT ")
        || upper.starts_with("INSERT INTO ")
        || upper.starts_with("UPDATE ")
        || upper.starts_with("DELETE FROM ")
        || upper.starts_with("CREATE TABLE ")
        || upper.starts_with("ALTER TABLE ")
        || upper.starts_with("DROP TABLE ")
    {
        return Some(CodeLanguage::Sql);
    }

    // 5. TOML configuration
    let is_toml_bracket =
        trimmed.starts_with('[') && trimmed.contains(']') && trimmed.contains('=');
    if is_toml_bracket
        && (trimmed.contains("[package]")
            || trimmed.contains("[dependencies]")
            || trimmed.contains("[lints"))
    {
        return Some(CodeLanguage::Toml);
    }

    // 6. Python detection
    let is_python = trimmed.contains("def ")
        || (trimmed.contains("import ")
            && (trimmed.contains("from ")
                || trimmed.contains("def ")
                || trimmed.contains("class ")))
        || trimmed.contains("elif ")
        || trimmed.contains("if __name__ == '__main__':")
        || (trimmed.contains("class ") && trimmed.contains(':'));

    if is_python && !trimmed.contains("fn ") && !trimmed.contains("pub ") {
        return Some(CodeLanguage::Python);
    }

    // 7. C++ & Objective-C++ detection
    let is_cpp_headers_or_ns = trimmed.contains("#include <")
        || trimmed.contains("#include \"")
        || trimmed.contains("std::")
        || trimmed.contains("using namespace ")
        || trimmed.contains("cout <<")
        || trimmed.contains("cin >>")
        || trimmed.contains("nullptr")
        || trimmed.contains("constexpr ")
        || trimmed.contains("template<")
        || trimmed.contains("template <")
        || ((trimmed.contains("public:") || trimmed.contains("private:"))
            && trimmed.contains("class "));

    let is_objc_cpp = trimmed.contains("NSString")
        || trimmed.contains("NSData")
        || trimmed.contains("NSDictionary")
        || trimmed.contains("NSArray")
        || trimmed.contains("NSError")
        || trimmed.contains("NSObject")
        || trimmed.contains("[[")
        || trimmed.contains("alloc]")
        || trimmed.contains("initWith")
        || trimmed.contains("UTF8String")
        || trimmed.contains("@interface")
        || trimmed.contains("@implementation")
        || trimmed.contains("@autoreleasepool");

    let is_cpp_patterns = trimmed.contains("->")
        || trimmed.contains("static_cast<")
        || trimmed.contains("dynamic_cast<")
        || trimmed.contains("reinterpret_cast<")
        || trimmed.contains("const char*")
        || trimmed.contains("for (int ")
        || trimmed.contains("for (auto ")
        || trimmed.contains("printf(")
        || (trimmed.contains("void ") && trimmed.contains('(') && trimmed.contains(')'));

    let is_cpp = (is_cpp_headers_or_ns || is_objc_cpp || is_cpp_patterns)
        && !trimmed.contains("fn ")
        && !trimmed.contains("pub fn ")
        && !trimmed.contains("let mut ");

    if is_cpp {
        return Some(CodeLanguage::Cpp);
    }

    // 8. Rust detection
    let is_rust = trimmed.contains("fn ")
        || trimmed.contains("pub fn ")
        || trimmed.contains("let mut ")
        || trimmed.contains("impl ")
        || trimmed.contains("-> Result<")
        || trimmed.contains("println!(")
        || trimmed.contains("#[derive(")
        || (trimmed.contains("struct ") && trimmed.contains('{'))
        || (trimmed.contains("enum ") && trimmed.contains('{'));

    if is_rust {
        return Some(CodeLanguage::Rust);
    }

    // 8. TypeScript vs JavaScript detection
    let has_js_indicators = trimmed.contains("const ")
        || trimmed.contains("let ")
        || trimmed.contains("function ")
        || trimmed.contains("console.log(")
        || trimmed.contains("export default ")
        || trimmed.contains("=> {");

    if has_js_indicators {
        let has_ts_types = trimmed.contains("interface ")
            || trimmed.contains(": string")
            || trimmed.contains(": number")
            || trimmed.contains(": boolean")
            || trimmed.contains(": any")
            || trimmed.contains("type ");
        if has_ts_types {
            return Some(CodeLanguage::TypeScript);
        }
        return Some(CodeLanguage::JavaScript);
    }

    // 9. YAML detection
    if trimmed.starts_with("---")
        || (trimmed.contains(": ")
            && trimmed.lines().count() >= 3
            && trimmed.lines().any(|l| l.trim_start().starts_with("- ")))
    {
        return Some(CodeLanguage::Yaml);
    }

    None
}

const KNOWN_SHELL_COMMANDS: &[&str] = &[
    // Version control & GitHub CLI
    "git",
    "gh",
    "glab",
    "svn",
    "hg",
    // Package managers & language toolchains
    "cargo",
    "rustc",
    "rustup",
    "npm",
    "npx",
    "yarn",
    "pnpm",
    "bun",
    "deno",
    "node",
    "pip",
    "pip3",
    "python",
    "python3",
    "ruby",
    "gem",
    "bundle",
    "go",
    "composer",
    "php",
    "mvn",
    "gradle",
    // Containers, orchestration & cloud
    "docker",
    "docker-compose",
    "podman",
    "kubectl",
    "helm",
    "aws",
    "az",
    "gcloud",
    "terraform",
    // System & package management
    "sudo",
    "brew",
    "apt",
    "apt-get",
    "dnf",
    "yum",
    "pacman",
    "apk",
    "systemctl",
    "journalctl",
    // Network & transfer
    "curl",
    "wget",
    "http",
    "ssh",
    "scp",
    "rsync",
    // Scripting & common utilities
    "export",
    "source",
    "chmod",
    "chown",
    "grep",
    "awk",
    "sed",
    "cat",
    "find",
    "make",
    "cmake",
    "echo",
    "kill",
    "pkill",
];

/// Returns true if the text matches common shell commands, shebangs, pipelines, or CLI invocations.
fn is_shell_command(text: &str) -> bool {
    let mut s = text.trim();

    // 1. Shebang
    if s.starts_with("#!/bin/") || s.starts_with("#!/usr/bin/env") {
        return true;
    }

    // 2. Strip terminal prompt prefixes: '$ ', '% ', '> '
    if let Some(stripped) = s
        .strip_prefix("$ ")
        .or_else(|| s.strip_prefix("% "))
        .or_else(|| s.strip_prefix("> "))
    {
        s = stripped.trim();
    }

    // 3. Strip outer enclosing quotes or backticks (e.g. "cmd" or 'cmd' or `cmd`)
    if (s.starts_with('"') && s.ends_with('"') && s.len() >= 2)
        || (s.starts_with('\'') && s.ends_with('\'') && s.len() >= 2)
        || (s.starts_with('`') && s.ends_with('`') && s.len() >= 2)
    {
        s = s[1..s.len() - 1].trim();
    }

    // Strip sudo prefix if present
    let base_cmd = s.strip_prefix("sudo ").unwrap_or(s).trim();

    let first_word = base_cmd.split_whitespace().next().unwrap_or("");
    if KNOWN_SHELL_COMMANDS.contains(&first_word)
        && (base_cmd.contains(' ') || base_cmd.contains('-') || base_cmd.contains('/'))
    {
        return true;
    }

    // Shell pipelines and compound chains (e.g. "cmd1 | cmd2", "cmd1 && cmd2")
    if (s.contains(" | ")
        || s.contains(" && ")
        || s.contains(" || ")
        || s.contains(" 2>&1")
        || s.contains(" > "))
        && !s.contains("class ")
        && !s.contains("fn ")
        && !s.contains(';')
    {
        return true;
    }

    // Shell variable assignment or expansion with CLI flags (e.g. $VAR with --flag or /path)
    if (s.contains('$') || s.contains("${"))
        && (s.contains(" --") || s.contains(" -") || s.contains('/'))
    {
        let word = s.split_whitespace().next().unwrap_or("");
        if !word.is_empty()
            && word
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-' || c == '.')
        {
            return true;
        }
    }

    false
}

/// Formats a valid JSON string with 2-space indentation.
pub(crate) fn format_json(text: &str) -> Option<String> {
    let val: serde_json::Value = serde_json::from_str(text.trim()).ok()?;
    serde_json::to_string_pretty(&val).ok()
}

/// Minifies a valid JSON string onto a single compact line.
#[allow(dead_code)]
pub(crate) fn minify_json(text: &str) -> Option<String> {
    let val: serde_json::Value = serde_json::from_str(text.trim()).ok()?;
    serde_json::to_string(&val).ok()
}
/// Builds an `egui::text::LayoutJob` with syntax highlighting for the given code snippet using `egui_extras` and `syntect`,
/// post-processing all token colors to guarantee minimum WCAG contrast ratio (>= 4.5:1) against the canvas background.
pub(crate) fn highlight_code(
    ctx: &egui::Context,
    style: &egui::Style,
    text: &str,
    lang: CodeLanguage,
) -> egui::text::LayoutJob {
    let theme = egui_extras::syntax_highlighting::CodeTheme::from_memory(ctx, style);
    let mut job =
        egui_extras::syntax_highlighting::highlight(ctx, style, &theme, text, lang.syntect_name());
    let canvas_bg = if style.visuals.dark_mode {
        egui::Color32::from_rgb(26, 27, 34)
    } else {
        egui::Color32::from_rgb(244, 244, 247)
    };
    for sec in &mut job.sections {
        sec.format.color = crate::theme::ensure_contrast(sec.format.color, canvas_bg, 4.5_f32);
    }
    job
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_detect_json() {
        let json = r#"{"name": "Alice", "age": 30, "admin": true}"#;
        assert_eq!(detect_code_language(json), Some(CodeLanguage::Json));

        let formatted = format_json(json).expect("Prettify failed");
        assert!(formatted.contains('\n'));
        assert!(formatted.contains(r#""name": "Alice""#));
    }

    #[test]
    fn test_detect_rust() {
        let snippet =
            "pub fn calculate_sum(a: i32, b: i32) -> Result<i32, String> {\n    Ok(a + b)\n}";
        assert_eq!(detect_code_language(snippet), Some(CodeLanguage::Rust));
    }

    #[test]
    fn test_detect_cpp() {
        let snippet = "#include <iostream>\n#include <vector>\n\nint main() {\n    std::vector<int> nums = {1, 2, 3};\n    std::cout << nums.size() << std::endl;\n    return 0;\n}";
        assert_eq!(detect_code_language(snippet), Some(CodeLanguage::Cpp));

        // Objective-C++ / macOS C++ excerpt with Foundation types
        let objc_cpp = "if (response_data) {\n        NSString* error_body = [[NSString alloc] initWithData:response_data encoding:NSUTF8StringEncoding];\n        if (error_body) {\n          error_msg += \": \";\n          error_msg += [error_body UTF8String];";
        assert_eq!(detect_code_language(objc_cpp), Some(CodeLanguage::Cpp));
    }

    #[test]
    fn test_detect_python() {
        let snippet = "def fetch_data(url: str):\n    response = requests.get(url)\n    return response.json()";
        assert_eq!(detect_code_language(snippet), Some(CodeLanguage::Python));
    }

    #[test]
    fn test_detect_sql() {
        let snippet = "SELECT id, name, created_at FROM users WHERE active = 1 ORDER BY id DESC;";
        assert_eq!(detect_code_language(snippet), Some(CodeLanguage::Sql));
    }

    #[test]
    fn test_detect_shell() {
        let snippet = "#!/bin/bash\ncargo test --all-targets --verbose";
        assert_eq!(detect_code_language(snippet), Some(CodeLanguage::Shell));

        // GitHub CLI commands with arguments, environment variables and quotes
        let gh_cmd = r#"gh api "/users/$USER/settings/billing/usage?year=2026&month=11""#;
        assert_eq!(detect_code_language(gh_cmd), Some(CodeLanguage::Shell));

        let quoted_gh_cmd =
            r#""gh api \"/users/$USER/settings/billing/usage?year=2026&month=11\"""#;
        assert_eq!(
            detect_code_language(quoted_gh_cmd),
            Some(CodeLanguage::Shell)
        );

        let prompt_cmd = "$ gh auth status";
        assert_eq!(detect_code_language(prompt_cmd), Some(CodeLanguage::Shell));

        let brew_cmd = "brew install gh";
        assert_eq!(detect_code_language(brew_cmd), Some(CodeLanguage::Shell));

        let pipeline_cmd = "cat /var/log/syslog | grep error";
        assert_eq!(
            detect_code_language(pipeline_cmd),
            Some(CodeLanguage::Shell)
        );
    }

    #[test]
    fn test_highlight_code_produces_sections() {
        let snippet = "let mut count: i32 = 42;\n// compute\nprintln!(\"Count: {}\", count);";
        let ctx = egui::Context::default();
        let style = egui::Style::default();
        let job = highlight_code(&ctx, &style, snippet, CodeLanguage::Rust);
        assert_ne!(job.sections.len(), 0);
        assert_eq!(job.text, snippet);
    }

    #[test]
    fn test_highlight_code_multilingual() {
        let ctx = egui::Context::default();
        let style = egui::Style::default();

        let cpp_snippet = "#include <vector>\nint x = 42;";
        let cpp_job = highlight_code(&ctx, &style, cpp_snippet, CodeLanguage::Cpp);
        assert_ne!(cpp_job.sections.len(), 0);

        let py_snippet = "def hello():\n    return 'world'";
        let py_job = highlight_code(&ctx, &style, py_snippet, CodeLanguage::Python);
        assert_ne!(py_job.sections.len(), 0);

        let sql_snippet = "SELECT * FROM users WHERE active = 1;";
        let sql_job = highlight_code(&ctx, &style, sql_snippet, CodeLanguage::Sql);
        assert_ne!(sql_job.sections.len(), 0);
    }

    #[test]
    fn test_objc_cpp_snippet() {
        let snippet = "if (response_data) {\n        NSString* error_body = [[NSString alloc] initWithData:response_data encoding:NSUTF8StringEncoding];\n        if (error_body) {\n          error_msg += \": \";\n          error_msg += [error_body UTF8String];\n        }\n}";
        let ctx = egui::Context::default();
        let style = egui::Style::default();
        let job = highlight_code(&ctx, &style, snippet, CodeLanguage::Cpp);
        assert_ne!(job.sections.len(), 0);
        assert_eq!(job.text, snippet);
    }
}
