//! MCP server and client for Model Context Protocol integration.
//!
//! - `server`: Exposes ContribAI's GitHub tools via stdio (for Claude/Antigravity).
//! - `client`: Consumes external MCP servers via stdio subprocess.

pub mod client;
pub mod server;
