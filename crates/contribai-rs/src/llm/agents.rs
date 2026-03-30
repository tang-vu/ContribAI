//! Multi-agent coordinator for ContribAI.
//!
//! Port from Python `llm/agents.py`.
//! Specialized agents collaborate on analysis, codegen, review, docs, planning.

use tracing::{info, warn};

use super::models::TaskType;
use super::router::{CostStrategy, TaskRouter};
use crate::llm::provider::LlmProvider;

/// Result from an agent execution.
#[derive(Debug, Clone)]
pub struct AgentResult {
    pub agent_name: String,
    pub model_used: String,
    pub task_type: String,
    pub output: String,
    pub tokens_used: u64,
    pub success: bool,
    pub error: String,
}

/// Base agent functionality.
struct Agent {
    name: &'static str,
    task_type: TaskType,
    system_prompt: &'static str,
}

const ANALYSIS_AGENT: Agent = Agent {
    name: "Analyzer",
    task_type: TaskType::Analysis,
    system_prompt: "You are a senior code reviewer specializing in security vulnerabilities, \
                    code quality issues, and best practices. Be precise and actionable.",
};

const CODEGEN_AGENT: Agent = Agent {
    name: "CodeGen",
    task_type: TaskType::CodeGen,
    system_prompt: "You are an expert programmer. Generate clean, well-documented, \
                    production-ready code. Follow the project's existing style and conventions.",
};

const REVIEW_AGENT: Agent = Agent {
    name: "Reviewer",
    task_type: TaskType::Review,
    system_prompt: "You are a meticulous code reviewer. Check for correctness, edge cases, \
                    style consistency, and potential regressions. Be critical but constructive.",
};

const DOCS_AGENT: Agent = Agent {
    name: "DocsWriter",
    task_type: TaskType::Docs,
    system_prompt: "You are a technical writer. Write clear, concise documentation. \
                    Use proper formatting, examples, and follow the project's documentation style.",
};

const PLANNER_AGENT: Agent = Agent {
    name: "Planner",
    task_type: TaskType::Planning,
    system_prompt: "You are a software architect. Analyze repositories and plan contributions \
                    strategically. Consider impact, feasibility, and maintainer expectations.",
};

/// Multi-agent coordinator.
///
/// Pipeline: Analyze → Plan → Generate → Review
pub struct AgentCoordinator {
    router: TaskRouter,
    results: Vec<AgentResult>,
}

impl AgentCoordinator {
    pub fn new(strategy: CostStrategy) -> Self {
        Self {
            router: TaskRouter::new(strategy),
            results: Vec::new(),
        }
    }

    /// Execute a single agent.
    async fn execute_agent(
        &mut self,
        agent: &Agent,
        llm: &dyn LlmProvider,
        prompt: &str,
        complexity: u32,
        file_count: u32,
    ) -> AgentResult {
        let decision = self.router.route(agent.task_type, complexity, file_count);
        info!(
            agent = agent.name,
            model = %decision.model.display_name,
            reason = %decision.reason,
            "Agent routing"
        );

        match llm
            .complete(prompt, Some(agent.system_prompt), None, None)
            .await
        {
            Ok(output) => {
                let result = AgentResult {
                    agent_name: agent.name.into(),
                    model_used: decision.model.name,
                    task_type: agent.task_type.to_string(),
                    output,
                    tokens_used: 0,
                    success: true,
                    error: String::new(),
                };
                self.results.push(result.clone());
                result
            }
            Err(e) => {
                warn!(agent = agent.name, error = %e, "Agent failed");
                let result = AgentResult {
                    agent_name: agent.name.into(),
                    model_used: decision.model.name,
                    task_type: agent.task_type.to_string(),
                    output: String::new(),
                    tokens_used: 0,
                    success: false,
                    error: e.to_string(),
                };
                self.results.push(result.clone());
                result
            }
        }
    }

    /// Run analysis agent.
    pub async fn run_analysis(
        &mut self,
        llm: &dyn LlmProvider,
        code: &str,
        language: &str,
        file_path: &str,
    ) -> AgentResult {
        let complexity = (code.len() / 500).min(10) as u32;
        let prompt = format!(
            "Analyze this {language} file for issues:\nFile: {file_path}\n\n\
             ```{language}\n{code}\n```\n\n\
             List all security, quality, and performance issues. \
             For each: severity (critical/high/medium/low), line numbers, description, and fix suggestion.",
        );
        self.execute_agent(&ANALYSIS_AGENT, llm, &prompt, complexity, 1).await
    }

    /// Run code generation agent.
    pub async fn run_codegen(
        &mut self,
        llm: &dyn LlmProvider,
        issue: &str,
        original_code: &str,
        language: &str,
    ) -> AgentResult {
        let prompt = format!(
            "Fix the following issue in this {language} code:\n\n\
             Issue: {issue}\n\n\
             Original code:\n```{language}\n{original_code}\n```\n\n\
             Provide the complete fixed code with comments explaining the changes.",
        );
        self.execute_agent(&CODEGEN_AGENT, llm, &prompt, 7, 1).await
    }

    /// Run review agent.
    pub async fn run_review(
        &mut self,
        llm: &dyn LlmProvider,
        original: &str,
        modified: &str,
        issue: &str,
    ) -> AgentResult {
        let prompt = format!(
            "Review this code change:\n\n\
             Issue being fixed: {issue}\n\n\
             Original:\n```\n{original}\n```\n\n\
             Modified:\n```\n{modified}\n```\n\n\
             Check: correctness, edge cases, style, regressions. Approve or suggest improvements.",
        );
        self.execute_agent(&REVIEW_AGENT, llm, &prompt, 5, 1).await
    }

    /// Run the full multi-agent pipeline: Analyze → Generate → Review.
    pub async fn run_full_pipeline(
        &mut self,
        llm: &dyn LlmProvider,
        code: &str,
        language: &str,
        file_path: &str,
    ) -> Vec<AgentResult> {
        let mut results = Vec::new();

        let analysis = self.run_analysis(llm, code, language, file_path).await;
        results.push(analysis.clone());
        if !analysis.success || analysis.output.is_empty() {
            return results;
        }

        let codegen = self.run_codegen(llm, &analysis.output, code, language).await;
        results.push(codegen.clone());
        if !codegen.success || codegen.output.is_empty() {
            return results;
        }

        let review = self.run_review(llm, code, &codegen.output, &analysis.output).await;
        results.push(review);

        results
    }

    pub fn routing_stats(&self) -> super::router::RouterStats {
        self.router.stats()
    }

    pub fn agent_stats(&self) -> Vec<AgentStatEntry> {
        self.results.iter().map(|r| AgentStatEntry {
            agent: r.agent_name.clone(),
            model: r.model_used.clone(),
            success: r.success,
            tokens: r.tokens_used,
        }).collect()
    }
}

#[derive(Debug)]
pub struct AgentStatEntry {
    pub agent: String,
    pub model: String,
    pub success: bool,
    pub tokens: u64,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_agent_result_default() {
        let r = AgentResult {
            agent_name: "test".into(),
            model_used: "test-model".into(),
            task_type: "analysis".into(),
            output: "result".into(),
            tokens_used: 100,
            success: true,
            error: String::new(),
        };
        assert!(r.success);
        assert_eq!(r.agent_name, "test");
    }

    #[test]
    fn test_coordinator_creation() {
        let coord = AgentCoordinator::new(CostStrategy::Balanced);
        assert_eq!(coord.results.len(), 0);
    }
}
