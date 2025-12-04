//! Generic and lifetime parameter analysis for dead code detection.
//!
//! This module provides functionality to detect unused generic parameters:
//! - Unused type parameters: `fn foo<T>()` where T is never used
//! - Unused lifetimes: `fn bar<'a>()` where 'a is never referenced
//! - Unused const generics: `struct Array<const N: usize>` where N isn't used
//! - Unused trait bounds: `T: Debug` where Debug methods are never called
//!
//! # Architecture
//!
//! ```text
//! ┌─────────────────────┐     ┌─────────────────────┐
//! │ generic_extractor.rs│     │  generic_usage.rs   │
//! │  ─────────────────  │     │  ─────────────────  │
//! │  Extract declared   │     │  Extract usages of  │
//! │  generics/lifetimes │     │  generics/lifetimes │
//! └──────────┬──────────┘     └──────────┬──────────┘
//!            │                           │
//!            └───────────┬───────────────┘
//!                        ▼
//!            ┌─────────────────────┐
//!            │   generic_graph.rs  │
//!            │  ─────────────────  │
//!            │  Compare declared   │
//!            │  vs used, find dead │
//!            └─────────────────────┘
//! ```
//!
//! # Example
//!
//! ```ignore
//! use deadmod_core::generics::{extract_declared_generics, extract_generic_usages, GenericGraph};
//!
//! // Extract declarations
//! let declarations = extract_declared_generics(&path, &content);
//!
//! // Extract usages
//! let usages = extract_generic_usages(&path, &content);
//!
//! // Build graph and analyze
//! let graph = GenericGraph::new(&[declarations], &[usages]);
//! let result = graph.analyze();
//!
//! for dead in &result.dead {
//!     println!("Unused {} '{}' in {}", dead.kind, dead.name, dead.parent);
//! }
//! ```

pub mod generic_extractor;
pub mod generic_graph;
pub mod generic_usage;

// Re-exports for convenience
pub use generic_extractor::{
    extract_declared_generics, DeclaredGeneric, GenericExtractionResult, GenericKind, ParentKind,
};
pub use generic_graph::{DeadGeneric, GenericAnalysisResult, GenericGraph, GenericStats};
pub use generic_usage::{extract_generic_usages, GenericUsageResult, ParentUsages};
