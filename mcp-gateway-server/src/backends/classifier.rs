//! Keyword-based risk classification for discovered MCP tools.
//!
//! Categories:
//!   "read"        – read-only / informational
//!   "write"       – creates or modifies resources
//!   "admin"       – settings, secrets, permissions
//!   "destructive" – deletes, drops, truncates
//!   "execute"     – runs workflows, dispatches actions
//!   "unclassified"– none of the above matched

const READ_KEYWORDS: &[&str] = &[
    "get_",
    "list_",
    "search_",
    "find_",
    "fetch_",
    "show_",
    "view_",
    "read_",
    "describe_",
    "inspect_",
    "check_",
    "count_",
    "preview_",
    "download_",
    "diff_",
    "compare_",
    "health_",
    "version",
    "info",
    "status",
    "log_preview",
    "revisions",
    "history",
    "documentation",
    "validate",
];

const WRITE_KEYWORDS: &[&str] = &[
    "create_", "add_", "update_", "edit_", "modify_", "set_", "put_", "patch_", "upsert_",
    "replace_", "rename_", "upload_", "write_", "save_", "submit_", "fork_", "merge_", "push_",
    "start_", "stop_", "track", "comment",
];

const DESTRUCTIVE_KEYWORDS: &[&str] = &[
    "delete_",
    "remove_",
    "drop_",
    "destroy_",
    "purge_",
    "truncate_",
    "clear_",
    "revoke_",
    "dismiss_",
    "cancel_",
    "prune_",
    "wipe_",
];

const ADMIN_KEYWORDS: &[&str] = &[
    "secret",
    "variable",
    "action_variable",
    "action_secret",
    "permission",
    "role",
    "config",
    "setting",
    "credential",
    "token",
    "key",
    "policy",
];

const EXECUTE_KEYWORDS: &[&str] = &[
    "run_",
    "exec_",
    "execute_",
    "dispatch_",
    "trigger_",
    "deploy_",
    "rerun_",
    "autofix_",
    "test_workflow",
];

/// The self-configuration tools an agent registers, and what each one is.
///
/// Keyword matching gets these wrong in ways that matter: "install" is in no
/// list at all, so `agent_install_mcp_server` — which runs a command of the
/// caller's choosing on somebody's Mac — would land as `unclassified` and slip
/// past every policy written against a category. These names are a fixed,
/// known set, so they are stated rather than guessed.
///
/// The gateway's own `gateway_*` tools carry their category in
/// `crate::gateway_tools`, and never reach the classifier: they are not
/// discovered from anywhere. This table is for the agent's mirror of them,
/// which arrives over the wire like any other discovered tool.
const CONTROL_TOOL_RISK: &[(&str, &str)] = &[
    ("agent_list_local_servers", "read"),
    ("agent_get_local_server_status", "read"),
    ("agent_get_local_server_logs", "read"),
    ("agent_install_mcp_server", "admin"),
    ("agent_update_config", "admin"),
    ("agent_remove_mcp_server", "destructive"),
    ("agent_stop_local_server", "destructive"),
    ("agent_start_local_server", "execute"),
    ("agent_restart_local_server", "execute"),
];

pub fn classify_tool(tool_name: &str, description: &str) -> &'static str {
    // Stated beats guessed: checked before any keyword rule.
    if let Some((_, risk)) = CONTROL_TOOL_RISK
        .iter()
        .find(|(name, _)| *name == tool_name)
    {
        return risk;
    }

    let name_lower = tool_name.to_lowercase();
    let desc_lower = description.to_lowercase();

    // Admin / secrets / settings take precedence: managing secrets, tokens, or
    // permissions is a governance concern even when the verb itself is destructive
    // or a write (e.g. `delete_org_action_secret` is admin, not a plain destructive
    // delete). Must be checked before `destructive` so the admin+delete combination
    // classifies as admin rather than short-circuiting to destructive.
    if matches_any(&name_lower, ADMIN_KEYWORDS)
        && (matches_any(&name_lower, WRITE_KEYWORDS)
            || matches_any(&name_lower, DESTRUCTIVE_KEYWORDS))
    {
        return "admin";
    }

    // Destructive (highest risk among the remaining verbs)
    if matches_any(&name_lower, DESTRUCTIVE_KEYWORDS)
        || desc_lower.contains("permanently delete")
        || desc_lower.contains("cannot be undone")
    {
        return "destructive";
    }

    // Execute / dispatch
    if matches_any(&name_lower, EXECUTE_KEYWORDS)
        || desc_lower.contains("trigger")
        || desc_lower.contains("dispatch")
    {
        return "execute";
    }

    // Write / mutate
    if matches_any(&name_lower, WRITE_KEYWORDS) {
        return "write";
    }

    // Read-only
    if matches_any(&name_lower, READ_KEYWORDS) {
        return "read";
    }

    "unclassified"
}

fn matches_any(value: &str, keywords: &[&str]) -> bool {
    keywords.iter().any(|kw| value.contains(kw))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_read_tools() {
        assert_eq!(
            classify_tool("get_my_user_info", "Get my user info"),
            "read"
        );
        assert_eq!(classify_tool("list_branches", "List branches"), "read");
        assert_eq!(classify_tool("search_repos", "search repos"), "read");
    }

    #[test]
    fn test_write_tools() {
        assert_eq!(classify_tool("create_issue", "create issue"), "write");
        assert_eq!(classify_tool("edit_milestone", "edit milestone"), "write");
        assert_eq!(classify_tool("update_file", "Update file"), "write");
        assert_eq!(classify_tool("fork_repo", "Fork repository"), "write");
    }

    #[test]
    fn test_destructive_tools() {
        assert_eq!(
            classify_tool("delete_branch", "Delete branch"),
            "destructive"
        );
        assert_eq!(
            classify_tool("clear_issue_labels", "Removes all labels"),
            "destructive"
        );
        assert_eq!(
            classify_tool("delete_wiki_page", "Delete a wiki page"),
            "destructive"
        );
    }

    #[test]
    fn test_admin_tools() {
        assert_eq!(
            classify_tool(
                "create_repo_action_variable",
                "Create a repository Actions variable"
            ),
            "admin"
        );
        assert_eq!(
            classify_tool("upsert_org_action_secret", "Create or update secret"),
            "admin"
        );
        assert_eq!(
            classify_tool("delete_org_action_secret", "Delete secret"),
            "admin"
        );
    }

    /// The agent's control tools are classified from the table, not from
    /// keywords. Left to the keyword rules, `agent_install_mcp_server` matches
    /// nothing and comes out `unclassified` — a tool that runs an arbitrary
    /// command on a user's Mac, sitting outside every category-scoped policy.
    #[test]
    fn agent_control_tools_are_classified_by_name() {
        assert_eq!(
            classify_tool("agent_install_mcp_server", "Add an MCP server to this Mac"),
            "admin"
        );
        assert_eq!(
            classify_tool("agent_update_config", "Change a server"),
            "admin"
        );
        assert_eq!(
            classify_tool("agent_remove_mcp_server", "Remove a server"),
            "destructive"
        );
        assert_eq!(
            classify_tool("agent_stop_local_server", "Stop a server"),
            "destructive"
        );
        assert_eq!(
            classify_tool("agent_start_local_server", "Start a server"),
            "execute"
        );
        assert_eq!(
            classify_tool("agent_list_local_servers", "List the servers"),
            "read"
        );
    }

    /// The table is an exception, not a prefix rule: a backend that happens to
    /// ship a tool starting with `agent_` is classified like anything else.
    #[test]
    fn the_table_matches_whole_names_only() {
        assert_eq!(
            classify_tool("agent_install_mcp_server_v2", "Something else entirely"),
            "unclassified"
        );
        assert_eq!(classify_tool("get_agent_status", "Read the status"), "read");
    }

    #[test]
    fn test_execute_tools() {
        assert_eq!(
            classify_tool("dispatch_repo_action_workflow", "Trigger a workflow"),
            "execute"
        );
        assert_eq!(
            classify_tool("rerun_repo_action_run", "Rerun a run"),
            "execute"
        );
    }
}
