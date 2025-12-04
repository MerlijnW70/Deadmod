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

pub mod cache;
pub mod callgraph;
pub mod config;
pub mod constants;
pub mod detect;
pub mod enums;
pub mod fix;
pub mod func;
pub mod generics;
pub mod graph;
pub mod logging;
pub mod macros;
pub mod matcharms;
pub mod parse;
pub mod report;
pub mod root;
pub mod scan;
pub mod traits;
pub mod visualize;
pub mod visualize_html;
pub mod visualize_pixi;
pub mod workspace;

// Re-export all public items for convenience
pub use callgraph::*;
pub use config::*;
pub use constants::*;
pub use detect::*;
pub use enums::*;
pub use fix::*;
pub use func::*;
pub use generics::*;
pub use graph::*;
pub use logging::*;
pub use macros::*;
pub use matcharms::*;
pub use parse::*;
pub use report::*;
pub use root::*;
pub use scan::*;
pub use traits::*;
pub use visualize_html::*;
pub use visualize_pixi::*;
pub use workspace::*;

#[cfg(test)]
mod tests;
