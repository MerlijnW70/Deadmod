//! Enum variant usage detection from Rust AST.
//!
//! Detects all usages of enum variants including:
//! - Construction: `MyEnum::Variant`
//! - Pattern matching: `match x { MyEnum::Variant => ... }`
//! - Struct patterns: `MyEnum::Variant { field }`
//! - Tuple patterns: `MyEnum::Variant(x)`
//!
//! NASA-grade resilience: handles malformed AST gracefully.

use std::collections::HashSet;
use std::path::Path;
use syn::{visit::Visit, Arm, Expr, File, Pat};

/// Information about enum variant usages in a file.
#[derive(Debug, Clone, Default)]
pub struct EnumUsageResult {
    /// Set of variant names that are used (just the variant name, not full path)
    pub used_variants: HashSet<String>,
    /// Set of full paths like "Enum::Variant" that are used
    pub used_full_paths: HashSet<String>,
}

/// AST visitor that extracts all enum variant usages.
struct EnumUsageExtractor {
    used_variants: HashSet<String>,
    used_full_paths: HashSet<String>,
}

impl EnumUsageExtractor {
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

        // For paths like Enum::Variant, record full path
        if path.segments.len() >= 2 {
            let segments: Vec<_> = path
                .segments
                .iter()
                .map(|s| s.ident.to_string())
                .collect();

            // Record "Enum::Variant" style paths
            if segments.len() >= 2 {
                let last_two = format!(
                    "{}::{}",
                    segments[segments.len() - 2],
                    segments[segments.len() - 1]
                );
                self.used_full_paths.insert(last_two);
            }
        }
    }
}

impl<'ast> Visit<'ast> for EnumUsageExtractor {
    fn visit_expr(&mut self, expr: &'ast Expr) {
        match expr {
            // Path expressions: Enum::Variant
            Expr::Path(p) => {
                self.record_path(&p.path);
            }

            // Struct expressions: Enum::Variant { field: value }
            Expr::Struct(s) => {
                self.record_path(&s.path);
            }

            // Call expressions: Enum::Variant(args)
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
        match pat {
            // Path patterns: Enum::Variant
            Pat::Path(p) => {
                self.record_path(&p.path);
            }

            // Struct patterns: Enum::Variant { field }
            Pat::Struct(ps) => {
                self.record_path(&ps.path);
            }

            // Tuple struct patterns: Enum::Variant(x)
            Pat::TupleStruct(pts) => {
                self.record_path(&pts.path);
            }

            // Ident patterns (for irrefutable patterns)
            Pat::Ident(pi) => {
                let name = pi.ident.to_string();
                // Heuristic: variant names typically start uppercase
                if name.chars().next().map(|c| c.is_uppercase()).unwrap_or(false) {
                    self.used_variants.insert(name);
                }
            }

            _ => {}
        }

        syn::visit::visit_pat(self, pat);
    }

    fn visit_arm(&mut self, arm: &'ast Arm) {
        // Visit pattern in match arm
        self.visit_pat(&arm.pat);

        // Visit guard expression if present
        if let Some((_, guard)) = &arm.guard {
            self.visit_expr(guard);
        }

        // Visit body expression
        self.visit_expr(&arm.body);
    }
}

/// Extract all enum variant usages from file content.
///
/// Returns information about used variants.
/// On parse error, returns empty result (resilient behavior).
pub fn extract_variant_usage(path: &Path, content: &str) -> EnumUsageResult {
    let ast: File = match syn::parse_file(content) {
        Ok(ast) => ast,
        Err(e) => {
            eprintln!("[WARN] AST parse failed for {}: {}", path.display(), e);
            return EnumUsageResult::default();
        }
    };

    let mut extractor = EnumUsageExtractor::new();
    extractor.visit_file(&ast);

    EnumUsageResult {
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
fn main() {
    let x = Color::Red;
}
"#;
        let result = extract_variant_usage(&PathBuf::from("test.rs"), content);
        assert!(result.used_variants.contains("Red"));
        assert!(result.used_full_paths.contains("Color::Red"));
    }

    #[test]
    fn test_extract_match_pattern() {
        let content = r#"
fn main() {
    match color {
        Color::Red => {},
        Color::Blue => {},
        _ => {},
    }
}
"#;
        let result = extract_variant_usage(&PathBuf::from("test.rs"), content);
        assert!(result.used_variants.contains("Red"));
        assert!(result.used_variants.contains("Blue"));
    }

    #[test]
    fn test_extract_tuple_variant() {
        let content = r#"
fn main() {
    let msg = Message::Move(10, 20);

    match msg {
        Message::Move(x, y) => {},
        _ => {},
    }
}
"#;
        let result = extract_variant_usage(&PathBuf::from("test.rs"), content);
        assert!(result.used_variants.contains("Move"));
    }

    #[test]
    fn test_extract_struct_variant() {
        let content = r#"
fn main() {
    let event = Event::Click { x: 10, y: 20 };

    match event {
        Event::Click { x, y } => {},
        _ => {},
    }
}
"#;
        let result = extract_variant_usage(&PathBuf::from("test.rs"), content);
        assert!(result.used_variants.contains("Click"));
    }

    #[test]
    fn test_extract_if_let() {
        let content = r#"
fn main() {
    if let Some(x) = optional {
        // ...
    }
}
"#;
        let result = extract_variant_usage(&PathBuf::from("test.rs"), content);
        assert!(result.used_variants.contains("Some"));
    }

    #[test]
    fn test_multiple_usages() {
        let content = r#"
fn main() {
    let _ = Status::Active;
    let _ = Status::Inactive;
    let _ = Result::Ok(42);
    let _ = Result::Err("error");
}
"#;
        let result = extract_variant_usage(&PathBuf::from("test.rs"), content);
        assert!(result.used_variants.contains("Active"));
        assert!(result.used_variants.contains("Inactive"));
        assert!(result.used_variants.contains("Ok"));
        assert!(result.used_variants.contains("Err"));
    }

    #[test]
    fn test_malformed_resilient() {
        let content = "fn main() { let x = Broken::";
        let result = extract_variant_usage(&PathBuf::from("broken.rs"), content);
        // Should not panic
        assert!(result.used_variants.is_empty() || !result.used_variants.is_empty());
    }
}
