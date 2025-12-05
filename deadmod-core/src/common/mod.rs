//! Common utilities shared across analyzer modules.
//!
//! This module provides shared functionality to reduce code duplication
//! across the various extractor and analyzer modules.

mod visibility;
mod path_builder;

pub use visibility::visibility_str;
pub use path_builder::ModulePathBuilder;
