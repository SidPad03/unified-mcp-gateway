//! Secret redaction for anything the app will display, copy, or export.
//!
//! These are deliberately the *same* rules as
//! `mcp-gateway-server/src/audit/redactor.rs`. Two redactors that disagree are
//! worse than one that is occasionally over-eager: a user who sees a value
//! redacted in the dashboard and printed in the app has learned the wrong thing
//! about where their secrets go.
//!
//! Keeping them in step means the PII patterns come along too, and those can
//! catch an innocent ten-digit number in a log line. That is the accepted cost —
//! the Logs page has a Copy and an Export button, and a leaked credential does
//! not get to be a "well, it was only local".

use regex::Regex;
use std::sync::OnceLock;

fn patterns() -> &'static [(Regex, &'static str)] {
    static PATTERNS: OnceLock<Vec<(Regex, &'static str)>> = OnceLock::new();
    PATTERNS.get_or_init(|| {
        vec![
            (
                Regex::new(r"Bearer\s+[A-Za-z0-9\-._~+/]+=*").unwrap(),
                "[REDACTED_BEARER_TOKEN]",
            ),
            (
                Regex::new(
                    r#"(?i)(api[_-]?key|token|secret|password|authorization)["']?\s*[=:]\s*["']?[A-Za-z0-9\-._~+/]{8,}["']?"#,
                )
                .unwrap(),
                "[REDACTED_CREDENTIAL]",
            ),
            // Bare gateway API keys, wherever they turn up.
            (
                Regex::new(r"mcpgw_[A-Za-z0-9]{12,}").unwrap(),
                "[REDACTED_API_KEY]",
            ),
            (
                Regex::new(r"[a-zA-Z0-9._%+-]+@[a-zA-Z0-9.-]+\.[a-zA-Z]{2,}").unwrap(),
                "[REDACTED_EMAIL]",
            ),
            (Regex::new(r"\b\d{3}-\d{2}-\d{4}\b").unwrap(), "[REDACTED_SSN]"),
            (
                Regex::new(r"\b\d{3}[-.]?\d{3}[-.]?\d{4}\b").unwrap(),
                "[REDACTED_PHONE]",
            ),
        ]
    })
}

pub fn redact(input: &str) -> String {
    let mut result = std::borrow::Cow::Borrowed(input);
    for (pattern, replacement) in patterns() {
        if pattern.is_match(&result) {
            result =
                std::borrow::Cow::Owned(pattern.replace_all(&result, *replacement).into_owned());
        }
    }
    result.into_owned()
}

/// Mask a credential for display: first four characters, then dots. Used by
/// Settings, which shows that a key exists without showing the key.
pub fn mask(secret: &str) -> String {
    let visible: String = secret.chars().take(4).collect();
    if secret.is_empty() {
        String::new()
    } else {
        format!("{visible}••••••••••••••••")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn matches_the_servers_rules() {
        assert!(!redact("Authorization: Bearer abc123def456").contains("abc123def456"));
        assert!(!redact(r#"{"password":"hunter2secret"}"#).contains("hunter2secret"));
        assert!(!redact("key=mcpgw_abcdef0123456789abcdef").contains("mcpgw_abcdef"));
        assert!(!redact("mailed sid@example.com").contains("sid@example.com"));
    }

    #[test]
    fn leaves_ordinary_log_lines_alone() {
        let line = "Local stdio backend started backend=blender tool_count=17";
        assert_eq!(redact(line), line);
    }

    #[test]
    fn masking_shows_a_prefix_and_nothing_else() {
        let masked = mask("mcpgw_abcdefghijklmnop");
        assert!(masked.starts_with("mcpg"));
        assert!(!masked.contains("abcdefghijklmnop"));
        assert_eq!(mask(""), "");
    }
}
