//! Smart task-to-model router.
//!
//! Port from Python `llm/router.py`.

use tracing::info;
use std::collections::HashMap;

use super::models::*;

/// Cost optimization strategies.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CostStrategy {
    Performance,
    Balanced,
    Economy,
}

/// Result of a routing decision.
#[derive(Debug, Clone)]
pub struct RoutingDecision {
    pub model: ModelSpec,
    pub task_type: TaskType,
    pub reason: String,
    pub fallback: Option<ModelSpec>,
}

/// Routes tasks to optimal models.
pub struct TaskRouter {
    strategy: CostStrategy,
    task_count: HashMap<String, u64>,
}

impl TaskRouter {
    pub fn new(strategy: CostStrategy) -> Self {
        Self {
            strategy,
            task_count: HashMap::new(),
        }
    }

    pub fn route(
        &mut self,
        task_type: TaskType,
        complexity: u32,
        file_count: u32,
    ) -> RoutingDecision {
        let decision = match self.strategy {
            CostStrategy::Performance => self.route_performance(task_type),
            CostStrategy::Economy => self.route_economy(task_type),
            CostStrategy::Balanced => self.route_balanced(task_type, complexity, file_count),
        };
        *self.task_count.entry(decision.model.name.clone()).or_insert(0) += 1;
        info!(
            model = %decision.model.display_name,
            reason = %decision.reason,
            "Routed task"
        );
        decision
    }

    fn route_performance(&self, task_type: TaskType) -> RoutingDecision {
        let models = get_models_for_task(task_type);
        let model = models.into_iter().next().unwrap_or_else(gemini_3_1_pro);
        RoutingDecision {
            reason: format!("Performance mode: {}", model.display_name),
            fallback: Some(gemini_3_flash()),
            model,
            task_type,
        }
    }

    fn route_economy(&self, task_type: TaskType) -> RoutingDecision {
        let model = get_cheapest_capable(task_type, 60.0)
            .unwrap_or_else(gemini_3_1_flash_lite);
        RoutingDecision {
            reason: format!("Economy mode: {}", model.display_name),
            fallback: Some(gemini_2_5_flash()),
            model,
            task_type,
        }
    }

    fn route_balanced(
        &self,
        task_type: TaskType,
        complexity: u32,
        file_count: u32,
    ) -> RoutingDecision {
        let (mut model, mut reason, mut fallback);

        if complexity >= 8 || file_count >= 10 {
            model = gemini_3_1_pro();
            reason = format!("High complexity ({complexity}/10, {file_count} files) → Pro");
            fallback = gemini_3_flash();
        } else if complexity >= 4 {
            model = gemini_3_flash();
            reason = format!("Medium complexity ({complexity}/10) → Flash");
            fallback = gemini_2_5_flash();
        } else {
            model = gemini_3_1_flash_lite();
            reason = format!("Low complexity ({complexity}/10) → Lite");
            fallback = gemini_3_flash();
        }

        // Override for specific task types
        if task_type == TaskType::CodeGen && complexity < 8 {
            model = gemini_3_flash();
            reason = "Code gen → Flash (balanced)".into();
            fallback = gemini_3_1_pro();
        }
        if task_type == TaskType::Planning {
            model = gemini_3_1_pro();
            reason = "Planning always → Pro".into();
            fallback = gemini_3_flash();
        }
        if task_type == TaskType::Bulk {
            model = gemini_3_1_flash_lite();
            reason = "Bulk → Flash Lite (cost)".into();
            fallback = gemini_3_flash();
        }

        RoutingDecision {
            model,
            task_type,
            reason,
            fallback: Some(fallback),
        }
    }

    pub fn stats(&self) -> RouterStats {
        RouterStats {
            strategy: self.strategy,
            tasks_routed: self.task_count.clone(),
            total_tasks: self.task_count.values().sum(),
        }
    }
}

#[derive(Debug)]
pub struct RouterStats {
    pub strategy: CostStrategy,
    pub tasks_routed: HashMap<String, u64>,
    pub total_tasks: u64,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_performance_routing() {
        let mut r = TaskRouter::new(CostStrategy::Performance);
        let d = r.route(TaskType::CodeGen, 5, 1);
        assert_eq!(d.model.tier, ModelTier::Pro);
    }

    #[test]
    fn test_economy_routing() {
        let mut r = TaskRouter::new(CostStrategy::Economy);
        let d = r.route(TaskType::Bulk, 2, 1);
        assert_eq!(d.model.tier, ModelTier::Lite);
    }

    #[test]
    fn test_balanced_high_complexity() {
        let mut r = TaskRouter::new(CostStrategy::Balanced);
        let d = r.route(TaskType::Analysis, 9, 1);
        assert_eq!(d.model.tier, ModelTier::Pro);
    }

    #[test]
    fn test_balanced_low_complexity() {
        let mut r = TaskRouter::new(CostStrategy::Balanced);
        let d = r.route(TaskType::Analysis, 2, 1);
        assert_eq!(d.model.tier, ModelTier::Lite);
    }

    #[test]
    fn test_planning_always_pro() {
        let mut r = TaskRouter::new(CostStrategy::Balanced);
        let d = r.route(TaskType::Planning, 1, 1);
        assert_eq!(d.model.tier, ModelTier::Pro);
    }

    #[test]
    fn test_stats() {
        let mut r = TaskRouter::new(CostStrategy::Balanced);
        r.route(TaskType::Analysis, 5, 1);
        r.route(TaskType::Analysis, 5, 1);
        assert_eq!(r.stats().total_tasks, 2);
    }
}
