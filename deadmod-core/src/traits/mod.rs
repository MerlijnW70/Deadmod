//! Trait and method analysis for dead code detection.
//!
//! This module provides functionality to detect dead methods:
//! - Trait definitions with required vs provided methods
//! - Trait impl blocks (`impl Trait for Type`)
//! - Inherent impl blocks (`impl Type { fn method() {} }`)
//! - Method call detection for all methods
//! - Dead method detection via reachability analysis
//!
//! # Architecture
//!
//! ```text
//! ┌─────────────────────┐     ┌─────────────────────┐
//! │  trait_extractor.rs │     │   trait_usage.rs    │
//! │  ─────────────────  │     │  ─────────────────  │
//! │  Extract trait defs │     │  Extract method     │
//! │  impl blocks, and   │     │  call sites         │
//! │  inherent impls     │     │                     │
//! └──────────┬──────────┘     └──────────┬──────────┘
//!            │                           │
//!            └───────────┬───────────────┘
//!                        ▼
//!            ┌─────────────────────┐
//!            │   trait_graph.rs    │
//!            │  ─────────────────  │
//!            │  Build call graph   │
//!            │  Find dead methods  │
//!            └─────────────────────┘
//! ```
//!
//! # Example
//!
//! ```ignore
//! use deadmod_core::traits::{extract_traits, extract_trait_usages, TraitGraph};
//!
//! // Extract trait definitions and implementations
//! let extraction = extract_traits(&path, &content);
//!
//! // Extract method usages
//! let usages = extract_trait_usages(&path, &content);
//!
//! // Build graph and analyze
//! let graph = TraitGraph::build(&[extraction], &[usages]);
//! let result = graph.analyze();
//!
//! for dead in &result.dead_trait_methods {
//!     println!("Dead: {}::{}", dead.trait_name, dead.method_name);
//! }
//!
//! for dead in &result.dead_inherent_methods {
//!     println!("Dead: {}", dead.full_id);
//! }
//! ```

pub mod trait_extractor;
pub mod trait_graph;
pub mod trait_usage;

// Re-exports for convenience
pub use trait_extractor::{
    extract_traits, InherentImplMethod, TraitExtractionResult, TraitImplMethod, TraitMethodDef,
};
pub use trait_graph::{TraitAnalysisResult, TraitGraph, TraitStats};
pub use trait_usage::{
    extract_called_method_names, extract_trait_usages, TraitMethodUsage, UsageKind,
};
