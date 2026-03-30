//! Model registry with capabilities, costs, and context windows.
//!
//! Port from Python `llm/models.py`.

use std::fmt;

/// Types of tasks that models can be assigned to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TaskType {
    Analysis,
    CodeGen,
    Review,
    Docs,
    QuickFix,
    Bulk,
    Planning,
    Multimodal,
}

impl fmt::Display for TaskType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Analysis => write!(f, "analysis"),
            Self::CodeGen => write!(f, "code_gen"),
            Self::Review => write!(f, "review"),
            Self::Docs => write!(f, "docs"),
            Self::QuickFix => write!(f, "quick_fix"),
            Self::Bulk => write!(f, "bulk"),
            Self::Planning => write!(f, "planning"),
            Self::Multimodal => write!(f, "multimodal"),
        }
    }
}

/// Model performance tiers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ModelTier {
    Pro,
    Flash,
    Lite,
}

/// Specification of a model's capabilities and costs.
#[derive(Debug, Clone)]
pub struct ModelSpec {
    pub name: String,
    pub display_name: String,
    pub tier: ModelTier,
    pub context_window: u64,
    pub max_output: u64,
    pub input_cost: f64,
    pub output_cost: f64,
    pub coding: u32,
    pub analysis: u32,
    pub reasoning: u32,
    pub speed: u32,
    pub multimodal: u32,
    pub best_for: Vec<TaskType>,
    pub description: String,
}

impl ModelSpec {
    pub fn overall_score(&self) -> f64 {
        (self.coding + self.analysis + self.reasoning + self.speed) as f64 / 4.0
    }

    pub fn cost_efficiency(&self) -> f64 {
        let total_cost = self.input_cost + self.output_cost;
        if total_cost == 0.0 { 100.0 } else { self.overall_score() / total_cost }
    }
}

// ── Model Catalog ─────────────────────────────────────

pub fn gemini_3_1_pro() -> ModelSpec {
    ModelSpec {
        name: "gemini-3.1-pro-preview".into(),
        display_name: "Gemini 3.1 Pro".into(),
        tier: ModelTier::Pro,
        context_window: 1_000_000,
        max_output: 65_536,
        input_cost: 1.25,
        output_cost: 10.0,
        coding: 98, analysis: 97, reasoning: 98, speed: 55, multimodal: 95,
        best_for: vec![TaskType::CodeGen, TaskType::Analysis, TaskType::Planning, TaskType::Review],
        description: "Most powerful agentic and coding model.".into(),
    }
}

pub fn gemini_3_flash() -> ModelSpec {
    ModelSpec {
        name: "gemini-3-flash-preview".into(),
        display_name: "Gemini 3 Flash".into(),
        tier: ModelTier::Flash,
        context_window: 1_000_000,
        max_output: 65_536,
        input_cost: 0.15,
        output_cost: 0.60,
        coding: 88, analysis: 87, reasoning: 85, speed: 85, multimodal: 80,
        best_for: vec![TaskType::Analysis, TaskType::Review, TaskType::QuickFix, TaskType::CodeGen],
        description: "Agentic workhorse — near-Pro intelligence with balanced cost.".into(),
    }
}

pub fn gemini_3_1_flash_lite() -> ModelSpec {
    ModelSpec {
        name: "gemini-3.1-flash-lite-preview".into(),
        display_name: "Gemini 3.1 Flash Lite".into(),
        tier: ModelTier::Lite,
        context_window: 1_000_000,
        max_output: 65_536,
        input_cost: 0.02,
        output_cost: 0.10,
        coding: 72, analysis: 70, reasoning: 68, speed: 95, multimodal: 60,
        best_for: vec![TaskType::Bulk, TaskType::Docs, TaskType::QuickFix],
        description: "High-volume, cost-sensitive.".into(),
    }
}

pub fn gemini_2_5_flash() -> ModelSpec {
    ModelSpec {
        name: "gemini-2.5-flash".into(),
        display_name: "Gemini 2.5 Flash".into(),
        tier: ModelTier::Flash,
        context_window: 1_000_000,
        max_output: 65_536,
        input_cost: 0.15,
        output_cost: 0.60,
        coding: 82, analysis: 80, reasoning: 78, speed: 88, multimodal: 70,
        best_for: vec![TaskType::Analysis, TaskType::Review, TaskType::Docs],
        description: "Previous-gen Flash, still solid.".into(),
    }
}

/// All available models.
pub fn all_models() -> Vec<ModelSpec> {
    vec![
        gemini_3_1_pro(),
        gemini_3_flash(),
        gemini_3_1_flash_lite(),
        gemini_2_5_flash(),
    ]
}

/// Get model by name.
pub fn get_model(name: &str) -> Option<ModelSpec> {
    all_models().into_iter().find(|m| m.name == name)
}

/// Get models best suited for a task type.
pub fn get_models_for_task(task_type: TaskType) -> Vec<ModelSpec> {
    let mut matching: Vec<ModelSpec> = all_models()
        .into_iter()
        .filter(|m| m.best_for.contains(&task_type))
        .collect();

    matching.sort_by(|a, b| {
        let score_a = match task_type {
            TaskType::CodeGen => a.coding,
            TaskType::Analysis => a.analysis,
            TaskType::Review | TaskType::Planning => a.reasoning,
            TaskType::Docs | TaskType::QuickFix => a.speed,
            _ => (a.overall_score() * 10.0) as u32,
        };
        let score_b = match task_type {
            TaskType::CodeGen => b.coding,
            TaskType::Analysis => b.analysis,
            TaskType::Review | TaskType::Planning => b.reasoning,
            TaskType::Docs | TaskType::QuickFix => b.speed,
            _ => (b.overall_score() * 10.0) as u32,
        };
        score_b.cmp(&score_a)
    });

    matching
}

/// Get the cheapest model meeting minimum capability.
pub fn get_cheapest_capable(task_type: TaskType, min_score: f64) -> Option<ModelSpec> {
    let candidates = get_models_for_task(task_type);
    let capable: Vec<ModelSpec> = candidates
        .into_iter()
        .filter(|m| m.overall_score() >= min_score)
        .collect();
    capable
        .into_iter()
        .min_by(|a, b| {
            let cost_a = a.input_cost + a.output_cost;
            let cost_b = b.input_cost + b.output_cost;
            cost_a.partial_cmp(&cost_b).unwrap()
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_all_models() {
        assert_eq!(all_models().len(), 4);
    }

    #[test]
    fn test_get_model() {
        let m = get_model("gemini-2.5-flash").unwrap();
        assert_eq!(m.tier, ModelTier::Flash);
    }

    #[test]
    fn test_get_models_for_codegen() {
        let models = get_models_for_task(TaskType::CodeGen);
        assert!(!models.is_empty());
        assert_eq!(models[0].name, "gemini-3.1-pro-preview");
    }

    #[test]
    fn test_get_cheapest() {
        let m = get_cheapest_capable(TaskType::Bulk, 60.0).unwrap();
        assert_eq!(m.tier, ModelTier::Lite);
    }

    #[test]
    fn test_overall_score() {
        let m = gemini_3_1_pro();
        assert!(m.overall_score() > 80.0);
    }

    #[test]
    fn test_cost_efficiency() {
        let lite = gemini_3_1_flash_lite();
        let pro = gemini_3_1_pro();
        assert!(lite.cost_efficiency() > pro.cost_efficiency());
    }
}
