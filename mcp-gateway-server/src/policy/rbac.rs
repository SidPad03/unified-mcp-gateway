use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Permission {
    pub tool_pattern: String,
    pub actions: Vec<String>,
    pub risk_categories: Vec<String>,
    pub rate_limit: Option<RateLimit>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RateLimit {
    pub max_calls: u32,
    pub window_seconds: u32,
    pub burst: Option<u32>,
}

pub fn check_permission(
    permissions: &[Permission],
    tool_name: &str,
    action: &str,
    risk_category: &str,
) -> bool {
    for perm in permissions {
        if matches_pattern(&perm.tool_pattern, tool_name)
            && perm.actions.contains(&action.to_string())
            && perm.risk_categories.contains(&risk_category.to_string())
        {
            return true;
        }
    }
    false
}

fn matches_pattern(pattern: &str, tool_name: &str) -> bool {
    if pattern == "*" {
        return true;
    }
    if pattern.ends_with(".*") {
        let prefix = &pattern[..pattern.len() - 2];
        return tool_name.starts_with(prefix);
    }
    pattern == tool_name
}
