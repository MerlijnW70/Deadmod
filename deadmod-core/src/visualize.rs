//! Graphviz DOT visualization for module dependency graphs.
//!
//! Optimized for memory efficiency with pre-allocated buffers
//! and the `std::fmt::Write` trait for clean string formatting.

use crate::parse::ModuleInfo;
use std::collections::{HashMap, HashSet};
use std::fmt::Write;

/// Generate a Graphviz DOT representation of the module graph.
///
/// - reachable modules are lightgreen
/// - dead modules are lightcoral
/// - edges represent "use" and "mod" dependencies
///
/// Performance optimizations:
/// - Pre-allocated string buffer based on graph size heuristics
/// - Uses `writeln!` macro for efficient, allocation-free formatting
/// - Single-pass iteration over nodes and edges
///
/// This is visually rich but simple enough for Graphviz to render on all platforms.
pub fn generate_dot(mods: &HashMap<String, ModuleInfo>, reachable: &HashSet<String>) -> String {
    // Estimate capacity: ~80 bytes/node + ~40 bytes/edge + 150 bytes header/footer
    let node_count = mods.len();
    let edge_count: usize = mods.values().map(|info| info.refs.len()).sum();
    let estimated_capacity = (node_count * 80) + (edge_count * 40) + 150;

    let mut dot = String::with_capacity(estimated_capacity);

    // Build DOT string using Write trait for efficient formatting
    let result = write_dot_content(&mut dot, mods, reachable);

    // Handle unlikely write errors gracefully (NASA-grade resilience)
    if let Err(e) = result {
        eprintln!("[ERROR] Failed to generate DOT string: {}", e);
        return "digraph deadmod {\n}\n".to_string();
    }

    dot
}

/// Internal function to write DOT content using the Write trait.
fn write_dot_content(
    dot: &mut String,
    mods: &HashMap<String, ModuleInfo>,
    reachable: &HashSet<String>,
) -> std::fmt::Result {
    // Graph header
    writeln!(dot, "digraph deadmod {{")?;
    writeln!(dot, "  rankdir=LR;")?;
    writeln!(
        dot,
        "  node [shape=box, style=filled, fontname=\"JetBrains Mono\"];"
    )?;
    writeln!(dot)?;

    // 1. NODES: Determine color based on reachability
    for name in mods.keys() {
        let color = if reachable.contains(name) {
            "lightgreen" // Reachable module
        } else {
            "lightcoral" // Dead module
        };
        writeln!(dot, "  \"{}\" [fillcolor={}];", name, color)?;
    }

    writeln!(dot)?;

    // 2. EDGES: Draw dependencies
    for (name, info) in mods {
        for dep in &info.refs {
            // Only draw edges to modules that exist in our graph
            if mods.contains_key(dep) {
                writeln!(dot, "  \"{}\" -> \"{}\";", name, dep)?;
            }
        }
    }

    writeln!(dot, "}}")?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn test_generate_dot_empty() {
        let mods = HashMap::new();
        let reachable = HashSet::new();
        let dot = generate_dot(&mods, &reachable);
        assert!(dot.contains("digraph deadmod"));
        assert!(dot.contains("rankdir=LR"));
    }

    #[test]
    fn test_generate_dot_with_modules() {
        let mut mods = HashMap::new();
        let mut main_info = ModuleInfo::new(PathBuf::from("src/main.rs"));
        main_info.refs.insert("utils".to_string());
        mods.insert("main".to_string(), main_info);
        mods.insert(
            "utils".to_string(),
            ModuleInfo::new(PathBuf::from("src/utils.rs")),
        );
        mods.insert(
            "dead".to_string(),
            ModuleInfo::new(PathBuf::from("src/dead.rs")),
        );

        let mut reachable = HashSet::new();
        reachable.insert("main".to_string());
        reachable.insert("utils".to_string());

        let dot = generate_dot(&mods, &reachable);

        // Check nodes exist
        assert!(dot.contains("\"main\""));
        assert!(dot.contains("\"utils\""));
        assert!(dot.contains("\"dead\""));

        // Check edge exists
        assert!(dot.contains("\"main\" -> \"utils\""));

        // Check colors
        assert!(dot.contains("lightgreen")); // for reachable
        assert!(dot.contains("lightcoral")); // for dead
    }

    #[test]
    fn test_generate_dot_font() {
        let mods = HashMap::new();
        let reachable = HashSet::new();
        let dot = generate_dot(&mods, &reachable);
        assert!(dot.contains("JetBrains Mono"));
    }
}
