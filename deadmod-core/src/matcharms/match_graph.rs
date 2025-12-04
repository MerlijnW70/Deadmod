//! Match arm graph construction and dead match arm detection.
//!
//! Builds a graph of match arms and identifies:
//! - Unreachable arms (wildcards masking later arms)
//! - Dead arms (patterns never matched)
//!
//! Performance characteristics:
//! - Graph build: O(|A| + |U|) where A = arms, U = usages
//! - Detection: O(|A|) single pass

use std::collections::HashSet;

use super::match_extractor::MatchArm;
use super::match_usage::MatchUsageResult;

/// A potentially dead match arm.
#[derive(Debug, Clone)]
pub struct DeadMatchArm {
    /// The pattern that is potentially dead
    pub pattern: String,
    /// Reason why this arm is considered dead
    pub reason: DeadArmReason,
    /// Source file
    pub file: String,
}

/// Reason why a match arm is considered dead.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DeadArmReason {
    /// The pattern's variant is never used/constructed
    NeverUsed,
    /// A wildcard pattern before this arm makes it unreachable
    MaskedByWildcard,
    /// A wildcard in non-final position (potential issue)
    NonFinalWildcard,
}

/// Statistics about match arm analysis.
#[derive(Debug, Clone, Default)]
pub struct MatchArmStats {
    pub total_match_expressions: usize,
    pub total_arms: usize,
    pub wildcard_count: usize,
    pub dead_arm_count: usize,
    pub masked_arm_count: usize,
}

/// Result of match arm analysis.
#[derive(Debug, Clone)]
pub struct MatchArmAnalysisResult {
    /// All potentially dead arms found
    pub dead_arms: Vec<DeadMatchArm>,
    /// Statistics
    pub stats: MatchArmStats,
}

/// Graph for analyzing match arm usage.
#[derive(Default)]
pub struct MatchGraph {
    /// All extracted match arms
    arms: Vec<MatchArm>,
    /// Total match expression count
    match_count: usize,
    /// Set of used variant names (for future NeverUsed detection)
    #[allow(dead_code)]
    used_variants: HashSet<String>,
    /// Set of used full paths (for future NeverUsed detection)
    #[allow(dead_code)]
    used_full_paths: HashSet<String>,
}

impl MatchGraph {
    /// Create a new match graph from extraction results.
    pub fn new(
        arms: Vec<MatchArm>,
        match_count: usize,
        usages: &[MatchUsageResult],
    ) -> Self {
        let mut used_variants = HashSet::new();
        let mut used_full_paths = HashSet::new();

        for usage in usages {
            used_variants.extend(usage.used_variants.clone());
            used_full_paths.extend(usage.used_full_paths.clone());
        }

        Self {
            arms,
            match_count,
            used_variants,
            used_full_paths,
        }
    }

    /// Check if a variant pattern is used anywhere (constructed or matched).
    /// Reserved for future NeverUsed detection.
    #[allow(dead_code)]
    fn is_variant_used(&self, variant_name: &str) -> bool {
        self.used_variants.contains(variant_name)
    }

    /// Find arms that might be masked by wildcards.
    ///
    /// Returns arms that come after a wildcard in a match expression.
    fn find_masked_arms(&self) -> Vec<DeadMatchArm> {
        let mut dead = Vec::new();

        // Group arms by file and position to detect wildcard masking
        let mut current_match: Vec<&MatchArm> = Vec::new();
        let mut prev_file = String::new();
        let mut prev_total = 0;

        for arm in &self.arms {
            // Detect if we've moved to a new match expression
            if arm.file != prev_file || arm.total_arms != prev_total || arm.position == 0 {
                // Process previous match expression
                self.check_wildcard_masking(&current_match, &mut dead);
                current_match.clear();
            }

            current_match.push(arm);
            prev_file = arm.file.clone();
            prev_total = arm.total_arms;
        }

        // Process last match expression
        self.check_wildcard_masking(&current_match, &mut dead);

        dead
    }

    fn check_wildcard_masking(&self, arms: &[&MatchArm], dead: &mut Vec<DeadMatchArm>) {
        let mut found_wildcard = false;
        let mut wildcard_pos = 0;

        for (i, arm) in arms.iter().enumerate() {
            if arm.is_wildcard {
                if i < arms.len() - 1 {
                    // Wildcard not in final position
                    dead.push(DeadMatchArm {
                        pattern: arm.pattern.clone(),
                        reason: DeadArmReason::NonFinalWildcard,
                        file: arm.file.clone(),
                    });
                }
                found_wildcard = true;
                wildcard_pos = i;
            } else if found_wildcard && i > wildcard_pos {
                // This arm comes after a wildcard
                dead.push(DeadMatchArm {
                    pattern: arm.pattern.clone(),
                    reason: DeadArmReason::MaskedByWildcard,
                    file: arm.file.clone(),
                });
            }
        }
    }

    /// Find all dead match arms.
    pub fn find_dead(&self) -> Vec<DeadMatchArm> {
        let mut dead = Vec::new();

        // Find masked arms (wildcards in wrong position)
        dead.extend(self.find_masked_arms());

        // Note: "NeverUsed" detection would require knowing ALL possible
        // enum variants, which we don't have from just match arm analysis.
        // This would need to be combined with enum variant extraction.

        // Sort for consistent output
        dead.sort_by(|a, b| a.file.cmp(&b.file).then_with(|| a.pattern.cmp(&b.pattern)));

        dead
    }

    /// Perform complete analysis and return structured result.
    pub fn analyze(&self) -> MatchArmAnalysisResult {
        let dead = self.find_dead();

        let wildcard_count = self.arms.iter().filter(|a| a.is_wildcard).count();
        let masked_count = dead
            .iter()
            .filter(|d| d.reason == DeadArmReason::MaskedByWildcard)
            .count();

        let stats = MatchArmStats {
            total_match_expressions: self.match_count,
            total_arms: self.arms.len(),
            wildcard_count,
            dead_arm_count: dead.len(),
            masked_arm_count: masked_count,
        };

        MatchArmAnalysisResult { dead_arms: dead, stats }
    }

    /// Get total number of match arms.
    pub fn arm_count(&self) -> usize {
        self.arms.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::matcharms::match_extractor::MatchArm;

    fn make_arm(pattern: &str, variant: Option<&str>, is_wild: bool, pos: usize, total: usize, file: &str) -> MatchArm {
        MatchArm {
            pattern: pattern.to_string(),
            variant_name: variant.map(|s| s.to_string()),
            is_wildcard: is_wild,
            position: pos,
            total_arms: total,
            file: file.to_string(),
        }
    }

    #[test]
    fn test_wildcard_in_final_position_ok() {
        let arms = vec![
            make_arm("Color::Red", Some("Red"), false, 0, 3, "test.rs"),
            make_arm("Color::Green", Some("Green"), false, 1, 3, "test.rs"),
            make_arm("_", None, true, 2, 3, "test.rs"),
        ];

        let graph = MatchGraph::new(arms, 1, &[]);
        let result = graph.analyze();

        // Wildcard in final position is fine
        assert_eq!(result.dead_arms.len(), 0);
    }

    #[test]
    fn test_wildcard_masks_later_arms() {
        let arms = vec![
            make_arm("Color::Red", Some("Red"), false, 0, 4, "test.rs"),
            make_arm("_", None, true, 1, 4, "test.rs"),
            make_arm("Color::Green", Some("Green"), false, 2, 4, "test.rs"),
            make_arm("Color::Blue", Some("Blue"), false, 3, 4, "test.rs"),
        ];

        let graph = MatchGraph::new(arms, 1, &[]);
        let result = graph.analyze();

        // Wildcard not in final position + masked arms
        assert!(result.dead_arms.len() >= 2);
        assert!(result.dead_arms.iter().any(|d| d.reason == DeadArmReason::NonFinalWildcard));
        assert!(result.dead_arms.iter().any(|d| d.reason == DeadArmReason::MaskedByWildcard));
    }

    #[test]
    fn test_all_variants_used() {
        let arms = vec![
            make_arm("Color::Red", Some("Red"), false, 0, 3, "test.rs"),
            make_arm("Color::Green", Some("Green"), false, 1, 3, "test.rs"),
            make_arm("Color::Blue", Some("Blue"), false, 2, 3, "test.rs"),
        ];

        let usages = MatchUsageResult {
            used_variants: HashSet::from([
                "Red".to_string(),
                "Green".to_string(),
                "Blue".to_string(),
            ]),
            used_full_paths: HashSet::new(),
        };

        let graph = MatchGraph::new(arms, 1, &[usages]);
        let result = graph.analyze();

        assert_eq!(result.dead_arms.len(), 0);
    }

    #[test]
    fn test_stats() {
        let arms = vec![
            make_arm("A", Some("A"), false, 0, 4, "test.rs"),
            make_arm("B", Some("B"), false, 1, 4, "test.rs"),
            make_arm("C", Some("C"), false, 2, 4, "test.rs"),
            make_arm("_", None, true, 3, 4, "test.rs"),
        ];

        let graph = MatchGraph::new(arms, 1, &[]);
        let result = graph.analyze();

        assert_eq!(result.stats.total_match_expressions, 1);
        assert_eq!(result.stats.total_arms, 4);
        assert_eq!(result.stats.wildcard_count, 1);
    }

    #[test]
    fn test_multiple_match_expressions() {
        let arms = vec![
            // First match
            make_arm("A", Some("A"), false, 0, 2, "test.rs"),
            make_arm("_", None, true, 1, 2, "test.rs"),
            // Second match (reset position)
            make_arm("X", Some("X"), false, 0, 2, "test.rs"),
            make_arm("_", None, true, 1, 2, "test.rs"),
        ];

        let graph = MatchGraph::new(arms, 2, &[]);
        let result = graph.analyze();

        // Both wildcards are in final position - should be OK
        assert_eq!(result.dead_arms.len(), 0);
    }
}
