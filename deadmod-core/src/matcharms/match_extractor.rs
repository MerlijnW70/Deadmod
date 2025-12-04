//! Match arm extraction from Rust AST.
//!
//! Extracts all match arms from match expressions including:
//! - Pattern types (ident, path, tuple struct, struct, wildcard)
//! - Match arm position within the expression
//! - File location information
//!
//! NASA-grade resilience: handles malformed AST gracefully.

use serde::{Deserialize, Serialize};
use std::path::Path;
use syn::{visit::Visit, Expr, File, Pat};

/// Information about a match arm.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MatchArm {
    /// The pattern as a string representation
    pub pattern: String,
    /// The variant name if this is an enum variant pattern
    pub variant_name: Option<String>,
    /// Whether this is a wildcard pattern (_)
    pub is_wildcard: bool,
    /// Position in the match expression (0-indexed)
    pub position: usize,
    /// Total number of arms in this match expression
    pub total_arms: usize,
    /// Source file path
    pub file: String,
}

/// Result of match arm extraction from a file.
#[derive(Debug, Clone, Default)]
pub struct MatchExtractionResult {
    /// All match arms found
    pub arms: Vec<MatchArm>,
    /// Count of match expressions found
    pub match_count: usize,
}

/// AST visitor that extracts all match arms.
struct MatchExtractor {
    file_path: String,
    result: MatchExtractionResult,
}

impl MatchExtractor {
    fn new(file_path: String) -> Self {
        Self {
            file_path,
            result: MatchExtractionResult::default(),
        }
    }

    fn extract_pattern_info(pat: &Pat) -> (String, Option<String>, bool) {
        match pat {
            Pat::Wild(_) => ("_".to_string(), None, true),

            Pat::Ident(pi) => {
                let name = pi.ident.to_string();
                (name.clone(), Some(name), false)
            }

            Pat::Path(p) => {
                let name = p
                    .path
                    .segments
                    .last()
                    .map(|s| s.ident.to_string())
                    .unwrap_or_else(|| "<unknown>".to_string());
                let full_path = p
                    .path
                    .segments
                    .iter()
                    .map(|s| s.ident.to_string())
                    .collect::<Vec<_>>()
                    .join("::");
                (full_path, Some(name), false)
            }

            Pat::TupleStruct(ts) => {
                let name = ts
                    .path
                    .segments
                    .last()
                    .map(|s| s.ident.to_string())
                    .unwrap_or_else(|| "<unknown>".to_string());
                let full_path = ts
                    .path
                    .segments
                    .iter()
                    .map(|s| s.ident.to_string())
                    .collect::<Vec<_>>()
                    .join("::");
                (format!("{}(..)", full_path), Some(name), false)
            }

            Pat::Struct(ps) => {
                let name = ps
                    .path
                    .segments
                    .last()
                    .map(|s| s.ident.to_string())
                    .unwrap_or_else(|| "<unknown>".to_string());
                let full_path = ps
                    .path
                    .segments
                    .iter()
                    .map(|s| s.ident.to_string())
                    .collect::<Vec<_>>()
                    .join("::");
                (format!("{} {{ .. }}", full_path), Some(name), false)
            }

            Pat::Or(por) => {
                // Multiple patterns: A | B | C
                let patterns: Vec<_> = por
                    .cases
                    .iter()
                    .map(|p| Self::extract_pattern_info(p).0)
                    .collect();
                (patterns.join(" | "), None, false)
            }

            Pat::Lit(_) => {
                // Literal patterns like 0, "string", etc.
                ("<literal>".to_string(), None, false)
            }

            Pat::Range(_) => {
                // Range patterns like 1..=5
                ("<range>".to_string(), None, false)
            }

            Pat::Slice(_) => {
                // Slice patterns like [a, b, c]
                ("<slice>".to_string(), None, false)
            }

            Pat::Tuple(_) => {
                // Tuple patterns like (a, b)
                ("<tuple>".to_string(), None, false)
            }

            Pat::Reference(pr) => {
                // Reference patterns like &x
                let (inner, variant, is_wild) = Self::extract_pattern_info(&pr.pat);
                (format!("&{}", inner), variant, is_wild)
            }

            _ => ("<complex>".to_string(), None, false),
        }
    }
}

impl<'ast> Visit<'ast> for MatchExtractor {
    fn visit_expr(&mut self, expr: &'ast Expr) {
        if let Expr::Match(m) = expr {
            self.result.match_count += 1;
            let total_arms = m.arms.len();

            for (position, arm) in m.arms.iter().enumerate() {
                let (pattern, variant_name, is_wildcard) = Self::extract_pattern_info(&arm.pat);

                self.result.arms.push(MatchArm {
                    pattern,
                    variant_name,
                    is_wildcard,
                    position,
                    total_arms,
                    file: self.file_path.clone(),
                });
            }
        }

        syn::visit::visit_expr(self, expr);
    }
}

/// Extract all match arms from file content.
///
/// Returns match arm information for all match expressions found.
/// On parse error, returns empty result (resilient behavior).
pub fn extract_match_arms(path: &Path, content: &str) -> MatchExtractionResult {
    let ast: File = match syn::parse_file(content) {
        Ok(ast) => ast,
        Err(e) => {
            eprintln!("[WARN] AST parse failed for {}: {}", path.display(), e);
            return MatchExtractionResult::default();
        }
    };

    let mut extractor = MatchExtractor::new(path.display().to_string());
    extractor.visit_file(&ast);
    extractor.result
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn test_extract_enum_match() {
        let content = r#"
enum Color { Red, Green, Blue }

fn f(c: Color) {
    match c {
        Color::Red => {}
        Color::Green => {}
        Color::Blue => {}
    }
}
"#;
        let result = extract_match_arms(&PathBuf::from("test.rs"), content);
        assert_eq!(result.match_count, 1);
        assert_eq!(result.arms.len(), 3);
        assert_eq!(result.arms[0].variant_name, Some("Red".to_string()));
        assert_eq!(result.arms[1].variant_name, Some("Green".to_string()));
        assert_eq!(result.arms[2].variant_name, Some("Blue".to_string()));
    }

    #[test]
    fn test_extract_wildcard() {
        let content = r#"
fn f(x: i32) {
    match x {
        0 => {}
        1 => {}
        _ => {}
    }
}
"#;
        let result = extract_match_arms(&PathBuf::from("test.rs"), content);
        assert_eq!(result.arms.len(), 3);
        assert!(result.arms[2].is_wildcard);
        assert_eq!(result.arms[2].pattern, "_");
    }

    #[test]
    fn test_extract_tuple_struct() {
        let content = r#"
enum Option<T> { Some(T), None }

fn f(o: Option<i32>) {
    match o {
        Option::Some(x) => {}
        Option::None => {}
    }
}
"#;
        let result = extract_match_arms(&PathBuf::from("test.rs"), content);
        assert_eq!(result.arms.len(), 2);
        assert!(result.arms[0].pattern.contains("Some"));
        assert_eq!(result.arms[0].variant_name, Some("Some".to_string()));
    }

    #[test]
    fn test_position_tracking() {
        let content = r#"
fn f(x: i32) {
    match x {
        0 => {}
        1 => {}
        2 => {}
        _ => {}
    }
}
"#;
        let result = extract_match_arms(&PathBuf::from("test.rs"), content);
        assert_eq!(result.arms[0].position, 0);
        assert_eq!(result.arms[1].position, 1);
        assert_eq!(result.arms[2].position, 2);
        assert_eq!(result.arms[3].position, 3);
        assert_eq!(result.arms[0].total_arms, 4);
    }

    #[test]
    fn test_malformed_resilient() {
        let content = "fn f() { match x {";
        let result = extract_match_arms(&PathBuf::from("broken.rs"), content);
        assert!(result.arms.is_empty());
    }
}
