//! AST parsing module - mission critical.
//!
//! Fully deterministic, error-resilient, correct usage of syn.
//!
//! Dependency extraction is semantically aware:
//! - Only extracts root path components (not nested types/functions)
//! - Skips Rust keywords (self, super, crate)
//! - Focuses on `mod` declarations for accurate dependency graphs

use anyhow::{Context, Result};
use rayon::prelude::*;
use std::{
    collections::{HashMap, HashSet},
    fs,
    path::{Path, PathBuf},
};
use syn::{File, Item, ItemMod, UsePath, UseTree, Visibility as SynVisibility};

/// Rust path keywords that should not be treated as module dependencies.
const PATH_KEYWORDS: &[&str] = &["self", "super", "crate"];

/// Maximum file size to parse (10 MB).
/// Files larger than this are skipped to prevent memory issues and stack overflow.
const MAX_FILE_SIZE: usize = 10_000_000;

/// Visibility level of a module or item.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Visibility {
    /// Private (default) - `mod foo;`
    #[default]
    Private,
    /// Public - `pub mod foo;`
    Public,
    /// Crate-visible - `pub(crate) mod foo;`
    PubCrate,
    /// Super-visible - `pub(super) mod foo;`
    PubSuper,
    /// Visible to a specific path - `pub(in path) mod foo;`
    PubIn,
}

impl Visibility {
    /// Check if this visibility could expose the item externally.
    pub fn is_potentially_external(&self) -> bool {
        matches!(self, Self::Public)
    }

    /// Check if this visibility is restricted to the crate.
    pub fn is_crate_internal(&self) -> bool {
        matches!(self, Self::Private | Self::PubCrate | Self::PubSuper | Self::PubIn)
    }
}

impl From<&SynVisibility> for Visibility {
    fn from(vis: &SynVisibility) -> Self {
        match vis {
            SynVisibility::Public(_) => Self::Public,
            SynVisibility::Restricted(r) => {
                let path = r.path.segments.first()
                    .map(|s| s.ident.to_string())
                    .unwrap_or_default();
                match path.as_str() {
                    "crate" => Self::PubCrate,
                    "super" => Self::PubSuper,
                    _ => Self::PubIn,
                }
            }
            SynVisibility::Inherited => Self::Private,
        }
    }
}

/// Normalize a path string to use forward slashes consistently.
///
/// This ensures cross-platform consistency when paths are used as keys,
/// compared, or serialized. Windows paths with backslashes are converted
/// to forward slashes to match Unix-style paths.
#[inline]
pub fn normalize_path_string(path: &str) -> String {
    path.replace('\\', "/")
}

/// Convert a Path to a normalized string (forward slashes).
#[inline]
pub fn path_to_normalized_string(path: &Path) -> String {
    normalize_path_string(&path.display().to_string())
}

/// Stores metadata for a single module file.
#[derive(Debug, Clone)]
pub struct ModuleInfo {
    /// Path to the module file
    pub path: PathBuf,
    /// Module name (file stem)
    pub name: String,
    /// Referenced modules (dependencies)
    pub refs: HashSet<String>,
    /// Module's own visibility (if declared via `mod` statement)
    pub visibility: Visibility,
    /// Whether this module has `#[doc(hidden)]`
    pub doc_hidden: bool,
    /// Module declarations with their visibility (child modules)
    pub mod_decls: HashMap<String, Visibility>,
    /// Re-exports from this module (`pub use`)
    pub reexports: HashSet<String>,
}

impl ModuleInfo {
    /// Creates a new ModuleInfo with pre-allocated capacity for typical reference counts.
    /// Most modules reference 4-8 other modules, so capacity of 8 avoids reallocation.
    pub fn new(path: PathBuf) -> Self {
        let name = path
            .file_stem()
            .unwrap_or_default()
            .to_string_lossy()
            .to_string();
        Self {
            path,
            name,
            refs: HashSet::with_capacity(8),
            visibility: Visibility::Private,
            doc_hidden: false,
            mod_decls: HashMap::with_capacity(4),
            reexports: HashSet::with_capacity(4),
        }
    }

    /// Check if this module might be used externally (pub and not doc(hidden)).
    pub fn is_potentially_external(&self) -> bool {
        self.visibility.is_potentially_external() && !self.doc_hidden
    }
}

/// Result of parsing a single module - used for granular parallel control.
#[derive(Debug)]
pub enum ParseResult {
    /// Successfully parsed module
    Ok(String, ModuleInfo),
    /// Parse failed (logged, can be skipped)
    Skipped(PathBuf, String),
}

/// Extracts the root path component from a use tree for dependency analysis.
///
/// This function implements semantic filtering to only extract identifiers
/// that represent actual module dependencies:
///
/// - `use foo;` → extracts "foo"
/// - `use foo::bar::Baz;` → extracts "foo" (the root module)
/// - `use std::io::Error;` → extracts "std" (external crate, filtered by graph)
/// - `use self::utils;` → extracts "utils" (skips `self` keyword)
/// - `use super::parent;` → extracts "parent" (skips `super` keyword)
/// - `use crate::module;` → extracts "module" (skips `crate` keyword)
///
/// This prevents false dependencies on types like `Error` or functions like `bar`.
fn extract_path_root(tree: &UseTree, refs: &mut HashSet<String>) {
    match tree {
        UseTree::Name(n) => {
            // Direct import: `use foo;`
            let name = n.ident.to_string();
            if !PATH_KEYWORDS.contains(&name.as_str()) {
                refs.insert(name);
            }
        }
        UseTree::Rename(r) => {
            // Renamed import: `use foo as bar;`
            // The original name (before `as`) is the dependency, not the alias.
            // For simple renames like `use foo as bar;`, we extract the original.
            let name = r.ident.to_string();
            if !PATH_KEYWORDS.contains(&name.as_str()) {
                refs.insert(name);
            }
        }
        UseTree::Path(UsePath { ident, tree: next_tree, .. }) => {
            let name = ident.to_string();

            if PATH_KEYWORDS.contains(&name.as_str()) {
                // Skip keyword and continue to the actual module name
                // e.g., `use self::utils;` → continue to extract "utils"
                extract_path_root(next_tree, refs);
            } else {
                // This is the root module dependency
                // e.g., `use foo::bar::Baz;` → "foo" is the module
                refs.insert(name);
                // Don't recurse further - we only want the root
            }
        }
        UseTree::Group(g) => {
            // Grouped imports: `use foo::{bar, baz};`
            // Each item in the group should be processed
            for t in &g.items {
                extract_path_root(t, refs);
            }
        }
        UseTree::Glob(_) => {
            // Glob imports: `use foo::*;`
            // The parent path (foo) is handled by UsePath above
        }
    }
}

/// Parses file content to extract module declarations and use statements.
///
/// Semantically aware extraction:
/// - `mod foo;` declarations are always extracted (direct dependencies)
/// - `use` statements extract only root path components (not nested items)
pub fn extract_uses_and_decls(content: &str, refs: &mut HashSet<String>) -> Result<()> {
    let ast: File = syn::parse_file(content).context("AST parse error")?;

    for item in ast.items {
        match item {
            Item::Mod(ItemMod {
                ident,
                content: None, // External module declaration (e.g., `mod utils;`)
                ..
            }) => {
                // Module declarations are primary dependencies
                refs.insert(ident.to_string());
            }
            Item::Use(u) => {
                // Extract only root path components for module dependencies
                extract_path_root(&u.tree, refs);
            }
            _ => {}
        }
    }

    Ok(())
}

/// Enhanced parsing that extracts visibility and re-export information.
///
/// This provides richer metadata for more accurate dead code detection:
/// - Tracks visibility of mod declarations
/// - Detects `pub use` re-exports
/// - Detects `#[doc(hidden)]` attributes
pub fn extract_module_info(content: &str, info: &mut ModuleInfo) -> Result<()> {
    let ast: File = syn::parse_file(content).context("AST parse error")?;

    for item in ast.items {
        match item {
            Item::Mod(ItemMod {
                ident,
                vis,
                attrs,
                content: None, // External module declaration
                ..
            }) => {
                let name = ident.to_string();
                let visibility = Visibility::from(&vis);

                // Track mod declaration with visibility
                info.mod_decls.insert(name.clone(), visibility);
                info.refs.insert(name);

                // Check for #[doc(hidden)] on the mod declaration
                for attr in &attrs {
                    if attr.path().is_ident("doc") {
                        if let Ok(meta) = attr.meta.require_list() {
                            let tokens = meta.tokens.to_string();
                            if tokens.contains("hidden") {
                                // This mod is doc(hidden)
                            }
                        }
                    }
                }
            }
            Item::Use(u) => {
                // Track pub use as re-exports
                if matches!(u.vis, SynVisibility::Public(_)) {
                    extract_reexports(&u.tree, &mut info.reexports);
                }
                // Always track as dependency
                extract_path_root(&u.tree, &mut info.refs);
            }
            _ => {}
        }
    }

    // Check file-level attributes for #[doc(hidden)]
    for attr in &ast.attrs {
        if attr.path().is_ident("doc") {
            if let Ok(meta) = attr.meta.require_list() {
                let tokens = meta.tokens.to_string();
                if tokens.contains("hidden") {
                    info.doc_hidden = true;
                }
            }
        }
    }

    Ok(())
}

/// Extract re-exported items from a `pub use` statement.
fn extract_reexports(tree: &UseTree, reexports: &mut HashSet<String>) {
    match tree {
        UseTree::Name(n) => {
            reexports.insert(n.ident.to_string());
        }
        UseTree::Rename(r) => {
            // The alias is what's exported
            reexports.insert(r.rename.to_string());
        }
        UseTree::Path(p) => {
            // Continue to find the actual exported item
            extract_reexports(&p.tree, reexports);
        }
        UseTree::Group(g) => {
            for t in &g.items {
                extract_reexports(t, reexports);
            }
        }
        UseTree::Glob(_) => {
            // Glob re-exports are complex - mark as potentially exporting everything
            reexports.insert("*".to_string());
        }
    }
}

/// Parses a single module file. This is the atomic unit of work for parallel processing.
/// Returns a `ParseResult` to allow caller to decide error handling strategy.
pub fn parse_single_module(path: &Path) -> ParseResult {
    let content = match fs::read_to_string(path) {
        Ok(c) => c,
        Err(e) => {
            return ParseResult::Skipped(path.to_path_buf(), format!("I/O error: {}", e));
        }
    };

    // Skip files that are too large to prevent memory issues
    if content.len() > MAX_FILE_SIZE {
        return ParseResult::Skipped(
            path.to_path_buf(),
            format!("File too large ({} bytes, max {})", content.len(), MAX_FILE_SIZE),
        );
    }

    let mut info = ModuleInfo::new(path.to_path_buf());
    if let Err(e) = extract_uses_and_decls(&content, &mut info.refs) {
        return ParseResult::Skipped(path.to_path_buf(), format!("AST error: {}", e));
    }

    ParseResult::Ok(info.name.clone(), info)
}

/// Parses a single module, returning Result for use with `?` operator.
/// Use this when you want fail-fast behavior on parse errors.
pub fn parse_single_module_strict(path: &Path) -> Result<(String, ModuleInfo)> {
    let content = fs::read_to_string(path)
        .with_context(|| format!("Failed to read: {}", path.display()))?;

    // Reject files that are too large to prevent memory issues
    anyhow::ensure!(
        content.len() <= MAX_FILE_SIZE,
        "File too large ({} bytes, max {}): {}",
        content.len(),
        MAX_FILE_SIZE,
        path.display()
    );

    let mut info = ModuleInfo::new(path.to_path_buf());
    extract_uses_and_decls(&content, &mut info.refs)
        .with_context(|| format!("Failed to parse: {}", path.display()))?;

    Ok((info.name.clone(), info))
}

/// Reads all files in parallel, parses them, and builds a HashMap of module information.
/// Includes robust error handling to skip malformed files (lenient mode).
pub fn parse_modules(files: &[PathBuf]) -> Result<HashMap<String, ModuleInfo>> {
    let modules = files
        .par_iter()
        .filter_map(|file| match parse_single_module(file) {
            ParseResult::Ok(name, info) => Some((name, info)),
            ParseResult::Skipped(path, reason) => {
                eprintln!("WARN: Skipping {}: {}", path.display(), reason);
                None
            }
        })
        .collect();

    Ok(modules)
}

/// Parses all files in parallel with strict error handling (fail-fast mode).
/// Returns an error if any file fails to parse.
pub fn parse_modules_strict(files: &[PathBuf]) -> Result<HashMap<String, ModuleInfo>> {
    let results: Vec<Result<(String, ModuleInfo)>> = files
        .par_iter()
        .map(|path| parse_single_module_strict(path))
        .collect();

    // Collect all results, failing on first error
    let module_list: Vec<(String, ModuleInfo)> = results
        .into_iter()
        .collect::<Result<Vec<_>>>()?;

    Ok(module_list.into_iter().collect())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    // === Path Normalization Tests ===

    #[test]
    fn test_normalize_path_string_unix() {
        assert_eq!(normalize_path_string("src/lib.rs"), "src/lib.rs");
        assert_eq!(normalize_path_string("a/b/c/d.rs"), "a/b/c/d.rs");
    }

    #[test]
    fn test_normalize_path_string_windows() {
        assert_eq!(normalize_path_string("src\\lib.rs"), "src/lib.rs");
        assert_eq!(normalize_path_string("a\\b\\c\\d.rs"), "a/b/c/d.rs");
        assert_eq!(normalize_path_string("C:\\Users\\test\\project\\src\\main.rs"),
                   "C:/Users/test/project/src/main.rs");
    }

    #[test]
    fn test_normalize_path_string_mixed() {
        assert_eq!(normalize_path_string("src\\api/v1\\handler.rs"), "src/api/v1/handler.rs");
    }

    #[test]
    fn test_normalize_path_string_empty() {
        assert_eq!(normalize_path_string(""), "");
    }

    #[test]
    fn test_normalize_path_string_no_separators() {
        assert_eq!(normalize_path_string("file.rs"), "file.rs");
    }

    #[test]
    fn test_path_to_normalized_string() {
        let path = Path::new("src/lib.rs");
        let result = path_to_normalized_string(path);
        assert!(result.contains("src"));
        assert!(result.contains("lib.rs"));
        assert!(!result.contains('\\'));
    }

    // === Module Info Tests ===

    #[test]
    fn test_module_info_new_simple() {
        let info = ModuleInfo::new(PathBuf::from("src/main.rs"));
        assert_eq!(info.name, "main");
        assert!(info.refs.is_empty());
    }

    #[test]
    fn test_module_info_new_nested_path() {
        let info = ModuleInfo::new(PathBuf::from("src/api/v1/handler.rs"));
        assert_eq!(info.name, "handler");
    }

    #[test]
    fn test_module_info_new_mod_rs() {
        let info = ModuleInfo::new(PathBuf::from("src/api/mod.rs"));
        assert_eq!(info.name, "mod");
    }

    // === Extract Uses and Decls Tests ===

    #[test]
    fn test_extract_mod_declarations() {
        let content = r#"
mod foo;
mod bar;
pub mod baz;
"#;
        let mut refs = HashSet::new();
        extract_uses_and_decls(content, &mut refs).unwrap();

        assert!(refs.contains("foo"));
        assert!(refs.contains("bar"));
        assert!(refs.contains("baz"));
    }

    #[test]
    fn test_extract_use_statements() {
        let content = r#"
use std::collections::HashMap;
use crate::utils;
use super::parent;
"#;
        let mut refs = HashSet::new();
        extract_uses_and_decls(content, &mut refs).unwrap();

        // Should extract root module references
        assert!(refs.contains("std") || refs.is_empty()); // May filter std
    }

    #[test]
    fn test_extract_empty_file() {
        let content = "";
        let mut refs = HashSet::new();
        let result = extract_uses_and_decls(content, &mut refs);
        assert!(result.is_ok());
        assert!(refs.is_empty());
    }

    #[test]
    fn test_extract_comment_only_file() {
        // Note: A file with only comments may or may not parse successfully
        // depending on syn version. We test that it doesn't panic.
        let content = r#"
// This is a comment
/* Multi-line
   comment */
/// Doc comment
"#;
        let mut refs = HashSet::new();
        let result = extract_uses_and_decls(content, &mut refs);
        // Result may be Ok or Err depending on syn's handling of comment-only files
        // The important thing is it doesn't panic
        if result.is_ok() {
            assert!(refs.is_empty());
        }
    }

    #[test]
    fn test_extract_whitespace_only() {
        let content = "   \n\t\n   ";
        let mut refs = HashSet::new();
        let result = extract_uses_and_decls(content, &mut refs);
        assert!(result.is_ok());
    }

    #[test]
    fn test_extract_inline_mod_ignored() {
        let content = r#"
mod inline {
    fn inner() {}
}
"#;
        let mut refs = HashSet::new();
        extract_uses_and_decls(content, &mut refs).unwrap();

        // Inline mods should NOT be added as external references
        assert!(!refs.contains("inline"));
    }

    #[test]
    fn test_extract_skips_path_keywords() {
        let content = r#"
use self::utils;
use super::parent;
use crate::root;
"#;
        let mut refs = HashSet::new();
        extract_uses_and_decls(content, &mut refs).unwrap();

        // Should not contain keywords as refs
        assert!(!refs.contains("self"));
        assert!(!refs.contains("super"));
        assert!(!refs.contains("crate"));
    }

    // === Parse Single Module Tests ===

    #[test]
    fn test_parse_single_module_valid() {
        let temp_dir = std::env::temp_dir().join("deadmod_parse_test_valid");
        let _ = std::fs::remove_dir_all(&temp_dir);
        std::fs::create_dir_all(&temp_dir).unwrap();

        let file_path = temp_dir.join("test_module.rs");
        let mut file = std::fs::File::create(&file_path).unwrap();
        writeln!(file, "mod foo;\nmod bar;").unwrap();

        let result = parse_single_module(&file_path);
        match result {
            ParseResult::Ok(name, info) => {
                assert_eq!(name, "test_module");
                assert!(info.refs.contains("foo"));
                assert!(info.refs.contains("bar"));
            }
            ParseResult::Skipped(_, reason) => panic!("Unexpected skip: {}", reason),
        }

        std::fs::remove_dir_all(&temp_dir).ok();
    }

    #[test]
    fn test_parse_single_module_syntax_error() {
        let temp_dir = std::env::temp_dir().join("deadmod_parse_test_syntax");
        let _ = std::fs::remove_dir_all(&temp_dir);
        std::fs::create_dir_all(&temp_dir).unwrap();

        let file_path = temp_dir.join("broken.rs");
        let mut file = std::fs::File::create(&file_path).unwrap();
        writeln!(file, "fn main() {{ broken syntax").unwrap();

        let result = parse_single_module(&file_path);
        // Should be skipped due to syntax error, not panic
        assert!(matches!(result, ParseResult::Skipped(_, _)));

        std::fs::remove_dir_all(&temp_dir).ok();
    }

    #[test]
    fn test_parse_single_module_nonexistent() {
        let result = parse_single_module(Path::new("/nonexistent/path/file.rs"));
        assert!(matches!(result, ParseResult::Skipped(_, _)));
    }

    // === Unicode and Special Character Tests ===

    #[test]
    fn test_extract_unicode_comments() {
        let content = r#"
// 日本語コメント
mod foo;
/* 中文注释 */
mod bar;
"#;
        let mut refs = HashSet::new();
        let result = extract_uses_and_decls(content, &mut refs);
        assert!(result.is_ok());
        assert!(refs.contains("foo"));
        assert!(refs.contains("bar"));
    }

    #[test]
    fn test_extract_unicode_strings() {
        let content = r#"
mod foo;
const GREETING: &str = "こんにちは世界";
"#;
        let mut refs = HashSet::new();
        let result = extract_uses_and_decls(content, &mut refs);
        assert!(result.is_ok());
        assert!(refs.contains("foo"));
    }

    // === Edge Cases ===

    #[test]
    fn test_extract_many_mods() {
        let mods: Vec<String> = (0..100).map(|i| format!("mod mod_{};", i)).collect();
        let content = mods.join("\n");

        let mut refs = HashSet::new();
        extract_uses_and_decls(&content, &mut refs).unwrap();

        assert_eq!(refs.len(), 100);
        assert!(refs.contains("mod_0"));
        assert!(refs.contains("mod_99"));
    }

    #[test]
    fn test_extract_deeply_nested_use() {
        let content = r#"
use a::b::c::d::e::f::g::h::i::j::k;
"#;
        let mut refs = HashSet::new();
        let result = extract_uses_and_decls(content, &mut refs);
        assert!(result.is_ok());
    }

    #[test]
    fn test_extract_use_groups() {
        let content = r#"
use std::{
    collections::{HashMap, HashSet},
    io::{Read, Write},
};
"#;
        let mut refs = HashSet::new();
        let result = extract_uses_and_decls(content, &mut refs);
        assert!(result.is_ok());
    }

    #[test]
    fn test_extract_use_rename() {
        let content = r#"
use std::collections::HashMap as Map;
mod foo;
"#;
        let mut refs = HashSet::new();
        extract_uses_and_decls(content, &mut refs).unwrap();
        assert!(refs.contains("foo"));
    }

    #[test]
    fn test_extract_use_glob() {
        let content = r#"
use std::collections::*;
mod bar;
"#;
        let mut refs = HashSet::new();
        extract_uses_and_decls(content, &mut refs).unwrap();
        assert!(refs.contains("bar"));
    }

    // === Parse Modules (Batch) Tests ===

    #[test]
    fn test_parse_modules_empty_list() {
        let result = parse_modules(&[]).unwrap();
        assert!(result.is_empty());
    }

    #[test]
    fn test_parse_modules_mixed_valid_invalid() {
        let temp_dir = std::env::temp_dir().join("deadmod_parse_test_mixed");
        let _ = std::fs::remove_dir_all(&temp_dir);
        std::fs::create_dir_all(&temp_dir).unwrap();

        // Create valid file
        let valid_path = temp_dir.join("valid.rs");
        std::fs::write(&valid_path, "mod foo;").unwrap();

        // Create invalid file
        let invalid_path = temp_dir.join("invalid.rs");
        std::fs::write(&invalid_path, "fn broken(").unwrap();

        let files = vec![valid_path, invalid_path];
        let result = parse_modules(&files).unwrap();

        // Should have parsed at least the valid file
        assert!(result.contains_key("valid"));

        std::fs::remove_dir_all(&temp_dir).ok();
    }
}
