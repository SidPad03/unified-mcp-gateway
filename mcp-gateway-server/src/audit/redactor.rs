use regex::Regex;

pub struct Redactor {
    patterns: Vec<(Regex, String)>,
}

impl Redactor {
    pub fn new() -> Self {
        let patterns = vec![
            // API keys and tokens
            (Regex::new(r"Bearer\s+[A-Za-z0-9\-._~+/]+=*").unwrap(), "[REDACTED_BEARER_TOKEN]".into()),
            (Regex::new(r#"(?i)(api[_-]?key|token|secret|password|authorization)["']?\s*[=:]\s*["']?[A-Za-z0-9\-._~+/]{8,}["']?"#).unwrap(), "[REDACTED_CREDENTIAL]".into()),
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
            result = pattern.replace_all(&result, replacement.as_str()).to_string();
        }
        result
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn redacts_bare_gateway_key_in_json() {
        let out = Redactor::new().redact(r#"{"arg":"mcpgw_abcdef0123456789abcdef"}"#);
        assert!(!out.contains("mcpgw_abcdef"), "bare mcpgw_ key leaked: {out}");
        assert!(out.contains("[REDACTED_API_KEY]"));
    }

    #[test]
    fn redacts_quoted_json_credential_field() {
        let out = Redactor::new().redact(r#"{"password":"hunter2secret"}"#);
        assert!(!out.contains("hunter2secret"), "json password leaked: {out}");
        assert!(out.contains("[REDACTED_CREDENTIAL]"));
    }
}
