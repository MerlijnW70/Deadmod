//! Constant and static analysis for dead code detection.
//!
//! This module provides functionality to detect unused constants and statics:
//! - `const NAME: T = ...` declarations that are never used
//! - `static NAME: T = ...` declarations that are never used
//! - Associated constants in impl blocks
//!
//! # Architecture
//!
//! ```text
//! ┌─────────────────────┐     ┌─────────────────────┐
//! │ const_extractor.rs  │     │   const_usage.rs    │
//! │  ─────────────────  │     │  ─────────────────  │
//! │  Extract const/     │     │  Extract constant   │
//! │  static definitions │     │  references         │
//! └──────────┬──────────┘     └──────────┬──────────┘
//!            │                           │
//!            └───────────┬───────────────┘
//!                        ▼
//!            ┌─────────────────────┐
//!            │   const_graph.rs    │
//!            │  ─────────────────  │
//!            │  Compare declared   │
//!            │  vs used, find dead │
//!            └─────────────────────┘
//! ```
//!
//! # Example
//!
//! ```ignore
//! use deadmod_core::constants::{extract_constants, extract_const_usage, ConstGraph};
//!
//! // Extract declarations
//! let declarations = extract_constants(&path, &content);
//!
//! // Extract usages
//! let usages = extract_const_usage(&path, &content);
//!
//! // Build graph and analyze
//! let graph = ConstGraph::new(declarations, &[usages]);
//! let result = graph.analyze();
//!
//! for dead in &result.dead {
//!     println!("Unused constant '{}' in {}", dead.name, dead.file);
//! }
//! ```

pub mod const_extractor;
pub mod const_graph;
pub mod const_usage;

// Re-exports for convenience
pub use const_extractor::{extract_constants, ConstDef, ConstExtractionResult};
pub use const_graph::{ConstAnalysisResult, ConstGraph, ConstStats, DeadConst};
pub use const_usage::{extract_const_usage, ConstUsageResult};
