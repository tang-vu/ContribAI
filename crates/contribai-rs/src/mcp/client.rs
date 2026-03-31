//! MCP Client — consume external MCP servers.
//!
//! Port from Python `mcp/mcp_client.py`.
//! Spawns an MCP server subprocess and communicates using JSON-RPC
//! over stdin/stdout.

use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::process::Stdio;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::{Child, Command};
use tracing::{debug, error, info};

/// Result from calling an MCP tool.
#[derive(Debug, Clone, Deserialize)]
pub struct McpToolResult {
    pub content: Vec<Value>,
    #[serde(default)]
    pub is_error: bool,
}

impl McpToolResult {
    /// Extract text content from result items.
    pub fn text(&self) -> String {
        self.content
            .iter()
            .filter_map(|item| {
                if item.get("type").and_then(|t| t.as_str()) == Some("text") {
                    item.get("text").and_then(|t| t.as_str()).map(String::from)
                } else {
                    None
                }
            })
            .collect::<Vec<_>>()
            .join("\n")
    }
}

/// JSON-RPC response wrapper.
#[derive(Debug, Deserialize)]
struct JsonRpcResponse {
    #[allow(dead_code)]
    jsonrpc: String,
    #[allow(dead_code)]
    id: Option<u64>,
    result: Option<Value>,
    error: Option<JsonRpcError>,
}

#[derive(Debug, Deserialize)]
struct JsonRpcError {
    #[allow(dead_code)]
    code: Option<i64>,
    message: String,
}

/// MCP client that communicates via stdio with a subprocess.
///
/// Spawns an MCP server process and communicates using JSON-RPC
/// over stdin/stdout.
///
/// # Example
/// ```ignore
/// let mut client = StdioMcpClient::new(&["python", "-m", "some_mcp_server"]);
/// client.connect().await?;
/// let tools = client.list_tools().await?;
/// let result = client.call_tool("search", serde_json::json!({"query": "test"})).await?;
/// client.disconnect().await;
/// ```
pub struct StdioMcpClient {
    cmd: Vec<String>,
    env: Vec<(String, String)>,
    timeout: Duration,
    process: Option<Child>,
    request_id: AtomicU64,
}

impl StdioMcpClient {
    /// Create a new MCP client with the given command.
    pub fn new(cmd: &[&str]) -> Self {
        Self {
            cmd: cmd.iter().map(|s| s.to_string()).collect(),
            env: Vec::new(),
            timeout: Duration::from_secs(30),
            process: None,
            request_id: AtomicU64::new(0),
        }
    }

    /// Set environment variables for the subprocess.
    pub fn with_env(mut self, env: Vec<(String, String)>) -> Self {
        self.env = env;
        self
    }

    /// Set the request timeout.
    pub fn with_timeout(mut self, timeout: Duration) -> Self {
        self.timeout = timeout;
        self
    }

    /// Spawn the MCP server subprocess and initialize the protocol.
    pub async fn connect(&mut self) -> Result<(), String> {
        if self.cmd.is_empty() {
            return Err("Empty command".into());
        }

        info!(cmd = ?self.cmd, "Connecting to MCP server");

        let mut command = Command::new(&self.cmd[0]);
        if self.cmd.len() > 1 {
            command.args(&self.cmd[1..]);
        }
        command
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());

        for (k, v) in &self.env {
            command.env(k, v);
        }

        let child = command
            .spawn()
            .map_err(|e| format!("Failed to spawn MCP server: {e}"))?;

        self.process = Some(child);

        // Send initialize request
        let init_result = self
            .send_request(
                "initialize",
                serde_json::json!({
                    "protocolVersion": "2024-11-05",
                    "capabilities": {},
                    "clientInfo": {"name": "contribai", "version": "5.0.0"}
                }),
            )
            .await?;

        if let Some(info) = init_result.get("serverInfo") {
            info!(server_info = %info, "MCP server initialized");
        }

        // Send initialized notification
        self.send_notification("notifications/initialized", serde_json::json!({}))
            .await?;

        Ok(())
    }

    /// Kill the MCP server subprocess.
    pub async fn disconnect(&mut self) {
        if let Some(ref mut proc) = self.process {
            let _ = proc.kill().await;
            info!("MCP server disconnected");
        }
        self.process = None;
    }

    /// List available tools from the MCP server.
    pub async fn list_tools(&mut self) -> Result<Vec<Value>, String> {
        let result = self
            .send_request("tools/list", serde_json::json!({}))
            .await?;
        Ok(result
            .get("tools")
            .and_then(|t| t.as_array())
            .cloned()
            .unwrap_or_default())
    }

    /// Call a tool on the MCP server.
    pub async fn call_tool(&mut self, name: &str, arguments: Value) -> Result<McpToolResult, String> {
        let result = self
            .send_request(
                "tools/call",
                serde_json::json!({
                    "name": name,
                    "arguments": arguments,
                }),
            )
            .await?;

        Ok(McpToolResult {
            content: result
                .get("content")
                .and_then(|c| c.as_array())
                .cloned()
                .unwrap_or_default(),
            is_error: result
                .get("isError")
                .and_then(|e| e.as_bool())
                .unwrap_or(false),
        })
    }

    /// Send a JSON-RPC request and wait for response.
    async fn send_request(&mut self, method: &str, params: Value) -> Result<Value, String> {
        let proc = self
            .process
            .as_mut()
            .ok_or("MCP server not connected")?;

        let id = self.request_id.fetch_add(1, Ordering::Relaxed) + 1;

        let request = serde_json::json!({
            "jsonrpc": "2.0",
            "id": id,
            "method": method,
            "params": params,
        });

        let mut payload = serde_json::to_string(&request)
            .map_err(|e| format!("Failed to serialize request: {e}"))?;
        payload.push('\n');

        // Write request
        let stdin = proc.stdin.as_mut().ok_or("No stdin")?;
        stdin
            .write_all(payload.as_bytes())
            .await
            .map_err(|e| format!("Failed to write to MCP server: {e}"))?;
        stdin
            .flush()
            .await
            .map_err(|e| format!("Failed to flush stdin: {e}"))?;

        // Read response with timeout
        let stdout = proc.stdout.as_mut().ok_or("No stdout")?;
        let mut reader = BufReader::new(stdout);
        let mut line = String::new();

        tokio::time::timeout(self.timeout, reader.read_line(&mut line))
            .await
            .map_err(|_| format!("MCP request timed out after {:?}", self.timeout))?
            .map_err(|e| format!("Failed to read from MCP server: {e}"))?;

        let response: JsonRpcResponse = serde_json::from_str(&line)
            .map_err(|e| format!("Invalid JSON-RPC response: {e}"))?;

        if let Some(err) = response.error {
            error!(message = %err.message, "MCP error");
            return Err(format!("MCP error: {}", err.message));
        }

        Ok(response.result.unwrap_or(Value::Null))
    }

    /// Send a JSON-RPC notification (no response expected).
    async fn send_notification(&mut self, method: &str, params: Value) -> Result<(), String> {
        let proc = self
            .process
            .as_mut()
            .ok_or("MCP server not connected")?;

        let notification = serde_json::json!({
            "jsonrpc": "2.0",
            "method": method,
            "params": params,
        });

        let mut payload = serde_json::to_string(&notification)
            .map_err(|e| format!("Failed to serialize notification: {e}"))?;
        payload.push('\n');

        let stdin = proc.stdin.as_mut().ok_or("No stdin")?;
        stdin
            .write_all(payload.as_bytes())
            .await
            .map_err(|e| format!("Failed to write notification: {e}"))?;
        stdin
            .flush()
            .await
            .map_err(|e| format!("Failed to flush: {e}"))?;

        debug!(method = method, "Sent MCP notification");
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_mcp_tool_result_text_extraction() {
        let result = McpToolResult {
            content: vec![
                serde_json::json!({"type": "text", "text": "hello"}),
                serde_json::json!({"type": "image", "data": "..."}),
                serde_json::json!({"type": "text", "text": "world"}),
            ],
            is_error: false,
        };
        assert_eq!(result.text(), "hello\nworld");
    }

    #[test]
    fn test_mcp_tool_result_empty() {
        let result = McpToolResult {
            content: vec![],
            is_error: false,
        };
        assert_eq!(result.text(), "");
    }

    #[test]
    fn test_stdio_client_creation() {
        let client = StdioMcpClient::new(&["python", "-m", "some_server"]);
        assert_eq!(client.cmd, vec!["python", "-m", "some_server"]);
        assert!(client.process.is_none());
    }

    #[test]
    fn test_client_with_timeout() {
        let client = StdioMcpClient::new(&["echo"])
            .with_timeout(Duration::from_secs(10));
        assert_eq!(client.timeout, Duration::from_secs(10));
    }

    #[test]
    fn test_client_with_env() {
        let client = StdioMcpClient::new(&["echo"])
            .with_env(vec![("KEY".into(), "val".into())]);
        assert_eq!(client.env.len(), 1);
    }
}
