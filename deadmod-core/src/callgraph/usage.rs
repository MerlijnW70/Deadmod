//! Function call extraction for call graph analysis.
//!
//! Extracts all function calls including:
//! - Direct calls: `foo()`
//! - Method calls: `x.method()`
//! - Qualified calls: `Type::method()`
//! - Path references: `module::function`
//!
//! With path resolution enabled, calls are resolved to fully qualified paths
//! based on `use` imports and module context.
//!
//! NASA-grade resilience: handles malformed AST gracefully.

use std::collections::HashSet;
use std::path::Path;
use syn::{visit::Visit, Expr, File};

use super::path_resolver::{collect_use_statements, resolve_call_path, segments_to_path, ModulePathContext};

/// Result of call extraction from a file.
#[derive(Debug, Clone, Default)]
pub struct CallUsageResult {
    /// Set of function/method names that are called
    pub calls: HashSet<String>,
    /// Set of full paths that are called (e.g., "Type::method")
    pub qualified_calls: HashSet<String>,
    /// Set of semantically resolved full paths (e.g., "db::query" from `use crate::db::query`)
    /// Empty if path resolution was not performed.
    pub resolved_calls: HashSet<String>,
}

/// AST visitor that extracts all function calls.
struct CallUsageExtractor {
    calls: HashSet<String>,
    qualified_calls: HashSet<String>,
}

impl CallUsageExtractor {
    fn new() -> Self {
        Self {
            calls: HashSet::with_capacity(64),
            qualified_calls: HashSet::with_capacity(32),
        }
    }

    fn record_path(&mut self, path: &syn::Path) {
        // Record the last segment (function name)
        if let Some(seg) = path.segments.last() {
            self.calls.insert(seg.ident.to_string());
        }

        // Record qualified path if multiple segments
        if path.segments.len() > 1 {
            let full_path = path
                .segments
                .iter()
                .map(|s| s.ident.to_string())
                .collect::<Vec<_>>()
                .join("::");
            self.qualified_calls.insert(full_path);
        }
    }
}

impl<'ast> Visit<'ast> for CallUsageExtractor {
    fn visit_expr(&mut self, expr: &'ast Expr) {
        match expr {
            // Direct function calls: foo(), Type::method()
            Expr::Call(call) => {
                if let Expr::Path(p) = &*call.func {
                    self.record_path(&p.path);
                }
            }

            // Method calls: x.method()
            Expr::MethodCall(mc) => {
                self.calls.insert(mc.method.to_string());
            }

            // Path expressions (function references without call)
            Expr::Path(p) => {
                // Only record if it looks like a function reference
                // (starts with lowercase, not a type)
                if let Some(seg) = p.path.segments.last() {
                    let name = seg.ident.to_string();
                    if name.chars().next().map(|c| c.is_lowercase()).unwrap_or(false) {
                        self.record_path(&p.path);
                    }
                }
            }

            _ => {}
        }

        syn::visit::visit_expr(self, expr);
    }
}

/// Extract all function calls from file content.
///
/// Returns a set of function names and qualified paths that are called.
/// On parse error, returns empty result (resilient behavior).
pub fn extract_call_usages(path: &Path, content: &str) -> CallUsageResult {
    let ast: File = match syn::parse_file(content) {
        Ok(ast) => ast,
        Err(e) => {
            eprintln!("[WARN] AST parse failed for {}: {}", path.display(), e);
            return CallUsageResult::default();
        }
    };

    let mut extractor = CallUsageExtractor::new();
    extractor.visit_file(&ast);

    CallUsageResult {
        calls: extractor.calls,
        qualified_calls: extractor.qualified_calls,
        resolved_calls: HashSet::new(), // No resolution in basic mode
    }
}

/// Extract all function calls with semantic path resolution.
///
/// This enhanced version resolves calls to fully qualified paths based on:
/// - `use` imports in the file
/// - Module context (file path position)
/// - `crate::`, `self::`, `super::` prefixes
///
/// Returns resolved paths that can be directly matched to function full_paths.
pub fn extract_call_usages_resolved(path: &Path, content: &str) -> CallUsageResult {
    let ast: File = match syn::parse_file(content) {
        Ok(ast) => ast,
        Err(e) => {
            eprintln!("[WARN] AST parse failed for {}: {}", path.display(), e);
            return CallUsageResult::default();
        }
    };

    // Build module context from file path
    let ctx = ModulePathContext::from_file_path(path);

    // Collect use imports for resolution
    let usemap = collect_use_statements(&ast, &ctx);

    // Extract raw calls
    let mut extractor = CallUsageExtractor::new();
    extractor.visit_file(&ast);

    // Resolve all calls to full paths
    let mut resolved_calls = HashSet::with_capacity(extractor.calls.len() + extractor.qualified_calls.len());

    // Resolve simple calls
    for call in &extractor.calls {
        let segments = resolve_call_path(call, &usemap, &ctx);
        let resolved = segments_to_path(&segments);
        resolved_calls.insert(resolved);
    }

    // Resolve qualified calls
    for qualified in &extractor.qualified_calls {
        let segments = resolve_call_path(qualified, &usemap, &ctx);
        let resolved = segments_to_path(&segments);
        resolved_calls.insert(resolved);
    }

    CallUsageResult {
        calls: extractor.calls,
        qualified_calls: extractor.qualified_calls,
        resolved_calls,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn test_extract_direct_call() {
        let content = r#"
fn main() {
    foo();
    bar(1, 2);
}
"#;
        let result = extract_call_usages(&PathBuf::from("test.rs"), content);
        assert!(result.calls.contains("foo"));
        assert!(result.calls.contains("bar"));
    }

    #[test]
    fn test_extract_method_call() {
        let content = r#"
fn main() {
    let x = Vec::new();
    x.push(1);
    x.len();
}
"#;
        let result = extract_call_usages(&PathBuf::from("test.rs"), content);
        assert!(result.calls.contains("push"));
        assert!(result.calls.contains("len"));
        assert!(result.calls.contains("new"));
    }

    #[test]
    fn test_extract_qualified_call() {
        let content = r#"
fn main() {
    String::from("hello");
    std::mem::drop(x);
}
"#;
        let result = extract_call_usages(&PathBuf::from("test.rs"), content);
        assert!(result.calls.contains("from"));
        assert!(result.calls.contains("drop"));
        assert!(result.qualified_calls.contains("String::from"));
        assert!(result.qualified_calls.contains("std::mem::drop"));
    }

    #[test]
    fn test_extract_chained_calls() {
        let content = r#"
fn main() {
    vec![1, 2, 3].iter().map(|x| x + 1).collect::<Vec<_>>();
}
"#;
        let result = extract_call_usages(&PathBuf::from("test.rs"), content);
        assert!(result.calls.contains("iter"));
        assert!(result.calls.contains("map"));
        assert!(result.calls.contains("collect"));
    }

    #[test]
    fn test_malformed_resilient() {
        let content = "fn main() { broken(";
        let result = extract_call_usages(&PathBuf::from("broken.rs"), content);
        assert!(result.calls.is_empty());
    }
}
