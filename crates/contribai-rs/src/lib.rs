//! ContribAI — AI agent that autonomously contributes to open source.
//!
//! # Architecture
//!
//! ```text
//! Discovery → Analysis (AST + Triage + PageRank) → Generation → PR
//! ```
//!
//! Built with:
//! - `reqwest` for GitHub API & LLM calls
//! - `tree-sitter` for AST-powered code intelligence
//! - `rusqlite` for outcome memory
//! - `tokio` for async concurrency

pub mod agents;
pub mod analysis;
pub mod core;
pub mod generator;
pub mod github;
pub mod issues;
pub mod llm;
pub mod mcp;
pub mod notifications;
pub mod orchestrator;
pub mod plugins;
pub mod pr;
pub mod sandbox;
pub mod scheduler;
pub mod templates;
pub mod tools;

/// Current version of ContribAI.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");
