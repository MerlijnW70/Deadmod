//! Constant graph construction and dead constant detection.
//!
//! Builds a graph of constant definitions and identifies unused constants.
//!
//! Entry points (always considered reachable):
//! - `pub` constants (could be used by external crates)
//!
//! Performance characteristics:
//! - Graph build: O(|C| + |U|) where C = constants, U = usages
//! - Detection: O(|C|) single pass

use std::collections::HashSet;

use super::const_extractor::ConstDef;
use super::const_usage::ConstUsageResult;

/// A dead constant that was declared but never used.
#[derive(Debug, Clone)]
pub struct DeadConst {
    /// The name of the unused constant
    pub name: String,
    /// Whether it's a static (vs const)
    pub is_static: bool,
    /// Visibility
    pub visibility: String,
    /// Source file
    pub file: String,
    /// Module path
    pub module_path: String,
    /// Impl type if applicable
    pub impl_type: Option<String>,
}

/// Statistics about constant analysis.
#[derive(Debug, Clone, Default)]
pub struct ConstStats {
    pub total_declared: usize,
    pub const_count: usize,
    pub static_count: usize,
    pub dead_count: usize,
    pub dead_const_count: usize,
    pub dead_static_count: usize,
}

/// Result of constant analysis.
#[derive(Debug, Clone)]
pub struct ConstAnalysisResult {
    /// All dead constants found
    pub dead: Vec<DeadConst>,
    /// Statistics
    pub stats: ConstStats,
}

/// Graph for analyzing constant usage.
#[derive(Default)]
pub struct ConstGraph {
    /// All declared constants
    declared: Vec<ConstDef>,
    /// Set of used constant names
    used: HashSet<String>,
}

impl ConstGraph {
    /// Create a new constant graph from extraction results.
    pub fn new(declared: Vec<ConstDef>, usages: &[ConstUsageResult]) -> Self {
        let mut used = HashSet::new();

        for usage in usages {
            used.extend(usage.used_constants.clone());
        }

        Self { declared, used }
    }

    /// Check if a constant is used.
    fn is_const_used(&self, c: &ConstDef) -> bool {
        self.used.contains(&c.name)
    }

    /// Find all dead constants.
    ///
    /// Note: Public constants are still reported as dead if unused within the crate,
    /// but can be filtered by the caller based on visibility.
    pub fn find_dead(&self) -> Vec<DeadConst> {
        let mut dead = Vec::new();

        for c in &self.declared {
            if !self.is_const_used(c) {
                dead.push(DeadConst {
                    name: c.name.clone(),
                    is_static: c.is_static,
                    visibility: c.visibility.clone(),
                    file: c.file.clone(),
                    module_path: c.module_path.clone(),
                    impl_type: c.impl_type.clone(),
                });
            }
        }

        // Sort by file, then name for consistent output
        dead.sort_by(|a, b| a.file.cmp(&b.file).then_with(|| a.name.cmp(&b.name)));

        dead
    }

    /// Perform complete analysis and return structured result.
    pub fn analyze(&self) -> ConstAnalysisResult {
        let dead = self.find_dead();

        let stats = ConstStats {
            total_declared: self.declared.len(),
            const_count: self.declared.iter().filter(|c| !c.is_static).count(),
            static_count: self.declared.iter().filter(|c| c.is_static).count(),
            dead_count: dead.len(),
            dead_const_count: dead.iter().filter(|c| !c.is_static).count(),
            dead_static_count: dead.iter().filter(|c| c.is_static).count(),
        };

        ConstAnalysisResult { dead, stats }
    }

    /// Get the total number of declared constants.
    pub fn declared_count(&self) -> usize {
        self.declared.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_const(name: &str, is_static: bool, file: &str) -> ConstDef {
        ConstDef {
            name: name.to_string(),
            file: file.to_string(),
            is_static,
            is_mutable: false,
            visibility: "private".to_string(),
            module_path: String::new(),
            impl_type: None,
        }
    }

    #[test]
    fn test_unused_const_is_dead() {
        let declared = vec![
            make_const("USED_CONST", false, "test.rs"),
            make_const("UNUSED_CONST", false, "test.rs"),
        ];

        let usages = vec![ConstUsageResult {
            used_constants: HashSet::from(["USED_CONST".to_string()]),
        }];

        let graph = ConstGraph::new(declared, &usages);
        let result = graph.analyze();

        assert_eq!(result.stats.total_declared, 2);
        assert_eq!(result.stats.dead_count, 1);
        assert_eq!(result.dead[0].name, "UNUSED_CONST");
    }

    #[test]
    fn test_unused_static_is_dead() {
        let declared = vec![
            make_const("USED_STATIC", true, "test.rs"),
            make_const("UNUSED_STATIC", true, "test.rs"),
        ];

        let usages = vec![ConstUsageResult {
            used_constants: HashSet::from(["USED_STATIC".to_string()]),
        }];

        let graph = ConstGraph::new(declared, &usages);
        let result = graph.analyze();

        assert_eq!(result.stats.dead_count, 1);
        assert_eq!(result.dead[0].name, "UNUSED_STATIC");
        assert!(result.dead[0].is_static);
    }

    #[test]
    fn test_all_used() {
        let declared = vec![
            make_const("CONST_A", false, "test.rs"),
            make_const("STATIC_B", true, "test.rs"),
        ];

        let usages = vec![ConstUsageResult {
            used_constants: HashSet::from(["CONST_A".to_string(), "STATIC_B".to_string()]),
        }];

        let graph = ConstGraph::new(declared, &usages);
        let result = graph.analyze();

        assert_eq!(result.stats.dead_count, 0);
        assert!(result.dead.is_empty());
    }

    #[test]
    fn test_stats() {
        let declared = vec![
            make_const("C1", false, "test.rs"),
            make_const("C2", false, "test.rs"),
            make_const("S1", true, "test.rs"),
        ];

        let usages = vec![ConstUsageResult {
            used_constants: HashSet::from(["C1".to_string()]),
        }];

        let graph = ConstGraph::new(declared, &usages);
        let result = graph.analyze();

        assert_eq!(result.stats.total_declared, 3);
        assert_eq!(result.stats.const_count, 2);
        assert_eq!(result.stats.static_count, 1);
        assert_eq!(result.stats.dead_count, 2);
        assert_eq!(result.stats.dead_const_count, 1);
        assert_eq!(result.stats.dead_static_count, 1);
    }
}
