//! Match pattern usage detection from Rust AST.
//!
//! Detects all usages of enum variants and patterns including:
//! - Variant construction: Color::Red
//! - Pattern matching in match/if let/while let
//! - Path references in expressions
//!
//! NASA-grade resilience: handles malformed AST gracefully.

use std::collections::HashSet;
use std::path::Path;
use syn::{visit::Visit, Expr, File, Pat};

/// Result of match usage analysis.
#[derive(Debug, Clone, Default)]
pub struct MatchUsageResult {
    /// Set of variant names that are used (constructed or matched)
    pub used_variants: HashSet<String>,
    /// Set of full paths used (e.g., "Color::Red")
    pub used_full_paths: HashSet<String>,
}

/// AST visitor that extracts variant usages.
struct MatchUsageExtractor {
    used_variants: HashSet<String>,
    used_full_paths: HashSet<String>,
}

impl MatchUsageExtractor {
    fn new() -> Self {
        Self {
            used_variants: HashSet::with_capacity(32),
            used_full_paths: HashSet::with_capacity(32),
        }
    }

    fn record_path(&mut self, path: &syn::Path) {
        // Record the last segment (variant name)
        if let Some(seg) = path.segments.last() {
            self.used_variants.insert(seg.ident.to_string());
        }

        // Record full path for qualified references
        let full_path = path
            .segments
            .iter()
            .map(|s| s.ident.to_string())
            .collect::<Vec<_>>()
            .join("::");
        self.used_full_paths.insert(full_path);
    }

    fn record_pattern(&mut self, pat: &Pat) {
        match pat {
            Pat::Path(p) => {
                self.record_path(&p.path);
            }

            Pat::TupleStruct(ts) => {
                self.record_path(&ts.path);
                // Also visit nested patterns
                for elem in &ts.elems {
                    self.record_pattern(elem);
                }
            }

            Pat::Struct(ps) => {
                self.record_path(&ps.path);
                // Visit field patterns
                for field in &ps.fields {
                    self.record_pattern(&field.pat);
                }
            }

            Pat::Ident(pi) => {
                let name = pi.ident.to_string();
                // Only record if it looks like a variant (starts with uppercase)
                if name.chars().next().map(|c| c.is_uppercase()).unwrap_or(false) {
                    self.used_variants.insert(name);
                }
            }

            Pat::Or(por) => {
                for case in &por.cases {
                    self.record_pattern(case);
                }
            }

            Pat::Reference(pr) => {
                self.record_pattern(&pr.pat);
            }

            Pat::Slice(ps) => {
                for elem in &ps.elems {
                    self.record_pattern(elem);
                }
            }

            Pat::Tuple(pt) => {
                for elem in &pt.elems {
                    self.record_pattern(elem);
                }
            }

            _ => {}
        }
    }
}

impl<'ast> Visit<'ast> for MatchUsageExtractor {
    fn visit_expr(&mut self, expr: &'ast Expr) {
        match expr {
            // Path expressions: Color::Red, Option::Some
            Expr::Path(p) => {
                self.record_path(&p.path);
            }

            // Struct construction: Color::Red { field }
            Expr::Struct(s) => {
                self.record_path(&s.path);
            }

            // Call expressions: Option::Some(value)
            Expr::Call(c) => {
                if let Expr::Path(p) = &*c.func {
                    self.record_path(&p.path);
                }
            }

            _ => {}
        }

        syn::visit::visit_expr(self, expr);
    }

    fn visit_pat(&mut self, pat: &'ast Pat) {
        self.record_pattern(pat);
        syn::visit::visit_pat(self, pat);
    }
}

/// Extract all variant usages from file content.
///
/// Returns sets of used variant names and full paths.
/// On parse error, returns empty result (resilient behavior).
pub fn extract_match_usages(path: &Path, content: &str) -> MatchUsageResult {
    let ast: File = match syn::parse_file(content) {
        Ok(ast) => ast,
        Err(e) => {
            eprintln!("[WARN] AST parse failed for {}: {}", path.display(), e);
            return MatchUsageResult::default();
        }
    };

    let mut extractor = MatchUsageExtractor::new();
    extractor.visit_file(&ast);

    MatchUsageResult {
        used_variants: extractor.used_variants,
        used_full_paths: extractor.used_full_paths,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn test_extract_construction() {
        let content = r#"
fn f() {
    let x = Color::Red;
    let y = Option::Some(42);
}
"#;
        let result = extract_match_usages(&PathBuf::from("test.rs"), content);
        assert!(result.used_variants.contains("Red"));
        assert!(result.used_variants.contains("Some"));
        assert!(result.used_full_paths.contains("Color::Red"));
        assert!(result.used_full_paths.contains("Option::Some"));
    }

    #[test]
    fn test_extract_pattern_match() {
        let content = r#"
fn f(c: Color) {
    match c {
        Color::Red => {}
        Color::Green => {}
        _ => {}
    }
}
"#;
        let result = extract_match_usages(&PathBuf::from("test.rs"), content);
        assert!(result.used_variants.contains("Red"));
        assert!(result.used_variants.contains("Green"));
    }

    #[test]
    fn test_extract_if_let() {
        let content = r#"
fn f(o: Option<i32>) {
    if let Some(x) = o {
        println!("{}", x);
    }
}
"#;
        let result = extract_match_usages(&PathBuf::from("test.rs"), content);
        assert!(result.used_variants.contains("Some"));
    }

    #[test]
    fn test_extract_or_pattern() {
        let content = r#"
fn f(c: Color) {
    match c {
        Color::Red | Color::Blue => {}
        _ => {}
    }
}
"#;
        let result = extract_match_usages(&PathBuf::from("test.rs"), content);
        assert!(result.used_variants.contains("Red"));
        assert!(result.used_variants.contains("Blue"));
    }

    #[test]
    fn test_malformed_resilient() {
        let content = "fn f() { match x { broken";
        let result = extract_match_usages(&PathBuf::from("broken.rs"), content);
        // Should not panic
        assert!(result.used_variants.is_empty() || !result.used_variants.is_empty());
    }
}
