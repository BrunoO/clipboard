//! Pure functional text transformation engine for `CipherClipboard`.
//!
//! Provides deterministic, allocation-bounded string transformations including source formatting
//! stripping (ANSI, HTML, Markdown, invisible characters, CRLF/NBSP normalization), whitespace/line
//! unwrapping, case and identifier conversions, line operations, structured data minification, Base64
//! encoding/decoding, and URL tracking parameter stripping.
//!
//! All transformations are 100% safe Rust and adhere to `unsafe_code = "deny"`.

use crate::db::{ClipboardItem, ItemKind};
use crate::icons;

/// Categories for organizing transformations in menus and palettes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum TransformCategory {
    AiPrompt,
    Cleanup,
    Case,
    Lines,
    Developer,
    Url,
}

impl TransformCategory {
    #[must_use]
    pub(crate) const fn label(self) -> &'static str {
        match self {
            Self::AiPrompt => "AI & Prompt",
            Self::Cleanup => "Cleanup",
            Self::Case => "Case",
            Self::Lines => "Lines",
            Self::Developer => "Developer & Encode",
            Self::Url => "URL & Web",
        }
    }

    #[must_use]
    pub(crate) const fn tooltip(self) -> &'static str {
        match self {
            Self::AiPrompt => {
                "Clean, sanitize, and format code, logs, or tables for Copilot and AI coding agents"
            }
            Self::Cleanup => {
                "Strip source formatting, unwrap hard line breaks, or collapse whitespace"
            }
            Self::Case => {
                "Convert text or identifiers between uppercase, lowercase, title, camel, snake, and kebab case"
            }
            Self::Lines => "Sort, deduplicate, remove blank lines, or join lines with commas",
            Self::Developer => {
                "Prettify or minify JSON, encode or decode Base64, or quote file paths for your shell"
            }
            Self::Url => "Remove analytics and tracking query parameters from copied URLs",
        }
    }
}

/// Output of a text transformation, including an optional summary of actions taken.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct TransformOutput {
    pub(crate) text: String,
    pub(crate) summary: Option<String>,
}

/// Errors that can occur during text transformation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum TransformError {
    InvalidJson,
    InvalidBase64,
    InvalidUtf8,
    InvalidUrl,
    InvalidTable,
    ExecutionFailed(String),
}

impl std::fmt::Display for TransformError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidJson => write!(f, "Invalid JSON payload"),
            Self::InvalidBase64 => write!(f, "Invalid Base64 sequence"),
            Self::InvalidUtf8 => write!(f, "Decoded payload is not valid UTF-8"),
            Self::InvalidUrl => write!(f, "Invalid URL format"),
            Self::InvalidTable => write!(f, "Input is not a valid TSV or CSV table"),
            Self::ExecutionFailed(msg) => write!(f, "{msg}"),
        }
    }
}

impl std::error::Error for TransformError {}

/// Unified descriptor for pure text transformations.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum TextTransform {
    // AI & Prompt
    PrepareCodeForAi,
    PrepareLogForAi,
    PrepareTableForAi,

    // Cleanup
    StripFormatting,
    UnwrapLines,
    CollapseWhitespace,

    // Case
    Uppercase,
    Lowercase,
    TitleCase,
    CamelCase,
    SnakeCase,
    KebabCase,

    // Lines
    SortLinesAsc,
    DeduplicateLines,
    RemoveEmptyLines,
    JoinWithComma,

    // Developer & Encode
    JsonPrettify,
    JsonMinify,
    Base64Encode,
    Base64Decode,
    QuotePathForShell,

    // URL
    StripTrackingParams,
}

impl TextTransform {
    /// Complete catalog of all transformations.
    pub(crate) const ALL: &[Self] = &[
        // AI & Prompt
        Self::PrepareCodeForAi,
        Self::PrepareLogForAi,
        Self::PrepareTableForAi,
        // Cleanup
        Self::StripFormatting,
        Self::UnwrapLines,
        Self::CollapseWhitespace,
        // Case
        Self::Uppercase,
        Self::Lowercase,
        Self::TitleCase,
        Self::CamelCase,
        Self::SnakeCase,
        Self::KebabCase,
        // Lines
        Self::SortLinesAsc,
        Self::DeduplicateLines,
        Self::RemoveEmptyLines,
        Self::JoinWithComma,
        // Developer & Encode
        Self::JsonPrettify,
        Self::JsonMinify,
        Self::Base64Encode,
        Self::Base64Decode,
        Self::QuotePathForShell,
        // URL
        Self::StripTrackingParams,
    ];

    /// Machine identifier and user-facing label for this transform.
    const fn id_and_label(self) -> (&'static str, &'static str) {
        match self {
            Self::PrepareCodeForAi => ("prepare_code_for_ai", "Prepare Code for AI"),
            Self::PrepareLogForAi => ("prepare_log_for_ai", "Prepare Log for AI"),
            Self::PrepareTableForAi => ("prepare_table_for_ai", "Prepare Table for AI"),
            Self::StripFormatting => ("strip_formatting", "Strip Formatting"),
            Self::UnwrapLines => ("unwrap_lines", "Unwrap Hard Line Breaks"),
            Self::CollapseWhitespace => ("collapse_whitespace", "Collapse Whitespace"),
            Self::Uppercase => ("uppercase", "UPPERCASE"),
            Self::Lowercase => ("lowercase", "lowercase"),
            Self::TitleCase => ("title_case", "Title Case"),
            Self::CamelCase => ("camel_case", "camelCase"),
            Self::SnakeCase => ("snake_case", "snake_case"),
            Self::KebabCase => ("kebab_case", "kebab-case"),
            Self::SortLinesAsc => ("sort_lines_asc", "Sort Lines (A–Z)"),
            Self::DeduplicateLines => ("deduplicate_lines", "Deduplicate Lines"),
            Self::RemoveEmptyLines => ("remove_empty_lines", "Remove Empty Lines"),
            Self::JoinWithComma => ("join_with_comma", "Join Lines with Comma"),
            Self::JsonPrettify => ("json_prettify", "Prettify JSON"),
            Self::JsonMinify => ("json_minify", "Minify JSON"),
            Self::Base64Encode => ("base64_encode", "Base64 Encode"),
            Self::Base64Decode => ("base64_decode", "Base64 Decode"),
            Self::QuotePathForShell => ("quote_path_for_shell", "Quote Path for Shell"),
            Self::StripTrackingParams => ("strip_tracking_params", "Strip Tracking Parameters"),
        }
    }

    /// Unique machine identifier for this transform.
    #[must_use]
    #[allow(dead_code)]
    pub(crate) const fn id(self) -> &'static str {
        self.id_and_label().0
    }

    /// User-facing label for this transform.
    #[must_use]
    pub(crate) const fn label(self) -> &'static str {
        self.id_and_label().1
    }

    /// Category for organizing in menus.
    #[must_use]
    pub(crate) const fn category(self) -> TransformCategory {
        match self {
            Self::PrepareCodeForAi | Self::PrepareLogForAi | Self::PrepareTableForAi => {
                TransformCategory::AiPrompt
            }
            Self::StripFormatting | Self::UnwrapLines | Self::CollapseWhitespace => {
                TransformCategory::Cleanup
            }
            Self::Uppercase
            | Self::Lowercase
            | Self::TitleCase
            | Self::CamelCase
            | Self::SnakeCase
            | Self::KebabCase => TransformCategory::Case,
            Self::SortLinesAsc
            | Self::DeduplicateLines
            | Self::RemoveEmptyLines
            | Self::JoinWithComma => TransformCategory::Lines,
            Self::JsonPrettify
            | Self::JsonMinify
            | Self::Base64Encode
            | Self::Base64Decode
            | Self::QuotePathForShell => TransformCategory::Developer,
            Self::StripTrackingParams => TransformCategory::Url,
        }
    }

    /// User-facing tooltip explaining what this transform does.
    #[must_use]
    pub(crate) const fn tooltip(self) -> &'static str {
        match self {
            Self::PrepareCodeForAi => {
                "Dedent, strip local home paths, redact secrets, and wrap in a fenced code block"
            }
            Self::PrepareLogForAi => {
                "Strip ANSI codes, collapse repeated lines, redact secrets & IPs, and wrap in <log> tags"
            }
            Self::PrepareTableForAi => {
                "Convert tab-separated (TSV) or comma-separated (CSV) rows into a Markdown table"
            }
            Self::StripFormatting => {
                "Strip HTML tags, Markdown syntax, ANSI escape codes, and zero-width characters"
            }
            Self::UnwrapLines => "Join lines broken mid-sentence while preserving paragraph breaks",
            Self::CollapseWhitespace => {
                "Collapse runs of whitespace and newlines into a single line"
            }
            Self::Uppercase => "Convert all letters to uppercase",
            Self::Lowercase => "Convert all letters to lowercase",
            Self::TitleCase => "Capitalize the first letter of each word",
            Self::CamelCase => "Convert text or identifiers to camelCase",
            Self::SnakeCase => "Convert text or identifiers to snake_case",
            Self::KebabCase => "Convert text or identifiers to kebab-case",
            Self::SortLinesAsc => "Sort lines alphabetically (A–Z)",
            Self::DeduplicateLines => "Remove duplicate lines while preserving original order",
            Self::RemoveEmptyLines => "Filter out empty or blank lines",
            Self::JoinWithComma => "Join non-empty lines with a comma delimiter",
            Self::JsonPrettify => "Format JSON with clean 2-space indentation",
            Self::JsonMinify => "Compress JSON into a single compact line",
            Self::Base64Encode => "Encode plain text as Base64",
            Self::Base64Decode => "Decode Base64 sequence back into plain text",
            Self::QuotePathForShell => match crate::platform::DEFAULT_SHELL_PATH_STYLE {
                crate::platform::ShellPathStyle::Posix => {
                    "Wrap file path(s) in shell-safe single quotes (escaping embedded quotes)"
                }
                crate::platform::ShellPathStyle::Windows => {
                    "Wrap file path(s) in shell-safe double quotes for PowerShell / cmd"
                }
            },
            Self::StripTrackingParams => {
                "Remove analytics query parameters (utm_*, fbclid, gclid, etc.) from URL"
            }
        }
    }

    /// Confirmation toast message on successful copy.
    #[must_use]
    pub(crate) fn toast_success(self) -> String {
        match self {
            Self::PrepareCodeForAi => {
                format!("{} Copied AI-ready code block to clipboard!", icons::CHECK)
            }
            Self::PrepareLogForAi => {
                format!("{} Copied AI-ready log block to clipboard!", icons::CHECK)
            }
            Self::PrepareTableForAi => {
                format!("{} Copied Markdown table to clipboard!", icons::CHECK)
            }
            Self::StripFormatting => {
                format!("{} Copied raw plain text to clipboard!", icons::CHECK)
            }
            Self::UnwrapLines => format!("{} Copied unwrapped text to clipboard!", icons::CHECK),
            Self::CollapseWhitespace => {
                format!("{} Copied collapsed text to clipboard!", icons::CHECK)
            }
            Self::Uppercase => format!("{} Copied uppercase text to clipboard!", icons::CHECK),
            Self::Lowercase => format!("{} Copied lowercase text to clipboard!", icons::CHECK),
            Self::TitleCase => format!("{} Copied Title Case text to clipboard!", icons::CHECK),
            Self::CamelCase => format!("{} Copied camelCase text to clipboard!", icons::CHECK),
            Self::SnakeCase => format!("{} Copied snake_case text to clipboard!", icons::CHECK),
            Self::KebabCase => format!("{} Copied kebab-case text to clipboard!", icons::CHECK),
            Self::SortLinesAsc => format!("{} Copied sorted lines to clipboard!", icons::CHECK),
            Self::DeduplicateLines => {
                format!("{} Copied deduplicated lines to clipboard!", icons::CHECK)
            }
            Self::RemoveEmptyLines => {
                format!("{} Copied text without empty lines!", icons::CHECK)
            }
            Self::JoinWithComma => {
                format!("{} Copied comma-joined lines to clipboard!", icons::CHECK)
            }
            Self::JsonPrettify => format!("{} Copied formatted JSON to clipboard!", icons::CHECK),
            Self::JsonMinify => format!("{} Copied minified JSON to clipboard!", icons::CHECK),
            Self::Base64Encode => {
                format!("{} Copied Base64 encoded text to clipboard!", icons::CHECK)
            }
            Self::Base64Decode => {
                format!("{} Copied Base64 decoded text to clipboard!", icons::CHECK)
            }
            Self::QuotePathForShell => {
                format!("{} Copied shell-quoted path to clipboard!", icons::CHECK)
            }
            Self::StripTrackingParams => format!("{} Copied clean URL to clipboard!", icons::CHECK),
        }
    }

    /// Checks if this transform is applicable to the given clipboard item.
    #[must_use]
    pub(crate) fn applies_to(self, item: &ClipboardItem) -> bool {
        if item.kind == ItemKind::Image {
            return false;
        }
        let text = &item.content;
        match self {
            Self::PrepareLogForAi => text.contains('\n') || text.contains('\x1b'),
            Self::PrepareTableForAi => is_likely_tabular(text),
            Self::StripFormatting => has_strippable_formatting(item),
            Self::UnwrapLines
            | Self::SortLinesAsc
            | Self::DeduplicateLines
            | Self::RemoveEmptyLines
            | Self::JoinWithComma => text.contains('\n'),
            Self::CollapseWhitespace => text.chars().any(char::is_whitespace),
            Self::PrepareCodeForAi
            | Self::Uppercase
            | Self::Lowercase
            | Self::TitleCase
            | Self::CamelCase
            | Self::SnakeCase
            | Self::KebabCase
            | Self::Base64Encode => !text.trim().is_empty(),
            Self::JsonPrettify | Self::JsonMinify => {
                item.code_language() == Some(crate::code::CodeLanguage::Json)
                    || is_json_heuristic(text)
            }
            Self::Base64Decode => is_likely_base64_text(text),
            Self::QuotePathForShell => is_likely_file_path_list(text),
            Self::StripTrackingParams => has_tracking_params(text),
        }
    }

    /// Confirmation toast message tailored to an optional transformation summary.
    #[must_use]
    pub(crate) fn toast_for_output(self, summary: Option<&str>) -> String {
        let Some(summary) = summary.filter(|s| !s.is_empty()) else {
            return self.toast_success();
        };
        match self {
            Self::PrepareCodeForAi => {
                format!("{} Copied AI-ready code block ({summary})", icons::CHECK)
            }
            Self::PrepareLogForAi => {
                format!("{} Copied AI-ready log block ({summary})", icons::CHECK)
            }
            Self::PrepareTableForAi => {
                format!("{} Copied Markdown table ({summary})", icons::CHECK)
            }
            Self::QuotePathForShell => {
                format!("{} Copied shell-quoted path ({summary})", icons::CHECK)
            }
            _ => self.toast_success(),
        }
    }

    /// Executes the transformation and returns both the output text and an optional action summary.
    pub(crate) fn apply_with_summary(self, input: &str) -> Result<TransformOutput, TransformError> {
        match self {
            Self::PrepareCodeForAi => Ok(prepare_code_for_ai_detailed(input)),
            Self::PrepareLogForAi => Ok(prepare_log_for_ai_detailed(input)),
            Self::PrepareTableForAi => prepare_table_for_ai_detailed(input),
            Self::QuotePathForShell => Ok(quote_paths_for_shell_detailed(
                input,
                crate::platform::DEFAULT_SHELL_PATH_STYLE,
            )),
            other => Ok(TransformOutput {
                text: other.apply(input)?,
                summary: None,
            }),
        }
    }

    /// Executes the transformation on the provided input text.
    pub(crate) fn apply(self, input: &str) -> Result<String, TransformError> {
        match self {
            Self::PrepareCodeForAi => Ok(prepare_code_for_ai_detailed(input).text),
            Self::PrepareLogForAi => Ok(prepare_log_for_ai_detailed(input).text),
            Self::PrepareTableForAi => prepare_table_for_ai_detailed(input).map(|o| o.text),
            Self::StripFormatting => Ok(strip_source_formatting(input)),
            Self::UnwrapLines => Ok(unwrap_lines(input)),
            Self::CollapseWhitespace => Ok(collapse_whitespace(input)),
            Self::Uppercase => Ok(input.to_uppercase()),
            Self::Lowercase => Ok(input.to_lowercase()),
            Self::TitleCase => Ok(to_title_case(input)),
            Self::CamelCase => Ok(to_camel_case(input)),
            Self::SnakeCase => Ok(to_snake_case(input)),
            Self::KebabCase => Ok(to_kebab_case(input)),
            Self::SortLinesAsc => Ok(sort_lines_asc(input)),
            Self::DeduplicateLines => Ok(deduplicate_lines(input)),
            Self::RemoveEmptyLines => Ok(remove_empty_lines(input)),
            Self::JoinWithComma => Ok(join_with_comma(input)),
            Self::JsonPrettify => {
                crate::code::format_json(input).ok_or(TransformError::InvalidJson)
            }
            Self::JsonMinify => json_minify(input),
            Self::Base64Encode => Ok(base64_encode(input.as_bytes())),
            Self::Base64Decode => {
                let bytes = base64_decode(input.trim())?;
                String::from_utf8(bytes).map_err(|_| TransformError::InvalidUtf8)
            }
            Self::QuotePathForShell => Ok(quote_paths_for_shell_detailed(
                input,
                crate::platform::DEFAULT_SHELL_PATH_STYLE,
            )
            .text),
            Self::StripTrackingParams => strip_url_tracking_params(input),
        }
    }
}

/// Strips all source formatting from text, returning clean plain text.
#[must_use]
pub(crate) fn strip_source_formatting(input: &str) -> String {
    let s1 = strip_ansi(input);
    let s2 = strip_invisible_and_normalize_newlines(&s1);
    let s3 = strip_html_tags(&s2);
    let s4 = decode_html_entities(&s3);
    let s5 = strip_markdown(&s4);
    normalize_whitespace(&s5)
}

/// Fast predicate checking whether `item` contains strippable source formatting.
#[must_use]
pub(crate) fn has_strippable_formatting(item: &ClipboardItem) -> bool {
    if item.kind == ItemKind::Image {
        return false;
    }

    let text = &item.content;
    let has_candidate_chars = text.chars().any(|c| {
        matches!(
            c,
            '\x1b'
                | '\r'
                | '\u{00a0}'
                | '\u{200B}'
                | '\u{200C}'
                | '\u{200D}'
                | '\u{FEFF}'
                | '\u{FE0E}'
                | '\u{FE0F}'
                | '<'
                | '&'
                | '`'
                | '*'
                | '_'
                | '~'
                | '#'
                | '>'
                | '['
        )
    });

    let has_excess_whitespace = text.starts_with(char::is_whitespace)
        || text.ends_with(char::is_whitespace)
        || text.contains("\n\n\n")
        || text.lines().any(|l| l.ends_with(' ') || l.ends_with('\t'));

    if !has_candidate_chars && !has_excess_whitespace {
        return false;
    }

    let stripped = strip_source_formatting(text);
    stripped != *text
}

/// Joins single newlines within paragraphs while preserving paragraph breaks (`\n\n`).
fn unwrap_lines(input: &str) -> String {
    let normalized = input.replace("\r\n", "\n").replace('\r', "\n");
    let paragraphs = normalized.split("\n\n");
    let mut out_paragraphs = Vec::new();

    for p in paragraphs {
        let lines: Vec<&str> = p.lines().map(str::trim).filter(|l| !l.is_empty()).collect();
        if lines.is_empty() {
            continue;
        }
        let mut joined = String::new();
        for (i, line) in lines.iter().enumerate() {
            if i > 0 {
                if joined.ends_with('-') && !joined.ends_with(" -") {
                    joined.pop(); // Remove hyphen from split word
                } else {
                    joined.push(' ');
                }
            }
            joined.push_str(line);
        }
        out_paragraphs.push(joined);
    }

    out_paragraphs.join("\n\n")
}

/// Collapses runs of whitespace and newlines into a single line.
fn collapse_whitespace(input: &str) -> String {
    input.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Sorts lines alphabetically case-insensitively.
fn sort_lines_asc(input: &str) -> String {
    let mut lines: Vec<&str> = input.lines().collect();
    lines.sort_by_key(|a| a.to_lowercase());
    lines.join("\n")
}

/// Deduplicates lines preserving their first occurrence.
fn deduplicate_lines(input: &str) -> String {
    let mut seen = std::collections::HashSet::new();
    let mut out = Vec::new();
    for line in input.lines() {
        if seen.insert(line) {
            out.push(line);
        }
    }
    out.join("\n")
}

/// Removes blank or whitespace-only lines.
fn remove_empty_lines(input: &str) -> String {
    input
        .lines()
        .filter(|line| !line.trim().is_empty())
        .collect::<Vec<_>>()
        .join("\n")
}

/// Joins non-empty lines with a comma delimiter.
fn join_with_comma(input: &str) -> String {
    input
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .collect::<Vec<_>>()
        .join(", ")
}

/// Minifies JSON payload into a single line.
fn json_minify(input: &str) -> Result<String, TransformError> {
    let val: serde_json::Value =
        serde_json::from_str(input).map_err(|_| TransformError::InvalidJson)?;
    serde_json::to_string(&val).map_err(|e| TransformError::ExecutionFailed(e.to_string()))
}

/// Strips query parameters associated with web tracking.
fn strip_url_tracking_params(input: &str) -> Result<String, TransformError> {
    let trimmed = input.trim();
    let mut url = url::Url::parse(trimmed).map_err(|_| TransformError::InvalidUrl)?;
    if !url.has_host() {
        return Err(TransformError::InvalidUrl);
    }

    let tracking_keys: [&str; 12] = [
        "fbclid", "gclid", "mc_eid", "igshid", "ref", "msclkid", "twclid", "_ga", "_gl", "dclid",
        "s_kwcid", "yclid",
    ];

    let pairs: Vec<(String, String)> = url
        .query_pairs()
        .filter(|(k, _)| {
            let key = k.to_ascii_lowercase();
            !key.starts_with("utm_") && !tracking_keys.contains(&key.as_str())
        })
        .map(|(k, v)| (k.into_owned(), v.into_owned()))
        .collect();

    if pairs.is_empty() {
        url.set_query(None);
    } else {
        url.query_pairs_mut().clear().extend_pairs(pairs).finish();
    }

    Ok(url.to_string())
}

const BASE64_CHARS: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

/// Encodes binary bytes as standard Base64 with padding.
fn base64_encode(input: &[u8]) -> String {
    let mut out = String::with_capacity(input.len().div_ceil(3) * 4);
    let (chunks, remainder) = input.as_chunks::<3>();
    for chunk in chunks {
        let n = ((u32::from(chunk[0])) << 16) | ((u32::from(chunk[1])) << 8) | u32::from(chunk[2]);
        out.push(BASE64_CHARS[((n >> 18) & 63) as usize] as char);
        out.push(BASE64_CHARS[((n >> 12) & 63) as usize] as char);
        out.push(BASE64_CHARS[((n >> 6) & 63) as usize] as char);
        out.push(BASE64_CHARS[(n & 63) as usize] as char);
    }
    if remainder.len() == 1 {
        let n = (u32::from(remainder[0])) << 16;
        out.push(BASE64_CHARS[((n >> 18) & 63) as usize] as char);
        out.push(BASE64_CHARS[((n >> 12) & 63) as usize] as char);
        out.push('=');
        out.push('=');
    } else if remainder.len() == 2 {
        let n = ((u32::from(remainder[0])) << 16) | ((u32::from(remainder[1])) << 8);
        out.push(BASE64_CHARS[((n >> 18) & 63) as usize] as char);
        out.push(BASE64_CHARS[((n >> 12) & 63) as usize] as char);
        out.push(BASE64_CHARS[((n >> 6) & 63) as usize] as char);
        out.push('=');
    }
    out
}

fn decode_base64_char(c: u8) -> Result<u32, TransformError> {
    match c {
        b'A'..=b'Z' => Ok(u32::from(c - b'A')),
        b'a'..=b'z' => Ok(u32::from(c - b'a' + 26)),
        b'0'..=b'9' => Ok(u32::from(c - b'0' + 52)),
        b'+' => Ok(62),
        b'/' => Ok(63),
        _ => Err(TransformError::InvalidBase64),
    }
}

/// Decodes standard Base64 string into bytes.
fn base64_decode(input: &str) -> Result<Vec<u8>, TransformError> {
    let clean: String = input.chars().filter(|c| !c.is_whitespace()).collect();
    if !clean.len().is_multiple_of(4) || clean.is_empty() {
        return Err(TransformError::InvalidBase64);
    }

    let mut out = Vec::with_capacity(clean.len() * 3 / 4);
    let bytes = clean.as_bytes();
    let (chunks, _) = bytes.as_chunks::<4>();

    for chunk in chunks {
        let c0 = decode_base64_char(chunk[0])?;
        let c1 = decode_base64_char(chunk[1])?;

        if chunk[2] == b'=' {
            if chunk[3] != b'=' {
                return Err(TransformError::InvalidBase64);
            }
            let n = (c0 << 18) | (c1 << 12);
            out.push(((n >> 16) & 0xFF) as u8);
        } else if chunk[3] == b'=' {
            let c2 = decode_base64_char(chunk[2])?;
            let n = (c0 << 18) | (c1 << 12) | (c2 << 6);
            out.push(((n >> 16) & 0xFF) as u8);
            out.push(((n >> 8) & 0xFF) as u8);
        } else {
            let c2 = decode_base64_char(chunk[2])?;
            let c3 = decode_base64_char(chunk[3])?;
            let n = (c0 << 18) | (c1 << 12) | (c2 << 6) | c3;
            out.push(((n >> 16) & 0xFF) as u8);
            out.push(((n >> 8) & 0xFF) as u8);
            out.push((n & 0xFF) as u8);
        }
    }

    Ok(out)
}

/// Splits text into word tokens across `camelCase`, `snake_case`, `kebab-case`, or whitespace boundaries.
fn split_words(input: &str) -> Vec<String> {
    let mut words = Vec::new();
    let mut current = String::new();
    let chars: Vec<char> = input.chars().collect();
    let len = chars.len();

    for i in 0..len {
        let ch = chars[i];
        if ch.is_alphanumeric() {
            if !current.is_empty() {
                let prev = chars[i - 1];
                let camel_transition =
                    (prev.is_lowercase() || prev.is_ascii_digit()) && ch.is_uppercase();
                let acronym_transition = prev.is_uppercase()
                    && ch.is_uppercase()
                    && i + 1 < len
                    && chars[i + 1].is_lowercase();

                if camel_transition || acronym_transition {
                    words.push(current);
                    current = String::new();
                }
            }
            current.push(ch);
        } else if !current.is_empty() {
            words.push(current);
            current = String::new();
        }
    }

    if !current.is_empty() {
        words.push(current);
    }

    words
}

fn to_snake_case(input: &str) -> String {
    let words = split_words(input);
    words
        .into_iter()
        .map(|w| w.to_lowercase())
        .collect::<Vec<_>>()
        .join("_")
}

fn to_kebab_case(input: &str) -> String {
    let words = split_words(input);
    words
        .into_iter()
        .map(|w| w.to_lowercase())
        .collect::<Vec<_>>()
        .join("-")
}

fn to_camel_case(input: &str) -> String {
    let words = split_words(input);
    let mut s = String::new();
    for (idx, w) in words.into_iter().enumerate() {
        if idx == 0 {
            s.push_str(&w.to_lowercase());
        } else {
            let mut chars = w.chars();
            if let Some(first) = chars.next() {
                s.extend(first.to_uppercase());
                s.extend(chars.map(|c| c.to_ascii_lowercase()));
            }
        }
    }
    s
}

fn to_title_case(input: &str) -> String {
    let words = split_words(input);
    let mut s = Vec::new();
    for w in words {
        let mut chars = w.chars();
        if let Some(first) = chars.next() {
            let mut capitalized = String::new();
            capitalized.extend(first.to_uppercase());
            capitalized.extend(chars.map(|c| c.to_ascii_lowercase()));
            s.push(capitalized);
        }
    }
    s.join(" ")
}

fn is_json_heuristic(text: &str) -> bool {
    let trimmed = text.trim();
    ((trimmed.starts_with('{') && trimmed.ends_with('}'))
        || (trimmed.starts_with('[') && trimmed.ends_with(']')))
        && serde_json::from_str::<serde_json::Value>(trimmed).is_ok()
}

fn is_likely_base64_text(text: &str) -> bool {
    let trimmed = text.trim();
    if trimmed.len() < 4 || !trimmed.len().is_multiple_of(4) {
        return false;
    }
    if !trimmed
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '+' || c == '/' || c == '=')
    {
        return false;
    }
    base64_decode(trimmed).is_ok_and(|bytes| String::from_utf8(bytes).is_ok())
}

fn has_tracking_params(text: &str) -> bool {
    let trimmed = text.trim();
    if let Ok(u) = url::Url::parse(trimmed) {
        if u.has_host() && u.query().is_some() {
            return u.query_pairs().any(|(k, _)| {
                let key = k.to_ascii_lowercase();
                key.starts_with("utm_")
                    || matches!(
                        key.as_str(),
                        "fbclid"
                            | "gclid"
                            | "mc_eid"
                            | "igshid"
                            | "ref"
                            | "msclkid"
                            | "twclid"
                            | "_ga"
                            | "_gl"
                            | "dclid"
                            | "s_kwcid"
                            | "yclid"
                    )
            });
        }
    }
    false
}

/// Strips ANSI CSI and OSC escape sequences from `input`.
fn strip_ansi(input: &str) -> String {
    if !input.contains('\x1b') {
        return input.to_string();
    }

    let mut out = String::with_capacity(input.len());
    let mut chars = input.chars().peekable();

    while let Some(ch) = chars.next() {
        if ch == '\x1b' {
            consume_escape_sequence(&mut chars);
        } else {
            out.push(ch);
        }
    }

    out
}

/// Consumes the payload following an ESC byte, if it is a recognized sequence.
fn consume_escape_sequence(chars: &mut std::iter::Peekable<std::str::Chars<'_>>) {
    match chars.peek().copied() {
        Some('[') => consume_csi_sequence(chars),
        Some(']') => consume_osc_sequence(chars),
        Some(c) if c.is_ascii() => {
            chars.next();
        }
        _ => {}
    }
}

/// Consumes CSI parameter bytes (0x20..=0x3F) and the final byte (0x40..=0x7E).
fn consume_csi_sequence(chars: &mut std::iter::Peekable<std::str::Chars<'_>>) {
    chars.next(); // Consume '['
    while let Some(&c) = chars.peek() {
        if matches!(c, ' '..='?') {
            chars.next();
        } else {
            break;
        }
    }
    if let Some(&c) = chars.peek() {
        if matches!(c, '@'..='~') {
            chars.next();
        }
    }
}

/// Consumes an OSC payload terminated by BEL or the ESC `\` string terminator.
fn consume_osc_sequence(chars: &mut std::iter::Peekable<std::str::Chars<'_>>) {
    chars.next(); // Consume ']'
    while let Some(c) = chars.next() {
        if c == '\x07' {
            break;
        }
        if c == '\x1b' && chars.peek() == Some(&'\\') {
            chars.next();
            break;
        }
    }
}

/// Strips zero-width/invisible characters and normalizes CRLF and NBSP.
fn strip_invisible_and_normalize_newlines(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    let mut chars = input.chars().peekable();

    while let Some(ch) = chars.next() {
        match ch {
            '\u{200B}' | '\u{200C}' | '\u{200D}' | '\u{FEFF}' | '\u{FE0E}' | '\u{FE0F}' => {}
            '\u{00A0}' => out.push(' '),
            '\r' => {
                if chars.peek() == Some(&'\n') {
                    chars.next();
                }
                out.push('\n');
            }
            other => out.push(other),
        }
    }

    out
}

/// Strips HTML/XML tags, drops script/style blocks, and converts block tags to newlines.
fn strip_html_tags(input: &str) -> String {
    if !input.contains('<') {
        return input.to_string();
    }

    let mut out = String::with_capacity(input.len());
    let chars: Vec<char> = input.chars().collect();
    let len = chars.len();
    let mut i = 0;

    while i < len {
        if chars[i] != '<' {
            out.push(chars[i]);
            i += 1;
            continue;
        }

        let remaining = &chars[i..];
        if starts_with_tag_ignore_case(remaining, "script") {
            i = skip_closed_block(&chars, i, "script", "/script");
            continue;
        }
        if starts_with_tag_ignore_case(remaining, "style") {
            i = skip_closed_block(&chars, i, "style", "/style");
            continue;
        }

        let (tag_content, next_i) = consume_tag_content(&chars, i);
        i = next_i;
        if is_block_tag_content(&tag_content) {
            out.push('\n');
        }
    }

    out
}

/// Skips an opening tag and everything up to and including its closing tag.
fn skip_closed_block(chars: &[char], mut i: usize, open_tag: &str, close_tag: &str) -> usize {
    let len = chars.len();
    i += open_tag.len() + 1; // Skip '<' and the opening tag name.

    while i < len {
        if chars[i] == '<' && starts_with_tag_ignore_case(&chars[i..], close_tag) {
            while i < len && chars[i] != '>' {
                i += 1;
            }
            if i < len {
                i += 1;
            }
            break;
        }
        i += 1;
    }

    i
}

/// Reads a tag body up to its unquoted `>`, returning the body and the next index.
fn consume_tag_content(chars: &[char], mut i: usize) -> (String, usize) {
    let len = chars.len();
    let mut tag_content = String::new();
    let mut in_quotes = None;
    i += 1; // Skip '<'

    while i < len {
        let ch = chars[i];
        if let Some(q) = in_quotes {
            if ch == q {
                in_quotes = None;
            }
        } else if ch == '"' || ch == '\'' {
            in_quotes = Some(ch);
        } else if ch == '>' {
            i += 1;
            break;
        }
        tag_content.push(ch);
        i += 1;
    }

    (tag_content, i)
}

/// Reports whether a raw tag body (e.g. `/div class="x"`) names a block tag.
fn is_block_tag_content(tag_content: &str) -> bool {
    let tag_name = tag_content
        .trim()
        .trim_start_matches('/')
        .split_whitespace()
        .next()
        .unwrap_or("")
        .to_ascii_lowercase();
    is_block_tag(&tag_name)
}

fn starts_with_tag_ignore_case(chars: &[char], tag_name: &str) -> bool {
    if chars.is_empty() || chars[0] != '<' {
        return false;
    }
    let slice = &chars[1..];
    let tag_chars: Vec<char> = tag_name.chars().collect();
    if slice.len() < tag_chars.len() {
        return false;
    }
    for (a, b) in slice.iter().zip(tag_chars.iter()) {
        if !a.eq_ignore_ascii_case(b) {
            return false;
        }
    }
    let next_idx = tag_chars.len();
    if next_idx < slice.len() {
        let next_ch = slice[next_idx];
        next_ch == '>' || next_ch == '/' || next_ch.is_whitespace()
    } else {
        true
    }
}

fn is_block_tag(name: &str) -> bool {
    matches!(
        name,
        "p" | "div"
            | "br"
            | "br/"
            | "li"
            | "ul"
            | "ol"
            | "tr"
            | "table"
            | "h1"
            | "h2"
            | "h3"
            | "h4"
            | "h5"
            | "h6"
            | "blockquote"
            | "hr"
            | "hr/"
            | "pre"
            | "section"
            | "header"
            | "footer"
            | "article"
    )
}

/// Decodes common HTML entities to their plain character equivalents.
fn decode_html_entities(input: &str) -> String {
    if !input.contains('&') {
        return input.to_string();
    }

    let mut out = String::with_capacity(input.len());
    let chars: Vec<char> = input.chars().collect();
    let len = chars.len();
    let mut i = 0;

    while i < len {
        if chars[i] == '&' {
            let mut end = i + 1;
            while end < len
                && end - i <= 10
                && chars[end] != ';'
                && chars[end] != '&'
                && !chars[end].is_whitespace()
            {
                end += 1;
            }

            if end < len && chars[end] == ';' {
                let entity: String = chars[i + 1..end].iter().collect();
                if let Some(decoded) = decode_entity(&entity) {
                    out.push_str(&decoded);
                    i = end + 1;
                    continue;
                }
            }
        }
        out.push(chars[i]);
        i += 1;
    }

    out
}

fn decode_entity(entity: &str) -> Option<String> {
    match entity {
        "amp" => Some("&".to_string()),
        "lt" => Some("<".to_string()),
        "gt" => Some(">".to_string()),
        "quot" => Some("\"".to_string()),
        "apos" => Some("'".to_string()),
        "nbsp" => Some(" ".to_string()),
        "copy" => Some("©".to_string()),
        "reg" => Some("®".to_string()),
        "trade" => Some("™".to_string()),
        "mdash" => Some("—".to_string()),
        "ndash" => Some("–".to_string()),
        "hellip" => Some("…".to_string()),
        "bull" => Some("•".to_string()),
        "euro" => Some("€".to_string()),
        "pound" => Some("£".to_string()),
        "yen" => Some("¥".to_string()),
        "cent" => Some("¢".to_string()),
        "deg" => Some("°".to_string()),
        "plusmn" => Some("±".to_string()),
        "times" => Some("×".to_string()),
        "divide" => Some("÷".to_string()),
        "lsquo" => Some("‘".to_string()),
        "rsquo" => Some("’".to_string()),
        "ldquo" => Some("“".to_string()),
        "rdquo" => Some("”".to_string()),
        _ if entity.starts_with('#') => {
            let val = if entity.starts_with("#x") || entity.starts_with("#X") {
                u32::from_str_radix(&entity[2..], 16).ok()
            } else {
                entity[1..].parse::<u32>().ok()
            };
            val.and_then(char::from_u32).map(|c| c.to_string())
        }
        _ => None,
    }
}

/// Strips common Markdown formatting tokens from `input`.
fn strip_markdown(input: &str) -> String {
    let mut out_lines = Vec::new();
    let mut in_fenced_code = false;

    for raw_line in input.lines() {
        let trimmed_start = raw_line.trim_start();

        if trimmed_start.starts_with("```") || trimmed_start.starts_with("~~~") {
            in_fenced_code = !in_fenced_code;
            continue;
        }

        if in_fenced_code {
            out_lines.push(raw_line.to_string());
            continue;
        }

        let mut line = raw_line.to_string();

        if let Some(rest) = strip_heading_prefix(&line) {
            line = rest;
        }

        if let Some(rest) = strip_blockquote_prefix(&line) {
            line = rest;
        }

        if let Some(rest) = strip_list_prefix(&line) {
            line = rest;
        }

        line = strip_inline_markdown(&line);
        out_lines.push(line);
    }

    out_lines.join("\n")
}

fn strip_heading_prefix(line: &str) -> Option<String> {
    let trimmed = line.trim_start();
    if !trimmed.starts_with('#') {
        return None;
    }
    let mut hashes = 0;
    for ch in trimmed.chars() {
        if ch == '#' {
            hashes += 1;
        } else {
            break;
        }
    }
    if (1..=6).contains(&hashes) {
        let remainder = &trimmed[hashes..];
        if remainder.is_empty() {
            return Some(String::new());
        }
        if remainder.starts_with(' ') {
            let heading_text = remainder.trim_start();
            let stripped_trailing = heading_text.trim_end_matches('#').trim_end();
            return Some(stripped_trailing.to_string());
        }
    }
    None
}

fn strip_blockquote_prefix(line: &str) -> Option<String> {
    let trimmed = line.trim_start();
    trimmed
        .strip_prefix("> ")
        .map(ToString::to_string)
        .or_else(|| (trimmed == ">").then(String::new))
}

fn strip_list_prefix(line: &str) -> Option<String> {
    let trimmed = line.trim_start();
    if let Some(rest) = trimmed
        .strip_prefix("- ")
        .or_else(|| trimmed.strip_prefix("* "))
        .or_else(|| trimmed.strip_prefix("+ "))
    {
        return Some(rest.to_string());
    }

    let digits = trimmed.chars().take_while(char::is_ascii_digit).count();
    if digits > 0 && digits < 10 {
        let after_digits = &trimmed[digits..];
        if let Some(rest) = after_digits.strip_prefix(". ") {
            return Some(rest.to_string());
        }
    }

    None
}

fn strip_inline_markdown(line: &str) -> String {
    let s1 = strip_images(line);
    let s2 = strip_links(&s1);
    let s3 = strip_inline_code(&s2);
    strip_emphasis(&s3)
}

fn strip_images(input: &str) -> String {
    strip_bracket_links(input, true)
}

fn strip_links(input: &str) -> String {
    strip_bracket_links(input, false)
}

fn strip_bracket_links(input: &str, is_image: bool) -> String {
    let prefix = if is_image { "![" } else { "[" };
    if !input.contains(prefix) {
        return input.to_string();
    }

    let mut out = String::with_capacity(input.len());
    let chars: Vec<char> = input.chars().collect();
    let len = chars.len();
    let mut i = 0;

    while i < len {
        if matches_link_prefix(&chars, i, is_image) {
            let label_start = if is_image { i + 2 } else { i + 1 };
            if let Some((label, next_i)) = consume_bracket_link(&chars, label_start) {
                out.push_str(&label);
                i = next_i;
                continue;
            }
        }

        out.push(chars[i]);
        i += 1;
    }

    out
}

/// Reports whether a `[label]` or `![label]` begins at `i`.
const fn matches_link_prefix(chars: &[char], i: usize, is_image: bool) -> bool {
    if is_image {
        i + 1 < chars.len() && chars[i] == '!' && chars[i + 1] == '['
    } else {
        chars[i] == '[' && (i == 0 || chars[i - 1] != '!')
    }
}

/// Consumes `[label](url)` starting at the label, returning the label and the next index.
fn consume_bracket_link(chars: &[char], label_start: usize) -> Option<(String, usize)> {
    let len = chars.len();
    let mut label_end = label_start;
    while label_end < len && chars[label_end] != ']' {
        label_end += 1;
    }

    if label_end + 1 >= len || chars[label_end] != ']' || chars[label_end + 1] != '(' {
        return None;
    }

    let url_start = label_end + 2;
    let mut url_end = url_start;
    while url_end < len && chars[url_end] != ')' {
        url_end += 1;
    }

    if url_end >= len || chars[url_end] != ')' {
        return None;
    }

    let label: String = chars[label_start..label_end].iter().collect();
    Some((label, url_end + 1))
}

fn strip_inline_code(input: &str) -> String {
    if !input.contains('`') {
        return input.to_string();
    }

    let mut out = String::with_capacity(input.len());
    let chars: Vec<char> = input.chars().collect();
    let len = chars.len();
    let mut i = 0;

    while i < len {
        if chars[i] == '`' {
            let start = i + 1;
            let mut end = start;
            while end < len && chars[end] != '`' {
                end += 1;
            }
            if end < len && chars[end] == '`' {
                let code: String = chars[start..end].iter().collect();
                out.push_str(&code);
                i = end + 1;
                continue;
            }
        }
        out.push(chars[i]);
        i += 1;
    }

    out
}

fn strip_emphasis(input: &str) -> String {
    let mut s = input.to_string();
    s = replace_delimiter_pair(&s, "~~");
    s = replace_delimiter_pair(&s, "**");
    s = replace_delimiter_pair(&s, "__");
    s = replace_single_delimiter(&s, '*');
    s = replace_single_delimiter(&s, '_');
    s
}

fn replace_delimiter_pair(input: &str, delim: &str) -> String {
    if !input.contains(delim) {
        return input.to_string();
    }

    let mut out = String::with_capacity(input.len());
    let mut rest = input;

    while let Some(start_idx) = rest.find(delim) {
        out.push_str(&rest[..start_idx]);
        let after_start = &rest[start_idx + delim.len()..];
        if let Some(end_idx) = after_start.find(delim) {
            let inner = &after_start[..end_idx];
            if !inner.trim().is_empty() {
                out.push_str(inner);
                rest = &after_start[end_idx + delim.len()..];
                continue;
            }
        }
        out.push_str(delim);
        rest = after_start;
    }

    out.push_str(rest);
    out
}

fn replace_single_delimiter(input: &str, delim: char) -> String {
    if !input.contains(delim) {
        return input.to_string();
    }

    let mut out = String::with_capacity(input.len());
    let chars: Vec<char> = input.chars().collect();
    let len = chars.len();
    let mut i = 0;

    while i < len {
        if chars[i] == delim
            && i + 1 < len
            && !chars[i + 1].is_whitespace()
            && chars[i + 1] != delim
        {
            let start = i + 1;
            let mut end = start;
            while end < len && chars[end] != delim {
                end += 1;
            }
            if end < len && chars[end] == delim && !chars[end - 1].is_whitespace() {
                let inner: String = chars[start..end].iter().collect();
                out.push_str(&inner);
                i = end + 1;
                continue;
            }
        }
        out.push(chars[i]);
        i += 1;
    }

    out
}

/// Normalizes whitespace by trimming trailing line spaces, collapsing 3+ newlines to 2,
/// and trimming outer whitespace.
fn normalize_whitespace(input: &str) -> String {
    let mut lines = Vec::new();
    for line in input.lines() {
        lines.push(line.trim_end());
    }
    let joined = lines.join("\n");

    let mut collapsed = String::with_capacity(joined.len());
    let mut consecutive_newlines = 0;

    for ch in joined.chars() {
        if ch == '\n' {
            consecutive_newlines += 1;
            if consecutive_newlines <= 2 {
                collapsed.push('\n');
            }
        } else {
            consecutive_newlines = 0;
            collapsed.push(ch);
        }
    }

    collapsed.trim().to_string()
}

// ============================================================================
// AI & Prompt Presets
// ============================================================================

/// Formats a bounded preview snippet suitable for hover tooltips.
#[must_use]
pub(crate) fn format_preview_snippet(
    text: &str,
    max_lines: usize,
    max_chars_per_line: usize,
) -> String {
    let all_lines: Vec<&str> = text.lines().collect();
    let total = all_lines.len();
    let mut out = Vec::with_capacity(max_lines.min(total) + 1);

    for line in all_lines.iter().take(max_lines) {
        let char_count = line.chars().count();
        if char_count > max_chars_per_line {
            let truncated: String = line
                .chars()
                .take(max_chars_per_line.saturating_sub(1))
                .collect();
            out.push(format!("{truncated}…"));
        } else {
            out.push((*line).to_string());
        }
    }

    if total > max_lines {
        let remaining = total - max_lines;
        let noun = if remaining == 1 { "line" } else { "lines" };
        out.push(format!("… (+{remaining} more {noun})"));
    }

    out.join("\n")
}

fn format_count(n: usize, singular: &str, plural: &str) -> String {
    let noun = if n == 1 { singular } else { plural };
    format!("{n} {noun}")
}

/// Prepares a code snippet for pasting into an AI tool:
/// dedents common indentation, strips local home paths, redacts secrets,
/// and wraps the result in a fenced Markdown code block.
fn prepare_code_for_ai_detailed(input: &str) -> TransformOutput {
    let normalized = strip_invisible_and_normalize_newlines(input);
    let (dedented, was_dedented) = dedent_with_flag(&normalized);
    let lang = crate::code::detect_code_language(&dedented);
    let (sanitized_paths, paths_stripped) = strip_local_paths_counted(&dedented);
    let (redacted, stats) = redact_secrets_counted(&sanitized_paths, false);
    let fenced = wrap_in_code_fence(&redacted, lang);

    let mut parts = Vec::new();
    if let Some(l) = lang {
        parts.push(format!("{} fence", l.markdown_fence_tag()));
    } else {
        parts.push("fenced".to_string());
    }
    if was_dedented {
        parts.push("dedented".to_string());
    }
    if paths_stripped > 0 {
        parts.push(format!(
            "{} stripped",
            format_count(paths_stripped, "path", "paths")
        ));
    }
    if stats.secrets > 0 {
        parts.push(format!(
            "{} redacted",
            format_count(stats.secrets, "secret", "secrets")
        ));
    }

    TransformOutput {
        text: fenced,
        summary: Some(parts.join(" · ")),
    }
}

/// Prepares terminal logs or stack traces for pasting into an AI tool:
/// strips ANSI escape codes, strips local home paths, redacts secrets and IPs,
/// collapses repeated consecutive lines, and wraps in `<log>...</log>`.
fn prepare_log_for_ai_detailed(input: &str) -> TransformOutput {
    let no_ansi = strip_ansi(input);
    let ansi_stripped = no_ansi != input;
    let normalized = strip_invisible_and_normalize_newlines(&no_ansi);
    let (sanitized_paths, paths_stripped) = strip_local_paths_counted(&normalized);
    let (redacted, stats) = redact_secrets_counted(&sanitized_paths, true);
    let (collapsed, lines_collapsed) = collapse_repeated_lines_counted(&redacted);
    let body = collapsed.trim_matches('\n');

    let mut parts = Vec::new();
    if ansi_stripped {
        parts.push("ANSI stripped".to_string());
    }
    if lines_collapsed > 0 {
        parts.push(format!(
            "{} collapsed",
            format_count(lines_collapsed, "line", "lines")
        ));
    }
    if paths_stripped > 0 {
        parts.push(format!(
            "{} stripped",
            format_count(paths_stripped, "path", "paths")
        ));
    }
    if stats.secrets > 0 {
        parts.push(format!(
            "{} redacted",
            format_count(stats.secrets, "secret", "secrets")
        ));
    }
    if stats.ips > 0 {
        parts.push(format!("{} redacted", format_count(stats.ips, "IP", "IPs")));
    }
    if parts.is_empty() {
        parts.push("wrapped in <log>".to_string());
    }

    TransformOutput {
        text: format!("<log>\n{body}\n</log>"),
        summary: Some(parts.join(" · ")),
    }
}

/// Converts TSV, CSV, or semicolon-separated tabular text into a Markdown pipe table.
fn prepare_table_for_ai_detailed(input: &str) -> Result<TransformOutput, TransformError> {
    let rows = parse_tabular_rows(input).ok_or(TransformError::InvalidTable)?;
    let max_cols = rows.iter().map(Vec::len).max().unwrap_or(0);
    if max_cols < 2 || rows.len() < 2 {
        return Err(TransformError::InvalidTable);
    }

    let mut out = Vec::with_capacity(rows.len() + 1);
    for (idx, row) in rows.iter().enumerate() {
        let mut cells = Vec::with_capacity(max_cols);
        for col in 0..max_cols {
            let raw = row.get(col).map_or("", String::as_str).trim();
            cells.push(raw.replace('|', "\\|"));
        }
        out.push(format!("| {} |", cells.join(" | ")));
        if idx == 0 {
            let sep = vec!["---"; max_cols].join(" | ");
            out.push(format!("| {sep} |"));
        }
    }

    Ok(TransformOutput {
        text: out.join("\n"),
        summary: Some(format!(
            "{} × {}",
            format_count(rows.len(), "row", "rows"),
            format_count(max_cols, "col", "cols")
        )),
    })
}

/// Strips the common leading whitespace prefix across all non-empty lines and trims outer blank lines.
fn dedent_with_flag(input: &str) -> (String, bool) {
    let all_lines: Vec<&str> = input.lines().collect();
    let Some(first) = all_lines.iter().position(|l| !l.trim().is_empty()) else {
        return (String::new(), false);
    };
    let last = all_lines
        .iter()
        .rposition(|l| !l.trim().is_empty())
        .unwrap_or(first);
    let slice = &all_lines[first..=last];

    let mut common_prefix: Option<&str> = None;
    for line in slice {
        if line.trim().is_empty() {
            continue;
        }
        let ws_len = line
            .bytes()
            .take_while(|&b| b == b' ' || b == b'\t')
            .count();
        let ws = &line[..ws_len];
        common_prefix = Some(common_prefix.map_or(ws, |prev| {
            let shared = prev
                .bytes()
                .zip(ws.bytes())
                .take_while(|(a, b)| a == b)
                .count();
            &prev[..shared]
        }));
    }

    let prefix_len = common_prefix.map_or(0, str::len);
    let result = slice
        .iter()
        .map(|line| {
            if line.trim().is_empty() {
                ""
            } else {
                line[prefix_len..].trim_end()
            }
        })
        .collect::<Vec<_>>()
        .join("\n");
    (result, prefix_len > 0)
}

/// Wraps `body` in a Markdown fenced code block using enough backticks to avoid inner collisions.
fn wrap_in_code_fence(body: &str, lang: Option<crate::code::CodeLanguage>) -> String {
    let mut max_run = 0usize;
    let mut current_run = 0usize;
    for b in body.bytes() {
        if b == b'`' {
            current_run += 1;
            if current_run > max_run {
                max_run = current_run;
            }
        } else {
            current_run = 0;
        }
    }
    let fence_len = max_run.saturating_add(1).max(3);
    let fence = "`".repeat(fence_len);
    let tag = lang.map_or("", crate::code::CodeLanguage::markdown_fence_tag);
    format!("{fence}{tag}\n{body}\n{fence}")
}

/// Replaces local macOS (`/Users/<user>`), Linux (`/home/<user>`), and Windows
/// (`C:\Users\<user>`, `C:/Users/<user>`, `C:\\Users\\<user>`) home directory prefixes with `<HOME>`.
#[cfg(test)]
fn strip_local_paths(input: &str) -> String {
    strip_local_paths_counted(input).0
}

fn strip_local_paths_counted(input: &str) -> (String, usize) {
    let bytes = input.as_bytes();
    let len = bytes.len();
    let mut out = String::with_capacity(len);
    let mut count = 0usize;
    let mut i = 0;

    while i < len {
        if let Some(end) = match_home_path_prefix(input, i) {
            out.push_str("<HOME>");
            count += 1;
            i = end;
            continue;
        }

        let ch = input[i..].chars().next().unwrap_or('\0');
        out.push(ch);
        i += ch.len_utf8();
    }

    (out, count)
}

/// If a local home directory path begins at byte index `i`, returns the byte index right after the username.
fn match_home_path_prefix(input: &str, i: usize) -> Option<usize> {
    let bytes = input.as_bytes();
    if i > 0 {
        let prev = bytes[i - 1];
        if prev.is_ascii_alphanumeric() || prev == b'_' {
            return None;
        }
    }

    let rest = &input[i..];

    // Unix: /Users/<name> or /home/<name>
    let after_root = if rest.starts_with("/Users/") {
        Some(i + "/Users/".len())
    } else if rest.starts_with("/home/") {
        Some(i + "/home/".len())
    } else {
        match_windows_users_root(rest).map(|offset| i + offset)
    }?;

    // Consume username: [A-Za-z0-9._-]+
    let mut end = after_root;
    while end < bytes.len() {
        let b = bytes[end];
        if b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b'-') {
            end += 1;
        } else {
            break;
        }
    }

    if end == after_root {
        return None;
    }

    // Username must be bounded by path separator, delimiter, or end of string.
    if end < bytes.len() {
        let next = bytes[end];
        let valid_boundary = matches!(
            next,
            b'/' | b'\\'
                | b' '
                | b'\t'
                | b'\n'
                | b'\r'
                | b'"'
                | b'\''
                | b'`'
                | b':'
                | b')'
                | b']'
                | b'}'
                | b'>'
                | b','
                | b';'
        );
        if !valid_boundary {
            return None;
        }
    }

    Some(end)
}

/// Matches Windows `[A-Za-z]:\Users\`, `[A-Za-z]:/Users/`, or `[A-Za-z]:\\Users\\` case-insensitively.
fn match_windows_users_root(rest: &str) -> Option<usize> {
    let bytes = rest.as_bytes();
    if bytes.len() < 9 || !bytes[0].is_ascii_alphabetic() || bytes[1] != b':' {
        return None;
    }
    let after_drive = &rest[2..];
    for prefix in ["\\\\Users\\\\", "\\Users\\", "/Users/"] {
        if after_drive.len() >= prefix.len()
            && after_drive[..prefix.len()].eq_ignore_ascii_case(prefix)
        {
            return Some(2 + prefix.len());
        }
    }
    None
}

#[derive(Default)]
struct RedactionStats {
    secrets: usize,
    ips: usize,
}

/// Redacts PEM private keys, sensitive key-value assignments, URL credentials, Bearer tokens,
/// known API key/JWT formats, and (optionally) non-loopback IPv4 addresses.
#[cfg(test)]
fn redact_secrets(input: &str, redact_ips: bool) -> String {
    redact_secrets_counted(input, redact_ips).0
}

fn redact_secrets_counted(input: &str, redact_ips: bool) -> (String, RedactionStats) {
    let mut out_lines = Vec::new();
    let mut in_pem_block = false;
    let mut stats = RedactionStats::default();

    for line in input.lines() {
        if in_pem_block {
            if is_pem_end_line(line) {
                in_pem_block = false;
            }
            continue;
        }
        if is_pem_begin_line(line) {
            out_lines.push("[REDACTED_PRIVATE_KEY]".to_string());
            stats.secrets += 1;
            if !is_pem_end_line(line) {
                in_pem_block = true;
            }
            continue;
        }

        let (step1, assign_redacted) = redact_sensitive_assignment_line(line);
        if assign_redacted {
            stats.secrets += 1;
        }
        let (step2, inline_redacted) = redact_inline_tokens_in_line(&step1);
        stats.secrets += inline_redacted;
        let step3 = if redact_ips {
            let (with_ips, ip_count) = redact_ipv4_addresses(&step2);
            stats.ips += ip_count;
            with_ips
        } else {
            step2
        };
        out_lines.push(step3);
    }

    (out_lines.join("\n"), stats)
}

fn is_pem_begin_line(line: &str) -> bool {
    line.contains("-----BEGIN ")
        && (line.contains("PRIVATE KEY-----") || line.contains("PGP PRIVATE KEY BLOCK-----"))
}

fn is_pem_end_line(line: &str) -> bool {
    line.contains("-----END ")
        && (line.contains("PRIVATE KEY-----") || line.contains("PGP PRIVATE KEY BLOCK-----"))
}

/// Redacts values in `.env`, YAML, TOML, or JSON lines whose key name is sensitive
/// (e.g., `API_KEY=...`, `"client_secret": "..."`, `password: ...`).
fn redact_sensitive_assignment_line(line: &str) -> (String, bool) {
    let trimmed = line.trim_start();
    if trimmed.is_empty() || trimmed.starts_with('#') || trimmed.starts_with("//") {
        return (line.to_string(), false);
    }

    let leading_len = line.len() - trimmed.len();
    let (export_prefix, body) = trimmed
        .strip_prefix("export ")
        .map_or(("", trimmed), |rest| ("export ", rest));

    let sep_pos = find_assignment_separator(body);
    let Some((sep_idx, sep_char)) = sep_pos else {
        return (line.to_string(), false);
    };

    let raw_key = body[..sep_idx].trim();
    let key_unquoted = raw_key.trim_matches(|c| c == '"' || c == '\'');
    let key_upper = key_unquoted.to_ascii_uppercase();
    if key_upper.is_empty()
        || !key_upper
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
    {
        return (line.to_string(), false);
    }

    let normalized_key = key_upper.replace('-', "_");
    if !crate::secret::is_sensitive_key_name(&normalized_key) {
        return (line.to_string(), false);
    }

    let after_sep = &body[sep_idx + 1..];
    let ws_after_sep = after_sep.len() - after_sep.trim_start().len();
    let val_region = &after_sep[ws_after_sep..];

    let (value_part, trailing_suffix) = split_assignment_value_and_suffix(val_region);
    let (quote_char, inner_val) = unwrap_matching_quotes(value_part);
    let inner_trimmed = inner_val.trim();

    if inner_trimmed.len() < 6
        || crate::secret::is_placeholder_value(inner_trimmed)
        || inner_trimmed.starts_with("[REDACTED")
    {
        return (line.to_string(), false);
    }

    let mut rebuilt = String::with_capacity(line.len());
    rebuilt.push_str(&line[..leading_len]);
    rebuilt.push_str(export_prefix);
    rebuilt.push_str(&body[..sep_idx]);
    rebuilt.push(sep_char);
    rebuilt.push_str(&after_sep[..ws_after_sep]);
    if let Some(q) = quote_char {
        rebuilt.push(q);
        rebuilt.push_str("[REDACTED]");
        rebuilt.push(q);
    } else {
        rebuilt.push_str("[REDACTED]");
    }
    rebuilt.push_str(trailing_suffix);
    (rebuilt, true)
}

fn find_assignment_separator(body: &str) -> Option<(usize, char)> {
    let mut in_quote: Option<char> = None;
    for (idx, ch) in body.char_indices() {
        if let Some(q) = in_quote {
            if ch == q {
                in_quote = None;
            }
            continue;
        }
        match ch {
            '"' | '\'' => in_quote = Some(ch),
            '=' | ':' => return Some((idx, ch)),
            _ => {}
        }
    }
    None
}

fn split_assignment_value_and_suffix(val_region: &str) -> (&str, &str) {
    let trimmed_end = val_region.trim_end();
    let trailing_ws_start = trimmed_end.len();
    trimmed_end
        .strip_suffix(',')
        .or_else(|| trimmed_end.strip_suffix(';'))
        .map_or_else(
            || (trimmed_end, &val_region[trailing_ws_start..]),
            |without_punct| (without_punct, &val_region[without_punct.len()..]),
        )
}

fn unwrap_matching_quotes(s: &str) -> (Option<char>, &str) {
    if s.len() >= 2 {
        if let Some(inner) = s.strip_prefix('"').and_then(|x| x.strip_suffix('"')) {
            return (Some('"'), inner);
        }
        if let Some(inner) = s.strip_prefix('\'').and_then(|x| x.strip_suffix('\'')) {
            return (Some('\''), inner);
        }
    }
    (None, s)
}

/// Redacts inline Bearer tokens, URL credentials, and known token/API-key formats while
/// preserving exact whitespace and surrounding delimiters.
fn redact_inline_tokens_in_line(line: &str) -> (String, usize) {
    let mut out = String::with_capacity(line.len());
    let mut count = 0usize;
    let mut prev_was_bearer = false;
    let mut rest = line;

    while !rest.is_empty() {
        let ws_len = rest
            .char_indices()
            .take_while(|&(_, c)| c.is_whitespace())
            .last()
            .map_or(0, |(idx, c)| idx + c.len_utf8());
        if ws_len > 0 {
            out.push_str(&rest[..ws_len]);
            rest = &rest[ws_len..];
            continue;
        }

        let tok_len = rest
            .char_indices()
            .take_while(|&(_, c)| !c.is_whitespace())
            .last()
            .map_or(0, |(idx, c)| idx + c.len_utf8());
        let token = &rest[..tok_len];
        rest = &rest[tok_len..];

        let (lead, cleaned, trail) = split_token_wrappers(token);
        if prev_was_bearer
            && cleaned.len() >= 16
            && !crate::secret::is_placeholder_value(cleaned)
            && !cleaned.starts_with("[REDACTED")
        {
            out.push_str(lead);
            out.push_str("[REDACTED_TOKEN]");
            out.push_str(trail);
            count += 1;
            prev_was_bearer = false;
            continue;
        }

        prev_was_bearer = cleaned.eq_ignore_ascii_case("bearer");

        if let Some(redacted) = redact_single_token_payload(cleaned) {
            out.push_str(lead);
            out.push_str(&redacted);
            out.push_str(trail);
            count += 1;
        } else {
            out.push_str(token);
        }
    }

    (out, count)
}

fn split_token_wrappers(token: &str) -> (&str, &str, &str) {
    let is_wrapper = |c: char| {
        matches!(
            c,
            '"' | '\'' | '`' | ',' | ';' | '(' | ')' | '[' | ']' | '{' | '}' | '<' | '>'
        )
    };
    let start = token
        .char_indices()
        .find(|&(_, c)| !is_wrapper(c))
        .map_or(token.len(), |(idx, _)| idx);
    let end = token
        .char_indices()
        .rev()
        .find(|&(_, c)| !is_wrapper(c))
        .map_or(start, |(idx, c)| idx + c.len_utf8());
    if start >= end {
        (token, "", "")
    } else {
        (&token[..start], &token[start..end], &token[end..])
    }
}

fn redact_single_token_payload(cleaned: &str) -> Option<String> {
    if cleaned.is_empty() || cleaned.starts_with("[REDACTED") {
        return None;
    }
    if let Some(redacted_url) = redact_url_credentials(cleaned) {
        return Some(redacted_url);
    }
    if crate::secret::is_known_token_format(cleaned) {
        return Some("[REDACTED_SECRET]".to_string());
    }

    // Handle inline key=value or key:value tokens like `--token=ghp_...` or `"key":"sk-proj-..."`
    let sep_idx = cleaned.find('=').or_else(|| cleaned.find(':'))?;
    let (lhs, rhs_with_sep) = cleaned.split_at(sep_idx);
    let sep = &rhs_with_sep[..1];
    let rhs = &rhs_with_sep[1..];
    let (r_lead, r_clean, r_trail) = split_token_wrappers(rhs);
    if let Some(redacted_url) = redact_url_credentials(r_clean) {
        return Some(format!("{lhs}{sep}{r_lead}{redacted_url}{r_trail}"));
    }
    if crate::secret::is_known_token_format(r_clean) {
        return Some(format!("{lhs}{sep}{r_lead}[REDACTED_SECRET]{r_trail}"));
    }
    None
}

/// Redacts the password portion of `scheme://user:password@host...` URLs.
fn redact_url_credentials(token: &str) -> Option<String> {
    let scheme_end = token.find("://")?;
    let after_scheme_idx = scheme_end + 3;
    let after_scheme = &token[after_scheme_idx..];
    let authority_len = after_scheme.find('/').unwrap_or(after_scheme.len());
    let authority = &after_scheme[..authority_len];
    let at_pos = authority.find('@')?;
    let userinfo = &authority[..at_pos];
    let host = &authority[at_pos + 1..];
    if host.is_empty() {
        return None;
    }
    let colon_pos = userinfo.find(':')?;
    let password = &userinfo[colon_pos + 1..];
    if password.is_empty() || password.starts_with("[REDACTED") {
        return None;
    }

    let user_end = after_scheme_idx + colon_pos + 1;
    let host_start = after_scheme_idx + at_pos;
    Some(format!(
        "{}[REDACTED]{}",
        &token[..user_end],
        &token[host_start..]
    ))
}

/// Redacts non-loopback IPv4 addresses in `line` to `[REDACTED_IP]`.
fn redact_ipv4_addresses(line: &str) -> (String, usize) {
    let bytes = line.as_bytes();
    let len = bytes.len();
    let mut out = String::with_capacity(len);
    let mut count = 0usize;
    let mut i = 0;

    while i < len {
        if bytes[i].is_ascii_digit() {
            let left_ok = i == 0 || (!bytes[i - 1].is_ascii_alphanumeric() && bytes[i - 1] != b'.');
            if left_ok {
                if let Some((ip_end, is_loopback)) = parse_ipv4_at(bytes, i) {
                    let right_ok = ip_end == len
                        || (!bytes[ip_end].is_ascii_alphanumeric() && bytes[ip_end] != b'.');
                    if right_ok && !is_loopback {
                        out.push_str("[REDACTED_IP]");
                        count += 1;
                        i = ip_end;
                        continue;
                    }
                }
            }
        }

        let ch = line[i..].chars().next().unwrap_or('\0');
        out.push(ch);
        i += ch.len_utf8();
    }

    (out, count)
}

/// Parses an IPv4 dotted-quad starting at `start`, returning `(end_index, is_loopback_or_zero)`.
fn parse_ipv4_at(bytes: &[u8], start: usize) -> Option<(usize, bool)> {
    let mut idx = start;
    let mut octets = [0u8; 4];

    for (part, slot) in octets.iter_mut().enumerate() {
        if part > 0 {
            if idx >= bytes.len() || bytes[idx] != b'.' {
                return None;
            }
            idx += 1;
        }
        let digit_start = idx;
        let mut val: u16 = 0;
        while idx < bytes.len() && bytes[idx].is_ascii_digit() && idx - digit_start < 4 {
            val = val * 10 + u16::from(bytes[idx] - b'0');
            idx += 1;
        }
        let digit_len = idx - digit_start;
        if digit_len == 0 || digit_len > 3 || val > 255 {
            return None;
        }
        if digit_len > 1 && bytes[digit_start] == b'0' {
            return None;
        }
        *slot = u8::try_from(val).ok()?;
    }

    let is_local = octets == [127, 0, 0, 1] || octets == [0, 0, 0, 0];
    Some((idx, is_local))
}

/// Collapses runs of 3+ identical consecutive non-empty lines into one line plus a repeat count,
/// and collapses consecutive blank lines into a single blank line.
fn collapse_repeated_lines_counted(input: &str) -> (String, usize) {
    let mut out: Vec<String> = Vec::new();
    let mut prev_line: Option<&str> = None;
    let mut repeat_count: usize = 0;
    let mut collapsed_duplicates: usize = 0;
    let mut prev_was_blank = false;

    let flush =
        |out: &mut Vec<String>, prev: &mut Option<&str>, count: &mut usize, saved: &mut usize| {
            if let Some(line) = prev.take() {
                if *count >= 3 {
                    out.push(line.to_string());
                    out.push(format!("... (repeated {}x)", *count));
                    *saved += *count - 1;
                } else {
                    for _ in 0..*count {
                        out.push(line.to_string());
                    }
                }
                *count = 0;
            }
        };

    for raw_line in input.lines() {
        let line = raw_line.trim_end();
        if line.is_empty() {
            flush(
                &mut out,
                &mut prev_line,
                &mut repeat_count,
                &mut collapsed_duplicates,
            );
            if !prev_was_blank && !out.is_empty() {
                out.push(String::new());
                prev_was_blank = true;
            }
            continue;
        }

        prev_was_blank = false;
        if prev_line == Some(line) {
            repeat_count += 1;
        } else {
            flush(
                &mut out,
                &mut prev_line,
                &mut repeat_count,
                &mut collapsed_duplicates,
            );
            prev_line = Some(line);
            repeat_count = 1;
        }
    }

    flush(
        &mut out,
        &mut prev_line,
        &mut repeat_count,
        &mut collapsed_duplicates,
    );
    while out.last().is_some_and(String::is_empty) {
        out.pop();
    }

    (out.join("\n"), collapsed_duplicates)
}

/// Checks whether `text` looks like a multi-row TSV, CSV, or semicolon-separated table.
fn is_likely_tabular(text: &str) -> bool {
    parse_tabular_rows(text).is_some()
}

/// Parses TSV, CSV, or semicolon-separated tabular rows (requiring at least 2 rows and 2 columns).
fn parse_tabular_rows(input: &str) -> Option<Vec<Vec<String>>> {
    let normalized = strip_invisible_and_normalize_newlines(input);
    let lines: Vec<&str> = normalized
        .lines()
        .filter(|line| !line.trim().is_empty())
        .collect();
    if lines.len() < 2 {
        return None;
    }

    // 1. TSV: unambiguous tab-delimited rows (e.g., copied from Excel, Sheets, or SQL clients)
    let tab_lines = lines.iter().filter(|l| l.contains('\t')).count();
    if lines[0].contains('\t') && tab_lines >= 2 {
        let rows: Vec<Vec<String>> = lines
            .iter()
            .map(|l| l.split('\t').map(ToString::to_string).collect())
            .collect();
        if rows[0].len() >= 2 {
            return Some(rows);
        }
    }

    // 2. CSV or semicolon-separated with RFC 4180 quoting and consistent column counts
    for delimiter in [',', ';'] {
        if !lines[0].contains(delimiter) {
            continue;
        }
        let rows: Vec<Vec<String>> = lines
            .iter()
            .map(|l| parse_delimited_line(l, delimiter))
            .collect();
        let cols = rows[0].len();
        if cols >= 2 && rows.iter().all(|r| r.len() == cols) {
            return Some(rows);
        }
    }

    None
}

/// Parses a single delimited line (CSV / semicolon) supporting quoted fields and `""` escapes.
fn parse_delimited_line(line: &str, delimiter: char) -> Vec<String> {
    let mut fields = Vec::new();
    let mut current = String::new();
    let mut chars = line.chars().peekable();
    let mut in_quotes = false;

    while let Some(ch) = chars.next() {
        if in_quotes {
            if ch == '"' {
                if chars.peek() == Some(&'"') {
                    chars.next();
                    current.push('"');
                } else {
                    in_quotes = false;
                }
            } else {
                current.push(ch);
            }
        } else if ch == '"' && current.trim().is_empty() {
            current.clear();
            in_quotes = true;
        } else if ch == delimiter {
            fields.push(std::mem::take(&mut current));
        } else {
            current.push(ch);
        }
    }

    fields.push(current);
    fields
}

// ============================================================================
// Shell Path Quoting
// ============================================================================

/// Returns true if every non-empty line in `text` looks like a Unix or Windows file path.
fn is_likely_file_path_list(text: &str) -> bool {
    let lines: Vec<&str> = text
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .collect();
    if lines.is_empty() || lines.len() > 200 {
        return false;
    }
    lines.iter().all(|line| is_likely_single_file_path(line))
}

fn is_likely_single_file_path(raw: &str) -> bool {
    let unquoted = strip_outer_shell_quotes(raw);
    let path = unquoted.as_str();
    if path.len() < 2 || path.contains("://") || path.chars().any(char::is_control) {
        return false;
    }

    // Unix absolute: /foo/bar (exclude //comment or / followed by whitespace)
    if let Some(rest) = path.strip_prefix('/') {
        return rest
            .chars()
            .next()
            .is_some_and(|c| !c.is_whitespace() && c != '/');
    }

    // Unix home or relative: ~/..., ./..., ../...
    if let Some(rest) = path
        .strip_prefix("~/")
        .or_else(|| path.strip_prefix("./"))
        .or_else(|| path.strip_prefix("../"))
    {
        return rest.chars().next().is_some_and(|c| !c.is_whitespace());
    }

    // Windows drive path: C:\... or C:/...
    let bytes = path.as_bytes();
    if bytes.len() >= 3
        && bytes[0].is_ascii_alphabetic()
        && bytes[1] == b':'
        && (bytes[2] == b'\\' || bytes[2] == b'/')
    {
        return true;
    }

    // Windows UNC path: \\server\share
    if let Some(rest) = path.strip_prefix("\\\\") {
        if let Some((host, share)) = rest.split_once('\\') {
            return !host.is_empty() && !share.is_empty() && !host.chars().any(char::is_whitespace);
        }
    }

    false
}

/// Strips one layer of matching outer quotes (`"..."`, `'...'`, or POSIX `~/'...'`) so quoting is idempotent.
fn strip_outer_shell_quotes(raw: &str) -> String {
    let trimmed = raw.trim();
    if let Some(rest) = trimmed.strip_prefix("~/") {
        if let Some(inner) = rest.strip_prefix('\'').and_then(|s| s.strip_suffix('\'')) {
            return format!("~/{}", inner.replace("'\\''", "'"));
        }
    }
    if trimmed.len() >= 2 {
        if let Some(inner) = trimmed
            .strip_prefix('\'')
            .and_then(|s| s.strip_suffix('\''))
        {
            return inner.replace("'\\''", "'");
        }
        if let Some(inner) = trimmed.strip_prefix('"').and_then(|s| s.strip_suffix('"')) {
            return inner.replace("\"\"", "\"");
        }
    }
    trimmed.to_string()
}

/// Quotes one or more newline-separated file paths for the given shell style.
/// Multiple paths are joined with spaces so pasting into a terminal never executes intermediate lines.
fn quote_paths_for_shell_detailed(
    input: &str,
    style: crate::platform::ShellPathStyle,
) -> TransformOutput {
    let quoted: Vec<String> = input
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .map(|line| {
            let clean = strip_outer_shell_quotes(line);
            quote_single_path(&clean, style)
        })
        .collect();

    let style_label = match style {
        crate::platform::ShellPathStyle::Posix => "POSIX",
        crate::platform::ShellPathStyle::Windows => "Windows",
    };

    let summary = if quoted.len() > 1 {
        Some(format!(
            "{style_label} · {}",
            format_count(quoted.len(), "path", "paths")
        ))
    } else {
        Some(style_label.to_string())
    };

    TransformOutput {
        text: quoted.join(" "),
        summary,
    }
}

fn quote_single_path(path: &str, style: crate::platform::ShellPathStyle) -> String {
    match style {
        crate::platform::ShellPathStyle::Posix => path.strip_prefix("~/").map_or_else(
            || format!("'{}'", path.replace('\'', "'\\''")),
            |rest| format!("~/'{}'", rest.replace('\'', "'\\''")),
        ),
        crate::platform::ShellPathStyle::Windows => {
            format!("\"{}\"", path.replace('"', "\"\""))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_strip_ansi_csi_and_osc() {
        let input = "\x1b[31;1mError:\x1b[0m Failed at step 1\r\n";
        assert_eq!(strip_source_formatting(input), "Error: Failed at step 1");

        let osc_input = "\x1b]0;Terminal Title\x07Hello from terminal";
        assert_eq!(strip_source_formatting(osc_input), "Hello from terminal");

        let osc_st_input = "\x1b]0;Title\x1b\\Command output";
        assert_eq!(strip_source_formatting(osc_st_input), "Command output");
    }

    #[test]
    fn test_strip_invisible_and_zero_width_chars() {
        let input = "Secret\u{200B}Key\u{200C}With\u{200D}BOM\u{FEFF}And\u{FE0E}VS\u{FE0F}";
        assert_eq!(strip_source_formatting(input), "SecretKeyWithBOMAndVS");
    }

    #[test]
    fn test_normalize_nbsp_and_newlines() {
        let input = "Hello\u{00A0}world!\r\nLine\rTwo\r\n\r\n\r\n\r\nLine Three  ";
        assert_eq!(
            strip_source_formatting(input),
            "Hello world!\nLine\nTwo\n\nLine Three"
        );
    }

    #[test]
    fn test_strip_html_tags_and_preserve_blocks() {
        let input = "<h1>Title</h1>\n<p>First paragraph with <a href=\"https://example.com\">link</a> &amp; <b>bold</b>.</p><br/><div>Second line.</div>";
        let expected = "Title\n\nFirst paragraph with link & bold.\n\nSecond line.";
        assert_eq!(strip_source_formatting(input), expected);
    }

    #[test]
    fn test_strip_html_script_and_style_blocks() {
        let input = "<script type=\"text/javascript\">alert('ignore');</script>\n<style>body { color: red; }</style>\n<p>Visible content</p>";
        assert_eq!(strip_source_formatting(input), "Visible content");
    }

    #[test]
    fn test_decode_html_entities() {
        let input = "&quot;Hello &amp; welcome!&quot; &lt;tag&gt; &#39;test&#39; &#x2014;";
        assert_eq!(
            strip_source_formatting(input),
            "\"Hello & welcome!\" <tag> 'test' —"
        );
    }

    #[test]
    fn test_strip_markdown_headings_quotes_lists() {
        let input = "# Heading 1\n## Heading 2 ###\n\n> Quoted block\n\n- Bullet 1\n* Bullet 2\n+ Bullet 3\n1. Numbered 1\n2. Numbered 2";
        let expected = "Heading 1\nHeading 2\n\nQuoted block\n\nBullet 1\nBullet 2\nBullet 3\nNumbered 1\nNumbered 2";
        assert_eq!(strip_source_formatting(input), expected);
    }

    #[test]
    fn test_strip_markdown_inline_elements() {
        let input = "Check out [Google](https://google.com) and ![Logo](https://img.png) with **bold**, *italic*, ~~strike~~, and `code`.";
        let expected = "Check out Google and Logo with bold, italic, strike, and code.";
        assert_eq!(strip_source_formatting(input), expected);
    }

    #[test]
    fn test_strip_markdown_fenced_code_blocks() {
        let input = "```rust\nfn main() {\n    println!(\"Hello\");\n}\n```";
        let expected = "fn main() {\n    println!(\"Hello\");\n}";
        assert_eq!(strip_source_formatting(input), expected);
    }

    #[test]
    fn test_unwrap_lines() {
        let input = "This is a sentence\nthat was broken by PDF.\n\nSecond paragraph here\nwith hyphen-\nated word.";
        let unwrapped = unwrap_lines(input);
        assert_eq!(
            unwrapped,
            "This is a sentence that was broken by PDF.\n\nSecond paragraph here with hyphenated word."
        );
    }

    #[test]
    fn test_collapse_whitespace() {
        let input = "  Hello   world \n\t from \n\n clipboard  ";
        assert_eq!(collapse_whitespace(input), "Hello world from clipboard");
    }

    #[test]
    fn test_case_conversions() {
        let input = "helloWorld_test-case XMLParser";
        assert_eq!(to_snake_case(input), "hello_world_test_case_xml_parser");
        assert_eq!(to_kebab_case(input), "hello-world-test-case-xml-parser");
        assert_eq!(to_camel_case(input), "helloWorldTestCaseXmlParser");
        assert_eq!(to_title_case(input), "Hello World Test Case Xml Parser");
    }

    #[test]
    fn test_line_operations() {
        let input = "banana\nApple\nbanana\n\norange\n";
        assert_eq!(sort_lines_asc(input), "\nApple\nbanana\nbanana\norange");
        assert_eq!(deduplicate_lines(input), "banana\nApple\n\norange");
        assert_eq!(remove_empty_lines(input), "banana\nApple\nbanana\norange");
        assert_eq!(join_with_comma(input), "banana, Apple, banana, orange");
    }

    #[test]
    fn test_json_minify() {
        let pretty = "{\n  \"key\": \"value\",\n  \"num\": 42\n}";
        let minified = json_minify(pretty).unwrap();
        assert_eq!(minified, "{\"key\":\"value\",\"num\":42}");
        assert!(json_minify("not json").is_err());
    }

    #[test]
    fn test_base64_roundtrip() {
        let original = "CipherClipboard encrypted management!";
        let encoded = base64_encode(original.as_bytes());
        assert_eq!(
            encoded,
            "Q2lwaGVyQ2xpcGJvYXJkIGVuY3J5cHRlZCBtYW5hZ2VtZW50IQ=="
        );
        let decoded = base64_decode(&encoded).unwrap();
        assert_eq!(String::from_utf8(decoded).unwrap(), original);

        assert!(base64_decode("invalid===base64").is_err());
    }

    #[test]
    fn test_strip_url_tracking_params() {
        let url = "https://example.com/item?id=99&utm_source=news&fbclid=12345&keep=true";
        let cleaned = strip_url_tracking_params(url).unwrap();
        assert_eq!(cleaned, "https://example.com/item?id=99&keep=true");

        let url_only_tracking = "https://example.com/item?utm_source=twitter&gclid=abcd";
        let cleaned_no_query = strip_url_tracking_params(url_only_tracking).unwrap();
        assert_eq!(cleaned_no_query, "https://example.com/item");
    }

    #[test]
    fn test_text_transform_execution() {
        let json_item = ClipboardItem {
            id: 5,
            kind: ItemKind::Code,
            content: "{\"name\":\"alice\",\"age\":30}".to_string(),
            width: None,
            height: None,
            created_at: "2026-01-01".to_string(),
            embedded_colors: vec![],
            label: None,
            notes: None,
            is_pinned: false,
        };

        assert!(TextTransform::JsonPrettify.applies_to(&json_item));
        let pretty = TextTransform::JsonPrettify
            .apply(&json_item.content)
            .unwrap();
        assert!(pretty.contains('\n'));

        let minified = TextTransform::JsonMinify.apply(&pretty).unwrap();
        assert_eq!(minified, "{\"age\":30,\"name\":\"alice\"}");
    }

    #[test]
    fn test_has_strippable_formatting_detection() {
        let clean_item = ClipboardItem {
            id: 1,
            kind: ItemKind::Text,
            content: "Plain unformatted text".to_string(),
            width: None,
            height: None,
            created_at: "2026-01-01".to_string(),
            embedded_colors: vec![],
            label: None,
            notes: None,
            is_pinned: false,
        };
        assert!(!has_strippable_formatting(&clean_item));

        let ansi_item = ClipboardItem {
            id: 2,
            kind: ItemKind::Text,
            content: "\x1b[32mOK\x1b[0m".to_string(),
            width: None,
            height: None,
            created_at: "2026-01-01".to_string(),
            embedded_colors: vec![],
            label: None,
            notes: None,
            is_pinned: false,
        };
        assert!(has_strippable_formatting(&ansi_item));

        let html_item = ClipboardItem {
            id: 3,
            kind: ItemKind::Text,
            content: "<p>Hello</p>".to_string(),
            width: None,
            height: None,
            created_at: "2026-01-01".to_string(),
            embedded_colors: vec![],
            label: None,
            notes: None,
            is_pinned: false,
        };
        assert!(has_strippable_formatting(&html_item));

        let image_item = ClipboardItem {
            id: 4,
            kind: ItemKind::Image,
            content: String::new(),
            width: Some(100),
            height: Some(100),
            created_at: "2026-01-01".to_string(),
            embedded_colors: vec![],
            label: None,
            notes: None,
            is_pinned: false,
        };
        assert!(!has_strippable_formatting(&image_item));
    }

    #[test]
    fn test_prepare_code_for_ai_dedent_paths_secrets_and_fence() {
        let input = "\n        fn connect() {\n            let cfg = \"/Users/alice/dev/app/.env\";\n            let db = \"postgres://admin:s3cr3tPass!@db.internal:5432/prod\";\n            let key = \"sk-proj-abcdefghijklmnopqrstuvwxyz123456\";\n        }\n    ";
        let result = TextTransform::PrepareCodeForAi.apply(input).unwrap();
        assert!(result.starts_with("```rust\n"));
        assert!(result.ends_with("\n```"));
        assert!(result.contains("fn connect() {\n    let cfg = \"<HOME>/dev/app/.env\";"));
        assert!(result.contains("postgres://admin:[REDACTED]@db.internal:5432/prod"));
        assert!(result.contains("let key = \"[REDACTED_SECRET]\";"));
        assert!(!result.contains("alice"));
        assert!(!result.contains("s3cr3tPass!"));
        assert!(!result.contains("sk-proj-"));
    }

    #[test]
    fn test_prepare_code_for_ai_expands_fence_for_inner_backticks() {
        let input = "    let doc = \"```json\\n{}\\n```\";";
        let result = TextTransform::PrepareCodeForAi.apply(input).unwrap();
        assert!(result.starts_with("````"));
        assert!(result.ends_with("\n````"));
    }

    #[test]
    fn test_strip_local_paths_cross_platform() {
        assert_eq!(
            strip_local_paths("at /Users/alice/src/main.rs:42"),
            "at <HOME>/src/main.rs:42"
        );
        assert_eq!(
            strip_local_paths("file:///home/bob.smith/project/Cargo.toml"),
            "file://<HOME>/project/Cargo.toml"
        );
        assert_eq!(
            strip_local_paths(r"C:\Users\carol\Documents\app.rs"),
            r"<HOME>\Documents\app.rs"
        );
        assert_eq!(
            strip_local_paths(r#"{"path": "C:\\Users\\carol\\app"}"#),
            r#"{"path": "<HOME>\\app"}"#
        );
    }

    #[test]
    fn test_redact_secrets_pem_env_and_bearer() {
        let input = "\
-----BEGIN RSA PRIVATE KEY-----
MIIEowIBAAKCAQEAtestkeydata
-----END RSA PRIVATE KEY-----
export API_KEY=\"super_secret_api_value_123\"
\"client_secret\": \"my_client_secret_value\",
Authorization: Bearer abcdef1234567890abcdef
API_KEY=<YOUR_KEY_HERE>";

        let redacted = redact_secrets(input, false);
        assert!(redacted.contains("[REDACTED_PRIVATE_KEY]"));
        assert!(!redacted.contains("MIIEowIBAAKCAQEA"));
        assert!(redacted.contains("export API_KEY=\"[REDACTED]\""));
        assert!(redacted.contains("\"client_secret\": \"[REDACTED]\","));
        assert!(redacted.contains("Authorization: Bearer [REDACTED_TOKEN]"));
        assert!(redacted.contains("API_KEY=<YOUR_KEY_HERE>"));
    }

    #[test]
    fn test_prepare_log_for_ai_ansi_collapse_ips_and_wrap() {
        let input = "\x1b[31mERROR\x1b[0m failed connecting to 10.24.8.15:5432 from /home/ubuntu/svc\n\
                     \x1b[31mERROR\x1b[0m failed connecting to 10.24.8.15:5432 from /home/ubuntu/svc\n\
                     \x1b[31mERROR\x1b[0m failed connecting to 10.24.8.15:5432 from /home/ubuntu/svc\n\
                     \x1b[31mERROR\x1b[0m failed connecting to 10.24.8.15:5432 from /home/ubuntu/svc\n\
                     INFO fallback listening on 127.0.0.1:8080\n\
                     INFO fallback listening on 127.0.0.1:8080";

        let result = TextTransform::PrepareLogForAi.apply(input).unwrap();
        let expected = "<log>\n\
                        ERROR failed connecting to [REDACTED_IP]:5432 from <HOME>/svc\n\
                        ... (repeated 4x)\n\
                        INFO fallback listening on 127.0.0.1:8080\n\
                        INFO fallback listening on 127.0.0.1:8080\n\
                        </log>";
        assert_eq!(result, expected);
    }

    #[test]
    fn test_prepare_table_for_ai_tsv_csv_semicolon_and_invalid() {
        let tsv = "Name\tRole\tNote\nAlice\tAdmin\ta | b\nBob\tUser\tok";
        let tab_table = TextTransform::PrepareTableForAi.apply(tsv).unwrap();
        assert_eq!(
            tab_table,
            "| Name | Role | Note |\n| --- | --- | --- |\n| Alice | Admin | a \\| b |\n| Bob | User | ok |"
        );

        let csv = "Name,City,Quote\n\"Smith, Alice\",Paris,\"He said \"\"hi\"\"\"\nBob,Lyon,Hello";
        let comma_table = TextTransform::PrepareTableForAi.apply(csv).unwrap();
        assert_eq!(
            comma_table,
            "| Name | City | Quote |\n| --- | --- | --- |\n| Smith, Alice | Paris | He said \"hi\" |\n| Bob | Lyon | Hello |"
        );

        let semi = "Item;Price\nCoffee;3.50\nTea;2.80";
        let semi_table = TextTransform::PrepareTableForAi.apply(semi).unwrap();
        assert_eq!(
            semi_table,
            "| Item | Price |\n| --- | --- |\n| Coffee | 3.50 |\n| Tea | 2.80 |"
        );

        let prose = "First line, with a comma.\nSecond line without any delimiter.";
        assert_eq!(
            TextTransform::PrepareTableForAi.apply(prose),
            Err(TransformError::InvalidTable)
        );
    }

    #[test]
    fn test_ai_presets_applicability() {
        let make_item = |kind: ItemKind, content: &str| ClipboardItem {
            id: 10,
            kind,
            content: content.to_string(),
            width: None,
            height: None,
            created_at: "2026-01-01".to_string(),
            embedded_colors: vec![],
            label: None,
            notes: None,
            is_pinned: false,
        };

        let single_line = make_item(ItemKind::Text, "hello world");
        assert!(TextTransform::PrepareCodeForAi.applies_to(&single_line));
        assert!(!TextTransform::PrepareLogForAi.applies_to(&single_line));
        assert!(!TextTransform::PrepareTableForAi.applies_to(&single_line));

        let ansi_line = make_item(ItemKind::Text, "\x1b[32mOK\x1b[0m");
        assert!(TextTransform::PrepareLogForAi.applies_to(&ansi_line));

        let tsv_item = make_item(ItemKind::Text, "A\tB\n1\t2");
        assert!(TextTransform::PrepareTableForAi.applies_to(&tsv_item));

        let image_item = make_item(ItemKind::Image, "");
        assert!(!TextTransform::PrepareCodeForAi.applies_to(&image_item));
        assert!(!TextTransform::PrepareLogForAi.applies_to(&image_item));
        assert!(!TextTransform::PrepareTableForAi.applies_to(&image_item));
    }

    #[test]
    fn test_ai_presets_summaries_and_toasts() {
        let code = "    fn demo() {\n        let p = \"/Users/alice/app\";\n        let k = \"sk-proj-abcdefghijklmnopqrstuvwxyz123456\";\n    }";
        let code_out = TextTransform::PrepareCodeForAi
            .apply_with_summary(code)
            .unwrap();
        assert_eq!(
            code_out.summary.as_deref(),
            Some("rust fence · dedented · 1 path stripped · 1 secret redacted")
        );
        let code_toast =
            TextTransform::PrepareCodeForAi.toast_for_output(code_out.summary.as_deref());
        assert!(code_toast.contains("rust fence · dedented · 1 path stripped · 1 secret redacted"));

        let log =
            "\x1b[31mERR\x1b[0m 10.0.0.5\n\x1b[31mERR\x1b[0m 10.0.0.5\n\x1b[31mERR\x1b[0m 10.0.0.5";
        let log_out = TextTransform::PrepareLogForAi
            .apply_with_summary(log)
            .unwrap();
        assert_eq!(
            log_out.summary.as_deref(),
            Some("ANSI stripped · 2 lines collapsed · 3 IPs redacted")
        );

        let tbl_out = TextTransform::PrepareTableForAi
            .apply_with_summary("A\tB\n1\t2\n3\t4")
            .unwrap();
        assert_eq!(tbl_out.summary.as_deref(), Some("3 rows × 2 cols"));
        assert!(TextTransform::PrepareTableForAi
            .toast_for_output(tbl_out.summary.as_deref())
            .contains("3 rows × 2 cols"));
    }

    #[test]
    fn test_format_preview_snippet_bounds_lines_and_width() {
        let text = "short\n123456789012345\nline3\nline4";
        let preview = format_preview_snippet(text, 2, 10);
        assert_eq!(preview, "short\n123456789…\n… (+2 more lines)");
    }

    #[test]
    fn test_quote_path_for_shell_posix_and_windows() {
        use crate::platform::ShellPathStyle;

        let posix_path = "/Users/alice/My Docs/Q4 (Final) O'Reilly.pdf";
        let posix_out = quote_paths_for_shell_detailed(posix_path, ShellPathStyle::Posix);
        assert_eq!(
            posix_out.text,
            "'/Users/alice/My Docs/Q4 (Final) O'\\''Reilly.pdf'"
        );
        // Idempotent when applied to an already-quoted POSIX path
        assert_eq!(
            quote_paths_for_shell_detailed(&posix_out.text, ShellPathStyle::Posix).text,
            posix_out.text
        );

        // Preserves tilde expansion on POSIX ~/ paths
        let tilde_out =
            quote_paths_for_shell_detailed("~/My Projects/app.rs", ShellPathStyle::Posix);
        assert_eq!(tilde_out.text, "~/'My Projects/app.rs'");
        assert_eq!(
            quote_paths_for_shell_detailed(&tilde_out.text, ShellPathStyle::Posix).text,
            "~/'My Projects/app.rs'"
        );

        // Windows double-quoting and Explorer already-quoted idempotency
        let win_path = r"C:\Program Files (x86)\My App\config.toml";
        let win_out = quote_paths_for_shell_detailed(win_path, ShellPathStyle::Windows);
        assert_eq!(
            win_out.text,
            r#""C:\Program Files (x86)\My App\config.toml""#
        );
        assert_eq!(
            quote_paths_for_shell_detailed(&win_out.text, ShellPathStyle::Windows).text,
            win_out.text
        );

        // Multi-line paths joined safely with spaces
        let multi = "/tmp/first file.txt\n/tmp/second file.txt";
        let multi_out = quote_paths_for_shell_detailed(multi, ShellPathStyle::Posix);
        assert_eq!(
            multi_out.text,
            "'/tmp/first file.txt' '/tmp/second file.txt'"
        );
        assert_eq!(multi_out.summary.as_deref(), Some("POSIX · 2 paths"));
    }

    #[test]
    fn test_quote_path_for_shell_applicability() {
        let make_item = |content: &str| ClipboardItem {
            id: 20,
            kind: ItemKind::Text,
            content: content.to_string(),
            width: None,
            height: None,
            created_at: "2026-01-01".to_string(),
            embedded_colors: vec![],
            label: None,
            notes: None,
            is_pinned: false,
        };

        assert!(TextTransform::QuotePathForShell.applies_to(&make_item("/Users/alice/my file.txt")));
        assert!(TextTransform::QuotePathForShell.applies_to(&make_item("~/Documents/notes.md")));
        assert!(TextTransform::QuotePathForShell.applies_to(&make_item(r"C:\Program Files\App")));
        assert!(
            TextTransform::QuotePathForShell.applies_to(&make_item(r#""C:\Program Files\App""#))
        );
        assert!(
            TextTransform::QuotePathForShell.applies_to(&make_item(r"\\fileserver\shared\doc.pdf"))
        );

        assert!(!TextTransform::QuotePathForShell.applies_to(&make_item("https://example.com/a/b")));
        assert!(
            !TextTransform::QuotePathForShell.applies_to(&make_item("plain sentence with spaces"))
        );
        assert!(!TextTransform::QuotePathForShell.applies_to(&make_item("// a C++ comment")));
    }
}
