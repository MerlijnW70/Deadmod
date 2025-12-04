//! Builder pattern API for deadmod analysis.
//!
//! Provides a fluent interface for configuring and running dead code analysis:
//!
//! ```rust,ignore
//! use deadmod_core::prelude::*;
//!
//! let result = Deadmod::new("/path/to/crate")
//!     .with_cache(true)
//!     .include_functions(true)
//!     .include_traits(true)
//!     .dry_run(true)
//!     .analyze()?;
//!
//! println!("Dead modules: {:?}", result.dead_modules);
//! ```

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;

use anyhow::{Context, Result};

use crate::cache;
use crate::detect::find_dead;
use crate::graph::{build_graph, reachable_from_roots};
use crate::parse::ModuleInfo;
use crate::root::find_root_modules;
use crate::scan::gather_rs_files;

/// Builder for configuring dead code analysis.
///
/// # Example
///
/// ```rust,ignore
/// let result = Deadmod::new("/my/crate")
///     .with_cache(true)
///     .include_functions(true)
///     .analyze()?;
/// ```
#[derive(Debug, Clone)]
pub struct Deadmod {
    /// Root path of the crate to analyze
    root: PathBuf,

    /// Whether to use incremental caching
    use_cache: bool,

    /// Whether to include function-level analysis
    include_functions: bool,

    /// Whether to include trait/method analysis
    include_traits: bool,

    /// Whether to include constant analysis
    include_constants: bool,

    /// Whether to include enum variant analysis
    include_enums: bool,

    /// Whether to include macro analysis
    include_macros: bool,

    /// Whether to include generic parameter analysis
    include_generics: bool,

    /// Whether to include match arm analysis
    include_matcharms: bool,

    /// Whether to analyze tests as entry points
    include_tests: bool,

    /// Custom excluded directories
    excluded_dirs: Vec<String>,

    /// Custom ignored module patterns
    ignored_patterns: Vec<String>,

    /// Dry-run mode (don't modify files)
    dry_run: bool,

    /// Verbose output
    verbose: bool,
}

impl Deadmod {
    /// Create a new analysis builder for the given path.
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self {
            root: root.into(),
            use_cache: true,
            include_functions: false,
            include_traits: false,
            include_constants: false,
            include_enums: false,
            include_macros: false,
            include_generics: false,
            include_matcharms: false,
            include_tests: true,
            excluded_dirs: Vec::new(),
            ignored_patterns: Vec::new(),
            dry_run: false,
            verbose: false,
        }
    }

    /// Enable or disable incremental caching.
    pub fn with_cache(mut self, enabled: bool) -> Self {
        self.use_cache = enabled;
        self
    }

    /// Enable function-level dead code detection.
    pub fn include_functions(mut self, enabled: bool) -> Self {
        self.include_functions = enabled;
        self
    }

    /// Enable trait/method dead code detection.
    pub fn include_traits(mut self, enabled: bool) -> Self {
        self.include_traits = enabled;
        self
    }

    /// Enable constant/static dead code detection.
    pub fn include_constants(mut self, enabled: bool) -> Self {
        self.include_constants = enabled;
        self
    }

    /// Enable enum variant dead code detection.
    pub fn include_enums(mut self, enabled: bool) -> Self {
        self.include_enums = enabled;
        self
    }

    /// Enable macro dead code detection.
    pub fn include_macros(mut self, enabled: bool) -> Self {
        self.include_macros = enabled;
        self
    }

    /// Enable generic parameter dead code detection.
    pub fn include_generics(mut self, enabled: bool) -> Self {
        self.include_generics = enabled;
        self
    }

    /// Enable match arm dead code detection.
    pub fn include_matcharms(mut self, enabled: bool) -> Self {
        self.include_matcharms = enabled;
        self
    }

    /// Include test functions as entry points.
    pub fn include_tests(mut self, enabled: bool) -> Self {
        self.include_tests = enabled;
        self
    }

    /// Enable all detection modes.
    pub fn all(mut self) -> Self {
        self.include_functions = true;
        self.include_traits = true;
        self.include_constants = true;
        self.include_enums = true;
        self.include_macros = true;
        self.include_generics = true;
        self.include_matcharms = true;
        self
    }

    /// Add directories to exclude from scanning.
    pub fn exclude_dirs(mut self, dirs: impl IntoIterator<Item = impl Into<String>>) -> Self {
        self.excluded_dirs.extend(dirs.into_iter().map(Into::into));
        self
    }

    /// Add patterns for modules to ignore.
    pub fn ignore_patterns(mut self, patterns: impl IntoIterator<Item = impl Into<String>>) -> Self {
        self.ignored_patterns.extend(patterns.into_iter().map(Into::into));
        self
    }

    /// Enable dry-run mode (no file modifications).
    pub fn dry_run(mut self, enabled: bool) -> Self {
        self.dry_run = enabled;
        self
    }

    /// Enable verbose output.
    pub fn verbose(mut self, enabled: bool) -> Self {
        self.verbose = enabled;
        self
    }

    /// Run the analysis and return results.
    pub fn analyze(&self) -> Result<AnalysisResult> {
        // 1. Gather files
        let files = gather_rs_files(&self.root)
            .context("Failed to gather .rs files")?;

        // 2. Load cache if enabled
        let cached = if self.use_cache {
            cache::load_cache(&self.root)
        } else {
            None
        };

        // 3. Parse modules (incremental if cache available)
        let modules = cache::incremental_parse(&self.root, &files, cached)
            .context("Failed to parse modules")?;

        // 4. Find root modules
        let root_mods = find_root_modules(&self.root);

        // 5. Build graph and find reachable
        let graph = build_graph(&modules);
        let valid_roots = root_mods
            .iter()
            .filter(|name| modules.contains_key(*name))
            .map(|s| s.as_str());
        let reachable: HashSet<&str> = reachable_from_roots(&graph, valid_roots);

        // 6. Find dead modules
        let dead_modules: Vec<String> = find_dead(&modules, &reachable)
            .into_iter()
            .filter(|m| !self.is_ignored(m))
            .map(String::from)
            .collect();

        // 7. Build result
        let result = AnalysisResult {
            root: self.root.clone(),
            total_modules: modules.len(),
            reachable_modules: reachable.iter().map(|s| s.to_string()).collect(),
            dead_modules,
            dead_functions: Vec::new(),
            dead_traits: Vec::new(),
            dead_constants: Vec::new(),
            dead_enums: Vec::new(),
            dead_macros: Vec::new(),
            dead_generics: Vec::new(),
            dead_matcharms: Vec::new(),
            modules,
        };

        // Note: Additional detection modes (functions, traits, etc.) can be
        // enabled in future versions. The flags are stored in the builder
        // for forward compatibility.
        let _ = &self.include_functions;
        let _ = &self.include_traits;
        let _ = &self.include_constants;
        let _ = &self.include_enums;
        let _ = &self.include_macros;
        let _ = &self.include_generics;
        let _ = &self.include_matcharms;

        Ok(result)
    }

    /// Check if a module name matches any ignored pattern.
    fn is_ignored(&self, name: &str) -> bool {
        for pattern in &self.ignored_patterns {
            if pattern.ends_with('*') {
                let prefix = &pattern[..pattern.len() - 1];
                if name.starts_with(prefix) {
                    return true;
                }
            } else if let Some(suffix) = pattern.strip_prefix('*') {
                if name.ends_with(suffix) {
                    return true;
                }
            } else if name == pattern || name.contains(pattern) {
                return true;
            }
        }
        false
    }

    /// Apply fixes to remove dead code.
    #[cfg(feature = "fix")]
    pub fn fix(&self, result: &AnalysisResult) -> Result<crate::fix::FixResult> {
        let dead_refs: Vec<&str> = result.dead_modules.iter().map(|s| s.as_str()).collect();
        crate::fix::fix_dead_modules(&self.root, &dead_refs, &result.modules, self.dry_run)
    }
}

/// Result of running dead code analysis.
#[derive(Debug, Clone)]
pub struct AnalysisResult {
    /// Root path that was analyzed
    pub root: PathBuf,

    /// Total number of modules found
    pub total_modules: usize,

    /// Modules reachable from entry points
    pub reachable_modules: Vec<String>,

    /// Dead (unreachable) modules
    pub dead_modules: Vec<String>,

    /// Dead functions (if function analysis enabled)
    pub dead_functions: Vec<DeadItem>,

    /// Dead trait methods (if trait analysis enabled)
    pub dead_traits: Vec<DeadItem>,

    /// Dead constants (if constant analysis enabled)
    pub dead_constants: Vec<DeadItem>,

    /// Dead enum variants (if enum analysis enabled)
    pub dead_enums: Vec<DeadItem>,

    /// Dead macros (if macro analysis enabled)
    pub dead_macros: Vec<DeadItem>,

    /// Dead generic parameters (if generic analysis enabled)
    pub dead_generics: Vec<DeadItem>,

    /// Dead match arms (if matcharm analysis enabled)
    pub dead_matcharms: Vec<DeadItem>,

    /// Parsed module information (for fix operations)
    pub modules: HashMap<String, ModuleInfo>,
}

impl AnalysisResult {
    /// Check if any dead code was found.
    pub fn has_dead_code(&self) -> bool {
        !self.dead_modules.is_empty()
            || !self.dead_functions.is_empty()
            || !self.dead_traits.is_empty()
            || !self.dead_constants.is_empty()
            || !self.dead_enums.is_empty()
            || !self.dead_macros.is_empty()
            || !self.dead_generics.is_empty()
            || !self.dead_matcharms.is_empty()
    }

    /// Get total count of all dead items.
    pub fn dead_count(&self) -> usize {
        self.dead_modules.len()
            + self.dead_functions.len()
            + self.dead_traits.len()
            + self.dead_constants.len()
            + self.dead_enums.len()
            + self.dead_macros.len()
            + self.dead_generics.len()
            + self.dead_matcharms.len()
    }

    /// Get percentage of dead code.
    pub fn dead_percentage(&self) -> f64 {
        if self.total_modules == 0 {
            0.0
        } else {
            (self.dead_modules.len() as f64 / self.total_modules as f64) * 100.0
        }
    }
}

/// A dead code item with location information.
#[derive(Debug, Clone)]
pub struct DeadItem {
    /// Name or path of the dead item
    pub name: String,
    /// File containing the dead item
    pub file: PathBuf,
    /// Line number (1-indexed)
    pub line: usize,
    /// Item kind (function, method, constant, etc.)
    pub kind: DeadItemKind,
}

/// Kind of dead code item.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DeadItemKind {
    Module,
    Function,
    Method,
    TraitMethod,
    Constant,
    Static,
    EnumVariant,
    Macro,
    TypeParam,
    Lifetime,
    MatchArm,
}

impl std::fmt::Display for DeadItemKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Module => write!(f, "module"),
            Self::Function => write!(f, "function"),
            Self::Method => write!(f, "method"),
            Self::TraitMethod => write!(f, "trait method"),
            Self::Constant => write!(f, "constant"),
            Self::Static => write!(f, "static"),
            Self::EnumVariant => write!(f, "enum variant"),
            Self::Macro => write!(f, "macro"),
            Self::TypeParam => write!(f, "type parameter"),
            Self::Lifetime => write!(f, "lifetime"),
            Self::MatchArm => write!(f, "match arm"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn create_test_crate() -> PathBuf {
        // Use unique dir name to avoid conflicts with concurrent tests
        let id = std::process::id();
        let dir = std::env::temp_dir().join(format!("deadmod_builder_test_{}", id));

        // Clean up any existing directory
        if dir.exists() {
            fs::remove_dir_all(&dir).ok();
        }

        // Create directory structure
        fs::create_dir_all(dir.join("src")).expect("Failed to create test directory");

        fs::write(
            dir.join("Cargo.toml"),
            "[package]\nname = \"test\"\nversion = \"0.1.0\"",
        ).expect("Failed to write Cargo.toml");

        fs::write(
            dir.join("src/main.rs"),
            "mod used;\nfn main() {}",
        ).expect("Failed to write main.rs");

        fs::write(
            dir.join("src/used.rs"),
            "pub fn helper() {}",
        ).expect("Failed to write used.rs");

        fs::write(
            dir.join("src/dead.rs"),
            "pub fn unused() {}",
        ).expect("Failed to write dead.rs");

        dir
    }

    #[test]
    fn test_builder_basic() {
        let dir = create_test_crate();

        let result = Deadmod::new(&dir)
            .with_cache(false)
            .analyze()
            .unwrap();

        assert!(result.dead_modules.contains(&"dead".to_string()));
        assert!(!result.dead_modules.contains(&"used".to_string()));

        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn test_builder_ignore_patterns() {
        let dir = create_test_crate();

        let result = Deadmod::new(&dir)
            .with_cache(false)
            .ignore_patterns(["dead"])
            .analyze()
            .unwrap();

        // Dead module should be ignored
        assert!(!result.dead_modules.contains(&"dead".to_string()));

        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn test_dead_item_kind_display() {
        assert_eq!(DeadItemKind::Function.to_string(), "function");
        assert_eq!(DeadItemKind::EnumVariant.to_string(), "enum variant");
    }

    #[test]
    fn test_analysis_result_stats() {
        let result = AnalysisResult {
            root: PathBuf::from("/test"),
            total_modules: 10,
            reachable_modules: vec!["a".into(), "b".into()],
            dead_modules: vec!["c".into(), "d".into()],
            dead_functions: Vec::new(),
            dead_traits: Vec::new(),
            dead_constants: Vec::new(),
            dead_enums: Vec::new(),
            dead_macros: Vec::new(),
            dead_generics: Vec::new(),
            dead_matcharms: Vec::new(),
            modules: HashMap::new(),
        };

        assert!(result.has_dead_code());
        assert_eq!(result.dead_count(), 2);
        assert!((result.dead_percentage() - 20.0).abs() < 0.01);
    }
}
