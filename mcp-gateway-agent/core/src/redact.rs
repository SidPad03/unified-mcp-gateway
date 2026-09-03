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
            // A labelled credential, in every shape one actually arrives in:
            // an escaped quote as the separator (JSON nested inside a JSON
            // string), a scheme word before the value ("Basic YWRt..."), and
            // Base64 '=' padding at the end. See the server's redactor, which
            // carries the same pattern and the longer note.
                Regex::new(
                    r#"(?i)(api[_-]?key|apikey|token|secret|password|passwd|authorization|auth)(?:\\?["'])?\s*[=:]\s*(?:\\?["'])?(?:(?:Bearer|Basic|Token)\s+)?[A-Za-z0-9\-._~+/]{8,}={0,2}(?:\\?["'])?"#,
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
mod cases {
    /// The payload shapes a real gateway actually sees, kept in one list so the
    /// server's redactor and the agent's cannot drift. A miss on either side is a
    /// credential in a database backup or on a Logs page with a Copy button.
    pub(crate) const MUST_REDACT: &[&str] = &[
        r#"{"password":"hunter2secret"}"#,
        // An MCP tool whose argument or result is itself JSON — the escaped quote
        // is what the old pattern could not cross.
        r#"{"args":"{\"password\": \"hunter2secret\"}"}"#,
        r#"{"args":"{\"api_key\": \"sk-proj-AAAABBBBCCCC\"}"}"#,
        r#"{"args":"{\"authorization\": \"Bearer eyJ0eXAiOiJKV1QifQ.p.s\"}"}"#,
        "Authorization: Bearer eyJhbGciOiJIUzI1NiJ9.abc.def",
        r#"{"h":"Bearer eyJhbGciOiJIUzI1NiJ9.abc.def"}"#,
        // A scheme word before the value, and Base64 padding after it.
        r#"{"Authorization":"Basic YWRtaW46aHVudGVyMg=="}"#,
        "api_key=sk-abc123def456",
        r#"{"client_secret":"abcdefgh12345678"}"#,
        "mcpgw_deadbeefcafe1234",
        r#"{"args":"{\"key\":\"mcpgw_deadbeefcafe1234\"}"}"#,
    ];

    /// Lines that must come through untouched. Over-redaction is a real cost on the
    /// Logs page, where the alternative to a readable line is no line.
    pub(crate) const MUST_NOT_REDACT: &[&str] = &[
        "Local stdio backend started backend=blender tool_count=17",
        "Spawning stdio backend command=npx arg_count=3",
        "tools_discovered=42 backend=filesystem",
    ];
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The same list the server's redactor is held to. Two redactors that
    /// disagree teach a user the wrong thing about where their secrets go.
    #[test]
    fn every_credential_shape_is_redacted() {
        for case in super::cases::MUST_REDACT {
            assert_ne!(redact(case), *case, "nothing was redacted in: {case}");
        }
    }

    #[test]
    fn an_ordinary_log_line_survives() {
        for case in super::cases::MUST_NOT_REDACT {
            assert_eq!(redact(case), *case, "over-redacted: {case}");
        }
    }

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
