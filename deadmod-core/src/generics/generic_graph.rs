//! Generic parameter analysis for dead generic detection.
//!
//! Combines declared generics with their usages to identify:
//! - Unused type parameters (T declared but never used)
//! - Unused lifetimes ('a declared but never referenced)
//! - Unused trait bounds (T: Debug where Debug is never utilized)
//!
//! Analysis is performed per-parent item to ensure accurate detection.

use std::collections::{HashMap, HashSet};

use super::generic_extractor::{DeclaredGeneric, GenericExtractionResult, GenericKind, ParentKind};
use super::generic_usage::GenericUsageResult;

/// A dead generic parameter that was declared but never used.
#[derive(Debug, Clone)]
pub struct DeadGeneric {
    /// The name of the unused generic
    pub name: String,
    /// Kind of generic (type, lifetime, const)
    pub kind: GenericKind,
    /// The parent item containing this generic
    pub parent: String,
    /// The kind of parent item
    pub parent_kind: ParentKind,
    /// Source file
    pub file: String,
    /// Unused bounds on this generic (if any)
    pub unused_bounds: Vec<String>,
}

/// Statistics about generic analysis.
#[derive(Debug, Clone, Default)]
pub struct GenericStats {
    pub total_declared_types: usize,
    pub total_declared_lifetimes: usize,
    pub total_declared_consts: usize,
    pub dead_types: usize,
    pub dead_lifetimes: usize,
    pub dead_consts: usize,
}

/// Result of generic analysis.
#[derive(Debug, Clone)]
pub struct GenericAnalysisResult {
    /// All dead generics found
    pub dead: Vec<DeadGeneric>,
    /// Statistics
    pub stats: GenericStats,
}

/// Graph for analyzing generic parameter usage.
#[derive(Default)]
pub struct GenericGraph {
    /// All declared generics
    declared: Vec<DeclaredGeneric>,
    /// Usages organized by parent
    usages_by_parent: HashMap<String, (HashSet<String>, HashSet<String>)>,
}

impl GenericGraph {
    /// Create a new generic graph from extraction results.
    pub fn new(
        extractions: &[GenericExtractionResult],
        usages: &[GenericUsageResult],
    ) -> Self {
        let mut declared = Vec::new();
        let mut usages_by_parent: HashMap<String, (HashSet<String>, HashSet<String>)> =
            HashMap::new();

        // Collect all declarations
        for extraction in extractions {
            declared.extend(extraction.declared.clone());
        }

        // Collect all usages by parent
        for usage in usages {
            for (parent, parent_usages) in &usage.usages_by_parent {
                let entry = usages_by_parent.entry(parent.clone()).or_default();
                entry.0.extend(parent_usages.used_types.clone());
                entry.1.extend(parent_usages.used_lifetimes.clone());
            }
        }

        Self {
            declared,
            usages_by_parent,
        }
    }

    /// Check if a declared generic is used within its parent scope.
    fn is_generic_used(&self, decl: &DeclaredGeneric) -> bool {
        let Some((used_types, used_lifetimes)) = self.usages_by_parent.get(&decl.parent) else {
            // No usages recorded for this parent - generic is unused
            return false;
        };

        match decl.kind {
            GenericKind::Type => used_types.contains(&decl.name),
            GenericKind::Lifetime => used_lifetimes.contains(&decl.name),
            GenericKind::Const => {
                // Const generics are typically used in array sizes or type-level computation
                // For now, we check if they appear as type arguments
                used_types.contains(&decl.name)
            }
        }
    }

    /// Find all dead generics.
    pub fn find_dead(&self) -> Vec<DeadGeneric> {
        let mut dead = Vec::new();

        for decl in &self.declared {
            if !self.is_generic_used(decl) {
                dead.push(DeadGeneric {
                    name: decl.name.clone(),
                    kind: decl.kind,
                    parent: decl.parent.clone(),
                    parent_kind: decl.parent_kind,
                    file: decl.file.clone(),
                    unused_bounds: decl.bounds.clone(), // All bounds are unused if generic is unused
                });
            }
        }

        // Sort by file, then parent, then name for consistent output
        dead.sort_by(|a, b| {
            a.file
                .cmp(&b.file)
                .then_with(|| a.parent.cmp(&b.parent))
                .then_with(|| a.name.cmp(&b.name))
        });

        dead
    }

    /// Perform complete analysis and return structured result.
    pub fn analyze(&self) -> GenericAnalysisResult {
        let dead = self.find_dead();

        let mut stats = GenericStats::default();

        // Count total declared by kind
        for decl in &self.declared {
            match decl.kind {
                GenericKind::Type => stats.total_declared_types += 1,
                GenericKind::Lifetime => stats.total_declared_lifetimes += 1,
                GenericKind::Const => stats.total_declared_consts += 1,
            }
        }

        // Count dead by kind
        for d in &dead {
            match d.kind {
                GenericKind::Type => stats.dead_types += 1,
                GenericKind::Lifetime => stats.dead_lifetimes += 1,
                GenericKind::Const => stats.dead_consts += 1,
            }
        }

        GenericAnalysisResult { dead, stats }
    }

    /// Get the total number of declared generics.
    pub fn declared_count(&self) -> usize {
        self.declared.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::generics::generic_extractor::extract_declared_generics;
    use crate::generics::generic_usage::extract_generic_usages;
    use std::path::PathBuf;

    fn analyze_code(content: &str) -> GenericAnalysisResult {
        let path = PathBuf::from("test.rs");
        let extractions = vec![extract_declared_generics(&path, content)];
        let usages = vec![extract_generic_usages(&path, content)];
        let graph = GenericGraph::new(&extractions, &usages);
        graph.analyze()
    }

    #[test]
    fn test_unused_type_param() {
        let content = r#"
fn foo<T, U>(x: T) -> T {
    x
}
"#;
        let result = analyze_code(content);

        assert_eq!(result.stats.total_declared_types, 2);
        assert_eq!(result.stats.dead_types, 1);

        let dead_u = result.dead.iter().find(|d| d.name == "U").unwrap();
        assert_eq!(dead_u.parent, "foo");
        assert!(matches!(dead_u.kind, GenericKind::Type));
    }

    #[test]
    fn test_unused_lifetime() {
        let content = r#"
fn bar<'a, 'b>(x: &'a str) -> &'a str {
    x
}
"#;
        let result = analyze_code(content);

        assert_eq!(result.stats.total_declared_lifetimes, 2);
        assert_eq!(result.stats.dead_lifetimes, 1);

        let dead_b = result.dead.iter().find(|d| d.name == "'b").unwrap();
        assert_eq!(dead_b.parent, "bar");
        assert!(matches!(dead_b.kind, GenericKind::Lifetime));
    }

    #[test]
    fn test_all_used() {
        let content = r#"
fn process<T>(x: T) -> T {
    x
}
"#;
        let result = analyze_code(content);

        assert_eq!(result.stats.total_declared_types, 1);
        assert_eq!(result.stats.dead_types, 0);
        assert!(result.dead.is_empty());
    }

    #[test]
    fn test_struct_unused_generic() {
        let content = r#"
struct Wrapper<T, U> {
    data: T,
}
"#;
        let result = analyze_code(content);

        assert_eq!(result.stats.dead_types, 1);

        let dead = &result.dead[0];
        assert_eq!(dead.name, "U");
        assert_eq!(dead.parent, "Wrapper");
    }

    #[test]
    fn test_struct_unused_lifetime() {
        let content = r#"
struct Holder<'a, 'b> {
    data: &'a str,
}
"#;
        let result = analyze_code(content);

        assert_eq!(result.stats.dead_lifetimes, 1);

        let dead = &result.dead[0];
        assert_eq!(dead.name, "'b");
    }

    #[test]
    fn test_enum_unused_generic() {
        let content = r#"
enum Either<L, R, X> {
    Left(L),
    Right(R),
}
"#;
        let result = analyze_code(content);

        assert_eq!(result.stats.dead_types, 1);

        let dead = &result.dead[0];
        assert_eq!(dead.name, "X");
    }

    #[test]
    fn test_impl_unused_generic() {
        let content = r#"
struct Foo<T>(T);

impl<T, U> Foo<T> {
    fn new(val: T) -> Self {
        Foo(val)
    }
}
"#;
        let result = analyze_code(content);

        // U in the impl block is unused
        let dead_u = result.dead.iter().find(|d| d.name == "U");
        assert!(dead_u.is_some());
    }

    #[test]
    fn test_unused_bounds_tracked() {
        let content = r#"
fn unused_bound<T: Debug + Clone, U>(x: U) -> U {
    x
}
"#;
        let result = analyze_code(content);

        let dead_t = result.dead.iter().find(|d| d.name == "T").unwrap();
        assert!(dead_t.unused_bounds.contains(&"Debug".to_string()));
        assert!(dead_t.unused_bounds.contains(&"Clone".to_string()));
    }

    #[test]
    fn test_stats() {
        let content = r#"
fn foo<'a, T, U>(x: &'a T) -> &'a T { x }
struct Bar<'b, V, W> { data: &'b V }
"#;
        let result = analyze_code(content);

        assert_eq!(result.stats.total_declared_types, 4); // T, U, V, W
        assert_eq!(result.stats.total_declared_lifetimes, 2); // 'a, 'b
        assert_eq!(result.stats.dead_types, 2); // U, W
        assert_eq!(result.stats.dead_lifetimes, 0); // both used
    }

    #[test]
    fn test_complex_generic_usage() {
        let content = r#"
fn process<K, V>(map: std::collections::HashMap<K, V>) -> Option<V> {
    None
}
"#;
        let result = analyze_code(content);

        // Both K and V should be detected as used
        assert_eq!(result.stats.dead_types, 0);
    }
}
