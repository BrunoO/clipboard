//! Pure functional text transformation engine for `CipherClipboard`.
//!
//! Provides deterministic, allocation-bounded string transformations including source formatting
//! stripping (ANSI, HTML, Markdown, invisible characters, CRLF/NBSP normalization), case conversions,
//! and structured data formatting.
//!
//! All transformations are 100% safe Rust and adhere to `unsafe_code = "deny"`.

use crate::db::{ClipboardItem, ItemKind};

/// Categories for organizing transformations in menus and palettes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[allow(dead_code)]
pub(crate) enum TransformCategory {
    Cleanup,
    Developer,
}

/// Errors that can occur during text transformation.
#[derive(Debug, Clone, PartialEq, Eq)]
#[allow(dead_code)]
pub(crate) enum TransformError {
    InvalidJson,
    ExecutionFailed(String),
}

impl std::fmt::Display for TransformError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidJson => write!(f, "Invalid JSON payload"),
            Self::ExecutionFailed(msg) => write!(f, "{msg}"),
        }
    }
}

impl std::error::Error for TransformError {}

/// Unified descriptor for pure text transformations.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[allow(dead_code)]
pub(crate) enum TextTransform {
    StripFormatting,
    JsonPrettify,
}

#[allow(dead_code)]
impl TextTransform {
    /// Unique machine identifier for this transform.
    #[must_use]
    pub(crate) const fn id(self) -> &'static str {
        match self {
            Self::StripFormatting => "strip_formatting",
            Self::JsonPrettify => "json_prettify",
        }
    }

    /// User-facing label for this transform.
    #[must_use]
    pub(crate) const fn label(self) -> &'static str {
        match self {
            Self::StripFormatting => "Strip Formatting",
            Self::JsonPrettify => "Prettify JSON",
        }
    }

    /// Category for organizing in menus.
    #[must_use]
    pub(crate) const fn category(self) -> TransformCategory {
        match self {
            Self::StripFormatting => TransformCategory::Cleanup,
            Self::JsonPrettify => TransformCategory::Developer,
        }
    }

    /// User-facing tooltip explaining what this transform does.
    #[must_use]
    pub(crate) const fn tooltip(self) -> &'static str {
        match self {
            Self::StripFormatting => {
                "Strip HTML tags, Markdown syntax, ANSI escape codes, and zero-width characters"
            }
            Self::JsonPrettify => "Format JSON with clean 2-space indentation",
        }
    }

    /// Checks if this transform is applicable to the given clipboard item.
    #[must_use]
    pub(crate) fn applies_to(self, item: &ClipboardItem) -> bool {
        match self {
            Self::StripFormatting => has_strippable_formatting(item),
            Self::JsonPrettify => item.code_language() == Some(crate::code::CodeLanguage::Json),
        }
    }

    /// Executes the transformation on the provided input text.
    pub(crate) fn apply(self, input: &str) -> Result<String, TransformError> {
        match self {
            Self::StripFormatting => Ok(strip_source_formatting(input)),
            Self::JsonPrettify => {
                crate::code::format_json(input).ok_or(TransformError::InvalidJson)
            }
        }
    }
}

/// Strips all source formatting from text, returning clean plain text.
///
/// Execution pipeline:
/// 1. ANSI escape code stripping (CSI and OSC sequences).
/// 2. Invisible character removal (`\u{200B}`, `\u{200C}`, `\u{200D}`, `\u{FEFF}`, `\u{FE0E}`, `\u{FE0F}`).
/// 3. Normalization of `\u{00A0}` (NBSP) to ASCII space and `\r\n`/`\r` to `\n`.
/// 4. HTML/XML stripping: removes `<script>` and `<style>` blocks entirely, preserves line breaks for block tags, strips all other tags, and decodes HTML entities.
/// 5. Markdown stripping: strips heading prefixes (`# `), blockquote prefixes (`> `), list markers (`- `, `* `, `1. `), unwraps inline/fenced code and emphasis (`**`, `__`, `*`, `_`, `~~`), and extracts link labels `[label](url)` and image alt text `![alt](url)`.
/// 6. Whitespace normalization: trims line trailing whitespace, collapses 3+ consecutive newlines to 2, and trims outer leading/trailing whitespace.
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

    // Fast check for triggers to avoid redundant allocations on clean plain text.
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

/// Strips ANSI CSI and OSC escape sequences from `input`.
fn strip_ansi(input: &str) -> String {
    if !input.contains('\x1b') {
        return input.to_string();
    }

    let mut out = String::with_capacity(input.len());
    let mut chars = input.chars().peekable();

    while let Some(ch) = chars.next() {
        if ch == '\x1b' {
            match chars.peek().copied() {
                Some('[') => {
                    chars.next(); // Consume '['
                                  // Consume parameter and intermediate characters (0x20..=0x3F)
                    while let Some(&c) = chars.peek() {
                        if (0x20..=0x3F).contains(&(c as u32)) {
                            chars.next();
                        } else {
                            break;
                        }
                    }
                    // Consume final character (0x40..=0x7E)
                    if let Some(&c) = chars.peek() {
                        if (0x40..=0x7E).contains(&(c as u32)) {
                            chars.next();
                        }
                    }
                }
                Some(']') => {
                    chars.next(); // Consume ']'
                                  // Consume until BEL (0x07) or ST (\x1b\)
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
                Some(c) if c.is_ascii() => {
                    // Consume single character escape (e.g. ESC N, ESC M)
                    chars.next();
                }
                _ => {}
            }
        } else {
            out.push(ch);
        }
    }

    out
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
        if chars[i] == '<' {
            let remaining = &chars[i..];
            if starts_with_tag_ignore_case(remaining, "script") {
                i += 7;
                while i < len {
                    if chars[i] == '<' && starts_with_tag_ignore_case(&chars[i..], "/script") {
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
                continue;
            } else if starts_with_tag_ignore_case(remaining, "style") {
                i += 6;
                while i < len {
                    if chars[i] == '<' && starts_with_tag_ignore_case(&chars[i..], "/style") {
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
                continue;
            }

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

            let clean_tag = tag_content.trim();
            let tag_name = clean_tag
                .trim_start_matches('/')
                .split_whitespace()
                .next()
                .unwrap_or("")
                .to_ascii_lowercase();

            if is_block_tag(&tag_name) {
                out.push('\n');
            }
        } else {
            out.push(chars[i]);
            i += 1;
        }
    }

    out
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

/// Decodes common HTML entities (named and numeric) to their plain character equivalents.
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

        // Check for fenced code block toggle
        if trimmed_start.starts_with("```") || trimmed_start.starts_with("~~~") {
            in_fenced_code = !in_fenced_code;
            continue;
        }

        if in_fenced_code {
            // Preserve code block contents verbatim
            out_lines.push(raw_line.to_string());
            continue;
        }

        let mut line = raw_line.to_string();

        // Strip ATX headings: # Heading
        if let Some(rest) = strip_heading_prefix(&line) {
            line = rest;
        }

        // Strip blockquote prefixes: > Quote
        if let Some(rest) = strip_blockquote_prefix(&line) {
            line = rest;
        }

        // Strip bullet / number list prefixes: - Item, * Item, 1. Item
        if let Some(rest) = strip_list_prefix(&line) {
            line = rest;
        }

        // Strip inline markdown elements: links, images, emphasis, inline code
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
    // Bullet lists: - , * , +
    if let Some(rest) = trimmed
        .strip_prefix("- ")
        .or_else(|| trimmed.strip_prefix("* "))
        .or_else(|| trimmed.strip_prefix("+ "))
    {
        return Some(rest.to_string());
    }

    // Numbered lists: 1. , 12.
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
    // 1. Strip images: ![alt](url) -> alt
    let s1 = strip_images(line);
    // 2. Strip links: [label](url) -> label
    let s2 = strip_links(&s1);
    // 3. Strip inline code: `code` -> code
    let s3 = strip_inline_code(&s2);
    // 4. Strip emphasis: **bold**, __bold__, *italic*, _italic_, ~~strike~~
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
        let matches_prefix = if is_image {
            i + 1 < len && chars[i] == '!' && chars[i + 1] == '['
        } else {
            chars[i] == '[' && (i == 0 || chars[i - 1] != '!')
        };

        if matches_prefix {
            let label_start = if is_image { i + 2 } else { i + 1 };
            let mut label_end = label_start;
            while label_end < len && chars[label_end] != ']' {
                label_end += 1;
            }

            if label_end < len
                && label_end + 1 < len
                && chars[label_end] == ']'
                && chars[label_end + 1] == '('
            {
                let url_start = label_end + 2;
                let mut url_end = url_start;
                while url_end < len && chars[url_end] != ')' {
                    url_end += 1;
                }

                if url_end < len && chars[url_end] == ')' {
                    let label: String = chars[label_start..label_end].iter().collect();
                    out.push_str(&label);
                    i = url_end + 1;
                    continue;
                }
            }
        }

        out.push(chars[i]);
        i += 1;
    }

    out
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
    // Strikethrough: ~~text~~
    s = replace_delimiter_pair(&s, "~~");
    // Bold: **text** and __text__
    s = replace_delimiter_pair(&s, "**");
    s = replace_delimiter_pair(&s, "__");
    // Italic: *text* and _text_
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
        if chars[i] == delim {
            // Delimiter must be followed by a non-whitespace character to be valid markdown opening
            if i + 1 < len && !chars[i + 1].is_whitespace() && chars[i + 1] != delim {
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
    fn test_has_strippable_formatting_detection() {
        let clean_item = ClipboardItem {
            id: 1,
            kind: ItemKind::Text,
            content: "Plain unformatted text".to_string(),
            width: None,
            height: None,
            created_at: "2026-01-01".to_string(),
            embedded_colors: vec![],
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
        };
        assert!(!has_strippable_formatting(&image_item));
    }

    #[test]
    fn test_text_transform_applies_and_execution() {
        let json_item = ClipboardItem {
            id: 5,
            kind: ItemKind::Code,
            content: "{\"name\":\"alice\",\"age\":30}".to_string(),
            width: None,
            height: None,
            created_at: "2026-01-01".to_string(),
            embedded_colors: vec![],
        };

        assert!(TextTransform::JsonPrettify.applies_to(&json_item));
        let pretty = TextTransform::JsonPrettify
            .apply(&json_item.content)
            .unwrap();
        assert!(pretty.contains('\n'));

        let formatted_item = ClipboardItem {
            id: 6,
            kind: ItemKind::Text,
            content: "**Bold** and [link](https://foo.com)".to_string(),
            width: None,
            height: None,
            created_at: "2026-01-01".to_string(),
            embedded_colors: vec![],
        };
        assert!(TextTransform::StripFormatting.applies_to(&formatted_item));
        let raw = TextTransform::StripFormatting
            .apply(&formatted_item.content)
            .unwrap();
        assert_eq!(raw, "Bold and link");
    }
}
