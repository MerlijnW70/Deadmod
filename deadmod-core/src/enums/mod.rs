//! Enum variant analysis for dead code detection.
//!
//! This module provides functionality to detect unused enum variants:
//! - Variants that are never constructed
//! - Variants that are never matched against
//! - Fully dead enums (all variants unused)
//!
//! # Architecture
//!
//! ```text
//! ┌─────────────────────┐     ┌─────────────────────┐
//! │ enum_extractor.rs   │     │   enum_usage.rs     │
//! │  ─────────────────  │     │  ─────────────────  │
//! │  Extract enum       │     │  Extract variant    │
//! │  variant definitions│     │  usages             │
//! └──────────┬──────────┘     └──────────┬──────────┘
//!            │                           │
//!            └───────────┬───────────────┘
//!                        ▼
//!            ┌─────────────────────┐
//!            │   enum_graph.rs     │
//!            │  ─────────────────  │
//!            │  Compare declared   │
//!            │  vs used, find dead │
//!            └─────────────────────┘
//! ```
//!
//! # Example
//!
//! ```ignore
//! use deadmod_core::enums::{extract_variants, extract_variant_usage, EnumGraph};
//!
//! // Extract declarations
//! let declarations = extract_variants(&path, &content);
//!
//! // Extract usages
//! let usages = extract_variant_usage(&path, &content);
//!
//! // Build graph and analyze
//! let graph = EnumGraph::new(declarations, &[usages]);
//! let result = graph.analyze();
//!
//! for dead in &result.dead {
//!     println!("Unused variant '{}' in {}", dead.full_name, dead.file);
//! }
//! ```

pub mod enum_extractor;
pub mod enum_graph;
pub mod enum_usage;

// Re-exports for convenience
pub use enum_extractor::{extract_variants, EnumExtractionResult, EnumVariantDef};
pub use enum_graph::{DeadVariant, EnumAnalysisResult, EnumGraph, EnumStats};
pub use enum_usage::{extract_variant_usage, EnumUsageResult};
