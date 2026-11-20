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
            | Self::Html => crate::icons::CODE,
            Self::Json => crate::icons::BRACKETS_CURLY,
            Self::Sql => crate::icons::DATABASE,
            Self::Shell => crate::icons::TERMINAL,
            Self::Toml | Self::Yaml => crate::icons::FILE_TEXT,
        }
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
pub(crate) fn detect_code_language(text: &str) -> Option<CodeLanguage> {
    let trimmed = text.trim();
    if trimmed.len() < 4 {
        return None;
    }

    if is_json_snippet(trimmed) {
        return Some(CodeLanguage::Json);
    }
    if is_html_snippet(trimmed) {
        return Some(CodeLanguage::Html);
    }
    if is_shell_command(trimmed) {
        return Some(CodeLanguage::Shell);
    }
    if is_sql_snippet(trimmed) {
        return Some(CodeLanguage::Sql);
    }
    if is_toml_snippet(trimmed) {
        return Some(CodeLanguage::Toml);
    }
    if is_python_snippet(trimmed) {
        return Some(CodeLanguage::Python);
    }
    if is_cpp_or_objc_snippet(trimmed) {
        return Some(CodeLanguage::Cpp);
    }
    if is_rust_snippet(trimmed) {
        return Some(CodeLanguage::Rust);
    }
    if let Some(lang) = detect_js_or_ts(trimmed) {
        return Some(lang);
    }
    if is_yaml_snippet(trimmed) {
        return Some(CodeLanguage::Yaml);
    }

    None
}

fn is_json_snippet(trimmed: &str) -> bool {
    let is_bracketed = (trimmed.starts_with('{') && trimmed.ends_with('}'))
        || (trimmed.starts_with('[') && trimmed.ends_with(']'));
    is_bracketed && serde_json::from_str::<serde_json::Value>(trimmed).is_ok()
}

fn is_html_snippet(trimmed: &str) -> bool {
    trimmed.starts_with('<')
        && (trimmed.contains("</") || trimmed.ends_with("/>") || trimmed.contains("<!DOCTYPE"))
}

fn is_sql_snippet(trimmed: &str) -> bool {
    const SQL_PREFIXES: &[&str] = &[
        "SELECT ",
        "INSERT INTO ",
        "UPDATE ",
        "DELETE FROM ",
        "CREATE TABLE ",
        "ALTER TABLE ",
        "DROP TABLE ",
    ];
    let upper = trimmed.to_uppercase();
    SQL_PREFIXES.iter().any(|p| upper.starts_with(p))
}

fn is_toml_snippet(trimmed: &str) -> bool {
    trimmed.starts_with('[')
        && trimmed.contains(']')
        && trimmed.contains('=')
        && (trimmed.contains("[package]")
            || trimmed.contains("[dependencies]")
            || trimmed.contains("[lints"))
}

fn is_python_snippet(trimmed: &str) -> bool {
    let has_python_syntax = trimmed.contains("def ")
        || (trimmed.contains("import ")
            && (trimmed.contains("from ")
                || trimmed.contains("def ")
                || trimmed.contains("class ")))
        || trimmed.contains("elif ")
        || trimmed.contains("if __name__ == '__main__':")
        || (trimmed.contains("class ") && trimmed.contains(':'));

    has_python_syntax && !trimmed.contains("fn ") && !trimmed.contains("pub ")
}

fn is_cpp_or_objc_snippet(trimmed: &str) -> bool {
    const CPP_TOKENS: &[&str] = &[
        "#include <",
        "#include \"",
        "std::",
        "using namespace ",
        "cout <<",
        "cin >>",
        "nullptr",
        "constexpr ",
        "template<",
        "template <",
        "NSString",
        "NSData",
        "NSDictionary",
        "NSArray",
        "NSError",
        "NSObject",
        "[[",
        "alloc]",
        "initWith",
        "UTF8String",
        "@interface",
        "@implementation",
        "@autoreleasepool",
        "->",
        "static_cast<",
        "dynamic_cast<",
        "reinterpret_cast<",
        "const char*",
        "for (int ",
        "for (auto ",
        "printf(",
    ];

    let has_cpp_token = CPP_TOKENS.iter().any(|t| trimmed.contains(t))
        || ((trimmed.contains("public:") || trimmed.contains("private:"))
            && trimmed.contains("class "))
        || (trimmed.contains("void ") && trimmed.contains('(') && trimmed.contains(')'));

    has_cpp_token
        && !trimmed.contains("fn ")
        && !trimmed.contains("pub fn ")
        && !trimmed.contains("let mut ")
}

fn is_rust_snippet(trimmed: &str) -> bool {
    const RUST_TOKENS: &[&str] = &[
        "fn ",
        "pub fn ",
        "let mut ",
        "impl ",
        "-> Result<",
        "println!(",
        "#[derive(",
    ];
    RUST_TOKENS.iter().any(|t| trimmed.contains(t))
        || (trimmed.contains("struct ") && trimmed.contains('{'))
        || (trimmed.contains("enum ") && trimmed.contains('{'))
}

fn detect_js_or_ts(trimmed: &str) -> Option<CodeLanguage> {
    const JS_TOKENS: &[&str] = &[
        "const ",
        "let ",
        "function ",
        "console.log(",
        "export default ",
        "=> {",
    ];
    const TS_TOKENS: &[&str] = &[
        "interface ",
        ": string",
        ": number",
        ": boolean",
        ": any",
        "type ",
    ];

    if !JS_TOKENS.iter().any(|t| trimmed.contains(t)) {
        return None;
    }
    if TS_TOKENS.iter().any(|t| trimmed.contains(t)) {
        Some(CodeLanguage::TypeScript)
    } else {
        Some(CodeLanguage::JavaScript)
    }
}

fn is_yaml_snippet(trimmed: &str) -> bool {
    trimmed.starts_with("---")
        || (trimmed.contains(": ")
            && trimmed.lines().count() >= 3
            && trimmed.lines().any(|l| l.trim_start().starts_with("- ")))
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
