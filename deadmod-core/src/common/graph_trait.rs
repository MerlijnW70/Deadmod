//! Shared graph traversal abstraction.
//!
//! Provides a common interface for graph traversal operations,
//! eliminating code duplication across module graph and call graph implementations.

use std::collections::{HashSet, VecDeque};
use std::hash::Hash;

/// Trait for graph traversal operations.
///
/// This abstraction allows sharing BFS reachability logic across
/// different graph implementations (module graph, call graph, etc.).
///
/// # Type Parameters
/// - `Node`: The node identifier type (e.g., `&str`, `String`)
///
/// # Example
/// ```ignore
/// impl GraphTraversal for CallGraph {
///     type Node = String;
///
///     fn neighbors(&self, node: &String) -> Vec<String> {
///         self.adjacency.get(node).cloned().unwrap_or_default()
///     }
///
///     fn contains_node(&self, node: &String) -> bool {
///         self.nodes.contains_key(node)
///     }
/// }
///
/// // Use default BFS implementation
/// let reachable = graph.reachable_from(entry_points);
/// ```
pub trait GraphTraversal {
    /// The type used to identify nodes in the graph.
    type Node: Clone + Eq + Hash;

    /// Returns all neighbors (outgoing edges) of a node.
    fn neighbors(&self, node: &Self::Node) -> Vec<Self::Node>;

    /// Checks if the graph contains a node.
    fn contains_node(&self, node: &Self::Node) -> bool;

    /// Performs multi-source BFS to find all nodes reachable from the given roots.
    ///
    /// This is the optimal approach for finding reachability from multiple entry points:
    /// - Complexity: O(|V| + |E|) regardless of number of roots
    /// - Eliminates redundant traversals compared to calling single-source BFS N times
    ///
    /// # Arguments
    /// * `roots` - Iterator of root node identifiers (entry points)
    ///
    /// # Returns
    /// Set of all node identifiers reachable from any root
    fn reachable_from<I>(&self, roots: I) -> HashSet<Self::Node>
    where
        I: IntoIterator<Item = Self::Node>,
    {
        let mut visited = HashSet::new();
        let mut queue = VecDeque::new();

        // Initialize BFS with all valid root nodes
        for root in roots {
            if self.contains_node(&root) && !visited.contains(&root) {
                visited.insert(root.clone());
                queue.push_back(root);
            }
        }

        // Perform single, unified BFS traversal
        // Total complexity: O(|V| + |E|) as each node/edge visited at most once
        while let Some(node) = queue.pop_front() {
            for neighbor in self.neighbors(&node) {
                if !visited.contains(&neighbor) {
                    visited.insert(neighbor.clone());
                    queue.push_back(neighbor);
                }
            }
        }

        visited
    }

    /// Performs BFS to find all nodes reachable from a single root.
    ///
    /// Convenience wrapper around `reachable_from` for single-root queries.
    fn reachable_from_single(&self, root: Self::Node) -> HashSet<Self::Node> {
        self.reachable_from(std::iter::once(root))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    /// Simple test graph implementation for unit testing.
    struct TestGraph {
        nodes: HashSet<String>,
        edges: HashMap<String, Vec<String>>,
    }

    impl TestGraph {
        fn new() -> Self {
            Self {
                nodes: HashSet::new(),
                edges: HashMap::new(),
            }
        }

        fn add_node(&mut self, node: &str) {
            self.nodes.insert(node.to_string());
        }

        fn add_edge(&mut self, from: &str, to: &str) {
            self.add_node(from);
            self.add_node(to);
            self.edges
                .entry(from.to_string())
                .or_default()
                .push(to.to_string());
        }
    }

    impl GraphTraversal for TestGraph {
        type Node = String;

        fn neighbors(&self, node: &String) -> Vec<String> {
            self.edges.get(node).cloned().unwrap_or_default()
        }

        fn contains_node(&self, node: &String) -> bool {
            self.nodes.contains(node)
        }
    }

    #[test]
    fn test_empty_graph() {
        let graph = TestGraph::new();
        let reachable = graph.reachable_from(Vec::<String>::new());
        assert!(reachable.is_empty());
    }

    #[test]
    fn test_single_node() {
        let mut graph = TestGraph::new();
        graph.add_node("a");

        let reachable = graph.reachable_from_single("a".to_string());
        assert_eq!(reachable.len(), 1);
        assert!(reachable.contains("a"));
    }

    #[test]
    fn test_linear_chain() {
        let mut graph = TestGraph::new();
        graph.add_edge("a", "b");
        graph.add_edge("b", "c");
        graph.add_edge("c", "d");

        let reachable = graph.reachable_from_single("a".to_string());
        assert_eq!(reachable.len(), 4);
        assert!(reachable.contains("a"));
        assert!(reachable.contains("b"));
        assert!(reachable.contains("c"));
        assert!(reachable.contains("d"));
    }

    #[test]
    fn test_multi_source() {
        let mut graph = TestGraph::new();
        // Branch 1: a -> b
        graph.add_edge("a", "b");
        // Branch 2: c -> d
        graph.add_edge("c", "d");
        // Unreachable node
        graph.add_node("unreachable");

        let reachable = graph.reachable_from(["a".to_string(), "c".to_string()]);
        assert_eq!(reachable.len(), 4);
        assert!(reachable.contains("a"));
        assert!(reachable.contains("b"));
        assert!(reachable.contains("c"));
        assert!(reachable.contains("d"));
        assert!(!reachable.contains("unreachable"));
    }

    #[test]
    fn test_cycle() {
        let mut graph = TestGraph::new();
        graph.add_edge("a", "b");
        graph.add_edge("b", "c");
        graph.add_edge("c", "a"); // Cycle back to a

        let reachable = graph.reachable_from_single("a".to_string());
        assert_eq!(reachable.len(), 3);
    }

    #[test]
    fn test_missing_root_ignored() {
        let mut graph = TestGraph::new();
        graph.add_node("a");

        // "missing" is not in graph, should be ignored
        let reachable = graph.reachable_from(["a".to_string(), "missing".to_string()]);
        assert_eq!(reachable.len(), 1);
        assert!(reachable.contains("a"));
    }
}
