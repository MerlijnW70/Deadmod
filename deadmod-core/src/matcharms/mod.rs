//! Match arm analysis for dead code detection.
//!
//! This module provides functionality to detect dead match arms:
//! - Wildcard patterns that mask later arms
//! - Non-final wildcard patterns (potential unreachable code)
//! - Pattern usage tracking across the codebase
//!
//! # Architecture
//!
//! ```text
//! ┌─────────────────────┐     ┌─────────────────────┐
//! │  match_extractor.rs │     │   match_usage.rs    │
//! │  ─────────────────  │     │  ─────────────────  │
//! │  Extract match arms │     │  Extract variant    │
//! │  from expressions   │     │  usage sites        │
//! └──────────┬──────────┘     └──────────┬──────────┘
//!            │                           │
//!            └───────────┬───────────────┘
//!                        ▼
//!            ┌─────────────────────┐
//!            │   match_graph.rs    │
//!            │  ─────────────────  │
//!            │  Build graph and    │
//!            │  find dead arms     │
//!            └─────────────────────┘
//! ```
//!
//! # Example
//!
//! ```ignore
//! use deadmod_core::matcharms::{extract_match_arms, extract_match_usages, MatchGraph};
//!
//! // Extract match arms from a file
//! let extraction = extract_match_arms(&path, &content);
//!
//! // Extract variant usages
//! let usages = extract_match_usages(&path, &content);
//!
//! // Build graph and analyze
//! let graph = MatchGraph::new(extraction.arms, extraction.match_count, &[usages]);
//! let result = graph.analyze();
//!
//! for dead in &result.dead_arms {
//!     println!("Dead arm: {} in {} ({:?})", dead.pattern, dead.file, dead.reason);
//! }
//! ```

pub mod match_extractor;
pub mod match_graph;
pub mod match_usage;

// Re-exports for convenience
pub use match_extractor::{extract_match_arms, MatchArm, MatchExtractionResult};
pub use match_graph::{DeadArmReason, DeadMatchArm, MatchArmAnalysisResult, MatchArmStats, MatchGraph};
pub use match_usage::{extract_match_usages, MatchUsageResult};
