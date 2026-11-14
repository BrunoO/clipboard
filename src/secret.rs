/// Heuristic and pattern-based detector for secrets, API keys, tokens, and credentials.
///
/// Designed to run on clipboard text before saving to history so sensitive items
/// are automatically classified as `ItemKind::Secret` and masked in the UI.
#[must_use]
pub(crate) fn is_likely_secret(content: &str) -> bool {
    let trimmed = content.trim();
    if trimmed.is_empty() || trimmed.len() > 8192 {
        return false;
    }

    // 1. PEM private keys / PGP private key blocks
    if is_pem_private_key(trimmed) {
        return true;
    }

    // 2. Authorization Bearer tokens
    if is_bearer_token(trimmed) {
        return true;
    }

    // 3. Credentials embedded in URLs (e.g., postgres://user:pass@host)
    if has_url_credentials(trimmed) {
        return true;
    }

    // 4. Known token / API key prefixes & JWTs (single-line tokens without whitespace)
    if !trimmed.chars().any(char::is_whitespace) && is_known_token_format(trimmed) {
        return true;
    }

    // 5. .env / key-value secret assignments (e.g., API_KEY=..., PASSWORD=...)
    if is_secret_env_assignment(trimmed) {
        return true;
    }

    // 6. High-entropy standalone password/secret heuristic
    is_high_entropy_secret(trimmed)
}

fn is_pem_private_key(text: &str) -> bool {
    text.contains("-----BEGIN ")
        && (text.contains("PRIVATE KEY-----") || text.contains("PGP PRIVATE KEY BLOCK-----"))
}

fn is_bearer_token(text: &str) -> bool {
    let Some(rest) = text
        .strip_prefix("Bearer ")
        .or_else(|| text.strip_prefix("bearer "))
    else {
        return false;
    };
    let token = rest.trim();
    token.len() >= 16 && !token.chars().any(char::is_whitespace)
}

fn has_url_credentials(text: &str) -> bool {
    if text.chars().any(char::is_whitespace) {
        return false;
    }
    let Some((_, after_scheme)) = text.split_once("://") else {
        return false;
    };
    let authority = after_scheme.split('/').next().unwrap_or("");
    let Some((userinfo, host)) = authority.split_once('@') else {
        return false;
    };
    if host.is_empty() {
        return false;
    }
    let Some((_, password)) = userinfo.split_once(':') else {
        return false;
    };
    !password.is_empty()
}

fn is_known_token_format(token: &str) -> bool {
    if token.len() < 16 {
        return false;
    }

    // JWT: three non-empty base64url segments starting with eyJ
    if token.starts_with("eyJ") && token.len() >= 30 {
        let parts: Vec<&str> = token.split('.').collect();
        if parts.len() == 3
            && parts.iter().all(|p| {
                !p.is_empty()
                    && p.bytes()
                        .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
            })
        {
            return true;
        }
    }

    // AWS Access Key ID: exact 20 uppercase alphanumeric chars starting with AKIA or ASIA
    if token.len() == 20
        && (token.starts_with("AKIA") || token.starts_with("ASIA"))
        && token
            .bytes()
            .all(|b| b.is_ascii_uppercase() || b.is_ascii_digit())
    {
        return true;
    }

    let prefixes = [
        "sk-proj-",
        "sk-ant-",
        "sk-",
        "ghp_",
        "gho_",
        "ghu_",
        "ghs_",
        "ghr_",
        "github_pat_",
        "glpat-",
        "xoxb-",
        "xoxp-",
        "xoxa-",
        "xoxr-",
        "xoxs-",
        "sk_live_",
        "rk_live_",
        "sk_test_",
        "rk_test_",
        "AIza",
        "hf_",
        "npm_",
        "pypi-",
        "SG.",
    ];

    for prefix in prefixes {
        if let Some(rest) = token.strip_prefix(prefix) {
            if rest.len() >= 12
                && rest
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-' || b == b'.')
            {
                return true;
            }
        }
    }

    false
}

fn is_secret_env_assignment(text: &str) -> bool {
    let lines: Vec<&str> = text.lines().collect();
    if lines.is_empty() || lines.len() > 10 {
        return false;
    }

    for line in lines {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let line = line.strip_prefix("export ").unwrap_or(line).trim();
        let Some((key, raw_val)) = line.split_once('=') else {
            continue;
        };
        let key_upper = key.trim().to_ascii_uppercase();
        if key_upper.is_empty()
            || !key_upper
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'_')
        {
            return false;
        }

        let is_sensitive_key = [
            "PASSWORD",
            "PASSWD",
            "SECRET",
            "API_KEY",
            "APIKEY",
            "ACCESS_KEY",
            "SECRET_KEY",
            "PRIVATE_KEY",
            "AUTH_TOKEN",
            "ACCESS_TOKEN",
            "REFRESH_TOKEN",
            "CLIENT_SECRET",
            "TOKEN",
        ]
        .iter()
        .any(|suffix| key_upper == *suffix || key_upper.ends_with(&format!("_{suffix}")));

        if !is_sensitive_key {
            continue;
        }

        let val = raw_val
            .trim()
            .trim_matches(|c| c == '"' || c == '\'')
            .trim();
        if val.len() >= 6 && !is_placeholder_value(val) {
            return true;
        }
    }

    false
}

fn is_placeholder_value(val: &str) -> bool {
    let lower = val.to_ascii_lowercase();
    matches!(
        lower.as_str(),
        "null" | "none" | "changeme" | "example" | "your_key_here" | "todo"
    ) || (val.starts_with('<') && val.ends_with('>'))
        || val.chars().all(|c| c == '*')
}

fn is_high_entropy_secret(text: &str) -> bool {
    let char_count = text.chars().count();
    if !(12..=128).contains(&char_count) || text.chars().any(char::is_whitespace) {
        return false;
    }

    // Exclude URLs, file paths, colors, and UUIDs
    if crate::db::is_valid_url(text) || crate::color::parse_color(text).is_some() {
        return false;
    }
    if text.starts_with('/')
        || text.starts_with("./")
        || text.starts_with("../")
        || text.starts_with("~/")
        || (text.len() >= 3 && text.as_bytes()[1] == b':' && text.as_bytes()[2] == b'\\')
        || is_uuid(text)
    {
        return false;
    }

    // Let is_secret_env_assignment handle KEY=VALUE strings so placeholders aren't flagged here.
    if let Some((lhs, _)) = text.split_once('=') {
        let lhs = lhs.trim();
        if !lhs.is_empty() && lhs.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_') {
            return false;
        }
    }

    let mut has_upper = false;
    let mut has_lower = false;
    let mut has_digit = false;
    let mut has_symbol = false;

    for ch in text.chars() {
        if ch.is_ascii_uppercase() {
            has_upper = true;
        } else if ch.is_ascii_lowercase() {
            has_lower = true;
        } else if ch.is_ascii_digit() {
            has_digit = true;
        } else if ch.is_ascii_punctuation() {
            has_symbol = true;
        }
    }

    let class_count =
        u8::from(has_upper) + u8::from(has_lower) + u8::from(has_digit) + u8::from(has_symbol);
    let entropy = shannon_entropy(text);

    // Either strong mixed-class password with symbols, or long 3-class token
    (class_count == 4 && entropy >= 3.3)
        || (has_symbol && class_count >= 3 && entropy >= 3.5)
        || (char_count >= 24 && class_count >= 3 && entropy >= 3.8 && !text.contains('/'))
}

fn is_uuid(text: &str) -> bool {
    let parts: Vec<&str> = text.split('-').collect();
    parts.len() == 5
        && parts[0].len() == 8
        && parts[1].len() == 4
        && parts[2].len() == 4
        && parts[3].len() == 4
        && parts[4].len() == 12
        && parts
            .iter()
            .all(|p| p.bytes().all(|b| b.is_ascii_hexdigit()))
}

fn shannon_entropy(text: &str) -> f64 {
    let mut counts = [0_u32; 256];
    let bytes = text.as_bytes();
    if bytes.is_empty() {
        return 0.0;
    }
    for &b in bytes {
        counts[usize::from(b)] += 1;
    }
    #[allow(clippy::cast_precision_loss)]
    let len = bytes.len() as f64;
    let mut entropy = 0.0_f64;
    for count in counts {
        if count > 0 {
            let p = f64::from(count) / len;
            entropy = p.mul_add(-p.log2(), entropy);
        }
    }
    entropy
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_detects_known_tokens_and_keys() {
        let gh = ["ghp", "1234567890abcdef1234567890abcdef1234"].join("_");
        let oai = ["sk", "proj", "abcdefghijklmnopqrstuvwxyz123456"].join("-");
        let ant = ["sk", "ant", "api03", "abcdefghijklmnopqrstuvwxyz"].join("-");
        let aws = ["AKIA", "IOSFODNN7EXAMPLE"].concat();
        let slack = [
            "xoxb",
            "123456789012",
            "123456789012",
            "abcdefghijklmnopqrst",
        ]
        .join("-");

        assert!(is_likely_secret(&gh));
        assert!(is_likely_secret(&oai));
        assert!(is_likely_secret(&ant));
        assert!(is_likely_secret(&aws));
        assert!(is_likely_secret(&slack));
        assert!(is_likely_secret(
            "Bearer eyJhbGciOiJIUzI1NiIsInR5cCI6IkpXVCJ9.abc"
        ));
        assert!(is_likely_secret(
            "eyJhbGciOiJIUzI1NiIsInR5cCI6IkpXVCJ9.eyJzdWIiOiIxMjM0NTY3ODkwIn0.dozjgNryP4J3jVmNHl0w5N_XgL0n3I9PlFUP0THsR8U"
        ));
        assert!(is_likely_secret(
            "-----BEGIN OPENSSH PRIVATE KEY-----\nb3BlbnNzaC1rZXktdjEAAAA...\n-----END OPENSSH PRIVATE KEY-----"
        ));
    }

    #[test]
    fn test_detects_url_credentials_and_env_secrets() {
        assert!(is_likely_secret(
            "postgres://admin:s3cr3tP@ss@db.internal:5432/prod"
        ));
        assert!(is_likely_secret(
            "OPENAI_API_KEY=sk-1234567890abcdef1234567890"
        ));
        assert!(is_likely_secret("export DB_PASSWORD=\"superSecret99!\""));
    }

    #[test]
    fn test_detects_high_entropy_passwords() {
        assert!(is_likely_secret(
            "T#hN}5Hws!d(K:fr0}MfGZ9.Ih4s]c_;Fd&qP|=q^C,<eSTHqk:RV0nsxe[zthvk"
        ));
        assert!(is_likely_secret("K9#mP2$vL8@qR4!x"));
    }

    #[test]
    fn test_avoids_false_positives() {
        assert!(!is_likely_secret("https://github.com/BrunoO/clipboard"));
        assert!(!is_likely_secret("/Users/bruno/Documents/notes.txt"));
        assert!(!is_likely_secret("#ff8800"));
        assert!(!is_likely_secret("550e8400-e29b-41d4-a716-446655440000"));
        assert!(!is_likely_secret("myCamelCaseVariableName123"));
        assert!(!is_likely_secret(
            "Hello, this is a normal sentence copied from an email."
        ));
        assert!(!is_likely_secret("API_KEY=<your-api-key>"));
        assert!(!is_likely_secret("fn main() { println!(\"hello\"); }"));
    }
}
