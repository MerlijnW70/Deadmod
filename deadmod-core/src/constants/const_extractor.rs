//! Constant and static extraction from Rust AST.
//!
//! Extracts all constant and static definitions including:
//! - `const NAME: T = ...`
//! - `static NAME: T = ...`
//! - `static mut NAME: T = ...`
//! - Constants inside impl blocks
//!
//! NASA-grade resilience: handles malformed AST gracefully.

use serde::{Deserialize, Serialize};
use std::path::Path;
use syn::{visit::Visit, File, ImplItem, Item, ItemConst, ItemImpl, ItemMod, ItemStatic, Visibility};

use crate::common::visibility_str;

/// Information about a constant or static definition.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConstDef {
    /// Name of the constant
    pub name: String,
    /// Source file path
    pub file: String,
    /// Whether this is a static (vs const)
    pub is_static: bool,
    /// Whether this is mutable (static mut)
    pub is_mutable: bool,
    /// Visibility: "pub", "pub(crate)", etc.
    pub visibility: String,
    /// Module path
    pub module_path: String,
    /// If inside an impl block, the type name
    pub impl_type: Option<String>,
}

/// AST visitor that extracts all constant definitions.
struct ConstExtractor {
    file_path: String,
    results: Vec<ConstDef>,
    current_mod: Vec<String>,
    current_impl: Option<String>,
}

impl ConstExtractor {
    fn new(file_path: String) -> Self {
        Self {
            file_path,
            results: Vec::with_capacity(16),
            current_mod: Vec::new(),
            current_impl: None,
        }
    }

    fn build_module_path(&self) -> String {
        self.current_mod.join("::")
    }

    fn record_const(&mut self, name: &str, vis: &Visibility) {
        self.results.push(ConstDef {
            name: name.to_string(),
            file: self.file_path.clone(),
            is_static: false,
            is_mutable: false,
            visibility: visibility_str(vis).to_string(),
            module_path: self.build_module_path(),
            impl_type: self.current_impl.clone(),
        });
    }

    fn record_static(&mut self, name: &str, vis: &Visibility, is_mut: bool) {
        self.results.push(ConstDef {
            name: name.to_string(),
            file: self.file_path.clone(),
            is_static: true,
            is_mutable: is_mut,
            visibility: visibility_str(vis).to_string(),
            module_path: self.build_module_path(),
            impl_type: self.current_impl.clone(),
        });
    }
}

impl<'ast> Visit<'ast> for ConstExtractor {
    fn visit_item(&mut self, item: &'ast Item) {
        match item {
            Item::Const(ItemConst { ident, vis, .. }) => {
                self.record_const(&ident.to_string(), vis);
            }

            Item::Static(ItemStatic {
                ident,
                vis,
                mutability,
                ..
            }) => {
                // In syn 2.x, mutability is StaticMutability enum, not Option
                let is_mut = matches!(mutability, syn::StaticMutability::Mut(_));
                self.record_static(&ident.to_string(), vis, is_mut);
            }

            Item::Impl(ItemImpl {
                self_ty, items, ..
            }) => {
                // Extract type name for impl block
                let type_name = extract_type_name(self_ty);
                self.current_impl = Some(type_name);

                for impl_item in items {
                    if let ImplItem::Const(c) = impl_item {
                        self.record_const(&c.ident.to_string(), &c.vis);
                    }
                }

                self.current_impl = None;
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

/// Extract a readable type name from a syn::Type.
fn extract_type_name(ty: &syn::Type) -> String {
    match ty {
        syn::Type::Path(p) => p
            .path
            .segments
            .last()
            .map(|s| s.ident.to_string())
            .unwrap_or_else(|| "<unknown>".to_string()),
        _ => "<unknown>".to_string(),
    }
}

/// Extract all constant and static definitions from file content.
///
/// Returns a list of ConstDef for each constant/static found.
/// On parse error, returns an empty list (resilient behavior).
pub fn extract_constants(path: &Path, content: &str) -> Vec<ConstDef> {
    let ast: File = match syn::parse_file(content) {
        Ok(ast) => ast,
        Err(e) => {
            eprintln!("[WARN] AST parse failed for {}: {}", path.display(), e);
            return Vec::new();
        }
    };

    let mut extractor = ConstExtractor::new(path.display().to_string());
    extractor.visit_file(&ast);
    extractor.results
}

/// Result of constant extraction from multiple files.
#[derive(Debug, Clone, Default)]
pub struct ConstExtractionResult {
    /// All declared constants
    pub declared: Vec<ConstDef>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn test_extract_const() {
        let content = r#"
const MY_CONST: i32 = 42;
"#;
        let result = extract_constants(&PathBuf::from("test.rs"), content);
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].name, "MY_CONST");
        assert!(!result[0].is_static);
    }

    #[test]
    fn test_extract_static() {
        let content = r#"
static MY_STATIC: i32 = 42;
"#;
        let result = extract_constants(&PathBuf::from("test.rs"), content);
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].name, "MY_STATIC");
        assert!(result[0].is_static);
        assert!(!result[0].is_mutable);
    }

    #[test]
    fn test_extract_static_mut() {
        let content = r#"
static mut MUTABLE: i32 = 0;
"#;
        let result = extract_constants(&PathBuf::from("test.rs"), content);
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].name, "MUTABLE");
        assert!(result[0].is_static);
        assert!(result[0].is_mutable);
    }

    #[test]
    fn test_extract_pub_const() {
        let content = r#"
pub const PUBLIC: i32 = 1;
pub(crate) const CRATE_VIS: i32 = 2;
const PRIVATE: i32 = 3;
"#;
        let result = extract_constants(&PathBuf::from("test.rs"), content);
        assert_eq!(result.len(), 3);

        let public = result.iter().find(|c| c.name == "PUBLIC").unwrap();
        assert_eq!(public.visibility, "pub");

        let crate_vis = result.iter().find(|c| c.name == "CRATE_VIS").unwrap();
        assert_eq!(crate_vis.visibility, "pub(crate)");

        let private = result.iter().find(|c| c.name == "PRIVATE").unwrap();
        assert_eq!(private.visibility, "private");
    }

    #[test]
    fn test_extract_impl_const() {
        let content = r#"
struct Foo;

impl Foo {
    const IMPL_CONST: i32 = 100;
}
"#;
        let result = extract_constants(&PathBuf::from("test.rs"), content);
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].name, "IMPL_CONST");
        assert_eq!(result[0].impl_type, Some("Foo".to_string()));
    }

    #[test]
    fn test_extract_nested_mod() {
        let content = r#"
mod inner {
    const NESTED: i32 = 1;
}
"#;
        let result = extract_constants(&PathBuf::from("test.rs"), content);
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].name, "NESTED");
        assert_eq!(result[0].module_path, "inner");
    }

    #[test]
    fn test_malformed_resilient() {
        let content = "const { broken";
        let result = extract_constants(&PathBuf::from("broken.rs"), content);
        assert!(result.is_empty());
    }
}
