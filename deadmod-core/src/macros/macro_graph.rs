//! Macro call graph construction and dead macro detection.
//!
//! Builds a graph of macro definitions and identifies unused macros.
//!
//! Entry points (always considered reachable):
//! - `#[macro_export]` macros (could be used by external crates)
//!
//! Performance characteristics:
//! - Graph build: O(|M| + |U|) where M = macros, U = usages
//! - Detection: O(|M|) single pass

use std::collections::HashSet;

use super::macro_extractor::MacroDef;
use super::macro_usage::MacroUsageResult;

/// A dead macro that was declared but never used.
#[derive(Debug, Clone)]
pub struct DeadMacro {
    /// The name of the unused macro
    pub name: String,
    /// Whether it was marked #[macro_export]
    pub exported: bool,
    /// Source file
    pub file: String,
    /// Module path
    pub module_path: String,
}

/// Statistics about macro analysis.
#[derive(Debug, Clone, Default)]
pub struct MacroStats {
    pub total_declared: usize,
    pub exported_count: usize,
    pub dead_count: usize,
    pub dead_exported_count: usize,
}

/// Result of macro analysis.
#[derive(Debug, Clone)]
pub struct MacroAnalysisResult {
    /// All dead macros found
    pub dead: Vec<DeadMacro>,
    /// Statistics
    pub stats: MacroStats,
}

/// Graph for analyzing macro usage.
#[derive(Default)]
pub struct MacroGraph {
    /// All declared macros
    declared: Vec<MacroDef>,
    /// Set of used macro names
    used: HashSet<String>,
}

impl MacroGraph {
    /// Create a new macro graph from extraction results.
    pub fn new(declared: Vec<MacroDef>, usages: &[MacroUsageResult]) -> Self {
        let mut used = HashSet::new();

        for usage in usages {
            used.extend(usage.used_macros.clone());
        }

        Self { declared, used }
    }

    /// Check if a macro is used.
    fn is_macro_used(&self, mac: &MacroDef) -> bool {
        // Check if the macro name appears in the used set
        self.used.contains(&mac.name)
    }

    /// Find all dead macros.
    ///
    /// Note: Exported macros are still reported as dead if unused within the crate,
    /// but marked as exported so the caller can decide whether to report them.
    pub fn find_dead(&self) -> Vec<DeadMacro> {
        let mut dead = Vec::new();

        for mac in &self.declared {
            if !self.is_macro_used(mac) {
                dead.push(DeadMacro {
                    name: mac.name.clone(),
                    exported: mac.exported,
                    file: mac.file.clone(),
                    module_path: mac.module_path.clone(),
                });
            }
        }

        // Sort by file, then name for consistent output
        dead.sort_by(|a, b| a.file.cmp(&b.file).then_with(|| a.name.cmp(&b.name)));

        dead
    }

    /// Perform complete analysis and return structured result.
    pub fn analyze(&self) -> MacroAnalysisResult {
        let dead = self.find_dead();

        let stats = MacroStats {
            total_declared: self.declared.len(),
            exported_count: self.declared.iter().filter(|m| m.exported).count(),
            dead_count: dead.len(),
            dead_exported_count: dead.iter().filter(|m| m.exported).count(),
        };

        MacroAnalysisResult { dead, stats }
    }

    /// Get the total number of declared macros.
    pub fn declared_count(&self) -> usize {
        self.declared.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_macro(name: &str, exported: bool, file: &str) -> MacroDef {
        MacroDef {
            name: name.to_string(),
            exported,
            file: file.to_string(),
            module_path: String::new(),
        }
    }

    #[test]
    fn test_unused_macro_is_dead() {
        let declared = vec![
            make_macro("used_macro", false, "test.rs"),
            make_macro("unused_macro", false, "test.rs"),
        ];

        let usages = vec![MacroUsageResult {
            used_macros: HashSet::from(["used_macro".to_string()]),
        }];

        let graph = MacroGraph::new(declared, &usages);
        let result = graph.analyze();

        assert_eq!(result.stats.total_declared, 2);
        assert_eq!(result.stats.dead_count, 1);
        assert_eq!(result.dead[0].name, "unused_macro");
    }

    #[test]
    fn test_exported_unused_still_dead() {
        let declared = vec![make_macro("exported_unused", true, "test.rs")];

        let usages = vec![MacroUsageResult::default()];

        let graph = MacroGraph::new(declared, &usages);
        let result = graph.analyze();

        assert_eq!(result.stats.dead_count, 1);
        assert!(result.dead[0].exported);
    }

    #[test]
    fn test_all_used() {
        let declared = vec![
            make_macro("foo", false, "test.rs"),
            make_macro("bar", true, "test.rs"),
        ];

        let usages = vec![MacroUsageResult {
            used_macros: HashSet::from(["foo".to_string(), "bar".to_string()]),
        }];

        let graph = MacroGraph::new(declared, &usages);
        let result = graph.analyze();

        assert_eq!(result.stats.dead_count, 0);
        assert!(result.dead.is_empty());
    }

    #[test]
    fn test_stats() {
        let declared = vec![
            make_macro("m1", false, "test.rs"),
            make_macro("m2", true, "test.rs"),
            make_macro("m3", true, "test.rs"),
        ];

        let usages = vec![MacroUsageResult {
            used_macros: HashSet::from(["m1".to_string()]),
        }];

        let graph = MacroGraph::new(declared, &usages);
        let result = graph.analyze();

        assert_eq!(result.stats.total_declared, 3);
        assert_eq!(result.stats.exported_count, 2);
        assert_eq!(result.stats.dead_count, 2);
        assert_eq!(result.stats.dead_exported_count, 2);
    }
}
