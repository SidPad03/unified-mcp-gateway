use std::collections::HashMap;

/// Tool router that maps namespaced tool names to backend destinations
pub struct ToolRouter {
    routes: HashMap<String, String>, // tool_name -> backend_name
}

impl ToolRouter {
    pub fn new() -> Self {
        Self {
            routes: HashMap::new(),
        }
    }

    pub fn register(&mut self, tool_name: &str, backend_name: &str) {
        self.routes.insert(tool_name.to_string(), backend_name.to_string());
    }

    pub fn resolve(&self, tool_name: &str) -> Option<&String> {
        self.routes.get(tool_name)
    }

    pub fn namespace_tool(backend_name: &str, tool_name: &str) -> String {
        format!("{}.{}", backend_name, tool_name)
    }
}
