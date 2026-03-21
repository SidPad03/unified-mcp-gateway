use serde::{Deserialize, Serialize};
use sqlx::PgPool;
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PolicyRule {
    pub policy_id: String,
    pub name: String,
    pub priority: i32,
    pub tool_pattern: String,
    pub decision: PolicyDecision,
    pub reason: Option<String>,
    pub risk_categories: Vec<String>,
    pub application_match: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum PolicyDecision {
    Allow,
    Deny,
}

impl std::fmt::Display for PolicyDecision {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            PolicyDecision::Allow => write!(f, "allow"),
            PolicyDecision::Deny => write!(f, "deny"),
        }
    }
}

pub struct PolicyEngine {
    rules: Vec<PolicyRule>,
    default_decision: PolicyDecision,
}

impl PolicyEngine {
    pub fn new(rules: Vec<PolicyRule>, default_decision: PolicyDecision) -> Self {
        let mut rules = rules;
        rules.sort_by_key(|r| r.priority);
        Self { rules, default_decision }
    }

    pub fn default_decision(&self) -> &PolicyDecision {
        &self.default_decision
    }

    /// Evaluate policies for a given tool_name and its risk_category.
    /// A rule matches when tool_pattern matches AND (risk_categories is empty
    /// OR the tool's risk is in the rule's risk_categories list).
    /// First matching rule wins. Falls back to the role's default_policy.
    pub fn evaluate(
        &self,
        tool_name: &str,
        tool_risk: &str,
        application: Option<&str>,
    ) -> (PolicyDecision, Option<String>, Option<String>) {
        for rule in &self.rules {
            let pattern_matches = matches_glob(&rule.tool_pattern, tool_name);
            let risk_matches = rule.risk_categories.is_empty()
                || rule.risk_categories.iter().any(|r| r == tool_risk);
            let app_matches = match (&rule.application_match, application) {
                (None, _) => true,
                (Some(pat), _) if pat.is_empty() => true,
                (Some(pat), Some(app)) => matches_glob(pat, app),
                (Some(_), None) => true,
            };

            if pattern_matches && risk_matches && app_matches {
                return (
                    rule.decision.clone(),
                    Some(rule.policy_id.clone()),
                    rule.reason.clone(),
                );
            }
        }
        let reason = match self.default_decision {
            PolicyDecision::Allow => "No matching policy — role default allow",
            PolicyDecision::Deny => "No matching policy — role default deny",
        };
        (self.default_decision.clone(), None, Some(reason.into()))
    }

    /// Load policies for a set of role names via the role_policies join table.
    /// Also resolves the effective default_policy: if any role is "allow", default is Allow.
    pub async fn for_roles(pool: &PgPool, role_names: &[String]) -> Result<Self, sqlx::Error> {
        if role_names.is_empty() {
            return Ok(Self::new(vec![], PolicyDecision::Deny));
        }

        // Resolve the most permissive default_policy across all roles
        let default_rows: Vec<(String,)> = sqlx::query_as(
            "SELECT default_policy FROM roles WHERE name = ANY($1)"
        )
        .bind(role_names)
        .fetch_all(pool)
        .await?;

        let default_decision = if default_rows.iter().any(|(dp,)| dp == "allow") {
            PolicyDecision::Allow
        } else {
            PolicyDecision::Deny
        };

        let rows: Vec<(Uuid, String, i32, String, String, Option<String>, Option<Vec<String>>, Option<String>)> = sqlx::query_as(
            "SELECT DISTINCT p.policy_id, p.name, p.priority, p.tool_pattern, p.decision, p.reason, p.risk_categories, p.application_match
             FROM policies p
             JOIN role_policies rp ON rp.policy_id = p.policy_id
             JOIN roles r ON r.role_id = rp.role_id
             WHERE r.name = ANY($1) AND p.is_active = TRUE
             ORDER BY p.priority ASC"
        )
        .bind(role_names)
        .fetch_all(pool)
        .await?;

        let rules: Vec<PolicyRule> = rows
            .into_iter()
            .map(|(id, name, priority, tool_pattern, decision_str, reason, risk_cats, application_match)| {
                let decision = match decision_str.as_str() {
                    "deny" => PolicyDecision::Deny,
                    _ => PolicyDecision::Allow,
                };
                PolicyRule {
                    policy_id: id.to_string(),
                    name,
                    priority,
                    tool_pattern,
                    decision,
                    reason,
                    risk_categories: risk_cats.unwrap_or_default(),
                    application_match,
                }
            })
            .collect();

        Ok(Self::new(rules, default_decision))
    }
}

pub fn matches_glob(pattern: &str, value: &str) -> bool {
    pattern.split(',').any(|p| matches_single_glob(p.trim(), value))
}

fn matches_single_glob(pattern: &str, value: &str) -> bool {
    if pattern == "*" {
        return true;
    }
    if !pattern.contains('*') {
        return pattern == value;
    }

    let parts: Vec<&str> = pattern.split('*').collect();

    // Check that value starts with the first segment and ends with the last
    if !value.starts_with(parts[0]) || !value.ends_with(parts[parts.len() - 1]) {
        return false;
    }

    // Walk through all segments in order, ensuring each appears in sequence
    let mut remaining = &value[parts[0].len()..];
    for segment in &parts[1..] {
        if segment.is_empty() {
            continue;
        }
        match remaining.find(segment) {
            Some(pos) => remaining = &remaining[pos + segment.len()..],
            None => return false,
        }
    }
    true
}
