//! Sub-agent registry — role-based agent orchestration.
//!
//! Port from Python `agents/registry.py`.
//! Implements DeerFlow-inspired sub-agent architecture with
//! scoped contexts, parallel execution, and role-based dispatch.

use async_trait::async_trait;
use serde_json::Value;
use std::collections::HashMap;
use std::sync::Arc;
use tracing::{info, error};


/// Roles available for sub-agents.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum AgentRole {
    Analyzer,
    Generator,
    Patrol,
    IssueSolver,
    Compliance,
}

impl AgentRole {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Analyzer => "analyzer",
            Self::Generator => "generator",
            Self::Patrol => "patrol",
            Self::IssueSolver => "issue_solver",
            Self::Compliance => "compliance",
        }
    }
}

impl std::fmt::Display for AgentRole {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

/// Isolated context for a sub-agent.
#[derive(Debug, Clone)]
pub struct AgentContext {
    pub role: AgentRole,
    pub repo_name: String,
    pub owner: String,
    pub data: HashMap<String, Value>,
    pub tools: Vec<String>,
    pub max_duration_sec: u64,
}

impl Default for AgentContext {
    fn default() -> Self {
        Self {
            role: AgentRole::Analyzer,
            repo_name: String::new(),
            owner: String::new(),
            data: HashMap::new(),
            tools: Vec::new(),
            max_duration_sec: 900, // 15min like DeerFlow
        }
    }
}

/// Trait for sub-agents.
#[async_trait]
pub trait SubAgent: Send + Sync {
    fn role(&self) -> AgentRole;
    fn description(&self) -> &str;
    async fn execute(&self, ctx: &AgentContext) -> HashMap<String, Value>;
}

/// Registry for discovering and managing sub-agents.
pub struct AgentRegistry {
    agents: HashMap<AgentRole, Arc<dyn SubAgent>>,
    max_concurrent: usize,
}

impl AgentRegistry {
    pub fn new(max_concurrent: usize) -> Self {
        Self {
            agents: HashMap::new(),
            max_concurrent,
        }
    }

    /// Register a sub-agent.
    pub fn register(&mut self, agent: impl SubAgent + 'static) {
        let agent = Arc::new(agent);
        let role = agent.role();
        info!(
            role = role.as_str(),
            desc = agent.description(),
            "Registered agent"
        );
        self.agents.insert(role, agent);
    }

    /// Get a registered agent by role.
    pub fn get(&self, role: AgentRole) -> Option<Arc<dyn SubAgent>> {
        self.agents.get(&role).cloned()
    }

    /// List all registered agents.
    pub fn list_agents(&self) -> Vec<(AgentRole, &str)> {
        self.agents
            .iter()
            .map(|(role, agent)| (*role, agent.description()))
            .collect()
    }

    /// Execute a specific agent.
    pub async fn execute(
        &self,
        role: AgentRole,
        ctx: &AgentContext,
    ) -> Result<HashMap<String, Value>, String> {
        let agent = self
            .agents
            .get(&role)
            .ok_or_else(|| format!("No agent registered for role: {}", role))?;

        info!(role = role.as_str(), repo = %ctx.repo_name, "Executing agent");
        Ok(agent.execute(ctx).await)
    }

    /// Execute multiple agents in **true parallel** using tokio::spawn.
    pub async fn execute_parallel(
        &self,
        tasks: Vec<(AgentRole, AgentContext)>,
    ) -> Vec<HashMap<String, Value>> {
        let sem = Arc::new(tokio::sync::Semaphore::new(self.max_concurrent));
        let mut handles = Vec::new();

        for (role, ctx) in tasks {
            if let Some(agent) = self.agents.get(&role).cloned() {
                let permit = sem.clone().acquire_owned().await;
                let handle = tokio::spawn(async move {
                    let _permit = permit; // released when dropped
                    agent.execute(&ctx).await
                });
                handles.push(handle);
            } else {
                error!(role = role.as_str(), "Agent not found for parallel exec");
                let mut err = HashMap::new();
                err.insert(
                    "error".into(),
                    Value::String(format!("Agent not found: {}", role)),
                );
                handles.push(tokio::spawn(async move { err }));
            }
        }

        // Collect all results
        let mut results = Vec::new();
        for handle in handles {
            match handle.await {
                Ok(result) => results.push(result),
                Err(e) => {
                    error!("Agent task panicked: {}", e);
                    let mut err = HashMap::new();
                    err.insert("error".into(), Value::String(format!("Task panicked: {}", e)));
                    results.push(err);
                }
            }
        }
        results
    }
}

/// Create a registry with default built-in agents.
pub fn create_default_registry() -> AgentRegistry {
    let mut registry = AgentRegistry::new(3);
    registry.register(AnalyzerAgent);
    registry.register(GeneratorAgent);
    registry.register(PatrolAgent);
    registry.register(ComplianceAgent);
    registry.register(IssueSolverAgent);
    registry
}

// ── Built-in Agent Stubs ─────────────────────────────────────────────

struct AnalyzerAgent;
#[async_trait]
impl SubAgent for AnalyzerAgent {
    fn role(&self) -> AgentRole { AgentRole::Analyzer }
    fn description(&self) -> &str {
        "Analyze repository code for security, quality, and performance issues"
    }
    async fn execute(&self, ctx: &AgentContext) -> HashMap<String, Value> {
        let mut result = HashMap::new();
        result.insert("role".into(), Value::String(self.role().to_string()));
        result.insert("repo".into(), Value::String(ctx.repo_name.clone()));
        result
    }
}

struct GeneratorAgent;
#[async_trait]
impl SubAgent for GeneratorAgent {
    fn role(&self) -> AgentRole { AgentRole::Generator }
    fn description(&self) -> &str {
        "Generate code fixes and contributions from analysis findings"
    }
    async fn execute(&self, ctx: &AgentContext) -> HashMap<String, Value> {
        let mut result = HashMap::new();
        result.insert("role".into(), Value::String(self.role().to_string()));
        result.insert("repo".into(), Value::String(ctx.repo_name.clone()));
        result
    }
}

struct PatrolAgent;
#[async_trait]
impl SubAgent for PatrolAgent {
    fn role(&self) -> AgentRole { AgentRole::Patrol }
    fn description(&self) -> &str {
        "Monitor open PRs for review feedback and auto-respond with fixes"
    }
    async fn execute(&self, _ctx: &AgentContext) -> HashMap<String, Value> {
        let mut result = HashMap::new();
        result.insert("role".into(), Value::String(self.role().to_string()));
        result
    }
}

struct ComplianceAgent;
#[async_trait]
impl SubAgent for ComplianceAgent {
    fn role(&self) -> AgentRole { AgentRole::Compliance }
    fn description(&self) -> &str {
        "Handle CLA auto-signing, DCO signoff, and post-PR CI monitoring"
    }
    async fn execute(&self, _ctx: &AgentContext) -> HashMap<String, Value> {
        let mut result = HashMap::new();
        result.insert("role".into(), Value::String(self.role().to_string()));
        result.insert("actions".into(), Value::Array(vec![]));
        result
    }
}

struct IssueSolverAgent;
#[async_trait]
impl SubAgent for IssueSolverAgent {
    fn role(&self) -> AgentRole { AgentRole::IssueSolver }
    fn description(&self) -> &str {
        "Solve open GitHub issues by generating targeted code contributions"
    }
    async fn execute(&self, ctx: &AgentContext) -> HashMap<String, Value> {
        let mut result = HashMap::new();
        result.insert("role".into(), Value::String(self.role().to_string()));
        result.insert("repo".into(), Value::String(ctx.repo_name.clone()));
        result
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_create_default_registry() {
        let registry = create_default_registry();
        assert_eq!(registry.list_agents().len(), 5);
    }

    #[test]
    fn test_get_agent() {
        let registry = create_default_registry();
        assert!(registry.get(AgentRole::Analyzer).is_some());
        assert!(registry.get(AgentRole::Compliance).is_some());
    }

    #[test]
    fn test_agent_role_display() {
        assert_eq!(AgentRole::Analyzer.as_str(), "analyzer");
        assert_eq!(AgentRole::IssueSolver.as_str(), "issue_solver");
    }

    #[tokio::test]
    async fn test_execute_agent() {
        let registry = create_default_registry();
        let ctx = AgentContext {
            role: AgentRole::Analyzer,
            repo_name: "test/repo".into(),
            ..Default::default()
        };
        let result = registry.execute(AgentRole::Analyzer, &ctx).await.unwrap();
        assert_eq!(result.get("role").unwrap(), "analyzer");
    }

    #[test]
    fn test_execute_missing_agent_err() {
        let registry = AgentRegistry::new(3); // empty
        let ctx = AgentContext::default();
        let rt = tokio::runtime::Runtime::new().unwrap();
        let result = rt.block_on(registry.execute(AgentRole::Analyzer, &ctx));
        assert!(result.is_err());
    }
}
