//! Macro usage detection from Rust AST.
//!
//! Detects all macro invocations including:
//! - Expression macros: `foo!()`
//! - Statement macros: `println!("...")`
//! - Pattern macros: `matches!(x, pat)`
//! - Type macros: `vec![]` in type position
//! - Attribute-like macros
//!
//! NASA-grade resilience: handles malformed AST gracefully.

use std::collections::HashSet;
use std::path::Path;
use syn::{visit::Visit, Expr, File, Item, Macro, Pat, Stmt, Type};

/// Information about macro usages in a file.
#[derive(Debug, Clone, Default)]
pub struct MacroUsageResult {
    /// Set of macro names that are invoked
    pub used_macros: HashSet<String>,
}

/// AST visitor that extracts all macro usages.
struct MacroUsageExtractor {
    used: HashSet<String>,
}

impl MacroUsageExtractor {
    fn new() -> Self {
        Self {
            used: HashSet::with_capacity(16),
        }
    }

    fn record_macro(&mut self, mac: &Macro) {
        if let Some(seg) = mac.path.segments.last() {
            self.used.insert(seg.ident.to_string());
        }
    }
}

impl<'ast> Visit<'ast> for MacroUsageExtractor {
    fn visit_expr(&mut self, expr: &'ast Expr) {
        // Macro expressions like foo!(), bar!(x, y)
        if let Expr::Macro(expr_macro) = expr {
            self.record_macro(&expr_macro.mac);
        }

        syn::visit::visit_expr(self, expr);
    }

    fn visit_stmt(&mut self, stmt: &'ast Stmt) {
        // Statement-level macros
        if let Stmt::Macro(stmt_macro) = stmt {
            self.record_macro(&stmt_macro.mac);
        }

        syn::visit::visit_stmt(self, stmt);
    }

    fn visit_macro(&mut self, mac: &'ast Macro) {
        // Any macro call in any position
        self.record_macro(mac);
        syn::visit::visit_macro(self, mac);
    }

    fn visit_pat(&mut self, pat: &'ast Pat) {
        // Pattern macros like matches!
        if let Pat::Macro(pat_macro) = pat {
            self.record_macro(&pat_macro.mac);
        }

        syn::visit::visit_pat(self, pat);
    }

    fn visit_type(&mut self, ty: &'ast Type) {
        // Type-level macros
        if let Type::Macro(type_macro) = ty {
            self.record_macro(&type_macro.mac);
        }

        syn::visit::visit_type(self, ty);
    }

    fn visit_item(&mut self, item: &'ast Item) {
        // Item-level macros (like derive, etc. handled via macro)
        if let Item::Macro(item_macro) = item {
            self.record_macro(&item_macro.mac);
        }

        syn::visit::visit_item(self, item);
    }
}

/// Extract all macro usages from file content.
///
/// Returns a set of macro names that are invoked.
/// On parse error, returns an empty set (resilient behavior).
pub fn extract_macro_usages(path: &Path, content: &str) -> MacroUsageResult {
    let ast: File = match syn::parse_file(content) {
        Ok(ast) => ast,
        Err(e) => {
            eprintln!("[WARN] AST parse failed for {}: {}", path.display(), e);
            return MacroUsageResult::default();
        }
    };

    let mut extractor = MacroUsageExtractor::new();
    extractor.visit_file(&ast);

    MacroUsageResult {
        used_macros: extractor.used,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn test_extract_expr_macro() {
        let content = r#"
fn main() {
    println!("hello");
}
"#;
        let result = extract_macro_usages(&PathBuf::from("test.rs"), content);
        assert!(result.used_macros.contains("println"));
    }

    #[test]
    fn test_extract_vec_macro() {
        let content = r#"
fn main() {
    let v = vec![1, 2, 3];
}
"#;
        let result = extract_macro_usages(&PathBuf::from("test.rs"), content);
        assert!(result.used_macros.contains("vec"));
    }

    #[test]
    fn test_extract_custom_macro() {
        let content = r#"
fn main() {
    my_macro!(foo, bar);
    another!();
}
"#;
        let result = extract_macro_usages(&PathBuf::from("test.rs"), content);
        assert!(result.used_macros.contains("my_macro"));
        assert!(result.used_macros.contains("another"));
    }

    #[test]
    fn test_extract_path_macro() {
        let content = r#"
fn main() {
    crate::utils::helper!();
    std::format!("test");
}
"#;
        let result = extract_macro_usages(&PathBuf::from("test.rs"), content);
        assert!(result.used_macros.contains("helper"));
        assert!(result.used_macros.contains("format"));
    }

    #[test]
    fn test_extract_nested_macro() {
        // Note: Macros nested inside another macro's token stream are NOT visible
        // to the AST without macro expansion. This is a fundamental limitation.
        // We can only detect the outer macro (println).
        let content = r#"
fn main() {
    println!("{}", format!("nested"));
}
"#;
        let result = extract_macro_usages(&PathBuf::from("test.rs"), content);
        assert!(result.used_macros.contains("println"));
        // format! inside println!'s token stream is opaque to AST analysis
    }

    #[test]
    fn test_extract_separate_macros() {
        // Multiple separate macro calls ARE detected
        let content = r#"
fn main() {
    let s = format!("hello");
    println!("{}", s);
}
"#;
        let result = extract_macro_usages(&PathBuf::from("test.rs"), content);
        assert!(result.used_macros.contains("println"));
        assert!(result.used_macros.contains("format"));
    }

    #[test]
    fn test_malformed_resilient() {
        let content = "fn main() { broken!(";
        let result = extract_macro_usages(&PathBuf::from("broken.rs"), content);
        assert!(result.used_macros.is_empty());
    }
}
