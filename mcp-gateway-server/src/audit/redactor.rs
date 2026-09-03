use regex::Regex;

pub struct Redactor {
    patterns: Vec<(Regex, String)>,
}

impl Redactor {
    pub fn new() -> Self {
        let patterns = vec![
            // API keys and tokens
            (Regex::new(r"Bearer\s+[A-Za-z0-9\-._~+/]+=*").unwrap(), "[REDACTED_BEARER_TOKEN]".into()),
            // A labelled credential, in every shape one actually arrives in.
            //
            // Three things the first version missed, all of them ordinary. A
            // payload that is itself JSON inside a JSON string writes the
            // separator as \" — an escaped quote, not a quote — so
            // {"args":"{\"api_key\": \"sk-...\"}"} went through
            // untouched, and a tool that takes or returns JSON is the common
            // case, not the exotic one. A value can carry a scheme word first
            // ("Basic YWRt..."), and the space broke the match. And Base64
            // padding is '=', which the value class did not include.
            (Regex::new(r#"(?i)(api[_-]?key|apikey|token|secret|password|passwd|authorization|auth)(?:\\?["'])?\s*[=:]\s*(?:\\?["'])?(?:(?:Bearer|Basic|Token)\s+)?[A-Za-z0-9\-._~+/]{8,}={0,2}(?:\\?["'])?"#).unwrap(), "[REDACTED_CREDENTIAL]".into()),
            // Bare gateway API keys (mcpgw_ prefix) appearing anywhere in a payload,
            // including serialized JSON tool arguments/responses.
            (Regex::new(r"mcpgw_[A-Za-z0-9]{12,}").unwrap(), "[REDACTED_API_KEY]".into()),
            // Email addresses
            (Regex::new(r"[a-zA-Z0-9._%+-]+@[a-zA-Z0-9.-]+\.[a-zA-Z]{2,}").unwrap(), "[REDACTED_EMAIL]".into()),
            // SSN-like patterns
            (Regex::new(r"\b\d{3}-\d{2}-\d{4}\b").unwrap(), "[REDACTED_SSN]".into()),
            // Phone numbers
            (Regex::new(r"\b\d{3}[-.]?\d{3}[-.]?\d{4}\b").unwrap(), "[REDACTED_PHONE]".into()),
        ];
        Self { patterns }
    }

    pub fn redact(&self, input: &str) -> String {
        let mut result = input.to_string();
        for (pattern, replacement) in &self.patterns {
            result = pattern
                .replace_all(&result, replacement.as_str())
                .to_string();
        }
        result
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

    /// The secret in each of these must not survive.
    #[test]
    fn every_credential_shape_is_redacted() {
        let r = Redactor::new();
        for case in super::cases::MUST_REDACT {
            let out = r.redact(case);
            assert_ne!(out, *case, "nothing was redacted in: {case}");
        }
    }

    #[test]
    fn an_ordinary_log_line_survives() {
        let r = Redactor::new();
        for case in super::cases::MUST_NOT_REDACT {
            assert_eq!(r.redact(case), *case, "over-redacted: {case}");
        }
    }

    #[test]
    fn redacts_bare_gateway_key_in_json() {
        let out = Redactor::new().redact(r#"{"arg":"mcpgw_abcdef0123456789abcdef"}"#);
        assert!(
            !out.contains("mcpgw_abcdef"),
            "bare mcpgw_ key leaked: {out}"
        );
        assert!(out.contains("[REDACTED_API_KEY]"));
    }

    #[test]
    fn redacts_quoted_json_credential_field() {
        let out = Redactor::new().redact(r#"{"password":"hunter2secret"}"#);
        assert!(
            !out.contains("hunter2secret"),
            "json password leaked: {out}"
        );
        assert!(out.contains("[REDACTED_CREDENTIAL]"));
    }
}
