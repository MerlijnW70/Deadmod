//! Enum variant graph construction and dead variant detection.
//!
//! Builds a graph of enum variant definitions and identifies unused variants.
//!
//! Entry points (always considered reachable):
//! - Variants of public enums (could be used by external crates)
//!
//! Performance characteristics:
//! - Graph build: O(|V| + |U|) where V = variants, U = usages
//! - Detection: O(|V|) single pass

use std::collections::HashSet;

use super::enum_extractor::EnumVariantDef;
use super::enum_usage::EnumUsageResult;

/// A dead enum variant that was declared but never used.
#[derive(Debug, Clone)]
pub struct DeadVariant {
    /// The parent enum name
    pub enum_name: String,
    /// The variant name
    pub variant_name: String,
    /// Full qualified name (Enum::Variant)
    pub full_name: String,
    /// Source file
    pub file: String,
    /// Module path
    pub module_path: String,
    /// Visibility of parent enum
    pub visibility: String,
}

/// Statistics about enum variant analysis.
#[derive(Debug, Clone, Default)]
pub struct EnumStats {
    pub total_variants: usize,
    pub total_enums: usize,
    pub dead_variant_count: usize,
    pub dead_enum_count: usize, // enums where ALL variants are dead
}

/// Result of enum analysis.
#[derive(Debug, Clone)]
pub struct EnumAnalysisResult {
    /// All dead variants found
    pub dead: Vec<DeadVariant>,
    /// Statistics
    pub stats: EnumStats,
}

/// Graph for analyzing enum variant usage.
#[derive(Default)]
pub struct EnumGraph {
    /// All declared variants
    declared: Vec<EnumVariantDef>,
    /// Set of used variant names
    used_variants: HashSet<String>,
    /// Set of used full paths like "Enum::Variant"
    used_full_paths: HashSet<String>,
}

impl EnumGraph {
    /// Create a new enum graph from extraction results.
    pub fn new(declared: Vec<EnumVariantDef>, usages: &[EnumUsageResult]) -> Self {
        let mut used_variants = HashSet::new();
        let mut used_full_paths = HashSet::new();

        for usage in usages {
            used_variants.extend(usage.used_variants.clone());
            used_full_paths.extend(usage.used_full_paths.clone());
        }

        Self {
            declared,
            used_variants,
            used_full_paths,
        }
    }

    /// Check if a variant is used.
    fn is_variant_used(&self, variant: &EnumVariantDef) -> bool {
        // Check by variant name (simple match)
        if self.used_variants.contains(&variant.variant_name) {
            return true;
        }

        // Check by full path (Enum::Variant)
        if self.used_full_paths.contains(&variant.full_name) {
            return true;
        }

        false
    }

    /// Find all dead variants.
    ///
    /// Note: Variants of public enums are still reported as dead if unused,
    /// but can be filtered by the caller based on visibility.
    pub fn find_dead(&self) -> Vec<DeadVariant> {
        let mut dead = Vec::new();

        for variant in &self.declared {
            if !self.is_variant_used(variant) {
                dead.push(DeadVariant {
                    enum_name: variant.enum_name.clone(),
                    variant_name: variant.variant_name.clone(),
                    full_name: variant.full_name.clone(),
                    file: variant.file.clone(),
                    module_path: variant.module_path.clone(),
                    visibility: variant.visibility.clone(),
                });
            }
        }

        // Sort by file, then full_name for consistent output
        dead.sort_by(|a, b| a.file.cmp(&b.file).then_with(|| a.full_name.cmp(&b.full_name)));

        dead
    }

    /// Perform complete analysis and return structured result.
    pub fn analyze(&self) -> EnumAnalysisResult {
        let dead = self.find_dead();

        // Count unique enums
        let unique_enums: HashSet<_> = self.declared.iter().map(|v| &v.enum_name).collect();

        // Count enums where ALL variants are dead
        let _dead_enums: HashSet<_> = dead.iter().map(|v| &v.enum_name).collect();
        let mut fully_dead_enum_count = 0;
        for enum_name in &unique_enums {
            let total = self
                .declared
                .iter()
                .filter(|v| &v.enum_name == *enum_name)
                .count();
            let dead_count = dead.iter().filter(|v| &v.enum_name == *enum_name).count();
            if total == dead_count && total > 0 {
                fully_dead_enum_count += 1;
            }
        }

        let stats = EnumStats {
            total_variants: self.declared.len(),
            total_enums: unique_enums.len(),
            dead_variant_count: dead.len(),
            dead_enum_count: fully_dead_enum_count,
        };

        EnumAnalysisResult { dead, stats }
    }

    /// Get the total number of declared variants.
    pub fn declared_count(&self) -> usize {
        self.declared.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_variant(enum_name: &str, variant_name: &str, file: &str) -> EnumVariantDef {
        EnumVariantDef {
            enum_name: enum_name.to_string(),
            variant_name: variant_name.to_string(),
            full_name: format!("{}::{}", enum_name, variant_name),
            file: file.to_string(),
            module_path: String::new(),
            visibility: "private".to_string(),
        }
    }

    #[test]
    fn test_unused_variant_is_dead() {
        let declared = vec![
            make_variant("Color", "Red", "test.rs"),
            make_variant("Color", "Green", "test.rs"),
            make_variant("Color", "Blue", "test.rs"),
        ];

        let usages = vec![EnumUsageResult {
            used_variants: HashSet::from(["Red".to_string(), "Green".to_string()]),
            used_full_paths: HashSet::new(),
        }];

        let graph = EnumGraph::new(declared, &usages);
        let result = graph.analyze();

        assert_eq!(result.stats.total_variants, 3);
        assert_eq!(result.stats.dead_variant_count, 1);
        assert_eq!(result.dead[0].variant_name, "Blue");
    }

    #[test]
    fn test_used_by_full_path() {
        let declared = vec![make_variant("Status", "Active", "test.rs")];

        let usages = vec![EnumUsageResult {
            used_variants: HashSet::new(),
            used_full_paths: HashSet::from(["Status::Active".to_string()]),
        }];

        let graph = EnumGraph::new(declared, &usages);
        let result = graph.analyze();

        assert_eq!(result.stats.dead_variant_count, 0);
    }

    #[test]
    fn test_all_used() {
        let declared = vec![
            make_variant("E", "A", "test.rs"),
            make_variant("E", "B", "test.rs"),
        ];

        let usages = vec![EnumUsageResult {
            used_variants: HashSet::from(["A".to_string(), "B".to_string()]),
            used_full_paths: HashSet::new(),
        }];

        let graph = EnumGraph::new(declared, &usages);
        let result = graph.analyze();

        assert_eq!(result.stats.dead_variant_count, 0);
        assert!(result.dead.is_empty());
    }

    #[test]
    fn test_fully_dead_enum() {
        let declared = vec![
            make_variant("DeadEnum", "A", "test.rs"),
            make_variant("DeadEnum", "B", "test.rs"),
            make_variant("AliveEnum", "X", "test.rs"),
        ];

        let usages = vec![EnumUsageResult {
            used_variants: HashSet::from(["X".to_string()]),
            used_full_paths: HashSet::new(),
        }];

        let graph = EnumGraph::new(declared, &usages);
        let result = graph.analyze();

        assert_eq!(result.stats.total_enums, 2);
        assert_eq!(result.stats.dead_variant_count, 2);
        assert_eq!(result.stats.dead_enum_count, 1); // DeadEnum is fully dead
    }

    #[test]
    fn test_stats() {
        let declared = vec![
            make_variant("E1", "A", "test.rs"),
            make_variant("E1", "B", "test.rs"),
            make_variant("E2", "X", "test.rs"),
            make_variant("E2", "Y", "test.rs"),
            make_variant("E2", "Z", "test.rs"),
        ];

        let usages = vec![EnumUsageResult {
            used_variants: HashSet::from(["A".to_string(), "X".to_string()]),
            used_full_paths: HashSet::new(),
        }];

        let graph = EnumGraph::new(declared, &usages);
        let result = graph.analyze();

        assert_eq!(result.stats.total_variants, 5);
        assert_eq!(result.stats.total_enums, 2);
        assert_eq!(result.stats.dead_variant_count, 3); // B, Y, Z
        assert_eq!(result.stats.dead_enum_count, 0); // Both have at least one used
    }
}
