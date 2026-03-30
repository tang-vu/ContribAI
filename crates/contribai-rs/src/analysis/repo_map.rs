//! PageRank-based file importance ranking.
//!
//! 🆕 NEW in Rust version — inspired by Aider's repo-map.
//! Builds a file dependency graph from imports and runs PageRank
//! to determine which files are most "important" (central) in the codebase.
//! This helps the analyzer focus on high-impact files first.

use std::collections::HashMap;
use tracing::debug;

/// PageRank parameters.
const DAMPING: f64 = 0.85;
const ITERATIONS: u32 = 20;
const EPSILON: f64 = 1e-6;

/// Build a file importance map using PageRank on import dependencies.
///
/// # Arguments
/// * `import_graph` - Map of file_path → list of imported file names
///
/// # Returns
/// Map of file_path → importance score (0.0 to 1.0, normalized).
pub fn rank_files(import_graph: &HashMap<String, Vec<String>>) -> HashMap<String, f64> {
    let files: Vec<&String> = import_graph.keys().collect();
    let n = files.len();

    if n == 0 {
        return HashMap::new();
    }

    // Build adjacency: if file A imports something with name matching file B, A → B
    let file_index: HashMap<&String, usize> = files.iter().enumerate().map(|(i, f)| (*f, i)).collect();

    // Build edge lists: edges[i] contains indices of files that file i links to
    let mut outlinks: Vec<Vec<usize>> = vec![Vec::new(); n];
    let mut inlinks: Vec<Vec<usize>> = vec![Vec::new(); n];

    for (file, imports) in import_graph {
        let from_idx = match file_index.get(file) {
            Some(&i) => i,
            None => continue,
        };

        for imp in imports {
            // Try to resolve import to a known file
            for (&target_file, &target_idx) in &file_index {
                if is_import_match(imp, target_file) && target_idx != from_idx {
                    outlinks[from_idx].push(target_idx);
                    inlinks[target_idx].push(from_idx);
                }
            }
        }
    }

    // Initialize PageRank
    let init = 1.0 / n as f64;
    let mut ranks = vec![init; n];
    let mut new_ranks = vec![0.0; n];

    // Iterate
    for iter in 0..ITERATIONS {
        let teleport = (1.0 - DAMPING) / n as f64;
        let mut max_diff = 0.0f64;

        for i in 0..n {
            let mut sum = 0.0;
            for &j in &inlinks[i] {
                let out_degree = outlinks[j].len();
                if out_degree > 0 {
                    sum += ranks[j] / out_degree as f64;
                }
            }
            new_ranks[i] = teleport + DAMPING * sum;
            max_diff = max_diff.max((new_ranks[i] - ranks[i]).abs());
        }

        std::mem::swap(&mut ranks, &mut new_ranks);

        if max_diff < EPSILON {
            debug!(iterations = iter + 1, "PageRank converged");
            break;
        }
    }

    // Normalize to 0.0-1.0
    let max_rank = ranks.iter().cloned().fold(0.0f64, f64::max);
    if max_rank > 0.0 {
        for r in &mut ranks {
            *r /= max_rank;
        }
    }

    // Build result map
    files
        .into_iter()
        .enumerate()
        .map(|(i, f)| (f.clone(), ranks[i]))
        .collect()
}

/// Check if an import name matches a file path.
///
/// Handles common patterns:
/// - `from pathlib import Path` → matches `pathlib.py`
/// - `import os` → matches `os.py`
/// - `from src.utils import helper` → matches `src/utils.py`
fn is_import_match(import_name: &str, file_path: &str) -> bool {
    let normalized_import = import_name
        .replace('.', "/")
        .replace("from ", "")
        .replace("import ", "")
        .trim()
        .to_string();

    // Extract the module path part (first segment of import)
    let module = normalized_import
        .split('/')
        .next()
        .unwrap_or(&normalized_import);

    // Check if file_path contains the module name
    let stem = file_path
        .rsplit('/')
        .next()
        .unwrap_or(file_path)
        .rsplit('.')
        .last()
        .unwrap_or("");

    let path_without_ext = file_path
        .rsplit_once('.')
        .map(|(p, _)| p)
        .unwrap_or(file_path);

    // Exact stem match or path contains the module
    stem == module || path_without_ext.ends_with(module) || path_without_ext.replace('/', ".").contains(&normalized_import)
}

/// Select top N files by PageRank score.
pub fn top_files(ranks: &HashMap<String, f64>, n: usize) -> Vec<(&String, f64)> {
    let mut sorted: Vec<_> = ranks.iter().map(|(k, v)| (k, *v)).collect();
    sorted.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
    sorted.into_iter().take(n).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_rank_files_simple() {
        let mut graph = HashMap::new();
        // main.py imports utils and config
        graph.insert(
            "main.py".to_string(),
            vec!["utils".to_string(), "config".to_string()],
        );
        // utils.py imports config
        graph.insert(
            "utils.py".to_string(),
            vec!["config".to_string()],
        );
        // config.py imports nothing
        graph.insert("config.py".to_string(), vec![]);

        let ranks = rank_files(&graph);

        assert_eq!(ranks.len(), 3);
        // config.py should rank highest (most imported)
        assert!(
            ranks["config.py"] >= ranks["utils.py"],
            "config should rank >= utils (config={:.3}, utils={:.3})",
            ranks["config.py"],
            ranks["utils.py"]
        );
    }

    #[test]
    fn test_rank_files_empty() {
        let graph = HashMap::new();
        let ranks = rank_files(&graph);
        assert!(ranks.is_empty());
    }

    #[test]
    fn test_top_files() {
        let mut ranks = HashMap::new();
        ranks.insert("a.py".to_string(), 0.9);
        ranks.insert("b.py".to_string(), 0.3);
        ranks.insert("c.py".to_string(), 0.7);

        let top = top_files(&ranks, 2);
        assert_eq!(top.len(), 2);
        assert_eq!(top[0].0, "a.py");
        assert_eq!(top[1].0, "c.py");
    }

    #[test]
    fn test_is_import_match() {
        assert!(is_import_match("config", "config.py"));
        assert!(is_import_match("utils", "utils.py"));
        assert!(is_import_match("config", "src/config.py"));
        assert!(!is_import_match("config", "main.py"));
    }
}
