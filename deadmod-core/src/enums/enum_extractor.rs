//! Enum variant extraction from Rust AST.
//!
//! Extracts all enum variant definitions including:
//! - Unit variants: `enum E { A, B }`
//! - Tuple variants: `enum E { A(i32) }`
//! - Struct variants: `enum E { A { x: i32 } }`
//!
//! NASA-grade resilience: handles malformed AST gracefully.

use serde::{Deserialize, Serialize};
use std::path::Path;
use syn::{visit::Visit, File, Item, ItemEnum, ItemMod, Visibility};

/// Information about an enum variant definition.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EnumVariantDef {
    /// Name of the parent enum
    pub enum_name: String,
    /// Name of the variant
    pub variant_name: String,
    /// Full qualified name (Enum::Variant)
    pub full_name: String,
    /// Source file path
    pub file: String,
    /// Module path
    pub module_path: String,
    /// Visibility of the parent enum
    pub visibility: String,
}

/// AST visitor that extracts all enum variant definitions.
struct EnumVariantExtractor {
    file_path: String,
    results: Vec<EnumVariantDef>,
    current_mod: Vec<String>,
}

impl EnumVariantExtractor {
    fn new(file_path: String) -> Self {
        Self {
            file_path,
            results: Vec::with_capacity(32),
            current_mod: Vec::new(),
        }
    }

    fn visibility_str(v: &Visibility) -> &'static str {
        match v {
            Visibility::Public(_) => "pub",
            Visibility::Restricted(r) => {
                if r.path.is_ident("crate") {
                    "pub(crate)"
                } else if r.path.is_ident("super") {
                    "pub(super)"
                } else {
                    "pub(restricted)"
                }
            }
            Visibility::Inherited => "private",
        }
    }

    fn build_module_path(&self) -> String {
        self.current_mod.join("::")
    }
}

impl<'ast> Visit<'ast> for EnumVariantExtractor {
    fn visit_item(&mut self, item: &'ast Item) {
        match item {
            Item::Enum(ItemEnum {
                ident,
                variants,
                vis,
                ..
            }) => {
                let enum_name = ident.to_string();
                let visibility = Self::visibility_str(vis);

                for variant in variants {
                    let variant_name = variant.ident.to_string();
                    self.results.push(EnumVariantDef {
                        enum_name: enum_name.clone(),
                        variant_name: variant_name.clone(),
                        full_name: format!("{}::{}", enum_name, variant_name),
                        file: self.file_path.clone(),
                        module_path: self.build_module_path(),
                        visibility: visibility.to_string(),
                    });
                }
            }

            Item::Mod(ItemMod {
                ident,
                content: Some((_, items)),
                ..
            }) => {
                self.current_mod.push(ident.to_string());
                for i in items {
                    self.visit_item(i);
                }
                self.current_mod.pop();
                return;
            }

            _ => {}
        }

        syn::visit::visit_item(self, item);
    }
}

/// Extract all enum variant definitions from file content.
///
/// Returns a list of EnumVariantDef for each variant found.
/// On parse error, returns an empty list (resilient behavior).
pub fn extract_variants(path: &Path, content: &str) -> Vec<EnumVariantDef> {
    let ast: File = match syn::parse_file(content) {
        Ok(ast) => ast,
        Err(e) => {
            eprintln!("[WARN] AST parse failed for {}: {}", path.display(), e);
            return Vec::new();
        }
    };

    let mut extractor = EnumVariantExtractor::new(path.display().to_string());
    extractor.visit_file(&ast);
    extractor.results
}

/// Result of enum extraction from multiple files.
#[derive(Debug, Clone, Default)]
pub struct EnumExtractionResult {
    /// All declared variants
    pub declared: Vec<EnumVariantDef>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn test_extract_unit_variants() {
        let content = r#"
enum Color {
    Red,
    Green,
    Blue,
}
"#;
        let result = extract_variants(&PathBuf::from("test.rs"), content);
        assert_eq!(result.len(), 3);
        assert_eq!(result[0].enum_name, "Color");
        assert_eq!(result[0].variant_name, "Red");
        assert_eq!(result[0].full_name, "Color::Red");
    }

    #[test]
    fn test_extract_tuple_variants() {
        let content = r#"
enum Message {
    Quit,
    Move(i32, i32),
    Write(String),
}
"#;
        let result = extract_variants(&PathBuf::from("test.rs"), content);
        assert_eq!(result.len(), 3);
        assert!(result.iter().any(|v| v.variant_name == "Quit"));
        assert!(result.iter().any(|v| v.variant_name == "Move"));
        assert!(result.iter().any(|v| v.variant_name == "Write"));
    }

    #[test]
    fn test_extract_struct_variants() {
        let content = r#"
enum Event {
    Click { x: i32, y: i32 },
    KeyPress { key: char },
}
"#;
        let result = extract_variants(&PathBuf::from("test.rs"), content);
        assert_eq!(result.len(), 2);
        assert!(result.iter().any(|v| v.variant_name == "Click"));
        assert!(result.iter().any(|v| v.variant_name == "KeyPress"));
    }

    #[test]
    fn test_extract_pub_enum() {
        let content = r#"
pub enum Status {
    Active,
    Inactive,
}
"#;
        let result = extract_variants(&PathBuf::from("test.rs"), content);
        assert_eq!(result.len(), 2);
        assert_eq!(result[0].visibility, "pub");
    }

    #[test]
    fn test_extract_nested_enum() {
        let content = r#"
mod inner {
    enum Nested {
        A,
        B,
    }
}
"#;
        let result = extract_variants(&PathBuf::from("test.rs"), content);
        assert_eq!(result.len(), 2);
        assert_eq!(result[0].module_path, "inner");
    }

    #[test]
    fn test_multiple_enums() {
        let content = r#"
enum First { A, B }
enum Second { X, Y, Z }
"#;
        let result = extract_variants(&PathBuf::from("test.rs"), content);
        assert_eq!(result.len(), 5);
    }

    #[test]
    fn test_malformed_resilient() {
        let content = "enum { broken }";
        let result = extract_variants(&PathBuf::from("broken.rs"), content);
        assert!(result.is_empty());
    }
}
