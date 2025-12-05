//! Graph construction and reachability analysis using BFS.
//!
//! Performance characteristics:
//! - Graph build: O(|V| + |E|) where V = modules, E = dependencies
//! - Multi-source reachability: O(|V| + |E|) single traversal
//!
//! The multi-source BFS pattern eliminates redundant traversals when
//! analyzing from multiple entry points (main, lib, binaries).

use crate::parse::ModuleInfo;
use petgraph::graphmap::DiGraphMap;
use std::collections::{HashMap, HashSet, VecDeque};

/// Builds the dependency graph (DiGraphMap) from module information.
///
/// Uses `DiGraphMap<&str, ()>` for memory efficiency:
/// - String slices avoid ownership/cloning overhead
/// - Unit type `()` for edges minimizes memory footprint
pub fn build_graph(mods: &HashMap<String, ModuleInfo>) -> DiGraphMap<&str, ()> {
    let mut g = DiGraphMap::new();

    // 1. Add all nodes
    for name in mods.keys() {
        g.add_node(name.as_str());
    }

    // 2. Add all edges (dependencies)
    for (name, info) in mods {
        for dep in &info.refs {
            if mods.contains_key(dep) {
                g.add_edge(name.as_str(), dep.as_str(), ());
            }
        }
    }

    g
}

/// Performs Multi-Source BFS to find all modules reachable from a set of roots.
///
/// This is the optimal approach for finding reachability from multiple entry points:
/// - Complexity: O(|V| + |E|) regardless of number of roots
/// - Eliminates redundant traversals compared to calling single-source BFS N times
///
/// # Arguments
/// * `g` - The dependency graph
/// * `roots` - Iterator of root module names (entry points)
///
/// # Returns
/// Set of all module names reachable from any root
///
/// # Logging
/// Logs a warning for any root not found in the graph (helpful for debugging).
pub fn reachable_from_roots<'a>(
    g: &DiGraphMap<&'a str, ()>,
    roots: impl IntoIterator<Item = &'a str>,
) -> HashSet<&'a str> {
    let mut visited = HashSet::new();
    let mut queue = VecDeque::new();

    // Initialize BFS with all valid root nodes
    for root in roots {
        if g.contains_node(root) {
            // Combined check: node exists AND not already visited
            if visited.insert(root) {
                queue.push_back(root);
            }
        } else {
            // Log warning for missing roots (helpful for debugging configuration issues)
            eprintln!("[WARN] Root module not found in graph: '{}'", root);
        }
    }

    // Perform single, unified BFS traversal
    // Total complexity: O(|V| + |E|) as each node/edge visited at most once
    while let Some(node) = queue.pop_front() {
        for n in g.neighbors(node) {
            if visited.insert(n) {
                queue.push_back(n);
            }
        }
    }

    visited
}

/// Performs BFS to find all modules reachable from a single root.
///
/// Preserved for backwards compatibility. Internally delegates to `reachable_from_roots`.
///
/// For multiple roots, prefer `reachable_from_roots` directly to avoid
/// redundant O(|V| + |E|) traversals.
pub fn reachable_from_root<'a>(g: &DiGraphMap<&'a str, ()>, root: &'a str) -> HashSet<&'a str> {
    reachable_from_roots(g, std::iter::once(root))
}

/// Export module dependency graph in visualizer-compatible JSON format.
///
/// Output format for PixiJS visualizer:
/// ```json
/// {
///   "nodes": [{ "id": 0, "name": "module_name", "dead": false }],
///   "edges": [{ "from": 0, "to": 1 }]
/// }
/// ```
pub fn module_graph_to_visualizer_json(
    mods: &HashMap<String, ModuleInfo>,
    reachable: &HashSet<&str>,
) -> serde_json::Value {
    // Build name -> numeric ID mapping (sorted for deterministic output)
    let mut names: Vec<&String> = mods.keys().collect();
    names.sort();
    let name_to_id: HashMap<&String, usize> = names.iter().enumerate().map(|(i, n)| (*n, i)).collect();

    // Build nodes with dead status
    let nodes: Vec<serde_json::Value> = names
        .iter()
        .enumerate()
        .map(|(i, name)| {
            let info = &mods[*name];
            let is_dead = !reachable.contains(name.as_str());
            serde_json::json!({
                "id": i,
                "name": name,
                "file": info.path.display().to_string(),
                "dead": is_dead,
            })
        })
        .collect();

    // Build edges using numeric IDs
    let mut edges: Vec<serde_json::Value> = Vec::new();
    for (name, info) in mods {
        if let Some(&from_id) = name_to_id.get(name) {
            for dep in &info.refs {
                if let Some(&to_id) = name_to_id.get(dep) {
                    edges.push(serde_json::json!({
                        "from": from_id,
                        "to": to_id,
                    }));
                }
            }
        }
    }

    // Count dead modules
    let dead_count = nodes.iter().filter(|n| n["dead"].as_bool().unwrap_or(false)).count();

    serde_json::json!({
        "nodes": nodes,
        "edges": edges,
        "stats": {
            "total_modules": mods.len(),
            "total_edges": edges.len(),
            "dead_modules": dead_count,
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn create_module(name: &str, refs: &[&str]) -> (String, ModuleInfo) {
        let mut info = ModuleInfo::new(PathBuf::from(format!("src/{}.rs", name)));
        for r in refs {
            info.refs.insert(r.to_string());
        }
        (name.to_string(), info)
    }

    #[test]
    fn test_build_graph_basic() {
        let mut mods = HashMap::new();
        mods.insert("main".to_string(), ModuleInfo::new(PathBuf::from("src/main.rs")));
        mods.insert("utils".to_string(), ModuleInfo::new(PathBuf::from("src/utils.rs")));

        let g = build_graph(&mods);
        assert!(g.contains_node("main"));
        assert!(g.contains_node("utils"));
    }

    #[test]
    fn test_reachable_from_root_simple() {
        let mut mods = HashMap::new();
        let (name, mut info) = create_module("main", &[]);
        info.refs.insert("utils".to_string());
        mods.insert(name, info);
        mods.insert(create_module("utils", &[]).0, create_module("utils", &[]).1);
        mods.insert(create_module("dead", &[]).0, create_module("dead", &[]).1);

        let g = build_graph(&mods);
        let reachable = reachable_from_root(&g, "main");

        assert!(reachable.contains("main"));
        assert!(reachable.contains("utils"));
        assert!(!reachable.contains("dead"));
    }

    #[test]
    fn test_reachable_from_roots_multi_source() {
        let mut mods = HashMap::new();

        // main -> utils
        let mut main_info = ModuleInfo::new(PathBuf::from("src/main.rs"));
        main_info.refs.insert("utils".to_string());
        mods.insert("main".to_string(), main_info);

        // lib -> config
        let mut lib_info = ModuleInfo::new(PathBuf::from("src/lib.rs"));
        lib_info.refs.insert("config".to_string());
        mods.insert("lib".to_string(), lib_info);

        mods.insert("utils".to_string(), ModuleInfo::new(PathBuf::from("src/utils.rs")));
        mods.insert("config".to_string(), ModuleInfo::new(PathBuf::from("src/config.rs")));
        mods.insert("dead".to_string(), ModuleInfo::new(PathBuf::from("src/dead.rs")));

        let g = build_graph(&mods);

        // Multi-source BFS from both main and lib
        let reachable = reachable_from_roots(&g, ["main", "lib"]);

        assert!(reachable.contains("main"));
        assert!(reachable.contains("lib"));
        assert!(reachable.contains("utils"));
        assert!(reachable.contains("config"));
        assert!(!reachable.contains("dead"));
    }

    #[test]
    fn test_reachable_from_roots_missing_root() {
        let mut mods = HashMap::new();
        mods.insert("main".to_string(), ModuleInfo::new(PathBuf::from("src/main.rs")));

        let g = build_graph(&mods);

        // Include a non-existent root - should be skipped gracefully
        let reachable = reachable_from_roots(&g, ["main", "nonexistent"]);

        assert!(reachable.contains("main"));
        assert_eq!(reachable.len(), 1);
    }

    #[test]
    fn test_reachable_from_roots_empty() {
        let mods: HashMap<String, ModuleInfo> = HashMap::new();
        let g = build_graph(&mods);

        let reachable = reachable_from_roots(&g, std::iter::empty::<&str>());
        assert!(reachable.is_empty());
    }

    #[test]
    fn test_module_graph_to_visualizer_json() {
        let mut mods = HashMap::new();

        // main -> utils
        let mut main_info = ModuleInfo::new(PathBuf::from("src/main.rs"));
        main_info.refs.insert("utils".to_string());
        mods.insert("main".to_string(), main_info);

        mods.insert("utils".to_string(), ModuleInfo::new(PathBuf::from("src/utils.rs")));
        mods.insert("dead".to_string(), ModuleInfo::new(PathBuf::from("src/dead.rs")));

        let g = build_graph(&mods);
        let reachable = reachable_from_roots(&g, ["main"]);
        let json = module_graph_to_visualizer_json(&mods, &reachable);

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

        // Check stats
        assert_eq!(json["stats"]["total_modules"].as_u64(), Some(3));
        assert_eq!(json["stats"]["dead_modules"].as_u64(), Some(1));
    }
}
