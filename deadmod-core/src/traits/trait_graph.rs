//! Trait method call graph construction and dead trait method detection.
//!
//! Builds a graph of trait method definitions and identifies unused methods.
//!
//! Entry points (always considered reachable):
//! - Required trait methods (no default body) - must be implemented
//! - Public trait methods that could be called externally
//! - Methods called from main() or pub functions
//!
//! Performance characteristics:
//! - Graph build: O(|M| + |U|) where M = methods, U = usages
//! - Reachability: O(|M| + |E|) single BFS traversal

use std::collections::{HashMap, HashSet};

use super::trait_extractor::{InherentImplMethod, TraitExtractionResult, TraitImplMethod, TraitMethodDef};
use super::trait_usage::TraitMethodUsage;

/// Result of trait method dead code analysis.
#[derive(Debug, Clone)]
pub struct TraitAnalysisResult {
    /// All trait method definitions found
    pub all_trait_methods: Vec<TraitMethodDef>,
    /// All trait impl methods found
    pub all_impl_methods: Vec<TraitImplMethod>,
    /// All inherent impl methods found
    pub all_inherent_methods: Vec<InherentImplMethod>,
    /// Dead (unreachable) trait method definitions
    pub dead_trait_methods: Vec<TraitMethodDef>,
    /// Dead (unreachable) impl methods
    pub dead_impl_methods: Vec<TraitImplMethod>,
    /// Dead (unreachable) inherent impl methods
    pub dead_inherent_methods: Vec<InherentImplMethod>,
    /// Statistics
    pub stats: TraitStats,
}

/// Statistics about trait method analysis.
#[derive(Debug, Clone, Default)]
pub struct TraitStats {
    pub total_trait_methods: usize,
    pub total_impl_methods: usize,
    pub total_inherent_methods: usize,
    pub dead_trait_method_count: usize,
    pub dead_impl_method_count: usize,
    pub dead_inherent_method_count: usize,
    pub required_methods: usize,
    pub provided_methods: usize,
}

/// Trait method call graph for dead code detection.
pub struct TraitGraph {
    /// Map from full_path to TraitMethodDef
    trait_methods: HashMap<String, TraitMethodDef>,
    /// Map from full_id to TraitImplMethod
    impl_methods: HashMap<String, TraitImplMethod>,
    /// Map from full_id to InherentImplMethod
    inherent_methods: HashMap<String, InherentImplMethod>,
    /// Set of method names that are called
    called_methods: HashSet<String>,
    /// Map from trait_name::method_name to usages
    method_usages: HashMap<String, Vec<TraitMethodUsage>>,
}

impl TraitGraph {
    /// Create a new empty trait graph.
    pub fn new() -> Self {
        Self {
            trait_methods: HashMap::new(),
            impl_methods: HashMap::new(),
            inherent_methods: HashMap::new(),
            called_methods: HashSet::new(),
            method_usages: HashMap::new(),
        }
    }

    /// Build the trait method graph from extracted data.
    ///
    /// # Arguments
    /// * `extractions` - All trait extractions from the codebase
    /// * `usages` - All trait method usages found
    pub fn build(
        extractions: &[TraitExtractionResult],
        usages: &[HashSet<TraitMethodUsage>],
    ) -> Self {
        let mut graph = Self::new();

        // Add all trait method definitions
        for extraction in extractions {
            for method in &extraction.trait_methods {
                graph
                    .trait_methods
                    .insert(method.full_path.clone(), method.clone());
            }

            for impl_method in &extraction.impl_methods {
                graph
                    .impl_methods
                    .insert(impl_method.full_id.clone(), impl_method.clone());
            }

            for inherent_method in &extraction.inherent_methods {
                graph
                    .inherent_methods
                    .insert(inherent_method.full_id.clone(), inherent_method.clone());
            }
        }

        // Collect all method calls
        for usage_set in usages {
            for usage in usage_set {
                graph.called_methods.insert(usage.method_name.clone());

                // Track specific usages for more precise analysis
                let key = if let Some(ref trait_name) = usage.trait_name {
                    format!("{}::{}", trait_name, usage.method_name)
                } else {
                    usage.method_name.clone()
                };

                graph
                    .method_usages
                    .entry(key)
                    .or_default()
                    .push(usage.clone());
            }
        }

        graph
    }

    /// Determine if a trait method is reachable.
    ///
    /// A trait method is reachable if:
    /// - It's a required method (must be implemented)
    /// - It's actually called somewhere in the codebase
    /// - It has a qualified call like `<T as Trait>::method()`
    ///
    /// Note: Public trait methods are NOT automatically considered reachable,
    /// as uncalled public methods in a crate should still be reported as dead.
    /// Library entry points should be handled at a higher level if needed.
    fn is_method_reachable(&self, method: &TraitMethodDef) -> bool {
        // Required methods are always "alive" - implementors must provide them
        if method.is_required {
            return true;
        }

        // Check if the method is called anywhere by simple name
        if self.called_methods.contains(&method.method_name) {
            return true;
        }

        // Check for qualified calls like Trait::method
        let qualified_name = format!("{}::{}", method.trait_name, method.method_name);
        if self.method_usages.contains_key(&qualified_name) {
            return true;
        }

        false
    }

    /// Determine if an impl method is reachable.
    ///
    /// An impl method is reachable if:
    /// - The trait method it implements is required
    /// - The method is called somewhere
    fn is_impl_method_reachable(&self, impl_method: &TraitImplMethod) -> bool {
        // Find the corresponding trait method definition
        let trait_method_key = format!("{}::{}", impl_method.trait_name, impl_method.method_name);

        // Check if the trait method is required
        for (path, def) in &self.trait_methods {
            let path_matches =
                path.ends_with(&trait_method_key) || path.ends_with(&impl_method.method_name);
            if path_matches && def.trait_name == impl_method.trait_name && def.is_required {
                return true;
            }
        }

        // Check if directly called
        self.called_methods.contains(&impl_method.method_name)
    }

    /// Find all dead trait method definitions.
    pub fn find_dead_trait_methods(&self) -> Vec<&TraitMethodDef> {
        self.trait_methods
            .values()
            .filter(|m| !self.is_method_reachable(m))
            .collect()
    }

    /// Find all dead impl methods.
    pub fn find_dead_impl_methods(&self) -> Vec<&TraitImplMethod> {
        self.impl_methods
            .values()
            .filter(|m| !self.is_impl_method_reachable(m))
            .collect()
    }

    /// Determine if an inherent impl method is reachable.
    ///
    /// An inherent impl method is reachable if:
    /// - The method is called somewhere
    /// - The method is called with Type::method syntax
    fn is_inherent_method_reachable(&self, method: &InherentImplMethod) -> bool {
        // Check if called by simple name
        if self.called_methods.contains(&method.method_name) {
            return true;
        }

        // Check for qualified calls like Type::method
        if self.method_usages.contains_key(&method.full_id) {
            return true;
        }

        false
    }

    /// Find all dead inherent impl methods.
    pub fn find_dead_inherent_methods(&self) -> Vec<&InherentImplMethod> {
        self.inherent_methods
            .values()
            .filter(|m| !self.is_inherent_method_reachable(m))
            .collect()
    }

    /// Perform complete analysis and return structured result.
    pub fn analyze(&self) -> TraitAnalysisResult {
        let mut dead_trait_methods: Vec<TraitMethodDef> = self
            .find_dead_trait_methods()
            .into_iter()
            .cloned()
            .collect();

        let mut dead_impl_methods: Vec<TraitImplMethod> = self
            .find_dead_impl_methods()
            .into_iter()
            .cloned()
            .collect();

        let mut dead_inherent_methods: Vec<InherentImplMethod> = self
            .find_dead_inherent_methods()
            .into_iter()
            .cloned()
            .collect();

        // Sort for consistent output
        dead_trait_methods.sort_by(|a, b| a.file.cmp(&b.file).then_with(|| a.full_path.cmp(&b.full_path)));
        dead_impl_methods.sort_by(|a, b| a.file.cmp(&b.file).then_with(|| a.full_id.cmp(&b.full_id)));
        dead_inherent_methods.sort_by(|a, b| a.file.cmp(&b.file).then_with(|| a.full_id.cmp(&b.full_id)));

        let required_methods = self.trait_methods.values().filter(|m| m.is_required).count();
        let provided_methods = self.trait_methods.values().filter(|m| !m.is_required).count();

        let dead_trait_count = dead_trait_methods.len();
        let dead_impl_count = dead_impl_methods.len();
        let dead_inherent_count = dead_inherent_methods.len();

        TraitAnalysisResult {
            all_trait_methods: self.trait_methods.values().cloned().collect(),
            all_impl_methods: self.impl_methods.values().cloned().collect(),
            all_inherent_methods: self.inherent_methods.values().cloned().collect(),
            dead_trait_methods,
            dead_impl_methods,
            dead_inherent_methods,
            stats: TraitStats {
                total_trait_methods: self.trait_methods.len(),
                total_impl_methods: self.impl_methods.len(),
                total_inherent_methods: self.inherent_methods.len(),
                dead_trait_method_count: dead_trait_count,
                dead_impl_method_count: dead_impl_count,
                dead_inherent_method_count: dead_inherent_count,
                required_methods,
                provided_methods,
            },
        }
    }

    /// Get the number of trait methods in the graph.
    pub fn trait_method_count(&self) -> usize {
        self.trait_methods.len()
    }

    /// Get the number of impl methods in the graph.
    pub fn impl_method_count(&self) -> usize {
        self.impl_methods.len()
    }

    /// Get the number of inherent impl methods in the graph.
    pub fn inherent_method_count(&self) -> usize {
        self.inherent_methods.len()
    }
}

impl Default for TraitGraph {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_trait_method(
        trait_name: &str,
        method_name: &str,
        vis: &str,
        is_required: bool,
        file: &str,
    ) -> TraitMethodDef {
        TraitMethodDef {
            trait_name: trait_name.to_string(),
            method_name: method_name.to_string(),
            full_path: format!("{}::{}", trait_name, method_name),
            visibility: vis.to_string(),
            is_required,
            file: file.to_string(),
        }
    }

    fn make_impl_method(
        trait_name: &str,
        type_name: &str,
        method_name: &str,
        file: &str,
    ) -> TraitImplMethod {
        TraitImplMethod {
            trait_name: trait_name.to_string(),
            type_name: type_name.to_string(),
            method_name: method_name.to_string(),
            full_id: format!("impl {} for {} :: {}", trait_name, type_name, method_name),
            file: file.to_string(),
        }
    }

    #[test]
    fn test_required_methods_are_alive() {
        let extraction = TraitExtractionResult {
            trait_methods: vec![
                make_trait_method("MyTrait", "required_method", "pub", true, "test.rs"),
                make_trait_method("MyTrait", "provided_method", "pub", false, "test.rs"),
            ],
            impl_methods: vec![],
            inherent_methods: vec![],
        };

        let graph = TraitGraph::build(&[extraction], &[]);
        let result = graph.analyze();

        // Required methods are never dead
        assert!(result
            .dead_trait_methods
            .iter()
            .all(|m| m.method_name != "required_method"));
    }

    #[test]
    fn test_called_methods_are_alive() {
        let extraction = TraitExtractionResult {
            trait_methods: vec![
                make_trait_method("Foo", "used_method", "private", false, "test.rs"),
                make_trait_method("Foo", "unused_method", "private", false, "test.rs"),
            ],
            impl_methods: vec![],
            inherent_methods: vec![],
        };

        let usage = TraitMethodUsage {
            method_name: "used_method".to_string(),
            trait_name: None,
            type_name: None,
            usage_kind: super::super::trait_usage::UsageKind::MethodCall,
        };

        let usages = HashSet::from([usage]);
        let graph = TraitGraph::build(&[extraction], &[usages]);
        let result = graph.analyze();

        // used_method should not be dead, unused_method should be dead
        // But since they're private with no default body check...
        // Actually provided_method with private visibility and not called = dead
        assert!(result
            .dead_trait_methods
            .iter()
            .any(|m| m.method_name == "unused_method"));
    }

    #[test]
    fn test_impl_methods_for_required_are_alive() {
        let extraction = TraitExtractionResult {
            trait_methods: vec![make_trait_method(
                "MyTrait",
                "required",
                "pub",
                true,
                "trait.rs",
            )],
            impl_methods: vec![make_impl_method("MyTrait", "MyStruct", "required", "impl.rs")],
            inherent_methods: vec![],
        };

        let graph = TraitGraph::build(&[extraction], &[]);
        let result = graph.analyze();

        // Impl method for required trait method should be alive
        assert!(result.dead_impl_methods.is_empty());
    }

    #[test]
    fn test_stats() {
        let extraction = TraitExtractionResult {
            trait_methods: vec![
                make_trait_method("T", "required", "pub", true, "test.rs"),
                make_trait_method("T", "provided", "pub", false, "test.rs"),
            ],
            impl_methods: vec![
                make_impl_method("T", "A", "required", "test.rs"),
                make_impl_method("T", "A", "provided", "test.rs"),
            ],
            inherent_methods: vec![],
        };

        let graph = TraitGraph::build(&[extraction], &[]);
        let result = graph.analyze();

        assert_eq!(result.stats.total_trait_methods, 2);
        assert_eq!(result.stats.total_impl_methods, 2);
        assert_eq!(result.stats.required_methods, 1);
        assert_eq!(result.stats.provided_methods, 1);
    }

    #[test]
    fn test_multiple_impls() {
        let extraction = TraitExtractionResult {
            trait_methods: vec![make_trait_method("Foo", "bar", "pub", true, "trait.rs")],
            impl_methods: vec![
                make_impl_method("Foo", "TypeA", "bar", "a.rs"),
                make_impl_method("Foo", "TypeB", "bar", "b.rs"),
                make_impl_method("Foo", "TypeC", "bar", "c.rs"),
            ],
            inherent_methods: vec![],
        };

        let graph = TraitGraph::build(&[extraction], &[]);
        let result = graph.analyze();

        // All impl methods for a required method should be alive
        assert_eq!(result.dead_impl_methods.len(), 0);
        assert_eq!(result.stats.total_impl_methods, 3);
    }

    #[test]
    fn test_uncalled_pub_method_is_dead() {
        // This test verifies the fix for the critical bug where all pub methods
        // were incorrectly marked as alive regardless of whether they were called.
        let extraction = TraitExtractionResult {
            trait_methods: vec![
                make_trait_method("MyTrait", "called_method", "pub", false, "test.rs"),
                make_trait_method("MyTrait", "uncalled_method", "pub", false, "test.rs"),
            ],
            impl_methods: vec![],
            inherent_methods: vec![],
        };

        // Only called_method is actually used
        let usage = TraitMethodUsage {
            method_name: "called_method".to_string(),
            trait_name: Some("MyTrait".to_string()),
            type_name: None,
            usage_kind: super::super::trait_usage::UsageKind::MethodCall,
        };
        let usages = HashSet::from([usage]);

        let graph = TraitGraph::build(&[extraction], &[usages]);
        let result = graph.analyze();

        // uncalled_method should be detected as dead even though it's public
        assert_eq!(result.dead_trait_methods.len(), 1);
        assert_eq!(result.dead_trait_methods[0].method_name, "uncalled_method");
    }

    #[test]
    fn test_qualified_call_marks_method_alive() {
        let extraction = TraitExtractionResult {
            trait_methods: vec![
                make_trait_method("MyTrait", "qualified_call", "pub", false, "test.rs"),
            ],
            impl_methods: vec![],
            inherent_methods: vec![],
        };

        // Method is called with qualified path: MyTrait::qualified_call
        let usage = TraitMethodUsage {
            method_name: "qualified_call".to_string(),
            trait_name: Some("MyTrait".to_string()),
            type_name: None,
            usage_kind: super::super::trait_usage::UsageKind::QualifiedCall,
        };
        let usages = HashSet::from([usage]);

        let graph = TraitGraph::build(&[extraction], &[usages]);
        let result = graph.analyze();

        // qualified_call should be alive due to qualified usage
        assert!(result.dead_trait_methods.is_empty());
    }

    fn make_inherent_method(
        type_name: &str,
        method_name: &str,
        vis: &str,
        is_static: bool,
        file: &str,
    ) -> InherentImplMethod {
        InherentImplMethod {
            type_name: type_name.to_string(),
            method_name: method_name.to_string(),
            full_id: format!("{}::{}", type_name, method_name),
            visibility: vis.to_string(),
            is_static,
            file: file.to_string(),
            module_path: String::new(),
        }
    }

    #[test]
    fn test_inherent_method_uncalled_is_dead() {
        let extraction = TraitExtractionResult {
            trait_methods: vec![],
            impl_methods: vec![],
            inherent_methods: vec![
                make_inherent_method("MyType", "called_method", "pub", false, "test.rs"),
                make_inherent_method("MyType", "uncalled_method", "pub", false, "test.rs"),
            ],
        };

        let usage = TraitMethodUsage {
            method_name: "called_method".to_string(),
            trait_name: None,
            type_name: Some("MyType".to_string()),
            usage_kind: super::super::trait_usage::UsageKind::MethodCall,
        };
        let usages = HashSet::from([usage]);

        let graph = TraitGraph::build(&[extraction], &[usages]);
        let result = graph.analyze();

        assert_eq!(result.stats.total_inherent_methods, 2);
        assert_eq!(result.stats.dead_inherent_method_count, 1);
        assert_eq!(result.dead_inherent_methods[0].method_name, "uncalled_method");
    }

    #[test]
    fn test_inherent_static_method() {
        let extraction = TraitExtractionResult {
            trait_methods: vec![],
            impl_methods: vec![],
            inherent_methods: vec![
                make_inherent_method("Factory", "new", "pub", true, "test.rs"),
                make_inherent_method("Factory", "unused_static", "pub", true, "test.rs"),
            ],
        };

        let usage = TraitMethodUsage {
            method_name: "new".to_string(),
            trait_name: None,
            type_name: Some("Factory".to_string()),
            usage_kind: super::super::trait_usage::UsageKind::QualifiedCall,
        };
        let usages = HashSet::from([usage]);

        let graph = TraitGraph::build(&[extraction], &[usages]);
        let result = graph.analyze();

        // new() is called, unused_static() is not
        assert_eq!(result.dead_inherent_methods.len(), 1);
        assert_eq!(result.dead_inherent_methods[0].method_name, "unused_static");
    }
}
