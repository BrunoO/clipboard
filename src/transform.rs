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
            Self::Cleanup => "Cleanup",
            Self::Case => "Case",
            Self::Lines => "Lines",
            Self::Developer => "Developer & Encode",
            Self::Url => "URL & Web",
        }
    }
}

/// Errors that can occur during text transformation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum TransformError {
    InvalidJson,
    InvalidBase64,
    InvalidUtf8,
    InvalidUrl,
    ExecutionFailed(String),
}

impl std::fmt::Display for TransformError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidJson => write!(f, "Invalid JSON payload"),
            Self::InvalidBase64 => write!(f, "Invalid Base64 sequence"),
            Self::InvalidUtf8 => write!(f, "Decoded payload is not valid UTF-8"),
            Self::InvalidUrl => write!(f, "Invalid URL format"),
            Self::ExecutionFailed(msg) => write!(f, "{msg}"),
        }
    }
}

impl std::error::Error for TransformError {}

/// Unified descriptor for pure text transformations.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum TextTransform {
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

    // URL
    StripTrackingParams,
}

impl TextTransform {
    /// Complete catalog of all transformations.
    pub(crate) const ALL: &[Self] = &[
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
        // URL
        Self::StripTrackingParams,
    ];

    /// Machine identifier and user-facing label for this transform.
    const fn id_and_label(self) -> (&'static str, &'static str) {
        match self {
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
            Self::JsonPrettify | Self::JsonMinify | Self::Base64Encode | Self::Base64Decode => {
                TransformCategory::Developer
            }
            Self::StripTrackingParams => TransformCategory::Url,
        }
    }

    /// User-facing tooltip explaining what this transform does.
    #[must_use]
    pub(crate) const fn tooltip(self) -> &'static str {
        match self {
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
            Self::StripTrackingParams => {
                "Remove analytics query parameters (utm_*, fbclid, gclid, etc.) from URL"
            }
        }
    }

    /// Confirmation toast message on successful copy.
    #[must_use]
    pub(crate) fn toast_success(self) -> String {
        match self {
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
            Self::StripFormatting => has_strippable_formatting(item),
            Self::UnwrapLines
            | Self::SortLinesAsc
            | Self::DeduplicateLines
            | Self::RemoveEmptyLines
            | Self::JoinWithComma => text.contains('\n'),
            Self::CollapseWhitespace => text.chars().any(char::is_whitespace),
            Self::Uppercase
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
            Self::StripTrackingParams => has_tracking_params(text),
        }
    }

    /// Executes the transformation on the provided input text.
    pub(crate) fn apply(self, input: &str) -> Result<String, TransformError> {
        match self {
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
            in_vault: false,
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
            in_vault: false,
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
            in_vault: false,
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
            in_vault: false,
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
            in_vault: false,
        };
        assert!(!has_strippable_formatting(&image_item));
    }
}
