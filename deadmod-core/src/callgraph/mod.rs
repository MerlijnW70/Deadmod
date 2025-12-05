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

use rayon::prelude::*;
use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;

pub mod extractor;
pub mod graph;
pub mod path_resolver;
pub mod usage;

// Re-exports for convenience
pub use extractor::{extract_callgraph_functions, FunctionDef};
pub use graph::{
    CallGraph, CallGraphAnalysis, CallGraphStats,
    VisualizerEdge, VisualizerGraph, VisualizerNode, VisualizerStats,
};
pub use path_resolver::{
    collect_use_statements, resolve_call_full, resolve_call_path, segments_to_path,
    ModulePathContext, ResolvedCall, UseMap,
};
pub use usage::{extract_call_usages, extract_call_usages_resolved, CallUsageResult};

/// Result of parallel callgraph extraction from multiple files.
#[derive(Debug, Default)]
pub struct CallgraphExtractionResult {
    /// All function definitions found across all files
    pub functions: Vec<FunctionDef>,
    /// Map from file path to call usages
    pub usage_map: HashMap<String, CallUsageResult>,
}

/// Extract function definitions and call usages from multiple files in parallel.
///
/// This is the recommended way to build callgraphs for large codebases,
/// as it processes files concurrently using Rayon's work-stealing scheduler.
///
/// # Performance
/// - Uses parallel file I/O and parsing
/// - Automatically scales to available CPU cores
/// - Estimated 4-8x speedup on multi-core systems
///
/// # Example
/// ```ignore
/// let files: Vec<PathBuf> = gather_rs_files(&root)?;
/// let result = extract_callgraph_parallel(&files);
/// let graph = CallGraph::build(&result.functions, &result.usage_map);
/// ```
pub fn extract_callgraph_parallel(files: &[PathBuf]) -> CallgraphExtractionResult {
    // Process files in parallel, collecting (functions, usages) tuples
    let results: Vec<(Vec<FunctionDef>, String, CallUsageResult)> = files
        .par_iter()
        .filter_map(|path| {
            // Read file content
            let content = fs::read_to_string(path).ok()?;

            // Extract functions and usages
            let functions = extractor::extract_callgraph_functions(path, &content);
            let usages = usage::extract_call_usages(path, &content);
            let path_str = path.display().to_string();

            Some((functions, path_str, usages))
        })
        .collect();

    // Combine results
    let mut combined = CallgraphExtractionResult::default();
    for (functions, path_str, usages) in results {
        combined.functions.extend(functions);
        combined.usage_map.insert(path_str, usages);
    }

    combined
}
