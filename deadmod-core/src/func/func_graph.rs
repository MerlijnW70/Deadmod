//! Function call graph construction and dead function detection.
//!
//! Builds a directed graph of function calls and identifies unreachable functions.
//!
//! Entry points (roots) are:
//! - `main()` function
//! - `pub` functions (externally visible)
//! - `#[test]` functions
//! - `#[no_mangle]` functions
//!
//! Performance characteristics:
//! - Graph build: O(|F| + |C|) where F = functions, C = calls
//! - Reachability: O(|F| + |E|) single BFS traversal

use std::collections::{HashMap, HashSet, VecDeque};

use super::func_extractor::FunctionInfo;

/// Result of function-level dead code analysis.
#[derive(Debug, Clone)]
pub struct FuncAnalysisResult {
    /// All functions found in the codebase
    pub all_functions: Vec<FunctionInfo>,
    /// Functions reachable from entry points
    pub reachable: HashSet<String>,
    /// Dead (unreachable) functions
    pub dead: Vec<FunctionInfo>,
    /// Statistics
    pub stats: FuncStats,
}

/// Statistics about function analysis.
#[derive(Debug, Clone, Default)]
pub struct FuncStats {
    pub total_functions: usize,
    pub reachable_count: usize,
    pub dead_count: usize,
    pub public_dead: usize,
    pub private_dead: usize,
}

/// Function call graph for dead code detection.
pub struct FuncGraph {
    /// Map from full_path to FunctionInfo
    nodes: HashMap<String, FunctionInfo>,
    /// Edges: caller -> set of callee names
    edges: HashMap<String, HashSet<String>>,
    /// Reverse lookup: function name -> set of full paths with that name
    name_to_paths: HashMap<String, HashSet<String>>,
}

impl FuncGraph {
    /// Create a new empty function graph.
    pub fn new() -> Self {
        Self {
            nodes: HashMap::new(),
            edges: HashMap::new(),
            name_to_paths: HashMap::new(),
        }
    }

    /// Build the function call graph from extracted data.
    ///
    /// # Arguments
    /// * `functions` - All functions extracted from the codebase
    /// * `file_calls` - Map from file path to set of function names called in that file
    pub fn build(
        functions: &[FunctionInfo],
        file_calls: &HashMap<String, HashSet<String>>,
    ) -> Self {
        let mut graph = Self::new();

        // Add all functions as nodes
        for func in functions {
            graph
                .nodes
                .insert(func.full_path.clone(), func.clone());

            // Build reverse lookup
            graph
                .name_to_paths
                .entry(func.name.clone())
                .or_default()
                .insert(func.full_path.clone());
        }

        // Build edges based on calls
        for func in functions {
            if let Some(calls) = file_calls.get(&func.file) {
                let mut func_edges = HashSet::new();

                for call_name in calls {
                    // Find all functions matching this call name
                    if let Some(targets) = graph.name_to_paths.get(call_name) {
                        for target in targets {
                            // Skip self-references
                            if target != &func.full_path {
                                func_edges.insert(target.clone());
                            }
                        }
                    }
                }

                if !func_edges.is_empty() {
                    graph.edges.insert(func.full_path.clone(), func_edges);
                }
            }
        }

        graph
    }

    /// Determine which functions are entry points (roots for reachability).
    ///
    /// Entry points are:
    /// - `main` function
    /// - Public functions (`pub`)
    /// - `#[test]` functions (test entry points)
    /// - `#[no_mangle]` functions (FFI/external entry points)
    fn find_entry_points(&self) -> HashSet<&str> {
        let mut roots = HashSet::new();

        for (path, func) in &self.nodes {
            // main() is always an entry point
            if func.name == "main" {
                roots.insert(path.as_str());
                continue;
            }

            // Public functions are entry points
            if func.visibility.starts_with("pub") {
                roots.insert(path.as_str());
                continue;
            }

            // #[test] functions are entry points (called by test harness)
            if func.is_test {
                roots.insert(path.as_str());
                continue;
            }

            // #[no_mangle] functions are entry points (can be called from FFI)
            if func.is_no_mangle {
                roots.insert(path.as_str());
                continue;
            }
        }

        roots
    }

    /// Compute reachable functions using BFS from all entry points.
    ///
    /// This is a multi-source BFS that finds all functions reachable
    /// from any entry point in O(|F| + |E|) time.
    pub fn compute_reachable(&self) -> HashSet<String> {
        let entry_points = self.find_entry_points();

        let mut visited: HashSet<String> = HashSet::with_capacity(self.nodes.len());
        let mut queue: VecDeque<&str> = VecDeque::new();

        // Initialize with all entry points
        for root in entry_points {
            if visited.insert(root.to_string()) {
                queue.push_back(root);
            }
        }

        // BFS traversal
        while let Some(current) = queue.pop_front() {
            if let Some(callees) = self.edges.get(current) {
                for callee in callees {
                    if visited.insert(callee.clone()) {
                        queue.push_back(callee);
                    }
                }
            }
        }

        visited
    }

    /// Find all dead (unreachable) functions.
    pub fn find_dead(&self) -> Vec<&FunctionInfo> {
        let reachable = self.compute_reachable();

        self.nodes
            .iter()
            .filter(|(path, _)| !reachable.contains(*path))
            .map(|(_, info)| info)
            .collect()
    }

    /// Perform complete analysis and return structured result.
    pub fn analyze(&self) -> FuncAnalysisResult {
        let reachable = self.compute_reachable();

        let mut dead = Vec::new();
        let mut public_dead = 0;
        let mut private_dead = 0;

        for (path, info) in &self.nodes {
            if !reachable.contains(path) {
                if info.visibility.starts_with("pub") {
                    public_dead += 1;
                } else {
                    private_dead += 1;
                }
                dead.push(info.clone());
            }
        }

        // Sort dead functions by file for consistent output
        dead.sort_by(|a, b| a.file.cmp(&b.file).then_with(|| a.name.cmp(&b.name)));

        let dead_count = dead.len();

        FuncAnalysisResult {
            all_functions: self.nodes.values().cloned().collect(),
            reachable,
            dead,
            stats: FuncStats {
                total_functions: self.nodes.len(),
                reachable_count: self.nodes.len() - dead_count,
                dead_count,
                public_dead,
                private_dead,
            },
        }
    }

    /// Get the number of functions in the graph.
    pub fn function_count(&self) -> usize {
        self.nodes.len()
    }

    /// Get the number of edges in the graph.
    pub fn edge_count(&self) -> usize {
        self.edges.values().map(|v| v.len()).sum()
    }
}

impl Default for FuncGraph {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_func(name: &str, full_path: &str, vis: &str, file: &str) -> FunctionInfo {
        FunctionInfo {
            name: name.to_string(),
            full_path: full_path.to_string(),
            visibility: vis.to_string(),
            file: file.to_string(),
            is_method: false,
            impl_type: None,
            is_test: false,
            is_no_mangle: false,
        }
    }

    fn make_test_func(name: &str, full_path: &str, file: &str) -> FunctionInfo {
        FunctionInfo {
            name: name.to_string(),
            full_path: full_path.to_string(),
            visibility: "private".to_string(),
            file: file.to_string(),
            is_method: false,
            impl_type: None,
            is_test: true,
            is_no_mangle: false,
        }
    }

    fn make_no_mangle_func(name: &str, full_path: &str, file: &str) -> FunctionInfo {
        FunctionInfo {
            name: name.to_string(),
            full_path: full_path.to_string(),
            visibility: "private".to_string(),
            file: file.to_string(),
            is_method: false,
            impl_type: None,
            is_test: false,
            is_no_mangle: true,
        }
    }

    #[test]
    fn test_simple_dead_function() {
        let funcs = vec![
            make_func("main", "main", "private", "main.rs"),
            make_func("used", "used", "private", "main.rs"),
            make_func("dead", "dead", "private", "main.rs"),
        ];

        let mut calls = HashMap::new();
        calls.insert(
            "main.rs".to_string(),
            HashSet::from(["used".to_string()]),
        );

        let graph = FuncGraph::build(&funcs, &calls);
        let result = graph.analyze();

        assert_eq!(result.stats.total_functions, 3);
        assert_eq!(result.stats.dead_count, 1);
        assert_eq!(result.dead[0].name, "dead");
    }

    #[test]
    fn test_public_functions_are_reachable() {
        let funcs = vec![
            make_func("main", "main", "private", "main.rs"),
            make_func("public_api", "public_api", "pub", "lib.rs"),
            make_func("helper", "helper", "private", "lib.rs"),
        ];

        let calls = HashMap::new(); // No calls

        let graph = FuncGraph::build(&funcs, &calls);
        let result = graph.analyze();

        // main and public_api are entry points, helper is dead
        assert_eq!(result.stats.dead_count, 1);
        assert_eq!(result.dead[0].name, "helper");
    }

    #[test]
    fn test_transitive_reachability() {
        let funcs = vec![
            make_func("main", "main", "private", "main.rs"),
            make_func("a", "a", "private", "main.rs"),
            make_func("b", "b", "private", "main.rs"),
            make_func("c", "c", "private", "main.rs"),
        ];

        let mut calls = HashMap::new();
        // main -> a -> b -> c
        calls.insert(
            "main.rs".to_string(),
            HashSet::from(["a".to_string(), "b".to_string(), "c".to_string()]),
        );

        let graph = FuncGraph::build(&funcs, &calls);
        let result = graph.analyze();

        // All reachable from main via calls
        assert_eq!(result.stats.dead_count, 0);
    }

    #[test]
    fn test_isolated_cluster() {
        let funcs = vec![
            make_func("main", "main", "private", "main.rs"),
            make_func("used", "used", "private", "main.rs"),
            // Isolated cluster - calls each other but unreachable from main
            make_func("island_a", "island_a", "private", "island.rs"),
            make_func("island_b", "island_b", "private", "island.rs"),
        ];

        let mut calls = HashMap::new();
        calls.insert("main.rs".to_string(), HashSet::from(["used".to_string()]));
        calls.insert(
            "island.rs".to_string(),
            HashSet::from(["island_a".to_string(), "island_b".to_string()]),
        );

        let graph = FuncGraph::build(&funcs, &calls);
        let result = graph.analyze();

        assert_eq!(result.stats.dead_count, 2);
        assert!(result.dead.iter().any(|f| f.name == "island_a"));
        assert!(result.dead.iter().any(|f| f.name == "island_b"));
    }

    #[test]
    fn test_method_detection() {
        let funcs = vec![
            make_func("main", "main", "private", "main.rs"),
            FunctionInfo {
                name: "new".to_string(),
                full_path: "Foo::new".to_string(),
                visibility: "pub".to_string(),
                file: "foo.rs".to_string(),
                is_method: true,
                impl_type: Some("Foo".to_string()),
                is_test: false,
                is_no_mangle: false,
            },
            FunctionInfo {
                name: "unused_method".to_string(),
                full_path: "Foo::unused_method".to_string(),
                visibility: "private".to_string(),
                file: "foo.rs".to_string(),
                is_method: true,
                impl_type: Some("Foo".to_string()),
                is_test: false,
                is_no_mangle: false,
            },
        ];

        let mut calls = HashMap::new();
        calls.insert("main.rs".to_string(), HashSet::from(["new".to_string()]));

        let graph = FuncGraph::build(&funcs, &calls);
        let result = graph.analyze();

        // Foo::new is pub so reachable, unused_method is dead
        assert_eq!(result.stats.dead_count, 1);
        assert_eq!(result.dead[0].name, "unused_method");
    }

    #[test]
    fn test_test_functions_are_entry_points() {
        let funcs = vec![
            make_func("main", "main", "private", "main.rs"),
            make_test_func("test_foo", "test_foo", "tests.rs"),
            make_func("helper", "helper", "private", "tests.rs"),
        ];

        let mut calls = HashMap::new();
        // test_foo calls helper
        calls.insert("tests.rs".to_string(), HashSet::from(["helper".to_string()]));

        let graph = FuncGraph::build(&funcs, &calls);
        let result = graph.analyze();

        // test_foo is an entry point, and it calls helper, so nothing is dead
        assert_eq!(result.stats.dead_count, 0);
    }

    #[test]
    fn test_no_mangle_functions_are_entry_points() {
        let funcs = vec![
            make_func("main", "main", "private", "main.rs"),
            make_no_mangle_func("ffi_export", "ffi_export", "ffi.rs"),
            make_func("internal_helper", "internal_helper", "private", "ffi.rs"),
        ];

        let mut calls = HashMap::new();
        // ffi_export calls internal_helper
        calls.insert("ffi.rs".to_string(), HashSet::from(["internal_helper".to_string()]));

        let graph = FuncGraph::build(&funcs, &calls);
        let result = graph.analyze();

        // ffi_export is an entry point, so internal_helper is also reachable
        assert_eq!(result.stats.dead_count, 0);
    }

    #[test]
    fn test_stats() {
        let funcs = vec![
            make_func("main", "main", "private", "main.rs"),
            make_func("pub_dead", "pub_dead", "pub", "lib.rs"),
            make_func("priv_dead", "priv_dead", "private", "lib.rs"),
        ];

        // pub_dead is public so it's reachable, priv_dead is unreachable
        let calls = HashMap::new();

        let graph = FuncGraph::build(&funcs, &calls);
        let result = graph.analyze();

        assert_eq!(result.stats.total_functions, 3);
        assert_eq!(result.stats.reachable_count, 2); // main + pub_dead
        assert_eq!(result.stats.dead_count, 1);
        assert_eq!(result.stats.private_dead, 1);
        assert_eq!(result.stats.public_dead, 0);
    }
}
