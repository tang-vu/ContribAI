//! Tool protocol and registry for extensible tool support.
//!
//! Port from Python `tools/protocol.py`.

use std::collections::HashMap;
use tracing::{error, info};

/// Result from a tool execution.
#[derive(Debug, Clone)]
pub struct ToolResult {
    pub success: bool,
    pub data: Option<String>,
    pub error: Option<String>,
}

impl ToolResult {
    pub fn ok(data: String) -> Self {
        Self { success: true, data: Some(data), error: None }
    }

    pub fn err(error: String) -> Self {
        Self { success: false, data: None, error: Some(error) }
    }
}

/// Protocol for all tools in the system.
#[async_trait::async_trait]
pub trait Tool: Send + Sync {
    fn name(&self) -> &str;
    fn description(&self) -> &str;
    async fn execute(&self, params: HashMap<String, String>) -> ToolResult;
}

/// Registry for managing tools.
pub struct ToolRegistry {
    tools: HashMap<String, Box<dyn Tool>>,
}

impl ToolRegistry {
    pub fn new() -> Self {
        Self { tools: HashMap::new() }
    }

    pub fn register(&mut self, tool: Box<dyn Tool>) {
        info!(name = tool.name(), desc = tool.description(), "Registered tool");
        self.tools.insert(tool.name().to_string(), tool);
    }

    pub fn get(&self, name: &str) -> Option<&dyn Tool> {
        self.tools.get(name).map(|t| t.as_ref())
    }

    pub fn list_tools(&self) -> Vec<ToolInfo> {
        self.tools
            .values()
            .map(|t| ToolInfo {
                name: t.name().to_string(),
                description: t.description().to_string(),
            })
            .collect()
    }

    pub fn has(&self, name: &str) -> bool {
        self.tools.contains_key(name)
    }

    pub async fn execute(&self, name: &str, params: HashMap<String, String>) -> ToolResult {
        match self.tools.get(name) {
            Some(tool) => {
                match tool.execute(params).await {
                    result if result.success => result,
                    result => {
                        error!(tool = name, error = ?result.error, "Tool failed");
                        result
                    }
                }
            }
            None => ToolResult::err(format!("Tool not found: {name}")),
        }
    }
}

impl Default for ToolRegistry {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Debug, Clone)]
pub struct ToolInfo {
    pub name: String,
    pub description: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    struct MockTool;

    #[async_trait::async_trait]
    impl Tool for MockTool {
        fn name(&self) -> &str { "mock" }
        fn description(&self) -> &str { "A mock tool" }
        async fn execute(&self, _params: HashMap<String, String>) -> ToolResult {
            ToolResult::ok("mock result".into())
        }
    }

    struct FailTool;

    #[async_trait::async_trait]
    impl Tool for FailTool {
        fn name(&self) -> &str { "fail" }
        fn description(&self) -> &str { "Always fails" }
        async fn execute(&self, _params: HashMap<String, String>) -> ToolResult {
            ToolResult::err("intentional failure".into())
        }
    }

    #[tokio::test]
    async fn test_register_and_execute() {
        let mut reg = ToolRegistry::new();
        reg.register(Box::new(MockTool));
        assert!(reg.has("mock"));
        let result = reg.execute("mock", HashMap::new()).await;
        assert!(result.success);
        assert_eq!(result.data.unwrap(), "mock result");
    }

    #[tokio::test]
    async fn test_tool_not_found() {
        let reg = ToolRegistry::new();
        let result = reg.execute("nonexistent", HashMap::new()).await;
        assert!(!result.success);
    }

    #[tokio::test]
    async fn test_failing_tool() {
        let mut reg = ToolRegistry::new();
        reg.register(Box::new(FailTool));
        let result = reg.execute("fail", HashMap::new()).await;
        assert!(!result.success);
    }

    #[test]
    fn test_list_tools() {
        let mut reg = ToolRegistry::new();
        reg.register(Box::new(MockTool));
        reg.register(Box::new(FailTool));
        assert_eq!(reg.list_tools().len(), 2);
    }
}
