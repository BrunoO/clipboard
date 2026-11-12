use eframe::egui::{self, Color32, FontId, TextFormat};

/// Supported programming and data languages for detection and syntax highlighting.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum CodeLanguage {
    Rust,
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

    pub(crate) const fn badge(self) -> &'static str {
        match self {
            Self::Rust => "🦀 Rust",
            Self::Python => "🐍 Python",
            Self::JavaScript => "⚡ JS",
            Self::TypeScript => "🔷 TS",
            Self::Json => "{ } JSON",
            Self::Html => "🌐 HTML",
            Self::Sql => "🗄️ SQL",
            Self::Shell => "💻 Bash",
            Self::Toml => "⚙️ TOML",
            Self::Yaml => "📄 YAML",
        }
    }
}

/// Detects if the given text represents a recognized code snippet or structured data format.
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
    if trimmed.starts_with("#!/bin/")
        || trimmed.starts_with("#!/usr/bin/env")
        || trimmed.starts_with("curl ")
        || trimmed.starts_with("cargo ")
        || trimmed.starts_with("git ")
        || trimmed.starts_with("docker ")
        || trimmed.starts_with("npm ")
        || trimmed.starts_with("sudo ")
    {
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

    // 7. Rust detection
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

struct CodeFormats {
    kw: TextFormat,
    type_fmt: TextFormat,
    str_fmt: TextFormat,
    num: TextFormat,
    comment: TextFormat,
    fn_fmt: TextFormat,
    default: TextFormat,
    punct: TextFormat,
}

impl CodeFormats {
    fn new() -> Self {
        let font_code = FontId::monospace(11.5);
        let format_of = |color: Color32| -> TextFormat {
            TextFormat {
                font_id: font_code.clone(),
                color,
                ..Default::default()
            }
        };

        Self {
            kw: format_of(Color32::from_rgb(224, 108, 117)),
            type_fmt: format_of(Color32::from_rgb(229, 192, 123)),
            str_fmt: format_of(Color32::from_rgb(152, 195, 121)),
            num: format_of(Color32::from_rgb(209, 154, 102)),
            comment: format_of(Color32::from_rgb(127, 132, 142)),
            fn_fmt: format_of(Color32::from_rgb(97, 175, 239)),
            default: format_of(Color32::from_rgb(220, 224, 230)),
            punct: format_of(Color32::from_rgb(171, 178, 191)),
        }
    }
}

/// Builds an `egui::text::LayoutJob` with syntax highlighting for the given code snippet.
pub(crate) fn highlight_code(text: &str, lang: CodeLanguage) -> egui::text::LayoutJob {
    let mut job = egui::text::LayoutJob::default();
    let fmts = CodeFormats::new();

    for line in text.lines() {
        let trimmed_line = line.trim_start();
        if is_line_comment(lang, trimmed_line) {
            job.append(line, 0.0, fmts.comment.clone());
        } else {
            tokenize_line(line, lang, &mut job, &fmts);
        }
        job.append("\n", 0.0, fmts.default.clone());
    }

    job
}

fn is_line_comment(lang: CodeLanguage, trimmed_line: &str) -> bool {
    match lang {
        CodeLanguage::Rust | CodeLanguage::JavaScript | CodeLanguage::TypeScript => {
            trimmed_line.starts_with("//")
                || trimmed_line.starts_with("/*")
                || trimmed_line.starts_with('*')
        }
        CodeLanguage::Python | CodeLanguage::Shell | CodeLanguage::Yaml | CodeLanguage::Toml => {
            trimmed_line.starts_with('#')
        }
        CodeLanguage::Sql => trimmed_line.starts_with("--"),
        CodeLanguage::Html => trimmed_line.starts_with("<!--"),
        CodeLanguage::Json => false,
    }
}

fn tokenize_line(
    line: &str,
    lang: CodeLanguage,
    job: &mut egui::text::LayoutJob,
    fmts: &CodeFormats,
) {
    let mut chars = line.chars().peekable();
    while let Some(ch) = chars.next() {
        if ch == '"' || ch == '\'' || ch == '`' {
            let quote = ch;
            let mut s = String::new();
            s.push(quote);
            while let Some(&c) = chars.peek() {
                chars.next();
                s.push(c);
                if c == quote {
                    break;
                }
                if c == '\\' {
                    if let Some(escaped) = chars.next() {
                        s.push(escaped);
                    }
                }
            }
            job.append(&s, 0.0, fmts.str_fmt.clone());
        } else if ch.is_alphabetic() || ch == '_' {
            let mut word = String::new();
            word.push(ch);
            while let Some(&c) = chars.peek() {
                if c.is_alphanumeric() || c == '_' {
                    word.push(c);
                    chars.next();
                } else {
                    break;
                }
            }

            if is_keyword(lang, &word) {
                job.append(&word, 0.0, fmts.kw.clone());
            } else if is_builtin_type(&word) {
                job.append(&word, 0.0, fmts.type_fmt.clone());
            } else if chars.peek() == Some(&'(') {
                job.append(&word, 0.0, fmts.fn_fmt.clone());
            } else {
                job.append(&word, 0.0, fmts.default.clone());
            }
        } else if ch.is_ascii_digit() {
            let mut num = String::new();
            num.push(ch);
            while let Some(&c) = chars.peek() {
                if c.is_ascii_digit() || c == '.' || c == 'x' || c == '_' {
                    num.push(c);
                    chars.next();
                } else {
                    break;
                }
            }
            job.append(&num, 0.0, fmts.num.clone());
        } else if ch == '{'
            || ch == '}'
            || ch == '('
            || ch == ')'
            || ch == '['
            || ch == ']'
            || ch == ';'
            || ch == ':'
        {
            job.append(&ch.to_string(), 0.0, fmts.punct.clone());
        } else {
            job.append(&ch.to_string(), 0.0, fmts.default.clone());
        }
    }
}

#[allow(clippy::too_many_lines)]
fn is_keyword(lang: CodeLanguage, word: &str) -> bool {
    match lang {
        CodeLanguage::Rust => matches!(
            word,
            "as" | "break"
                | "const"
                | "continue"
                | "crate"
                | "else"
                | "enum"
                | "extern"
                | "false"
                | "fn"
                | "for"
                | "if"
                | "impl"
                | "in"
                | "let"
                | "loop"
                | "match"
                | "mod"
                | "move"
                | "mut"
                | "pub"
                | "ref"
                | "return"
                | "self"
                | "Self"
                | "static"
                | "struct"
                | "super"
                | "trait"
                | "true"
                | "type"
                | "unsafe"
                | "use"
                | "where"
                | "while"
                | "async"
                | "await"
        ),
        CodeLanguage::Python => matches!(
            word,
            "False"
                | "None"
                | "True"
                | "and"
                | "as"
                | "assert"
                | "async"
                | "await"
                | "break"
                | "class"
                | "continue"
                | "def"
                | "del"
                | "elif"
                | "else"
                | "except"
                | "finally"
                | "for"
                | "from"
                | "global"
                | "if"
                | "import"
                | "in"
                | "is"
                | "lambda"
                | "nonlocal"
                | "not"
                | "or"
                | "pass"
                | "raise"
                | "return"
                | "try"
                | "while"
                | "with"
                | "yield"
        ),
        CodeLanguage::JavaScript | CodeLanguage::TypeScript => matches!(
            word,
            "break"
                | "case"
                | "catch"
                | "class"
                | "const"
                | "continue"
                | "debugger"
                | "default"
                | "delete"
                | "do"
                | "else"
                | "export"
                | "extends"
                | "finally"
                | "for"
                | "function"
                | "if"
                | "import"
                | "in"
                | "instanceof"
                | "new"
                | "return"
                | "super"
                | "switch"
                | "this"
                | "throw"
                | "try"
                | "typeof"
                | "var"
                | "void"
                | "while"
                | "with"
                | "yield"
                | "let"
                | "static"
                | "interface"
                | "type"
                | "async"
                | "await"
                | "null"
                | "true"
                | "false"
        ),
        CodeLanguage::Sql => matches!(
            word.to_uppercase().as_str(),
            "SELECT"
                | "FROM"
                | "WHERE"
                | "INSERT"
                | "INTO"
                | "UPDATE"
                | "SET"
                | "DELETE"
                | "CREATE"
                | "TABLE"
                | "DROP"
                | "ALTER"
                | "INDEX"
                | "ORDER"
                | "BY"
                | "GROUP"
                | "HAVING"
                | "LIMIT"
                | "JOIN"
                | "INNER"
                | "LEFT"
                | "RIGHT"
                | "ON"
                | "AND"
                | "OR"
                | "NOT"
                | "NULL"
                | "AS"
                | "DISTINCT"
                | "UNION"
        ),
        CodeLanguage::Shell => matches!(
            word,
            "if" | "then"
                | "else"
                | "elif"
                | "fi"
                | "case"
                | "esac"
                | "for"
                | "while"
                | "until"
                | "do"
                | "done"
                | "in"
                | "function"
                | "select"
                | "export"
                | "echo"
        ),
        CodeLanguage::Json => matches!(word, "true" | "false" | "null"),
        CodeLanguage::Html | CodeLanguage::Toml | CodeLanguage::Yaml => false,
    }
}

fn is_builtin_type(word: &str) -> bool {
    matches!(
        word,
        "bool"
            | "u8"
            | "u16"
            | "u32"
            | "u64"
            | "u128"
            | "usize"
            | "i8"
            | "i16"
            | "i32"
            | "i64"
            | "i128"
            | "isize"
            | "f32"
            | "f64"
            | "char"
            | "str"
            | "String"
            | "Option"
            | "Result"
            | "Vec"
            | "int"
            | "float"
            | "dict"
            | "list"
            | "set"
            | "tuple"
            | "number"
            | "string"
            | "boolean"
            | "any"
            | "void"
    )
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
    }

    #[test]
    fn test_highlight_code_produces_sections() {
        let snippet = "let mut count: i32 = 42;\n// compute\nprintln!(\"Count: {}\", count);";
        let job = highlight_code(snippet, CodeLanguage::Rust);
        assert_ne!(job.sections.len(), 0);
        assert_eq!(job.text, format!("{snippet}\n"));
    }
}
