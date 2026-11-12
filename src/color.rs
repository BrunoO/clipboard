use eframe::egui::Color32;

/// A parsed RGBA color with conversion helpers for HEX, RGB, and HSL formats.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct ParsedColor {
    pub(crate) r: u8,
    pub(crate) g: u8,
    pub(crate) b: u8,
    pub(crate) a: u8,
}

impl ParsedColor {
    /// Constructs a fully opaque RGB color.
    #[must_use]
    pub(crate) const fn rgb(r: u8, g: u8, b: u8) -> Self {
        Self { r, g, b, a: 255 }
    }

    /// Constructs an RGBA color.
    #[must_use]
    pub(crate) const fn rgba(r: u8, g: u8, b: u8, a: u8) -> Self {
        Self { r, g, b, a }
    }

    /// Converts to `egui::Color32`.
    #[must_use]
    pub(crate) fn to_color32(self) -> Color32 {
        Color32::from_rgba_unmultiplied(self.r, self.g, self.b, self.a)
    }

    /// Returns perceived luminance (0.0 to 1.0) to determine contrast.
    #[must_use]
    #[allow(dead_code, clippy::suboptimal_flops)]
    pub(crate) fn luminance(self) -> f32 {
        let r = f32::from(self.r) / 255.0;
        let g = f32::from(self.g) / 255.0;
        let b = f32::from(self.b) / 255.0;
        0.299 * r + 0.587 * g + 0.114 * b
    }

    /// Returns `true` if text drawn over this color should be dark for readability.
    #[must_use]
    #[allow(dead_code)]
    pub(crate) fn is_light(self) -> bool {
        self.luminance() > 0.55
    }

    /// Formats as standard uppercase HEX string (e.g. `#FF5733` or `#FF5733CC`).
    #[must_use]
    pub(crate) fn to_hex(self) -> String {
        if self.a == 255 {
            format!("#{:02X}{:02X}{:02X}", self.r, self.g, self.b)
        } else {
            format!("#{:02X}{:02X}{:02X}{:02X}", self.r, self.g, self.b, self.a)
        }
    }

    /// Formats as standard CSS RGB/RGBA string (e.g. `rgb(255, 87, 51)` or `rgba(255, 87, 51, 0.80)`).
    #[must_use]
    pub(crate) fn to_rgb_string(self) -> String {
        if self.a == 255 {
            format!("rgb({}, {}, {})", self.r, self.g, self.b)
        } else {
            let alpha = f32::from(self.a) / 255.0;
            format!("rgba({}, {}, {}, {:.2})", self.r, self.g, self.b, alpha)
        }
    }

    /// Formats as standard CSS HSL/HSLA string (e.g. `hsl(14, 100%, 60%)`).
    #[must_use]
    pub(crate) fn to_hsl_string(self) -> String {
        let (h, s, l) = self.to_hsl();
        if self.a == 255 {
            format!("hsl({h}, {s}%, {l}%)")
        } else {
            let alpha = f32::from(self.a) / 255.0;
            format!("hsla({h}, {s}%, {l}%, {alpha:.2})")
        }
    }

    /// Calculates `(hue [0..360], saturation [0..100], lightness [0..100])`.
    #[must_use]
    #[allow(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        clippy::many_single_char_names
    )]
    pub(crate) fn to_hsl(self) -> (u16, u8, u8) {
        let r = f32::from(self.r) / 255.0;
        let g = f32::from(self.g) / 255.0;
        let b = f32::from(self.b) / 255.0;

        let max = r.max(g).max(b);
        let min = r.min(g).min(b);
        let delta = max - min;

        let l = f32::midpoint(max, min);

        if delta.abs() < 1e-5 {
            return (0, 0, (l * 100.0).round() as u8);
        }

        let s = if l > 0.5 {
            delta / (2.0 - max - min)
        } else {
            delta / (max + min)
        };

        let mut h = if (max - r).abs() < 1e-5 {
            let seg = (g - b) / delta;
            if g < b {
                seg + 6.0
            } else {
                seg
            }
        } else if (max - g).abs() < 1e-5 {
            ((b - r) / delta) + 2.0
        } else {
            ((r - g) / delta) + 4.0
        };

        h *= 60.0;
        let h_rounded = (h.round() as i32).rem_euclid(360) as u16;
        let s_rounded = (s * 100.0).round().clamp(0.0, 100.0) as u8;
        let l_rounded = (l * 100.0).round().clamp(0.0, 100.0) as u8;

        (h_rounded, s_rounded, l_rounded)
    }
}

/// Attempts to parse a text string as a recognized color representation.
/// Supports HEX (`#RGB`, `#RGBA`, `#RRGGBB`, `#RRGGBBAA`, `0x...`),
/// CSS `rgb(...)`, `rgba(...)`, `hsl(...)`, and `hsla(...)`.
#[must_use]
pub(crate) fn parse_color(input: &str) -> Option<ParsedColor> {
    let trimmed = input.trim();
    if trimmed.is_empty() || trimmed.len() > 60 {
        return None;
    }

    // 1. HEX color parsing: #RGB, #RGBA, #RRGGBB, #RRGGBBAA, 0xRRGGBB
    if let Some(c) = parse_hex_color(trimmed) {
        return Some(c);
    }

    // 2. CSS rgb(...) or rgba(...)
    let lower = trimmed.to_ascii_lowercase();
    if lower.starts_with("rgb(") || lower.starts_with("rgba(") {
        if let Some(c) = parse_rgb_function(&lower) {
            return Some(c);
        }
    }

    // 3. CSS hsl(...) or hsla(...)
    if lower.starts_with("hsl(") || lower.starts_with("hsla(") {
        if let Some(c) = parse_hsl_function(&lower) {
            return Some(c);
        }
    }

    None
}

fn strip_hex_prefix(input: &str) -> &str {
    input
        .strip_prefix('#')
        .or_else(|| input.strip_prefix("0x"))
        .or_else(|| input.strip_prefix("0X"))
        .unwrap_or(input)
}

/// Returns true if the string represents an 8-character hex color (e.g. `#792A2467` or `792A2467`).
#[must_use]
pub(crate) fn is_eight_digit_hex(input: &str) -> bool {
    let hex = strip_hex_prefix(input.trim());
    hex.len() == 8 && hex.chars().all(|c| c.is_ascii_hexdigit())
}

/// Parses an 8-digit hex string under the ARGB convention (Android/Flutter order: #AARRGGBB).
#[must_use]
pub(crate) fn parse_argb_hex(input: &str) -> Option<ParsedColor> {
    let hex = strip_hex_prefix(input.trim());

    if hex.len() != 8 || !hex.chars().all(|c| c.is_ascii_hexdigit()) {
        return None;
    }

    let a = u8::from_str_radix(&hex[0..2], 16).ok()?;
    let r = u8::from_str_radix(&hex[2..4], 16).ok()?;
    let g = u8::from_str_radix(&hex[4..6], 16).ok()?;
    let b = u8::from_str_radix(&hex[6..8], 16).ok()?;
    Some(ParsedColor::rgba(r, g, b, a))
}

fn parse_hex_color(text: &str) -> Option<ParsedColor> {
    let hex_part = if let Some(stripped) = text.strip_prefix('#') {
        stripped
    } else if let Some(stripped) = text.strip_prefix("0x").or_else(|| text.strip_prefix("0X")) {
        stripped
    } else {
        // Only accept bare hex if 6 or 8 chars and contains at least one digit to avoid normal words like "decade"
        if (text.len() == 6 || text.len() == 8)
            && text.chars().all(|c| c.is_ascii_hexdigit())
            && text.chars().any(|c| c.is_ascii_digit())
        {
            text
        } else {
            return None;
        }
    };

    if !hex_part.chars().all(|c| c.is_ascii_hexdigit()) {
        return None;
    }

    match hex_part.len() {
        3 => {
            // #RGB -> #RRGGBB
            let r = u8::from_str_radix(&hex_part[0..1], 16).ok()?;
            let g = u8::from_str_radix(&hex_part[1..2], 16).ok()?;
            let b = u8::from_str_radix(&hex_part[2..3], 16).ok()?;
            Some(ParsedColor::rgb(r * 17, g * 17, b * 17))
        }
        4 => {
            // #RGBA -> #RRGGBBAA
            let r = u8::from_str_radix(&hex_part[0..1], 16).ok()?;
            let g = u8::from_str_radix(&hex_part[1..2], 16).ok()?;
            let b = u8::from_str_radix(&hex_part[2..3], 16).ok()?;
            let a = u8::from_str_radix(&hex_part[3..4], 16).ok()?;
            Some(ParsedColor::rgba(r * 17, g * 17, b * 17, a * 17))
        }
        6 => {
            // #RRGGBB
            let r = u8::from_str_radix(&hex_part[0..2], 16).ok()?;
            let g = u8::from_str_radix(&hex_part[2..4], 16).ok()?;
            let b = u8::from_str_radix(&hex_part[4..6], 16).ok()?;
            Some(ParsedColor::rgb(r, g, b))
        }
        8 => {
            // #RRGGBBAA
            let r = u8::from_str_radix(&hex_part[0..2], 16).ok()?;
            let g = u8::from_str_radix(&hex_part[2..4], 16).ok()?;
            let b = u8::from_str_radix(&hex_part[4..6], 16).ok()?;
            let a = u8::from_str_radix(&hex_part[6..8], 16).ok()?;
            Some(ParsedColor::rgba(r, g, b, a))
        }
        _ => None,
    }
}

fn parse_rgb_function(text: &str) -> Option<ParsedColor> {
    let start = text.find('(')?;
    let end = text.rfind(')')?;
    if end <= start {
        return None;
    }

    let inner = &text[start + 1..end];
    // Normalize commas, slashes, and spaces
    let normalized = inner.replace([',', '/'], " ");
    let parts: Vec<&str> = normalized.split_whitespace().collect();

    if parts.len() < 3 || parts.len() > 4 {
        return None;
    }

    let r = parse_rgb_component(parts[0])?;
    let g = parse_rgb_component(parts[1])?;
    let b = parse_rgb_component(parts[2])?;
    let a = if parts.len() == 4 {
        parse_alpha_component(parts[3])?
    } else {
        255
    };

    Some(ParsedColor::rgba(r, g, b, a))
}

fn parse_rgb_component(part: &str) -> Option<u8> {
    if let Some(percent_str) = part.strip_suffix('%') {
        let pct: f32 = percent_str.parse().ok()?;
        if (0.0..=100.0).contains(&pct) {
            #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
            return Some((pct * 2.55).round().clamp(0.0, 255.0) as u8);
        }
        return None;
    }
    let val: f32 = part.parse().ok()?;
    if (0.0..=255.0).contains(&val) {
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        return Some(val.round() as u8);
    }
    None
}

fn parse_alpha_component(part: &str) -> Option<u8> {
    if let Some(percent_str) = part.strip_suffix('%') {
        let pct: f32 = percent_str.parse().ok()?;
        if (0.0..=100.0).contains(&pct) {
            #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
            return Some((pct * 2.55).round().clamp(0.0, 255.0) as u8);
        }
        return None;
    }
    let val: f32 = part.parse().ok()?;
    if (0.0..=1.0).contains(&val) {
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        return Some((val * 255.0).round().clamp(0.0, 255.0) as u8);
    }
    if (0.0..=255.0).contains(&val) {
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        return Some(val.round() as u8);
    }
    None
}

#[allow(clippy::many_single_char_names)]
fn parse_hsl_function(text: &str) -> Option<ParsedColor> {
    let start = text.find('(')?;
    let end = text.rfind(')')?;
    if end <= start {
        return None;
    }

    let inner = &text[start + 1..end];
    let normalized = inner.replace([',', '/'], " ");
    let parts: Vec<&str> = normalized.split_whitespace().collect();

    if parts.len() < 3 || parts.len() > 4 {
        return None;
    }

    let h_str = parts[0]
        .strip_suffix("deg")
        .or_else(|| parts[0].strip_suffix("grad"))
        .unwrap_or(parts[0]);
    let h: f32 = h_str.parse().ok()?;
    let h_norm = h.rem_euclid(360.0);

    let s_str = parts[1].strip_suffix('%')?;
    let s: f32 = s_str.parse().ok()?;
    if !(0.0..=100.0).contains(&s) {
        return None;
    }
    let s_norm = s / 100.0;

    let l_str = parts[2].strip_suffix('%')?;
    let l: f32 = l_str.parse().ok()?;
    if !(0.0..=100.0).contains(&l) {
        return None;
    }
    let l_norm = l / 100.0;

    let a = if parts.len() == 4 {
        parse_alpha_component(parts[3])?
    } else {
        255
    };

    let (r, g, b) = hsl_to_rgb(h_norm, s_norm, l_norm);
    Some(ParsedColor::rgba(r, g, b, a))
}

#[allow(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    clippy::many_single_char_names,
    clippy::suboptimal_flops
)]
fn hsl_to_rgb(h: f32, s: f32, l: f32) -> (u8, u8, u8) {
    let c = (1.0 - (2.0 * l - 1.0).abs()) * s;
    let x = c * (1.0 - ((h / 60.0).rem_euclid(2.0) - 1.0).abs());
    let m = l - c / 2.0;

    let (r1, g1, b1) = if h < 60.0 {
        (c, x, 0.0)
    } else if h < 120.0 {
        (x, c, 0.0)
    } else if h < 180.0 {
        (0.0, c, x)
    } else if h < 240.0 {
        (0.0, x, c)
    } else if h < 300.0 {
        (x, 0.0, c)
    } else {
        (c, 0.0, x)
    };

    let r = ((r1 + m) * 255.0).round().clamp(0.0, 255.0) as u8;
    let g = ((g1 + m) * 255.0).round().clamp(0.0, 255.0) as u8;
    let b = ((b1 + m) * 255.0).round().clamp(0.0, 255.0) as u8;

    (r, g, b)
}

fn push_unique_color(colors: &mut Vec<ParsedColor>, color: ParsedColor) {
    if !colors.contains(&color) {
        colors.push(color);
    }
}

fn try_extract_hex(chars: &[char], i: usize) -> Option<(ParsedColor, usize)> {
    if *chars.get(i)? != '#' {
        return None;
    }

    let prev_char = if i > 0 {
        chars.get(i - 1).copied()
    } else {
        None
    };
    let valid_start = prev_char.is_none_or(|c| {
        c.is_ascii_whitespace()
            || matches!(
                c,
                '"' | '\''
                    | '`'
                    | ':'
                    | '='
                    | '('
                    | '['
                    | '{'
                    | ','
                    | ';'
                    | '>'
                    | '<'
                    | '!'
                    | '|'
                    | '&'
            )
    });

    if !valid_start {
        return None;
    }

    let start = i + 1;
    let mut end = start;
    while end < chars.len() && chars[end].is_ascii_hexdigit() {
        end += 1;
    }
    let hex_len = end - start;

    let next_char = chars.get(end).copied();
    let valid_end = next_char.is_none_or(|c| !c.is_ascii_alphanumeric() && c != '_' && c != '-');

    if !valid_end || !matches!(hex_len, 3 | 4 | 6 | 8) {
        return None;
    }

    let hex_slice: String = chars[start..end].iter().collect();

    // Prevent false positives on issue numbers like #123
    if hex_len == 3 && hex_slice.chars().all(|c| c.is_ascii_digit()) {
        let first = hex_slice.chars().next();
        let all_same = hex_slice.chars().all(|c| Some(c) == first);
        if !all_same
            && !prev_char
                .is_some_and(|c| matches!(c, ':' | '=' | '\'' | '"' | '`' | '(' | '{' | '[' | ','))
        {
            return None;
        }
    }

    let candidate = format!("#{hex_slice}");
    let parsed = parse_hex_color(&candidate)?;
    Some((parsed, end))
}

fn try_extract_0x(chars: &[char], i: usize) -> Option<(ParsedColor, usize)> {
    if *chars.get(i)? != '0' {
        return None;
    }
    let next = *chars.get(i + 1)?;
    if next != 'x' && next != 'X' {
        return None;
    }

    let prev_char = if i > 0 {
        chars.get(i - 1).copied()
    } else {
        None
    };
    let valid_start = prev_char.is_none_or(|c| !c.is_ascii_alphanumeric() && c != '_');
    if !valid_start {
        return None;
    }

    let start = i + 2;
    let mut end = start;
    while end < chars.len() && chars[end].is_ascii_hexdigit() {
        end += 1;
    }
    let hex_len = end - start;

    let next_char = chars.get(end).copied();
    let valid_end = next_char.is_none_or(|c| !c.is_ascii_alphanumeric() && c != '_');

    if !valid_end || !matches!(hex_len, 6 | 8) {
        return None;
    }

    let hex_slice: String = chars[start..end].iter().collect();
    let candidate = format!("0x{hex_slice}");
    let parsed = parse_hex_color(&candidate)?;
    Some((parsed, end))
}

fn try_extract_func(chars: &[char], i: usize) -> Option<(ParsedColor, usize)> {
    let remaining: String = chars[i..].iter().take(6).collect();
    let is_rgb = remaining.starts_with("rgb(");
    let is_rgba = remaining.starts_with("rgba(");
    let is_hsl = remaining.starts_with("hsl(");
    let is_hsla = remaining.starts_with("hsla(");

    if !is_rgb && !is_rgba && !is_hsl && !is_hsla {
        return None;
    }

    let prev_char = if i > 0 {
        chars.get(i - 1).copied()
    } else {
        None
    };
    if !prev_char.is_none_or(|c| !c.is_ascii_alphanumeric() && c != '_') {
        return None;
    }

    let prefix_len = if is_rgba || is_hsla { 5 } else { 4 };
    let mut end = i + prefix_len;
    while end < chars.len() && end - i < 40 && chars[end] != ')' {
        end += 1;
    }

    if end >= chars.len() || chars[end] != ')' {
        return None;
    }

    let fn_str: String = chars[i..=end].iter().collect();
    let lower = fn_str.to_ascii_lowercase();

    let parsed = if is_rgb || is_rgba {
        parse_rgb_function(&lower)?
    } else {
        parse_hsl_function(&lower)?
    };

    Some((parsed, end + 1))
}

/// Extracts distinct colors embedded within arbitrary text or code snippets.
/// Supports `#RGB`, `#RGBA`, `#RRGGBB`, `#RRGGBBAA`, `0xRRGGBB`, `0xRRGGBBAA`,
/// and CSS functional notations `rgb(...)`, `rgba(...)`, `hsl(...)`, `hsla(...)`.
#[must_use]
pub(crate) fn extract_colors(text: &str) -> Vec<ParsedColor> {
    let mut colors = Vec::new();
    let chars: Vec<char> = text.chars().collect();
    let len = chars.len();
    let mut i = 0;

    while i < len {
        if let Some((color, next_idx)) = try_extract_hex(&chars, i) {
            push_unique_color(&mut colors, color);
            i = next_idx;
        } else if let Some((color, next_idx)) = try_extract_0x(&chars, i) {
            push_unique_color(&mut colors, color);
            i = next_idx;
        } else if let Some((color, next_idx)) = try_extract_func(&chars, i) {
            push_unique_color(&mut colors, color);
            i = next_idx;
        } else {
            i += 1;
        }

        if colors.len() >= 8 {
            break;
        }
    }

    colors
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_hex() {
        assert_eq!(parse_color("#FF5733"), Some(ParsedColor::rgb(255, 87, 51)));
        assert_eq!(parse_color("#ff5733"), Some(ParsedColor::rgb(255, 87, 51)));
        assert_eq!(parse_color("#F00"), Some(ParsedColor::rgb(255, 0, 0)));
        assert_eq!(
            parse_color("#F00A"),
            Some(ParsedColor::rgba(255, 0, 0, 170))
        );
        assert_eq!(
            parse_color("#12345678"),
            Some(ParsedColor::rgba(0x12, 0x34, 0x56, 0x78))
        );
        assert_eq!(
            parse_color("0x2ECC71"),
            Some(ParsedColor::rgb(46, 204, 113))
        );
        assert_eq!(parse_color("2ECC71"), Some(ParsedColor::rgb(46, 204, 113)));
    }

    #[test]
    fn test_parse_rgb() {
        assert_eq!(
            parse_color("rgb(255, 87, 51)"),
            Some(ParsedColor::rgb(255, 87, 51))
        );
        assert_eq!(
            parse_color("rgba(255, 87, 51, 0.5)"),
            Some(ParsedColor::rgba(255, 87, 51, 128))
        );
        assert_eq!(
            parse_color("rgb(100% 50% 0%)"),
            Some(ParsedColor::rgb(255, 128, 0))
        );
        assert_eq!(
            parse_color("rgba(255 87 51 / 0.8)"),
            Some(ParsedColor::rgba(255, 87, 51, 204))
        );
    }

    #[test]
    fn test_parse_hsl() {
        let color = parse_color("hsl(14, 100%, 60%)").expect("Valid HSL");
        assert_eq!(color.r, 255);
        assert_eq!(color.g, 99);
        assert_eq!(color.b, 51);

        let hsla = parse_color("hsla(0, 100%, 50%, 0.5)").expect("Valid HSLA");
        assert_eq!(hsla, ParsedColor::rgba(255, 0, 0, 128));
    }

    #[test]
    fn test_conversions() {
        let c = ParsedColor::rgb(255, 87, 51);
        assert_eq!(c.to_hex(), "#FF5733");
        assert_eq!(c.to_rgb_string(), "rgb(255, 87, 51)");
        let (h, s, l) = c.to_hsl();
        assert_eq!(h, 11);
        assert_eq!(s, 100);
        assert_eq!(l, 60);
    }

    #[test]
    fn test_eight_digit_hex_and_argb() {
        let code = "#792A2467";
        assert!(is_eight_digit_hex(code));

        // Default W3C CSS RGBA interpretation:
        let rgba = parse_color(code).expect("Should parse as RGBA");
        assert_eq!(rgba.r, 0x79);
        assert_eq!(rgba.g, 0x2A);
        assert_eq!(rgba.b, 0x24);
        assert_eq!(rgba.a, 0x67);

        // Android / ARGB interpretation:
        let argb = parse_argb_hex(code).expect("Should parse as ARGB");
        assert_eq!(argb.a, 0x79);
        assert_eq!(argb.r, 0x2A);
        assert_eq!(argb.g, 0x24);
        assert_eq!(argb.b, 0x67);
    }

    #[test]
    fn test_extract_colors_from_css() {
        let css = r"
            button {
                background: #3498db;
                color: #ffffff;
                border: 1px solid rgba(0, 0, 0, 0.15);
                box-shadow: 0 2px 4px #00000022;
            }
        ";
        let colors = extract_colors(css);
        assert_eq!(colors.len(), 4);
        assert_eq!(colors[0], ParsedColor::rgb(52, 152, 219));
        assert_eq!(colors[1], ParsedColor::rgb(255, 255, 255));
        assert_eq!(colors[2], ParsedColor::rgba(0, 0, 0, 38));
        assert_eq!(colors[3], ParsedColor::rgba(0, 0, 0, 34));
    }

    #[test]
    fn test_extract_colors_avoids_false_positives() {
        // Plain text mentioning issue numbers or git commits
        let text =
            "Fixed issue #123 in commit 4f48247 on branch main; visit https://example.com#section";
        let colors = extract_colors(text);
        assert!(
            colors.is_empty(),
            "Should not extract issue numbers or URLs as colors"
        );

        // User's exact prompt snippet
        let prompt_text = r"
            One-Click Format Conversion Buttons:
            [ # HEX ] — copies #RRGGBB or #RRGGBBAA.
            [ RGB ] — copies rgb(r, g, b) or rgba(r, g, b, a).
            [ HSL ] — copies hsl(h, s%, l%) or hsla(h, s%, l%, a).
        ";
        let colors = extract_colors(prompt_text);
        assert!(
            colors.is_empty(),
            "Placeholder docs text should not produce phantom colors"
        );
    }

    #[test]
    fn test_extract_colors_embedded_and_deduplication() {
        let text = "Palette: #792A2467, #792a2467, #000, 0x2ECC71, hsl(14, 100%, 60%)";
        let colors = extract_colors(text);
        assert_eq!(
            colors.len(),
            4,
            "Duplicate #792A2467 should be deduplicated"
        );
        assert_eq!(colors[0], ParsedColor::rgba(0x79, 0x2A, 0x24, 0x67));
        assert_eq!(colors[1], ParsedColor::rgb(0, 0, 0));
        assert_eq!(colors[2], ParsedColor::rgb(46, 204, 113));
        assert_eq!(colors[3].r, 255);
    }
}
