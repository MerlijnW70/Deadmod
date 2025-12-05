//! deadmod-core: NASA-grade dead module detection library for Rust
//!
//! This library provides modular components for scanning, parsing, and analyzing
//! Rust codebases to detect dead (unreachable) modules and functions.
//!
//! # Features
//!
//! - **Module-level detection**: Find unused modules (`mod foo;`)
//! - **Function-level detection**: Find unused functions, methods, and impl blocks
//! - **Generic detection**: Find unused type parameters and lifetimes
//! - **Trait detection**: Find unused trait methods and inherent impl methods
//! - **Macro detection**: Find unused `macro_rules!` definitions
//! - **Constant detection**: Find unused `const` and `static` items
//! - **Enum variant detection**: Find unused enum variants
//! - **Match arm detection**: Find dead match arms and wildcard masking
//! - **Call graph analysis**: Build and visualize function call graphs
//! - **Incremental caching**: Only re-parse changed files
//! - **Workspace support**: Analyze entire Cargo workspaces
//!
//! # Quick Start
//!
//! Use the [`prelude`] module for convenient imports:
//!
//! ```rust,ignore
//! use deadmod_core::prelude::*;
//!
//! let result = Deadmod::new("/path/to/crate")
//!     .with_cache(true)
//!     .analyze()?;
//!
//! for dead in &result.dead_modules {
//!     println!("Dead module: {}", dead);
//! }
//! ```
//!
//! # Module Organization
//!
//! - [`cache`]: Incremental parsing cache with SHA-256 change detection
//! - [`parse`]: AST parsing and module dependency extraction
//! - [`graph`]: Dependency graph construction and reachability analysis
//! - [`detect`]: Dead module detection logic
//! - [`scan`]: Parallel file discovery
//! - [`fix`]: Auto-fix functionality to remove dead code
//! - [`builder`]: Fluent builder API for configuration
//! - [`error`]: Typed error handling
//!
//! # Cargo Features
//!
//! - `fix` (default): Enable auto-fix functionality
//! - `html` (default): Enable HTML visualization output
//! - `callgraph` (default): Enable function call graph analysis
//! - `pixi`: Enable WebGL/PixiJS visualization
//! - `full`: Enable all optional features

// Core modules (always available)
pub mod builder;
pub mod cache;
pub mod common;
pub mod config;
pub mod detect;
pub mod error;
pub mod graph;
pub mod logging;
pub mod parse;
pub mod prelude;
pub mod report;
pub mod root;
pub mod scan;
pub mod workspace;

// Common trait re-exports
pub use common::GraphTraversal;

// Feature-gated modules
#[cfg(feature = "fix")]
pub mod fix;

#[cfg(feature = "callgraph")]
pub mod callgraph;

#[cfg(feature = "html")]
pub mod visualize;
#[cfg(feature = "html")]
pub mod visualize_html;

#[cfg(feature = "pixi")]
pub mod visualize_pixi;

// Detection modules (always available as core functionality)
pub mod constants;
pub mod enums;
pub mod func;
pub mod generics;
pub mod macros;
pub mod matcharms;
pub mod traits;

// ============================================================================
// Explicit Re-exports (avoiding glob imports for clear API surface)
// ============================================================================

// Error types
pub use error::{DeadmodError, DeadmodResult, IoResultExt};

// Builder API
pub use builder::{AnalysisResult, Deadmod, DeadItem, DeadItemKind};

// Cache types
pub use cache::{
    incremental_parse, load_cache, save_cache, file_hash,
    CacheMetadata, CachedModule, CachedVisibility, DeadmodCache,
};

// Configuration
pub use config::{load_config, DeadmodConfig, OutputConfig};

// Core detection
pub use detect::find_dead;

// Graph building
pub use graph::{
    build_graph, module_graph_to_visualizer_json,
    reachable_from_root, reachable_from_roots,
};

// Logging
pub use logging::{init_structured_logging, log_error, log_event, log_info, log_warn};

// Parsing
pub use parse::{
    extract_module_info, extract_uses_and_decls,
    normalize_path_string, parse_modules, parse_modules_strict,
    parse_single_module, parse_single_module_strict,
    path_to_normalized_string,
    ModuleInfo, ParseResult, Visibility,
};

// Reporting
pub use report::{print_json, print_plain};

// Root detection
pub use root::find_root_modules;

// File scanning and module discovery
pub use scan::{
    gather_rs_files, gather_rs_files_with_excludes,
    discover_modules, get_cluster_tree,
    DiscoveredModule, ModuleCluster, ModuleDiscovery,
};

// Workspace analysis
pub use workspace::{
    analyze_crate, analyze_workspace, find_all_crates, find_crate_root,
    is_workspace_root, CrateAnalysis,
};

// Feature-gated re-exports
#[cfg(feature = "fix")]
pub use fix::{clean_empty_dirs, fix_dead_modules, remove_file, remove_mod_declaration, FixResult};

#[cfg(feature = "callgraph")]
pub use callgraph::{
    extract_call_usages, extract_call_usages_resolved, extract_callgraph_functions,
    extract_callgraph_parallel,
    collect_use_statements, resolve_call_full, resolve_call_path, segments_to_path,
    CallGraph, CallGraphAnalysis, CallGraphStats, CallgraphExtractionResult, CallUsageResult,
    FunctionDef, ModulePathContext, ResolvedCall, UseMap,
    VisualizerEdge, VisualizerGraph, VisualizerNode, VisualizerStats,
};

#[cfg(feature = "html")]
pub use visualize::generate_dot;
#[cfg(feature = "html")]
pub use visualize_html::generate_html_graph;

#[cfg(feature = "pixi")]
pub use visualize_pixi::generate_pixi_graph;

// Detection module re-exports
pub use constants::{
    extract_const_usage, extract_constants,
    ConstAnalysisResult, ConstDef, ConstExtractionResult, ConstGraph, ConstStats,
    ConstUsageResult, DeadConst,
};

pub use enums::{
    extract_variant_usage, extract_variants,
    DeadVariant, EnumAnalysisResult, EnumExtractionResult, EnumGraph, EnumStats,
    EnumUsageResult, EnumVariantDef,
};

pub use func::{
    extract_call_names, extract_calls, extract_functions, extract_functions_strict,
    CallSite, FuncAnalysisResult, FuncGraph, FuncStats, FunctionInfo,
};

pub use generics::{
    extract_generic_usages, extract_declared_generics,
    DeadGeneric, DeclaredGeneric, GenericAnalysisResult, GenericExtractionResult,
    GenericGraph, GenericKind, GenericStats, GenericUsageResult, ParentKind, ParentUsages,
};

pub use macros::{
    extract_macro_usages, extract_macros,
    DeadMacro, MacroAnalysisResult, MacroDef, MacroExtractionResult,
    MacroGraph, MacroStats, MacroUsageResult,
};

pub use matcharms::{
    extract_match_arms, extract_match_usages,
    DeadArmReason, DeadMatchArm, MatchArm, MatchArmAnalysisResult, MatchArmStats,
    MatchExtractionResult, MatchGraph, MatchUsageResult,
};

pub use traits::{
    extract_called_method_names, extract_trait_usages, extract_traits,
    InherentImplMethod, TraitAnalysisResult, TraitExtractionResult, TraitGraph,
    TraitImplMethod, TraitMethodDef, TraitMethodUsage, TraitStats, UsageKind,
};

#[cfg(test)]
mod tests;
