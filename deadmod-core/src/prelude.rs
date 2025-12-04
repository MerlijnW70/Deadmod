//! Prelude module for convenient imports.
//!
//! Import commonly used types with a single line:
//!
//! ```rust,ignore
//! use deadmod_core::prelude::*;
//! ```
//!
//! This provides the most commonly needed types for dead code analysis
//! without polluting the namespace with rarely-used items.

// Core analysis types
pub use crate::error::{DeadmodError, DeadmodResult};
pub use crate::parse::{ModuleInfo, ParseResult};

// Graph building and traversal
pub use crate::graph::{build_graph, reachable_from_root, reachable_from_roots};

// Dead code detection
pub use crate::detect::find_dead;

// File scanning
pub use crate::scan::{gather_rs_files, gather_rs_files_with_excludes};

// Root module detection
pub use crate::root::find_root_modules;

// Workspace analysis
pub use crate::workspace::{analyze_crate, analyze_workspace, CrateAnalysis};

// Caching
pub use crate::cache::{incremental_parse, load_cache, save_cache, DeadmodCache};

// Configuration
pub use crate::config::{load_config, DeadmodConfig};

// Builder API
pub use crate::builder::{AnalysisResult, Deadmod};

// Fix functionality
#[cfg(feature = "fix")]
pub use crate::fix::{clean_empty_dirs, fix_dead_modules, FixResult};
