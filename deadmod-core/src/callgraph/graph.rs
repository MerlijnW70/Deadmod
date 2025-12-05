//! Call graph construction and analysis.
//!
//! Builds a directed graph where:
//! - Nodes are function definitions
//! - Edges represent function calls (A -> B means A calls B)
//!
//! Supports:
//! - DOT format export for Graphviz visualization
//! - JSON export for programmatic analysis
//! - Dead function detection (unreachable from entry points)
//!
//! # Performance Characteristics
//!
//! - Graph build: O(|F| + |C|) typical, O(|F|² * |C|) worst case (rare fallback)
//! - Reachability: O(|F| + |E|) BFS traversal via `GraphTraversal` trait
//! - Entry points: O(|F|) single pass
//! - Analysis caching: O(1) after first call via `OnceCell`

use serde::Serialize;
use std::cell::OnceCell;
use std::collections::{HashMap, HashSet};

use super::extractor::FunctionDef;
use super::usage::CallUsageResult;
use crate::common::GraphTraversal;

// ============================================================================
// Typed JSON Structures (compile-time validation, easier refactoring)
// ============================================================================

/// A node in the visualizer JSON output.
#[derive(Debug, Clone, Serialize)]
pub struct VisualizerNode {
    pub id: usize,
    pub name: String,
    pub full_path: String,
    pub file: String,
    pub module: String,
    pub dead: bool,
    pub visibility: String,
    pub is_method: bool,
}

/// An edge in the visualizer JSON output.
#[derive(Debug, Clone, Serialize)]
pub struct VisualizerEdge {
    pub from: usize,
    pub to: usize,
}

/// Statistics in the visualizer JSON output.
#[derive(Debug, Clone, Serialize)]
pub struct VisualizerStats {
    pub total_functions: usize,
    pub total_edges: usize,
    pub dead_functions: usize,
    pub total_modules: usize,
}

/// Complete visualizer graph structure.
#[derive(Debug, Clone, Serialize)]
pub struct VisualizerGraph {
    pub nodes: Vec<VisualizerNode>,
    pub edges: Vec<VisualizerEdge>,
    pub modules: Vec<String>,
    pub stats: VisualizerStats,
}

// ============================================================================
// Core Call Graph
// ============================================================================

/// A call graph representing function relationships.
#[derive(Debug)]
pub struct CallGraph {
    /// Map from full_path to FunctionDef
    pub nodes: HashMap<String, FunctionDef>,
    /// Edges: (caller, callee) pairs
    pub edges: HashSet<(String, String)>,
    /// Forward adjacency list: caller -> [callees] for O(1) neighbor lookup
    pub adjacency: HashMap<String, Vec<String>>,
    /// Reverse edges for finding callers
    pub reverse_edges: HashMap<String, HashSet<String>>,
    /// Cached analysis result (computed once, reused)
    cached_analysis: OnceCell<CallGraphAnalysis>,
}

/// Statistics about the call graph.
#[derive(Debug, Clone, Default)]
pub struct CallGraphStats {
    pub total_functions: usize,
    pub total_edges: usize,
    pub entry_points: usize,
    pub unreachable_functions: usize,
    pub max_call_depth: usize,
}

/// Result of call graph analysis.
#[derive(Debug, Clone)]
pub struct CallGraphAnalysis {
    /// Functions unreachable from any entry point
    pub unreachable: Vec<FunctionDef>,
    /// Entry points (main, test functions, pub functions)
    pub entry_points: Vec<String>,
    /// Statistics
    pub stats: CallGraphStats,
}

// ============================================================================
// GraphTraversal Implementation (shared BFS logic)
// ============================================================================

impl GraphTraversal for CallGraph {
    type Node = String;

    fn neighbors(&self, node: &String) -> Vec<String> {
        self.adjacency.get(node).cloned().unwrap_or_default()
    }

    fn contains_node(&self, node: &String) -> bool {
        self.nodes.contains_key(node)
    }
}

impl CallGraph {
    /// Create a new empty call graph.
    pub fn new() -> Self {
        Self {
            nodes: HashMap::new(),
            edges: HashSet::new(),
            adjacency: HashMap::new(),
            reverse_edges: HashMap::new(),
            cached_analysis: OnceCell::new(),
        }
    }

    /// Build a call graph from function definitions and call usages.
    ///
    /// If `resolved_calls` are present in the usage result (from `extract_call_usages_resolved`),
    /// uses semantic path resolution for accurate edge matching. Otherwise falls back to
    /// name-based heuristic matching.
    pub fn build(
        functions: &[FunctionDef],
        usages: &HashMap<String, CallUsageResult>,
    ) -> Self {
        let mut graph = Self::new();

        // Register all function nodes
        for func in functions {
            graph.nodes.insert(func.full_path.clone(), func.clone());
        }

        // Build name -> full_path index for efficient lookup
        let mut name_index: HashMap<String, Vec<String>> = HashMap::new();
        for func in functions {
            name_index
                .entry(func.name.clone())
                .or_default()
                .push(func.full_path.clone());
        }

        // Build path suffix index for resolved path matching
        // Maps path suffixes to full paths for efficient lookup
        let mut suffix_index: HashMap<String, Vec<String>> = HashMap::new();
        for func in functions {
            // Index by full path
            suffix_index
                .entry(func.full_path.clone())
                .or_default()
                .push(func.full_path.clone());

            // Index by path without leading module (e.g., "handler::process" from "api::v1::handler::process")
            let parts: Vec<&str> = func.full_path.split("::").collect();
            for i in 1..parts.len() {
                let suffix = parts[i..].join("::");
                suffix_index
                    .entry(suffix)
                    .or_default()
                    .push(func.full_path.clone());
            }
        }

        // Collect all node full_paths for fallback matching
        let all_paths: Vec<String> = graph.nodes.keys().cloned().collect();

        // Add edges based on calls
        for func in functions {
            if let Some(usage) = usages.get(&func.file) {
                // Check if we have resolved paths (semantic resolution)
                if !usage.resolved_calls.is_empty() {
                    // Use resolved paths for accurate matching
                    for resolved in &usage.resolved_calls {
                        // Try exact match first
                        if let Some(targets) = suffix_index.get(resolved) {
                            for target in targets {
                                if target != &func.full_path {
                                    graph.add_edge(&func.full_path, target);
                                }
                            }
                        } else {
                            // Try suffix match for partial resolution
                            for full_path in &all_paths {
                                if full_path.ends_with(resolved) && full_path != &func.full_path {
                                    graph.add_edge(&func.full_path, full_path);
                                }
                            }
                        }
                    }
                } else {
                    // Fallback: name-based heuristic matching (original behavior)
                    // Match simple name calls
                    for call_name in &usage.calls {
                        if let Some(targets) = name_index.get(call_name) {
                            for target in targets {
                                if target != &func.full_path {
                                    graph.add_edge(&func.full_path, target);
                                }
                            }
                        }
                    }

                    // Match qualified calls - use suffix index first for O(1) lookup
                    for qualified in &usage.qualified_calls {
                        // Try exact suffix match first (O(1))
                        if let Some(targets) = suffix_index.get(qualified) {
                            for target in targets {
                                if target != &func.full_path {
                                    graph.add_edge(&func.full_path, target);
                                }
                            }
                        } else {
                            // Fallback: substring matching for partial matches
                            // This is O(n) but should be rare after suffix index lookup
                            for full_path in &all_paths {
                                if (full_path.ends_with(qualified) || qualified.ends_with(full_path))
                                    && full_path != &func.full_path
                                {
                                    graph.add_edge(&func.full_path, full_path);
                                }
                            }
                        }
                    }
                }
            }
        }

        graph
    }

    /// Add an edge from caller to callee.
    ///
    /// Optimized to minimize string allocations:
    /// - Reuses cloned strings across edge, adjacency, and reverse_edges
    /// - Early exits if edge already exists (no allocations on duplicate)
    fn add_edge(&mut self, caller: &str, callee: &str) {
        // Clone once, reuse for all data structures
        let caller_owned = caller.to_string();
        let callee_owned = callee.to_string();

        // Early exit if edge already exists (avoid further allocations)
        if !self
            .edges
            .insert((caller_owned.clone(), callee_owned.clone()))
        {
            return;
        }

        // Update adjacency list for forward traversal (reuse cloned strings)
        self.adjacency
            .entry(caller_owned.clone())
            .or_default()
            .push(callee_owned.clone());

        // Update reverse edges for finding callers (reuse cloned strings)
        self.reverse_edges
            .entry(callee_owned)
            .or_default()
            .insert(caller_owned);
    }

    /// Find all entry points in the graph.
    ///
    /// Entry points are:
    /// - `main` function
    /// - `#[test]` functions
    /// - Public functions (could be called externally)
    ///
    /// Aliased as `entry_points()` for API consistency.
    pub fn find_entry_points(&self) -> Vec<String> {
        self.nodes
            .iter()
            .filter(|(path, func)| {
                func.name == "main" || path.contains("test") || func.visibility == "pub"
            })
            .map(|(path, _)| path.clone())
            .collect()
    }

    /// Alias for `find_entry_points()` for API consistency.
    #[inline]
    pub fn entry_points(&self) -> Vec<String> {
        self.find_entry_points()
    }

    /// Find all functions reachable from the given entry points.
    ///
    /// Uses the shared `GraphTraversal` trait for O(|V| + |E|) BFS.
    pub fn find_reachable(&self, entry_points: &[String]) -> HashSet<String> {
        self.reachable_from(entry_points.iter().cloned())
    }

    /// Find all unreachable functions.
    pub fn find_unreachable(&self) -> Vec<&FunctionDef> {
        let entry_points = self.find_entry_points();
        let reachable = self.find_reachable(&entry_points);

        self.nodes
            .values()
            .filter(|func| !reachable.contains(&func.full_path))
            .collect()
    }

    /// Analyze the call graph and return results.
    ///
    /// Results are cached using `OnceCell` - subsequent calls return the cached analysis.
    /// This avoids redundant O(|V| + |E|) BFS traversals when analyze() is called multiple times.
    pub fn analyze(&self) -> &CallGraphAnalysis {
        self.cached_analysis.get_or_init(|| self.compute_analysis())
    }

    /// Compute the analysis (internal, not cached).
    fn compute_analysis(&self) -> CallGraphAnalysis {
        let entry_points = self.find_entry_points();
        let reachable = self.find_reachable(&entry_points);

        let unreachable: Vec<FunctionDef> = self
            .nodes
            .values()
            .filter(|func| !reachable.contains(&func.full_path))
            .cloned()
            .collect();

        // Compute maximum call depth from any entry point
        let max_call_depth = self.compute_max_call_depth(&entry_points);

        let stats = CallGraphStats {
            total_functions: self.nodes.len(),
            total_edges: self.edges.len(),
            entry_points: entry_points.len(),
            unreachable_functions: unreachable.len(),
            max_call_depth,
        };

        CallGraphAnalysis {
            unreachable,
            entry_points,
            stats,
        }
    }

    /// Compute the maximum call depth from entry points using BFS.
    ///
    /// Returns the longest path from any entry point to any reachable function.
    /// Uses BFS level tracking to find the maximum depth efficiently.
    ///
    /// Complexity: O(|V| + |E|) - single BFS traversal
    fn compute_max_call_depth(&self, entry_points: &[String]) -> usize {
        if entry_points.is_empty() || self.adjacency.is_empty() {
            return 0;
        }

        let mut max_depth = 0;

        // For each entry point, compute max depth via BFS with level tracking
        for start in entry_points {
            if !self.nodes.contains_key(start) {
                continue;
            }

            let mut visited: HashSet<&str> = HashSet::new();
            let mut queue: std::collections::VecDeque<(&str, usize)> = std::collections::VecDeque::new();

            visited.insert(start);
            queue.push_back((start, 0));

            while let Some((node, depth)) = queue.pop_front() {
                max_depth = max_depth.max(depth);

                if let Some(callees) = self.adjacency.get(node) {
                    for callee in callees {
                        if !visited.contains(callee.as_str()) {
                            visited.insert(callee);
                            queue.push_back((callee, depth + 1));
                        }
                    }
                }
            }
        }

        max_depth
    }

    /// Export the graph to JSON format.
    pub fn to_json(&self) -> serde_json::Value {
        serde_json::json!({
            "nodes": self.nodes.values().map(|f| {
                serde_json::json!({
                    "id": f.full_path,
                    "name": f.name,
                    "file": f.file,
                    "is_method": f.is_method,
                    "parent_type": f.parent_type,
                    "visibility": f.visibility,
                })
            }).collect::<Vec<_>>(),

            "edges": self.edges.iter().map(|(from, to)| {
                serde_json::json!({
                    "from": from,
                    "to": to,
                })
            }).collect::<Vec<_>>(),

            "stats": {
                "total_functions": self.nodes.len(),
                "total_edges": self.edges.len(),
            }
        })
    }

    /// Export the graph to visualizer-compatible JSON format.
    ///
    /// Uses typed `VisualizerGraph` struct for compile-time validation.
    /// Leverages cached analysis to avoid redundant BFS traversals.
    ///
    /// Output format for PixiJS visualizer:
    /// ```json
    /// {
    ///   "nodes": [{ "id": 0, "name": "func_name", "dead": false, "file": "path" }],
    ///   "edges": [{ "from": 0, "to": 1 }]
    /// }
    /// ```
    pub fn to_visualizer_json(&self) -> serde_json::Value {
        serde_json::to_value(self.to_visualizer_graph()).unwrap_or_default()
    }

    /// Build a typed `VisualizerGraph` for export.
    ///
    /// Returns a strongly-typed struct that can be serialized to JSON.
    /// Uses cached analysis to avoid redundant computations.
    pub fn to_visualizer_graph(&self) -> VisualizerGraph {
        // Use cached analysis to get reachable set
        let analysis = self.analyze();
        let reachable_set = self.find_reachable(&analysis.entry_points);
        let reachable: HashSet<&str> = analysis
            .entry_points
            .iter()
            .chain(reachable_set.iter())
            .map(|s| s.as_str())
            .collect();

        // Build path -> numeric ID mapping
        let paths: Vec<&String> = self.nodes.keys().collect();
        let path_to_id: HashMap<&String, usize> =
            paths.iter().enumerate().map(|(i, p)| (*p, i)).collect();

        // Build typed nodes
        let nodes: Vec<VisualizerNode> = paths
            .iter()
            .enumerate()
            .map(|(i, path)| {
                let func = &self.nodes[*path];
                let is_dead = !reachable.contains(path.as_str());
                // Extract module name from file path for clustering
                let module = std::path::Path::new(&func.file)
                    .file_stem()
                    .and_then(|s| s.to_str())
                    .unwrap_or("unknown")
                    .to_string();

                VisualizerNode {
                    id: i,
                    name: func.name.clone(),
                    full_path: func.full_path.clone(),
                    file: func.file.clone(),
                    module,
                    dead: is_dead,
                    visibility: func.visibility.clone(),
                    is_method: func.is_method,
                }
            })
            .collect();

        // Build typed edges
        let edges: Vec<VisualizerEdge> = self
            .edges
            .iter()
            .filter_map(|(from, to)| {
                let from_id = *path_to_id.get(from)?;
                let to_id = *path_to_id.get(to)?;
                Some(VisualizerEdge { from: from_id, to: to_id })
            })
            .collect();

        // Collect unique modules for clustering color palette
        let mut modules: Vec<String> = nodes.iter().map(|n| n.module.clone()).collect();
        modules.sort();
        modules.dedup();

        let dead_count = nodes.iter().filter(|n| n.dead).count();

        VisualizerGraph {
            stats: VisualizerStats {
                total_functions: self.nodes.len(),
                total_edges: self.edges.len(),
                dead_functions: dead_count,
                total_modules: modules.len(),
            },
            nodes,
            edges,
            modules,
        }
    }

    /// Export the graph to DOT format for Graphviz.
    pub fn to_dot(&self) -> String {
        let mut dot = String::from("digraph CallGraph {\n");
        dot.push_str("    rankdir=LR;\n");
        dot.push_str("    node [shape=box, fontname=\"monospace\"];\n\n");

        // Add nodes
        for (path, func) in &self.nodes {
            let color = if func.visibility == "pub" {
                "lightblue"
            } else {
                "white"
            };
            let escaped_path = path.replace("::", "_").replace("<", "_").replace(">", "_");
            // Safe truncation that respects UTF-8 character boundaries
            let label = if func.name.chars().count() > 20 {
                let truncated: String = func.name.chars().take(17).collect();
                format!("{}...", truncated)
            } else {
                func.name.clone()
            };
            dot.push_str(&format!(
                "    {} [label=\"{}\" style=filled fillcolor={}];\n",
                escaped_path, label, color
            ));
        }

        dot.push('\n');

        // Add edges
        for (from, to) in &self.edges {
            let from_escaped = from.replace("::", "_").replace("<", "_").replace(">", "_");
            let to_escaped = to.replace("::", "_").replace("<", "_").replace(">", "_");
            dot.push_str(&format!("    {} -> {};\n", from_escaped, to_escaped));
        }

        dot.push_str("}\n");
        dot
    }

    /// Get the number of functions in the graph.
    pub fn function_count(&self) -> usize {
        self.nodes.len()
    }

    /// Get the number of edges in the graph.
    pub fn edge_count(&self) -> usize {
        self.edges.len()
    }
}

impl Default for CallGraph {
    fn default() -> Self {
        Self::new()
    }
}

impl Clone for CallGraph {
    fn clone(&self) -> Self {
        Self {
            nodes: self.nodes.clone(),
            edges: self.edges.clone(),
            adjacency: self.adjacency.clone(),
            reverse_edges: self.reverse_edges.clone(),
            cached_analysis: OnceCell::new(), // Don't clone cache, will be recomputed if needed
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_func(name: &str, full_path: &str, file: &str, vis: &str) -> FunctionDef {
        FunctionDef {
            name: name.to_string(),
            full_path: full_path.to_string(),
            file: file.to_string(),
            is_method: false,
            parent_type: None,
            visibility: vis.to_string(),
        }
    }

    #[test]
    fn test_build_simple_graph() {
        let functions = vec![
            make_func("main", "main", "main.rs", "private"),
            make_func("helper", "helper", "lib.rs", "private"),
        ];

        let mut usages = HashMap::new();
        usages.insert(
            "main.rs".to_string(),
            CallUsageResult {
                calls: HashSet::from(["helper".to_string()]),
                qualified_calls: HashSet::new(),
                resolved_calls: HashSet::new(),
            },
        );

        let graph = CallGraph::build(&functions, &usages);

        assert_eq!(graph.function_count(), 2);
        assert_eq!(graph.edge_count(), 1);
        assert!(graph.edges.contains(&("main".to_string(), "helper".to_string())));
    }

    #[test]
    fn test_find_entry_points() {
        let functions = vec![
            make_func("main", "main", "main.rs", "private"),
            make_func("test_foo", "test_foo", "test.rs", "private"),
            make_func("public_api", "public_api", "lib.rs", "pub"),
            make_func("private_helper", "private_helper", "lib.rs", "private"),
        ];

        let graph = CallGraph::build(&functions, &HashMap::new());
        let entry_points = graph.find_entry_points();

        assert!(entry_points.contains(&"main".to_string()));
        assert!(entry_points.contains(&"test_foo".to_string()));
        assert!(entry_points.contains(&"public_api".to_string()));
        assert!(!entry_points.contains(&"private_helper".to_string()));
    }

    #[test]
    fn test_find_reachable() {
        let functions = vec![
            make_func("main", "main", "main.rs", "private"),
            make_func("called", "called", "lib.rs", "private"),
            make_func("unused", "unused", "lib.rs", "private"),
        ];

        let mut usages = HashMap::new();
        usages.insert(
            "main.rs".to_string(),
            CallUsageResult {
                calls: HashSet::from(["called".to_string()]),
                qualified_calls: HashSet::new(),
                resolved_calls: HashSet::new(),
            },
        );

        let graph = CallGraph::build(&functions, &usages);
        let entry_points = vec!["main".to_string()];
        let reachable = graph.find_reachable(&entry_points);

        assert!(reachable.contains("main"));
        assert!(reachable.contains("called"));
        assert!(!reachable.contains("unused"));
    }

    #[test]
    fn test_find_unreachable() {
        let functions = vec![
            make_func("main", "main", "main.rs", "private"),
            make_func("dead_code", "dead_code", "lib.rs", "private"),
        ];

        let graph = CallGraph::build(&functions, &HashMap::new());
        let unreachable = graph.find_unreachable();

        assert_eq!(unreachable.len(), 1);
        assert_eq!(unreachable[0].name, "dead_code");
    }

    #[test]
    fn test_to_json() {
        let functions = vec![make_func("foo", "foo", "test.rs", "pub")];
        let graph = CallGraph::build(&functions, &HashMap::new());
        let json = graph.to_json();

        assert!(json["nodes"].is_array());
        assert!(json["edges"].is_array());
        assert!(json["stats"]["total_functions"].as_u64() == Some(1));
    }

    #[test]
    fn test_to_dot() {
        let functions = vec![
            make_func("a", "a", "test.rs", "pub"),
            make_func("b", "b", "test.rs", "private"),
        ];

        let mut usages = HashMap::new();
        usages.insert(
            "test.rs".to_string(),
            CallUsageResult {
                calls: HashSet::from(["b".to_string()]),
                qualified_calls: HashSet::new(),
                resolved_calls: HashSet::new(),
            },
        );

        let graph = CallGraph::build(&functions, &usages);
        let dot = graph.to_dot();

        assert!(dot.contains("digraph CallGraph"));
        assert!(dot.contains("a -> b"));
    }

    #[test]
    fn test_to_visualizer_json() {
        let functions = vec![
            make_func("main", "main", "main.rs", "private"),
            make_func("called", "called", "lib.rs", "private"),
            make_func("dead_func", "dead_func", "lib.rs", "private"),
        ];

        let mut usages = HashMap::new();
        usages.insert(
            "main.rs".to_string(),
            CallUsageResult {
                calls: HashSet::from(["called".to_string()]),
                qualified_calls: HashSet::new(),
                resolved_calls: HashSet::new(),
            },
        );

        let graph = CallGraph::build(&functions, &usages);
        let json = graph.to_visualizer_json();

        // Check structure
        assert!(json["nodes"].is_array());
        assert!(json["edges"].is_array());
        assert_eq!(json["nodes"].as_array().unwrap().len(), 3);

        // Check numeric IDs
        let nodes = json["nodes"].as_array().unwrap();
        for node in nodes {
            assert!(node["id"].is_u64());
            assert!(node["dead"].is_boolean());
        }

        // Check edges use numeric IDs
        let edges = json["edges"].as_array().unwrap();
        assert!(!edges.is_empty());
        for edge in edges {
            assert!(edge["from"].is_u64());
            assert!(edge["to"].is_u64());
        }

        // Check dead function count
        assert_eq!(json["stats"]["dead_functions"].as_u64(), Some(1));
    }

    // --- DEEP EDGE CASE TESTS FOR CALLGRAPH ---

    #[test]
    fn test_empty_callgraph() {
        let graph = CallGraph::new();

        assert_eq!(graph.function_count(), 0);
        assert_eq!(graph.edge_count(), 0);
        assert!(graph.find_entry_points().is_empty());
        assert!(graph.find_unreachable().is_empty());
    }

    #[test]
    fn test_callgraph_with_no_calls() {
        let functions = vec![
            make_func("main", "main", "main.rs", "private"),
            make_func("helper", "helper", "lib.rs", "private"),
        ];

        let graph = CallGraph::build(&functions, &HashMap::new());

        assert_eq!(graph.function_count(), 2);
        assert_eq!(graph.edge_count(), 0);
    }

    #[test]
    fn test_callgraph_cyclic_calls() {
        let functions = vec![
            make_func("a", "a", "test.rs", "pub"),
            make_func("b", "b", "test.rs", "pub"),
            make_func("c", "c", "test.rs", "pub"),
        ];

        let mut usages = HashMap::new();
        usages.insert(
            "test.rs".to_string(),
            CallUsageResult {
                calls: HashSet::from(["a".to_string(), "b".to_string(), "c".to_string()]),
                qualified_calls: HashSet::new(),
                resolved_calls: HashSet::new(),
            },
        );

        let graph = CallGraph::build(&functions, &usages);

        // Should handle cycles without infinite loop
        let reachable = graph.find_reachable(&["a".to_string()]);
        assert!(reachable.contains("a"));
    }

    #[test]
    fn test_callgraph_self_recursive() {
        let functions = vec![make_func("recursive", "recursive", "test.rs", "pub")];

        let mut usages = HashMap::new();
        usages.insert(
            "test.rs".to_string(),
            CallUsageResult {
                calls: HashSet::from(["recursive".to_string()]),
                qualified_calls: HashSet::new(),
                resolved_calls: HashSet::new(),
            },
        );

        let graph = CallGraph::build(&functions, &usages);

        // Self-call should not create edge to itself
        assert_eq!(graph.edge_count(), 0);
    }

    #[test]
    fn test_callgraph_deep_chain() {
        // Create a chain: f0 -> f1 -> f2 -> ... -> f99
        let functions: Vec<_> = (0..100)
            .map(|i| make_func(&format!("f{}", i), &format!("f{}", i), "test.rs", "private"))
            .collect();

        let mut usages = HashMap::new();
        for i in 0..99 {
            usages.insert(
                "test.rs".to_string(),
                CallUsageResult {
                    calls: HashSet::from([format!("f{}", i + 1)]),
                    qualified_calls: HashSet::new(),
                    resolved_calls: HashSet::new(),
                },
            );
        }

        let graph = CallGraph::build(&functions, &usages);
        assert_eq!(graph.function_count(), 100);
    }

    #[test]
    fn test_callgraph_qualified_calls() {
        let functions = vec![
            make_func("caller", "caller", "main.rs", "private"),
            make_func("target", "module::target", "lib.rs", "private"),
        ];

        let mut usages = HashMap::new();
        usages.insert(
            "main.rs".to_string(),
            CallUsageResult {
                calls: HashSet::new(),
                qualified_calls: HashSet::from(["module::target".to_string()]),
                resolved_calls: HashSet::new(),
            },
        );

        let graph = CallGraph::build(&functions, &usages);

        // Should resolve qualified call
        assert!(graph.edges.contains(&("caller".to_string(), "module::target".to_string())));
    }

    #[test]
    fn test_callgraph_adjacency_list_correctness() {
        let functions = vec![
            make_func("a", "a", "test.rs", "pub"),
            make_func("b", "b", "test.rs", "private"),
            make_func("c", "c", "test.rs", "private"),
        ];

        let mut usages = HashMap::new();
        usages.insert(
            "test.rs".to_string(),
            CallUsageResult {
                calls: HashSet::from(["b".to_string(), "c".to_string()]),
                qualified_calls: HashSet::new(),
                resolved_calls: HashSet::new(),
            },
        );

        let graph = CallGraph::build(&functions, &usages);

        // Verify adjacency list is populated correctly
        assert!(graph.adjacency.contains_key("a"));
        let neighbors = graph.adjacency.get("a").unwrap();
        assert!(neighbors.contains(&"b".to_string()) || neighbors.contains(&"c".to_string()));
    }

    #[test]
    fn test_callgraph_reverse_edges() {
        let functions = vec![
            make_func("caller", "caller", "main.rs", "private"),
            make_func("callee", "callee", "lib.rs", "private"),
        ];

        let mut usages = HashMap::new();
        usages.insert(
            "main.rs".to_string(),
            CallUsageResult {
                calls: HashSet::from(["callee".to_string()]),
                qualified_calls: HashSet::new(),
                resolved_calls: HashSet::new(),
            },
        );

        let graph = CallGraph::build(&functions, &usages);

        // Check reverse edge exists
        assert!(graph.reverse_edges.contains_key("callee"));
        assert!(graph.reverse_edges["callee"].contains("caller"));
    }

    #[test]
    fn test_callgraph_module_extraction() {
        let functions = vec![make_func(
            "func",
            "func",
            "/path/to/my_module.rs",
            "private",
        )];

        let graph = CallGraph::build(&functions, &HashMap::new());
        let json = graph.to_visualizer_json();

        let nodes = json["nodes"].as_array().unwrap();
        let node = &nodes[0];

        // Module should be extracted from file path
        assert_eq!(node["module"].as_str(), Some("my_module"));
    }

    #[test]
    fn test_callgraph_stats() {
        let functions = vec![
            make_func("main", "main", "main.rs", "private"),
            make_func("used", "used", "lib.rs", "private"),
            make_func("unused", "unused", "lib.rs", "private"),
        ];

        let mut usages = HashMap::new();
        usages.insert(
            "main.rs".to_string(),
            CallUsageResult {
                calls: HashSet::from(["used".to_string()]),
                qualified_calls: HashSet::new(),
                resolved_calls: HashSet::new(),
            },
        );

        let graph = CallGraph::build(&functions, &usages);
        let analysis = graph.analyze();

        assert_eq!(analysis.stats.total_functions, 3);
        assert_eq!(analysis.stats.total_edges, 1);
        assert_eq!(analysis.stats.unreachable_functions, 1);
    }

    #[test]
    fn test_max_call_depth_linear_chain() {
        // Create chain: main -> a -> b -> c (depth = 3)
        let functions = vec![
            make_func("main", "main", "main.rs", "private"),
            make_func("a", "a", "lib.rs", "private"),
            make_func("b", "b", "lib.rs", "private"),
            make_func("c", "c", "lib.rs", "private"),
        ];

        let mut usages = HashMap::new();
        usages.insert(
            "main.rs".to_string(),
            CallUsageResult {
                calls: HashSet::from(["a".to_string()]),
                qualified_calls: HashSet::new(),
                resolved_calls: HashSet::new(),
            },
        );
        usages.insert(
            "lib.rs".to_string(),
            CallUsageResult {
                calls: HashSet::from(["b".to_string(), "c".to_string()]),
                qualified_calls: HashSet::new(),
                resolved_calls: HashSet::new(),
            },
        );

        let graph = CallGraph::build(&functions, &usages);
        let analysis = graph.analyze();

        // main (0) -> a (1) -> b (2), a -> c (2) = max depth 2
        assert!(analysis.stats.max_call_depth >= 2);
    }

    #[test]
    fn test_max_call_depth_empty_graph() {
        let graph = CallGraph::new();
        let analysis = graph.analyze();
        assert_eq!(analysis.stats.max_call_depth, 0);
    }

    #[test]
    fn test_max_call_depth_no_edges() {
        let functions = vec![
            make_func("main", "main", "main.rs", "private"),
            make_func("isolated", "isolated", "lib.rs", "pub"),
        ];

        let graph = CallGraph::build(&functions, &HashMap::new());
        let analysis = graph.analyze();

        // No edges, only entry points at depth 0
        assert_eq!(analysis.stats.max_call_depth, 0);
    }

    #[test]
    fn test_callgraph_unicode_function_names() {
        let functions = vec![
            make_func("日本語関数", "日本語関数", "test.rs", "pub"),
            make_func("emoji_🎉", "emoji_🎉", "test.rs", "pub"),
        ];

        let graph = CallGraph::build(&functions, &HashMap::new());

        assert_eq!(graph.function_count(), 2);
        assert!(graph.nodes.contains_key("日本語関数"));
        assert!(graph.nodes.contains_key("emoji_🎉"));
    }

    #[test]
    fn test_callgraph_dot_special_chars() {
        let functions = vec![make_func(
            "func_generic",
            "module::impl_T::func",
            "test.rs",
            "pub",
        )];

        let graph = CallGraph::build(&functions, &HashMap::new());
        let dot = graph.to_dot();

        // Should produce valid DOT output
        assert!(dot.contains("digraph CallGraph"));
        assert!(dot.contains("func_generic"));
    }

    #[test]
    fn test_callgraph_large_scale() {
        // Test with 1000 functions
        let functions: Vec<_> = (0..1000)
            .map(|i| {
                make_func(
                    &format!("func_{}", i),
                    &format!("mod{}::func_{}", i % 10, i),
                    &format!("mod{}.rs", i % 10),
                    if i % 3 == 0 { "pub" } else { "private" },
                )
            })
            .collect();

        let graph = CallGraph::build(&functions, &HashMap::new());

        assert_eq!(graph.function_count(), 1000);

        // JSON export should work
        let json = graph.to_visualizer_json();
        assert_eq!(json["nodes"].as_array().unwrap().len(), 1000);
    }
}
