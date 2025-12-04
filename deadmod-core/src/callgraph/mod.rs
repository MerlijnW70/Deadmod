//! Function call graph analysis.
//!
//! This module provides functionality to build and analyze function call graphs:
//! - Extract all function definitions from the codebase
//! - Track all function calls and method invocations
//! - Build a directed graph of caller -> callee relationships
//! - Find unreachable (dead) functions
//! - Export to DOT (Graphviz) and JSON formats
//!
//! # Architecture
//!
//! ```text
//! ┌─────────────────────┐     ┌─────────────────────┐
//! │    extractor.rs     │     │      usage.rs       │
//! │  ─────────────────  │     │  ─────────────────  │
//! │  Extract function   │     │  Extract function   │
//! │  definitions        │     │  calls & usages     │
//! └──────────┬──────────┘     └──────────┬──────────┘
//!            │                           │
//!            └───────────┬───────────────┘
//!                        ▼
//!            ┌─────────────────────┐
//!            │      graph.rs       │
//!            │  ─────────────────  │
//!            │  Build call graph   │
//!            │  Find dead code     │
//!            │  Export DOT/JSON    │
//!            └─────────────────────┘
//! ```
//!
//! # Example
//!
//! ```ignore
//! use deadmod_core::callgraph::{extract_callgraph_functions, extract_call_usages, CallGraph};
//! use std::collections::HashMap;
//!
//! // Extract function definitions
//! let functions = extract_callgraph_functions(&path, &content);
//!
//! // Extract call usages
//! let usages = extract_call_usages(&path, &content);
//!
//! // Build usage map (file -> usages)
//! let mut usage_map = HashMap::new();
//! usage_map.insert(path.display().to_string(), usages);
//!
//! // Build call graph
//! let graph = CallGraph::build(&functions, &usage_map);
//!
//! // Analyze for dead code
//! let analysis = graph.analyze();
//! for func in &analysis.unreachable {
//!     println!("Unreachable: {} in {}", func.name, func.file);
//! }
//!
//! // Export to DOT
//! let dot = graph.to_dot();
//! println!("{}", dot);
//! ```

pub mod extractor;
pub mod graph;
pub mod path_resolver;
pub mod usage;

// Re-exports for convenience
pub use extractor::{extract_callgraph_functions, FunctionDef};
pub use graph::{CallGraph, CallGraphAnalysis, CallGraphStats};
pub use path_resolver::{
    collect_use_statements, resolve_call_full, resolve_call_path, segments_to_path,
    ModulePathContext, ResolvedCall, UseMap,
};
pub use usage::{extract_call_usages, extract_call_usages_resolved, CallUsageResult};
