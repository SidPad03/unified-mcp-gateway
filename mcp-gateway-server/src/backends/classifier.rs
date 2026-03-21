/// Keyword-based risk classification for discovered MCP tools.
///
/// Categories:
///   "read"        – read-only / informational
///   "write"       – creates or modifies resources
///   "admin"       – settings, secrets, permissions
///   "destructive" – deletes, drops, truncates
///   "execute"     – runs workflows, dispatches actions
///   "unclassified"– none of the above matched

const READ_KEYWORDS: &[&str] = &[
    "get_", "list_", "search_", "find_", "fetch_", "show_",
    "view_", "read_", "describe_", "inspect_", "check_",
    "count_", "preview_", "download_", "diff_", "compare_",
    "health_", "version", "info", "status", "log_preview",
    "revisions", "history", "documentation", "validate",
];

const WRITE_KEYWORDS: &[&str] = &[
    "create_", "add_", "update_", "edit_", "modify_", "set_",
    "put_", "patch_", "upsert_", "replace_", "rename_",
    "upload_", "write_", "save_", "submit_", "fork_",
    "merge_", "push_", "start_", "stop_", "track",
    "comment",
];

const DESTRUCTIVE_KEYWORDS: &[&str] = &[
    "delete_", "remove_", "drop_", "destroy_", "purge_",
    "truncate_", "clear_", "revoke_", "dismiss_", "cancel_",
    "prune_", "wipe_",
];

const ADMIN_KEYWORDS: &[&str] = &[
    "secret", "variable", "action_variable", "action_secret",
    "permission", "role", "config", "setting", "credential",
    "token", "key", "policy",
];

const EXECUTE_KEYWORDS: &[&str] = &[
    "run_", "exec_", "execute_", "dispatch_", "trigger_",
    "deploy_", "rerun_", "autofix_", "test_workflow",
];

pub fn classify_tool(tool_name: &str, description: &str) -> &'static str {
    let name_lower = tool_name.to_lowercase();
    let desc_lower = description.to_lowercase();

    // Check destructive first (highest risk)
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

    // Admin / secrets / settings
    if matches_any(&name_lower, ADMIN_KEYWORDS) && matches_any(&name_lower, WRITE_KEYWORDS) {
        return "admin";
    }
    if matches_any(&name_lower, ADMIN_KEYWORDS) && matches_any(&name_lower, DESTRUCTIVE_KEYWORDS) {
        return "admin";
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
        assert_eq!(classify_tool("get_my_user_info", "Get my user info"), "read");
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
        assert_eq!(classify_tool("delete_branch", "Delete branch"), "destructive");
        assert_eq!(classify_tool("clear_issue_labels", "Removes all labels"), "destructive");
        assert_eq!(classify_tool("delete_wiki_page", "Delete a wiki page"), "destructive");
    }

    #[test]
    fn test_admin_tools() {
        assert_eq!(classify_tool("create_repo_action_variable", "Create a repository Actions variable"), "admin");
        assert_eq!(classify_tool("upsert_org_action_secret", "Create or update secret"), "admin");
        assert_eq!(classify_tool("delete_org_action_secret", "Delete secret"), "admin");
    }

    #[test]
    fn test_execute_tools() {
        assert_eq!(classify_tool("dispatch_repo_action_workflow", "Trigger a workflow"), "execute");
        assert_eq!(classify_tool("rerun_repo_action_run", "Rerun a run"), "execute");
    }
}
