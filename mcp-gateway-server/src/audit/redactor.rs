use regex::Regex;

pub struct Redactor {
    patterns: Vec<(Regex, String)>,
}

impl Redactor {
    pub fn new() -> Self {
        let patterns = vec![
            // API keys and tokens
            (Regex::new(r"Bearer\s+[A-Za-z0-9\-._~+/]+=*").unwrap(), "[REDACTED_BEARER_TOKEN]".into()),
            (Regex::new(r#"(?i)(api[_-]?key|token|secret|password|authorization)\s*[=:]\s*["']?[A-Za-z0-9\-._~+/]{8,}["']?"#).unwrap(), "[REDACTED_CREDENTIAL]".into()),
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
