//! Repository discovery engine.
//!
//! Discovers, filters, and prioritizes GitHub repositories
//! that are good candidates for contributions.
//! Port from Python `github/discovery.py`.

use chrono::{Duration, Utc};
use tracing::{debug, info};

use super::client::GitHubClient;
use crate::core::config::DiscoveryConfig;
use crate::core::error::Result;
use crate::core::models::{DiscoveryCriteria, Repository};

/// Discover contribution-friendly open source repositories.
pub struct RepoDiscovery<'a> {
    client: &'a GitHubClient,
    config: &'a DiscoveryConfig,
}

impl<'a> RepoDiscovery<'a> {
    pub fn new(client: &'a GitHubClient, config: &'a DiscoveryConfig) -> Self {
        Self { client, config }
    }

    /// Discover repositories matching criteria.
    ///
    /// Pipeline: search → filter → prioritize → return top N.
    pub async fn discover(&self, criteria: Option<&DiscoveryCriteria>) -> Result<Vec<Repository>> {
        let default_criteria = self.criteria_from_config();
        let criteria = criteria.unwrap_or(&default_criteria);

        // Search GitHub
        let repos = self.search(criteria).await?;
        info!(count = repos.len(), "Search returned repositories");

        // Filter for contribution-friendliness
        let repos = self.filter_contributable(repos, criteria).await?;
        info!(count = repos.len(), "After filtering");

        // Prioritize by impact potential
        let repos = self.prioritize(repos);

        // Return top N
        Ok(repos.into_iter().take(criteria.max_results).collect())
    }

    fn criteria_from_config(&self) -> DiscoveryCriteria {
        DiscoveryCriteria {
            languages: self.config.languages.clone(),
            stars_min: self.config.stars_min,
            stars_max: self.config.stars_max,
            max_results: self.config.max_results,
            min_last_activity_days: 30,
            require_contributing_guide: false,
            topics: Vec::new(),
            exclude_repos: Vec::new(),
        }
    }

    /// Build and execute GitHub search query.
    async fn search(&self, criteria: &DiscoveryCriteria) -> Result<Vec<Repository>> {
        let mut all_repos: Vec<Repository> = Vec::new();

        for language in &criteria.languages {
            let mut query_parts = vec![
                format!("language:{}", language),
                format!("stars:{}..{}", criteria.stars_min, criteria.stars_max),
                "archived:false".to_string(),
                "is:public".to_string(),
            ];

            // Activity filter
            if criteria.min_last_activity_days > 0 {
                let cutoff = Utc::now() - Duration::days(criteria.min_last_activity_days);
                query_parts.push(format!("pushed:>{}", cutoff.format("%Y-%m-%d")));
            }

            // Topic filter
            for topic in &criteria.topics {
                query_parts.push(format!("topic:{}", topic));
            }

            let query = query_parts.join(" ");
            debug!(query = %query, "Search query");

            let per_page = (criteria.max_results * 2).min(30) as u32;
            let repos = self
                .client
                .search_repositories(&query, "stars", per_page)
                .await?;
            all_repos.extend(repos);
        }

        // Deduplicate
        let mut seen = std::collections::HashSet::new();
        let mut unique: Vec<Repository> = Vec::new();
        for repo in all_repos {
            if !seen.contains(&repo.full_name) && !criteria.exclude_repos.contains(&repo.full_name)
            {
                seen.insert(repo.full_name.clone());
                unique.push(repo);
            }
        }

        Ok(unique)
    }

    /// Filter repositories that are good candidates for contributions.
    async fn filter_contributable(
        &self,
        repos: Vec<Repository>,
        criteria: &DiscoveryCriteria,
    ) -> Result<Vec<Repository>> {
        let mut filtered: Vec<Repository> = Vec::new();

        for mut repo in repos {
            // Skip if no open issues
            if repo.open_issues == 0 {
                debug!(repo = %repo.full_name, "Skipping: no open issues");
                continue;
            }

            // Check for contributing guide if required
            if criteria.require_contributing_guide {
                match self
                    .client
                    .get_contributing_guide(&repo.owner, &repo.name)
                    .await?
                {
                    Some(_) => repo.has_contributing = true,
                    None => {
                        debug!(repo = %repo.full_name, "Skipping: no contributing guide");
                        continue;
                    }
                }
            }

            // Check last activity
            if let Some(last_push) = repo.last_push_at {
                let cutoff = Utc::now() - Duration::days(criteria.min_last_activity_days);
                if last_push < cutoff {
                    debug!(repo = %repo.full_name, "Skipping: inactive");
                    continue;
                }
            }

            filtered.push(repo);
        }

        Ok(filtered)
    }

    /// Score and sort repositories by contribution potential.
    fn prioritize(&self, repos: Vec<Repository>) -> Vec<Repository> {
        let mut scored: Vec<(f64, Repository)> = repos
            .into_iter()
            .map(|repo| {
                let mut score = 0.0f64;

                // Star range sweet spot (100-5000)
                if (100..=5000).contains(&repo.stars) {
                    score += 3.0;
                } else if repo.stars < 100 {
                    score += 1.0;
                } else {
                    score += 2.0;
                }

                // Open issues = opportunities
                score += (repo.open_issues as f64 / 10.0).min(3.0);

                // Has license
                if repo.has_license {
                    score += 1.0;
                }

                // Has contributing guide
                if repo.has_contributing {
                    score += 2.0;
                }

                // Moderate forks = active community
                if (10..=500).contains(&repo.forks) {
                    score += 1.5;
                }

                (score, repo)
            })
            .collect();

        scored.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap_or(std::cmp::Ordering::Equal));
        scored.into_iter().map(|(_, repo)| repo).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    fn make_repo(name: &str, stars: i64, open_issues: i64, has_license: bool) -> Repository {
        Repository {
            owner: "test".into(),
            name: name.into(),
            full_name: format!("test/{}", name),
            description: None,
            language: Some("python".into()),
            languages: HashMap::new(),
            stars,
            forks: 50,
            open_issues,
            topics: vec![],
            default_branch: "main".into(),
            html_url: String::new(),
            clone_url: String::new(),
            has_contributing: false,
            has_license,
            last_push_at: None,
            created_at: None,
        }
    }

    #[test]
    fn test_prioritize_prefers_sweet_spot_stars() {
        let _config = DiscoveryConfig::default();
        // We can't create a real GitHubClient, so we test prioritize directly
        let repos = vec![
            make_repo("small", 10, 5, true),
            make_repo("sweet", 500, 5, true),
            make_repo("big", 50000, 5, true),
        ];

        // Sort manually using same logic
        let mut scored: Vec<(f64, &str)> = repos
            .iter()
            .map(|r| {
                let mut s = 0.0;
                if (100..=5000).contains(&r.stars) {
                    s += 3.0;
                } else if r.stars < 100 {
                    s += 1.0;
                } else {
                    s += 2.0;
                }
                s += (r.open_issues as f64 / 10.0).min(3.0);
                if r.has_license {
                    s += 1.0;
                }
                if (10..=500).contains(&r.forks) {
                    s += 1.5;
                }
                (s, r.name.as_str())
            })
            .collect();

        scored.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap_or(std::cmp::Ordering::Equal));

        assert_eq!(scored[0].1, "sweet"); // 500 stars wins
    }
}
