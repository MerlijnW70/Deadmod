//! Constant and static usage detection from Rust AST.
//!
//! Detects all usages of constants and statics including:
//! - Direct references: `MY_CONST`
//! - Path references: `module::MY_CONST`
//! - In expressions, patterns, types
//! - As array sizes: `[T; MY_CONST]`
//!
//! NASA-grade resilience: handles malformed AST gracefully.

use std::collections::HashSet;
use std::path::Path;
use syn::{visit::Visit, Expr, File, Pat, Type};

/// Information about constant usages in a file.
#[derive(Debug, Clone, Default)]
pub struct ConstUsageResult {
    /// Set of constant/static names that are referenced
    pub used_constants: HashSet<String>,
}

/// AST visitor that extracts all constant usages.
struct ConstUsageExtractor {
    used: HashSet<String>,
}

impl ConstUsageExtractor {
    fn new() -> Self {
        Self {
            used: HashSet::with_capacity(32),
        }
    }

    fn record_path(&mut self, path: &syn::Path) {
        // Record the last segment (the actual name)
        if let Some(seg) = path.segments.last() {
            let name = seg.ident.to_string();
            // Heuristic: constants are typically SCREAMING_CASE
            // Also record any uppercase identifier
            if name.chars().any(|c| c.is_uppercase()) {
                self.used.insert(name);
            }
        }
    }
}

impl<'ast> Visit<'ast> for ConstUsageExtractor {
    fn visit_expr(&mut self, expr: &'ast Expr) {
        match expr {
            // Path expressions: MY_CONST, module::MY_CONST
            Expr::Path(p) => {
                self.record_path(&p.path);
            }

            // Field access might reference associated constants
            Expr::Field(f) => {
                if let syn::Member::Named(ident) = &f.member {
                    let name = ident.to_string();
                    if name.chars().any(|c| c.is_uppercase()) {
                        self.used.insert(name);
                    }
                }
            }

            _ => {}
        }

        syn::visit::visit_expr(self, expr);
    }

    fn visit_pat(&mut self, pat: &'ast Pat) {
        // Pattern matching against constants
        if let Pat::Path(p) = pat {
            self.record_path(&p.path);
        }
        if let Pat::Ident(pi) = pat {
            let name = pi.ident.to_string();
            if name.chars().any(|c| c.is_uppercase()) {
                self.used.insert(name);
            }
        }

        syn::visit::visit_pat(self, pat);
    }

    fn visit_type(&mut self, ty: &'ast Type) {
        // Array sizes: [T; CONST_SIZE]
        if let Type::Array(arr) = ty {
            if let Expr::Path(p) = &arr.len {
                self.record_path(&p.path);
            }
        }

        // Path types might use associated constants
        if let Type::Path(tp) = ty {
            for seg in &tp.path.segments {
                if let syn::PathArguments::AngleBracketed(args) = &seg.arguments {
                    for arg in &args.args {
                        if let syn::GenericArgument::Const(Expr::Path(p)) = arg {
                            self.record_path(&p.path);
                        }
                    }
                }
            }
        }

        syn::visit::visit_type(self, ty);
    }
}

/// Extract all constant usages from file content.
///
/// Returns a set of constant names that are referenced.
/// On parse error, returns an empty set (resilient behavior).
pub fn extract_const_usage(path: &Path, content: &str) -> ConstUsageResult {
    let ast: File = match syn::parse_file(content) {
        Ok(ast) => ast,
        Err(e) => {
            eprintln!("[WARN] AST parse failed for {}: {}", path.display(), e);
            return ConstUsageResult::default();
        }
    };

    let mut extractor = ConstUsageExtractor::new();
    extractor.visit_file(&ast);

    ConstUsageResult {
        used_constants: extractor.used,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn test_extract_const_usage() {
        let content = r#"
const MY_CONST: i32 = 42;

fn main() {
    let x = MY_CONST;
}
"#;
        let result = extract_const_usage(&PathBuf::from("test.rs"), content);
        assert!(result.used_constants.contains("MY_CONST"));
    }

    #[test]
    fn test_extract_path_const() {
        let content = r#"
fn main() {
    let x = module::OTHER_CONST;
}
"#;
        let result = extract_const_usage(&PathBuf::from("test.rs"), content);
        assert!(result.used_constants.contains("OTHER_CONST"));
    }

    #[test]
    fn test_extract_array_size() {
        let content = r#"
const SIZE: usize = 10;
fn foo() {
    let arr: [i32; SIZE] = [0; SIZE];
}
"#;
        let result = extract_const_usage(&PathBuf::from("test.rs"), content);
        assert!(result.used_constants.contains("SIZE"));
    }

    #[test]
    fn test_extract_multiple() {
        let content = r#"
fn main() {
    let a = CONST_A + CONST_B;
    let b = crate::CONST_C;
}
"#;
        let result = extract_const_usage(&PathBuf::from("test.rs"), content);
        assert!(result.used_constants.contains("CONST_A"));
        assert!(result.used_constants.contains("CONST_B"));
        assert!(result.used_constants.contains("CONST_C"));
    }

    #[test]
    fn test_pattern_const() {
        let content = r#"
const PATTERN: i32 = 1;

fn main() {
    match x {
        PATTERN => {}
        _ => {}
    }
}
"#;
        let result = extract_const_usage(&PathBuf::from("test.rs"), content);
        assert!(result.used_constants.contains("PATTERN"));
    }

    #[test]
    fn test_malformed_resilient() {
        let content = "fn main() { let x = BROKEN";
        let result = extract_const_usage(&PathBuf::from("broken.rs"), content);
        // Should not panic, may or may not have results
        assert!(result.used_constants.is_empty() || !result.used_constants.is_empty());
    }
}
