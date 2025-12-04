//! Macro analysis for dead macro detection.
//!
//! This module provides functionality to detect unused macros:
//! - `macro_rules!` definitions that are never invoked
//! - `#[macro_export]` macros that aren't used within the crate
//!
//! # Architecture
//!
//! ```text
//! ┌─────────────────────┐     ┌─────────────────────┐
//! │ macro_extractor.rs  │     │   macro_usage.rs    │
//! │  ─────────────────  │     │  ─────────────────  │
//! │  Extract macro_rules│     │  Extract macro!()   │
//! │  definitions        │     │  invocations        │
//! └──────────┬──────────┘     └──────────┬──────────┘
//!            │                           │
//!            └───────────┬───────────────┘
//!                        ▼
//!            ┌─────────────────────┐
//!            │   macro_graph.rs    │
//!            │  ─────────────────  │
//!            │  Compare declared   │
//!            │  vs used, find dead │
//!            └─────────────────────┘
//! ```
//!
//! # Example
//!
//! ```ignore
//! use deadmod_core::macros::{extract_macros, extract_macro_usages, MacroGraph};
//!
//! // Extract declarations
//! let declarations = extract_macros(&path, &content);
//!
//! // Extract usages
//! let usages = extract_macro_usages(&path, &content);
//!
//! // Build graph and analyze
//! let graph = MacroGraph::new(declarations, &[usages]);
//! let result = graph.analyze();
//!
//! for dead in &result.dead {
//!     println!("Unused macro '{}' in {}", dead.name, dead.file);
//! }
//! ```

pub mod macro_extractor;
pub mod macro_graph;
pub mod macro_usage;

// Re-exports for convenience
pub use macro_extractor::{extract_macros, MacroDef, MacroExtractionResult};
pub use macro_graph::{DeadMacro, MacroAnalysisResult, MacroGraph, MacroStats};
pub use macro_usage::{extract_macro_usages, MacroUsageResult};
