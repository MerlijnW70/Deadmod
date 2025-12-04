//! Function-level dead code detection.
//!
//! This module provides fine-grained analysis of unused functions,
//! methods, and associated functions within Rust codebases.
//!
//! # Components
//!
//! - `func_extractor`: Extracts all function declarations from AST
//! - `func_calls`: Detects all function call sites
//! - `func_graph`: Builds call graph and computes reachability
//!
//! # Example Usage
//!
//! ```ignore
//! use deadmod_core::func::{extract_functions, extract_call_names, FuncGraph};
//!
//! let content = std::fs::read_to_string("src/lib.rs")?;
//! let funcs = extract_functions(Path::new("src/lib.rs"), &content);
//! let calls = extract_call_names(Path::new("src/lib.rs"), &content);
//!
//! let mut file_calls = HashMap::new();
//! file_calls.insert("src/lib.rs".to_string(), calls);
//!
//! let graph = FuncGraph::build(&funcs, &file_calls);
//! let result = graph.analyze();
//!
//! for dead_func in &result.dead {
//!     println!("Dead: {} in {}", dead_func.full_path, dead_func.file);
//! }
//! ```

pub mod func_calls;
pub mod func_extractor;
pub mod func_graph;

pub use func_calls::{extract_call_names, extract_calls, CallSite};
pub use func_extractor::{extract_functions, extract_functions_strict, FunctionInfo};
pub use func_graph::{FuncAnalysisResult, FuncGraph, FuncStats};
