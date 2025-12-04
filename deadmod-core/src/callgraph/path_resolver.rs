//! Semantic path resolution for call graph analysis.
//!
//! Resolves partial or ambiguous function calls to fully qualified paths:
//! - `query()` → `crate::db::query` (via `use crate::db::query`)
//! - `super::config::load()` → `crate::api::config::load`
//! - `self::router::Route::new()` → `crate::api::v1::router::Route::new`
//!
//! This enables accurate call graph edges instead of name-based heuristics.

use std::collections::HashMap;
use std::path::Path;
use syn::{File, Item, UseTree};

/// Module's position in the crate hierarchy.
///
/// Example: `src/api/v1/mod.rs` → `["api", "v1"]`
#[derive(Debug, Clone, Default)]
pub struct ModulePathContext {
    /// Path segments from crate root (excluding "crate::")
    pub segments: Vec<String>,
}

impl ModulePathContext {
    /// Create context from file path relative to crate root.
    ///
    /// Examples:
    /// - `src/lib.rs` → `[]`
    /// - `src/api/mod.rs` → `["api"]`
    /// - `src/api/v1/handler.rs` → `["api", "v1", "handler"]`
    pub fn from_file_path(path: &Path) -> Self {
        let mut segments = Vec::new();
        let mut inside_src = false;

        for component in path.iter() {
            let part = component.to_string_lossy();

            if part == "src" {
                inside_src = true;
                continue;
            }

            if inside_src {
                // Skip mod.rs and lib.rs - they represent the parent module
                if part == "mod.rs" || part == "lib.rs" || part == "main.rs" {
                    continue;
                }

                // Strip .rs extension
                let segment = if let Some(stripped) = part.strip_suffix(".rs") {
                    stripped.to_string()
                } else {
                    part.to_string()
                };

                segments.push(segment);
            }
        }

        Self { segments }
    }

    /// Get fully qualified path with crate prefix.
    pub fn to_crate_path(&self) -> String {
        if self.segments.is_empty() {
            "crate".to_string()
        } else {
            format!("crate::{}", self.segments.join("::"))
        }
    }

    /// Get parent module path (for `super::` resolution).
    pub fn parent(&self) -> Self {
        Self {
            segments: if self.segments.is_empty() {
                Vec::new()
            } else {
                self.segments[..self.segments.len() - 1].to_vec()
            },
        }
    }
}

/// Maps imported names to their fully qualified paths.
///
/// Tracks all `use` statements in a file to resolve short names.
#[derive(Debug, Clone, Default)]
pub struct UseMap {
    /// Map from local name (or alias) to full path segments
    map: HashMap<String, Vec<String>>,
}

impl UseMap {
    pub fn new() -> Self {
        Self {
            map: HashMap::new(),
        }
    }

    /// Record an import mapping.
    pub fn record(&mut self, local_name: String, full_path: Vec<String>) {
        self.map.insert(local_name, full_path);
    }

    /// Resolve a local name to its full path.
    pub fn resolve(&self, name: &str) -> Option<&Vec<String>> {
        self.map.get(name)
    }

    /// Check if a name is imported.
    pub fn contains(&self, name: &str) -> bool {
        self.map.contains_key(name)
    }

    /// Get all imported names.
    pub fn imported_names(&self) -> impl Iterator<Item = &String> {
        self.map.keys()
    }
}

/// Extract all `use` statements from a file and build a UseMap.
pub fn collect_use_statements(ast: &File, ctx: &ModulePathContext) -> UseMap {
    let mut usemap = UseMap::new();

    for item in &ast.items {
        if let Item::Use(u) = item {
            handle_use_tree(&u.tree, ctx, &mut usemap, Vec::new());
        }
    }

    usemap
}

/// Recursively process a use tree to extract all imports.
fn handle_use_tree(
    tree: &UseTree,
    ctx: &ModulePathContext,
    map: &mut UseMap,
    mut prefix: Vec<String>,
) {
    match tree {
        UseTree::Path(p) => {
            prefix.push(p.ident.to_string());
            handle_use_tree(&p.tree, ctx, map, prefix);
        }
        UseTree::Name(n) => {
            let name = n.ident.to_string();
            prefix.push(name.clone());
            let resolved = resolve_prefix_path(&prefix, ctx);
            map.record(name, resolved);
        }
        UseTree::Rename(r) => {
            let alias = r.rename.to_string();
            prefix.push(r.ident.to_string());
            let resolved = resolve_prefix_path(&prefix, ctx);
            map.record(alias, resolved);
        }
        UseTree::Group(g) => {
            for t in &g.items {
                handle_use_tree(t, ctx, map, prefix.clone());
            }
        }
        UseTree::Glob(_) => {
            // Glob imports can't be statically resolved to specific names
            // Record the prefix module for partial matching
            if !prefix.is_empty() {
                let resolved = resolve_prefix_path(&prefix, ctx);
                // Use last segment as key (heuristic for module-level globs)
                if let Some(last) = prefix.last() {
                    map.record(format!("{}::*", last), resolved);
                }
            }
        }
    }
}

/// Resolve a use path prefix considering crate/self/super.
fn resolve_prefix_path(path: &[String], ctx: &ModulePathContext) -> Vec<String> {
    if path.is_empty() {
        return ctx.segments.clone();
    }

    match path[0].as_str() {
        "crate" => {
            // crate::foo::bar → ["foo", "bar"]
            path[1..].to_vec()
        }
        "self" => {
            // self::foo → current_module::foo
            let mut result = ctx.segments.clone();
            result.extend_from_slice(&path[1..]);
            result
        }
        "super" => {
            // super::foo → parent_module::foo
            let parent = ctx.parent();
            let mut result = parent.segments;
            result.extend_from_slice(&path[1..]);
            result
        }
        _ => {
            // External crate or relative path
            // Keep as-is (external crates like std::, anyhow::, etc.)
            path.to_vec()
        }
    }
}

/// Resolve a function call to its fully qualified path.
///
/// Resolution order:
/// 1. Check if name matches an import in UseMap
/// 2. Check if it's a qualified path (contains ::)
/// 3. Assume local function in current module
pub fn resolve_call_path(
    call: &str,
    usemap: &UseMap,
    ctx: &ModulePathContext,
) -> Vec<String> {
    // Handle qualified paths (foo::bar, Type::method)
    if call.contains("::") {
        let parts: Vec<&str> = call.split("::").collect();
        let first = parts[0];

        // Check if first segment is an import
        if let Some(resolved) = usemap.resolve(first) {
            let mut result = resolved.clone();
            result.extend(parts[1..].iter().map(|s| s.to_string()));
            return result;
        }

        // Check for crate/self/super
        match first {
            "crate" => return parts[1..].iter().map(|s| s.to_string()).collect(),
            "self" => {
                let mut result = ctx.segments.clone();
                result.extend(parts[1..].iter().map(|s| s.to_string()));
                return result;
            }
            "super" => {
                let parent = ctx.parent();
                let mut result = parent.segments;
                result.extend(parts[1..].iter().map(|s| s.to_string()));
                return result;
            }
            _ => {
                // Could be external crate or type path
                return parts.iter().map(|s| s.to_string()).collect();
            }
        }
    }

    // Simple name - check imports first
    if let Some(resolved) = usemap.resolve(call) {
        return resolved.clone();
    }

    // Assume local function in current module
    let mut result = ctx.segments.clone();
    result.push(call.to_string());
    result
}

/// Convert resolved path segments to a full path string.
pub fn segments_to_path(segments: &[String]) -> String {
    if segments.is_empty() {
        "crate".to_string()
    } else {
        segments.join("::")
    }
}

/// Full resolution result for a call.
#[derive(Debug, Clone)]
pub struct ResolvedCall {
    /// Original call expression
    pub original: String,
    /// Resolved full path (without crate:: prefix)
    pub resolved_path: String,
    /// Path segments
    pub segments: Vec<String>,
    /// Whether this was resolved via an import
    pub via_import: bool,
}

/// Resolve a call with full metadata.
pub fn resolve_call_full(
    call: &str,
    usemap: &UseMap,
    ctx: &ModulePathContext,
) -> ResolvedCall {
    let via_import = usemap.contains(call) ||
        (call.contains("::") && usemap.contains(call.split("::").next().unwrap_or("")));

    let segments = resolve_call_path(call, usemap, ctx);
    let resolved_path = segments_to_path(&segments);

    ResolvedCall {
        original: call.to_string(),
        resolved_path,
        segments,
        via_import,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_module_path_from_lib_rs() {
        let ctx = ModulePathContext::from_file_path(Path::new("src/lib.rs"));
        assert!(ctx.segments.is_empty());
        assert_eq!(ctx.to_crate_path(), "crate");
    }

    #[test]
    fn test_module_path_from_mod_rs() {
        let ctx = ModulePathContext::from_file_path(Path::new("src/api/mod.rs"));
        assert_eq!(ctx.segments, vec!["api"]);
        assert_eq!(ctx.to_crate_path(), "crate::api");
    }

    #[test]
    fn test_module_path_from_file() {
        let ctx = ModulePathContext::from_file_path(Path::new("src/api/v1/handler.rs"));
        assert_eq!(ctx.segments, vec!["api", "v1", "handler"]);
        assert_eq!(ctx.to_crate_path(), "crate::api::v1::handler");
    }

    #[test]
    fn test_module_path_parent() {
        let ctx = ModulePathContext::from_file_path(Path::new("src/api/v1/handler.rs"));
        let parent = ctx.parent();
        assert_eq!(parent.segments, vec!["api", "v1"]);
    }

    #[test]
    fn test_use_map_simple() {
        let mut map = UseMap::new();
        map.record("query".to_string(), vec!["db".to_string(), "query".to_string()]);

        assert!(map.contains("query"));
        assert_eq!(map.resolve("query"), Some(&vec!["db".to_string(), "query".to_string()]));
    }

    #[test]
    fn test_resolve_crate_path() {
        let ctx = ModulePathContext::from_file_path(Path::new("src/api/handler.rs"));
        let usemap = UseMap::new();

        let resolved = resolve_call_path("crate::db::query", &usemap, &ctx);
        assert_eq!(resolved, vec!["db", "query"]);
    }

    #[test]
    fn test_resolve_self_path() {
        let ctx = ModulePathContext::from_file_path(Path::new("src/api/handler.rs"));
        let usemap = UseMap::new();

        let resolved = resolve_call_path("self::utils::helper", &usemap, &ctx);
        assert_eq!(resolved, vec!["api", "handler", "utils", "helper"]);
    }

    #[test]
    fn test_resolve_super_path() {
        let ctx = ModulePathContext::from_file_path(Path::new("src/api/v1/handler.rs"));
        let usemap = UseMap::new();

        let resolved = resolve_call_path("super::config::load", &usemap, &ctx);
        assert_eq!(resolved, vec!["api", "v1", "config", "load"]);
    }

    #[test]
    fn test_resolve_imported_name() {
        let ctx = ModulePathContext::from_file_path(Path::new("src/api/handler.rs"));
        let mut usemap = UseMap::new();
        usemap.record("query".to_string(), vec!["db".to_string(), "query".to_string()]);

        let resolved = resolve_call_path("query", &usemap, &ctx);
        assert_eq!(resolved, vec!["db", "query"]);
    }

    #[test]
    fn test_resolve_local_function() {
        let ctx = ModulePathContext::from_file_path(Path::new("src/api/handler.rs"));
        let usemap = UseMap::new();

        let resolved = resolve_call_path("process", &usemap, &ctx);
        assert_eq!(resolved, vec!["api", "handler", "process"]);
    }

    #[test]
    fn test_resolve_qualified_import() {
        let ctx = ModulePathContext::from_file_path(Path::new("src/api/handler.rs"));
        let mut usemap = UseMap::new();
        usemap.record("Client".to_string(), vec!["db".to_string(), "Client".to_string()]);

        let resolved = resolve_call_path("Client::new", &usemap, &ctx);
        assert_eq!(resolved, vec!["db", "Client", "new"]);
    }

    #[test]
    fn test_collect_use_crate() {
        let code = r#"
            use crate::db::query;
            use crate::config::{Config, Settings};
        "#;
        let ast = syn::parse_file(code).unwrap();
        let ctx = ModulePathContext::from_file_path(Path::new("src/api/handler.rs"));

        let usemap = collect_use_statements(&ast, &ctx);

        assert!(usemap.contains("query"));
        assert_eq!(usemap.resolve("query"), Some(&vec!["db".to_string(), "query".to_string()]));
        assert!(usemap.contains("Config"));
        assert!(usemap.contains("Settings"));
    }

    #[test]
    fn test_collect_use_self() {
        let code = r#"
            use self::router::Route;
        "#;
        let ast = syn::parse_file(code).unwrap();
        let ctx = ModulePathContext::from_file_path(Path::new("src/api/v1/mod.rs"));

        let usemap = collect_use_statements(&ast, &ctx);

        assert!(usemap.contains("Route"));
        assert_eq!(usemap.resolve("Route"), Some(&vec!["api".to_string(), "v1".to_string(), "router".to_string(), "Route".to_string()]));
    }

    #[test]
    fn test_collect_use_super() {
        let code = r#"
            use super::config;
        "#;
        let ast = syn::parse_file(code).unwrap();
        let ctx = ModulePathContext::from_file_path(Path::new("src/api/v1/handler.rs"));

        let usemap = collect_use_statements(&ast, &ctx);

        assert!(usemap.contains("config"));
        // super from api/v1/handler = api/v1, then config
        assert_eq!(usemap.resolve("config"), Some(&vec!["api".to_string(), "v1".to_string(), "config".to_string()]));
    }

    #[test]
    fn test_collect_use_rename() {
        let code = r#"
            use crate::db::client as C;
        "#;
        let ast = syn::parse_file(code).unwrap();
        let ctx = ModulePathContext::from_file_path(Path::new("src/handler.rs"));

        let usemap = collect_use_statements(&ast, &ctx);

        assert!(usemap.contains("C"));
        assert!(!usemap.contains("client"));
        assert_eq!(usemap.resolve("C"), Some(&vec!["db".to_string(), "client".to_string()]));
    }

    #[test]
    fn test_resolve_full_metadata() {
        let ctx = ModulePathContext::from_file_path(Path::new("src/api/handler.rs"));
        let mut usemap = UseMap::new();
        usemap.record("query".to_string(), vec!["db".to_string(), "query".to_string()]);

        let result = resolve_call_full("query", &usemap, &ctx);
        assert_eq!(result.original, "query");
        assert_eq!(result.resolved_path, "db::query");
        assert!(result.via_import);

        let result2 = resolve_call_full("local_fn", &usemap, &ctx);
        assert_eq!(result2.resolved_path, "api::handler::local_fn");
        assert!(!result2.via_import);
    }
}
