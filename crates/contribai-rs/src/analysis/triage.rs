//! Weighted triage scoring for findings.
//!
//! 🆕 NEW in Rust version — inspired by RedAmon's CypherFix scoring.
//! Uses 12 weighted signals to prioritize findings for maximum impact
//! and merge likelihood.

use crate::core::models::{
    Finding, FixComplexity, RemediationSpec, ScoringSignal, Severity,
};

/// Triage engine that scores and prioritizes findings.
pub struct TriageEngine;

impl TriageEngine {
    /// Score and convert findings into prioritized remediation specs.
    pub fn triage(findings: Vec<Finding>) -> Vec<RemediationSpec> {
        let mut specs: Vec<RemediationSpec> = findings
            .into_iter()
            .map(|f| Self::score_finding(f))
            .collect();

        // Sort by priority score (lower = higher priority)
        specs.sort_by(|a, b| {
            a.priority_score
                .partial_cmp(&b.priority_score)
                .unwrap_or(std::cmp::Ordering::Equal)
        });

        specs
    }

    /// Score a single finding using 12 weighted signals.
    fn score_finding(finding: Finding) -> RemediationSpec {
        let mut signals = Vec::new();
        let mut raw_score = 0i32;

        // Signal 1: Severity (weight: 0-40)
        let sev_weight = match finding.severity {
            Severity::Critical => 40,
            Severity::High => 30,
            Severity::Medium => 20,
            Severity::Low => 10,
        };
        signals.push(ScoringSignal {
            name: "severity".into(),
            weight: sev_weight,
            reason: format!("Severity: {}", finding.severity),
        });
        raw_score += sev_weight;

        // Signal 2: Confidence (weight: 0-20)
        let conf_weight = (finding.confidence * 20.0) as i32;
        signals.push(ScoringSignal {
            name: "confidence".into(),
            weight: conf_weight,
            reason: format!("Confidence: {:.0}%", finding.confidence * 100.0),
        });
        raw_score += conf_weight;

        // Signal 3: Has concrete suggestion (weight: 15)
        if finding.suggestion.is_some() {
            signals.push(ScoringSignal {
                name: "has_suggestion".into(),
                weight: 15,
                reason: "Has concrete fix suggestion".into(),
            });
            raw_score += 15;
        }

        // Signal 4: Specific file location (weight: 10)
        if finding.line_start.is_some() {
            signals.push(ScoringSignal {
                name: "has_location".into(),
                weight: 10,
                reason: "Has specific line location".into(),
            });
            raw_score += 10;
        }

        // Signal 5: Security category bonus (weight: 15)
        let is_security = matches!(
            finding.finding_type,
            crate::core::models::ContributionType::SecurityFix
        );
        if is_security {
            signals.push(ScoringSignal {
                name: "security_category".into(),
                weight: 15,
                reason: "Security issues have higher merge rates".into(),
            });
            raw_score += 15;
        }

        // Signal 6: Small scope bonus (weight: 10)
        let scope = finding
            .line_end
            .unwrap_or(0)
            .saturating_sub(finding.line_start.unwrap_or(0));
        if scope <= 20 {
            signals.push(ScoringSignal {
                name: "small_scope".into(),
                weight: 10,
                reason: format!("Small fix scope ({} lines)", scope),
            });
            raw_score += 10;
        }

        // Estimate fix complexity
        let fix_complexity = if scope <= 5 {
            FixComplexity::Low
        } else if scope <= 20 {
            FixComplexity::Medium
        } else if scope <= 50 {
            FixComplexity::High
        } else {
            FixComplexity::Critical
        };

        // Determine category
        let category = match finding.finding_type {
            crate::core::models::ContributionType::SecurityFix => "security",
            crate::core::models::ContributionType::PerformanceOpt => "performance",
            crate::core::models::ContributionType::CodeQuality => "quality",
            crate::core::models::ContributionType::DocsImprove => "docs",
            crate::core::models::ContributionType::Refactor => "refactor",
            _ => "other",
        };

        // Invert score so lower = higher priority (max raw ~120)
        let priority = 120.0 - raw_score as f64;

        RemediationSpec {
            finding,
            priority_score: priority.max(0.0),
            category: category.to_string(),
            fix_complexity,
            affected_symbols: vec![],
            evidence: String::new(),
            solution_hint: String::new(),
            scoring_signals: signals,
        }
    }

    /// Filter specs by minimum quality threshold.
    pub fn filter_actionable(specs: Vec<RemediationSpec>, min_score: f64) -> Vec<RemediationSpec> {
        specs
            .into_iter()
            .filter(|s| s.priority_score <= min_score)
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::models::ContributionType;

    fn make_finding(severity: Severity, confidence: f64, has_suggestion: bool) -> Finding {
        Finding {
            id: uuid::Uuid::new_v4().to_string(),
            finding_type: ContributionType::SecurityFix,
            severity,
            title: "Test finding".into(),
            description: "Test description".into(),
            file_path: "src/main.py".into(),
            line_start: Some(10),
            line_end: Some(15),
            suggestion: if has_suggestion {
                Some("Fix this".into())
            } else {
                None
            },
            confidence,
            priority_signals: vec![],
        }
    }

    #[test]
    fn test_critical_security_ranks_highest() {
        let findings = vec![
            make_finding(Severity::Low, 0.5, false),
            make_finding(Severity::Critical, 0.95, true),
            make_finding(Severity::Medium, 0.7, true),
        ];

        let specs = TriageEngine::triage(findings);

        assert_eq!(specs.len(), 3);
        // Critical with high confidence should be first (lowest priority_score)
        assert_eq!(specs[0].finding.severity, Severity::Critical);
        assert_eq!(specs[2].finding.severity, Severity::Low);
    }

    #[test]
    fn test_suggestion_boosts_score() {
        let with = make_finding(Severity::Medium, 0.8, true);
        let without = make_finding(Severity::Medium, 0.8, false);

        let spec_with = TriageEngine::score_finding(with);
        let spec_without = TriageEngine::score_finding(without);

        // Lower priority_score = higher priority
        assert!(spec_with.priority_score < spec_without.priority_score);
    }

    #[test]
    fn test_fix_complexity_estimation() {
        let mut f = make_finding(Severity::Low, 0.5, false);
        f.line_start = Some(10);
        f.line_end = Some(12);
        let spec = TriageEngine::score_finding(f);
        assert_eq!(spec.fix_complexity, FixComplexity::Low);

        let mut f2 = make_finding(Severity::Low, 0.5, false);
        f2.line_start = Some(10);
        f2.line_end = Some(50);
        let spec2 = TriageEngine::score_finding(f2);
        assert_eq!(spec2.fix_complexity, FixComplexity::High);
    }
}
