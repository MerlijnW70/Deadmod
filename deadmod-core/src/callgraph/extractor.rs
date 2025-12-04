//! Function definition extraction for call graph analysis.
//!
//! Extracts all function definitions including:
//! - Free functions: `fn foo() {}`
//! - Impl methods: `impl Type { fn method() {} }`
//! - Trait methods: `trait T { fn method(); }`
//! - Nested functions inside other functions
//!
//! NASA-grade resilience: handles malformed AST gracefully.

use serde::{Deserialize, Serialize};
use std::path::Path;
use syn::{
    visit::Visit, File, ImplItem, Item, ItemFn, ItemImpl, ItemMod, ItemTrait, TraitItem,
    Visibility,
};

/// Information about a function definition.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FunctionDef {
    /// Simple function name
    pub name: String,
    /// Full qualified path (e.g., "module::Type::method")
    pub full_path: String,
    /// Source file path
    pub file: String,
    /// Whether this is a method (has self receiver)
    pub is_method: bool,
    /// Parent type name if this is an impl method
    pub parent_type: Option<String>,
    /// Visibility
    pub visibility: String,
}

/// AST visitor that extracts all function definitions.
struct FunctionExtractor {
    file_path: String,
    mod_stack: Vec<String>,
    results: Vec<FunctionDef>,
}

impl FunctionExtractor {
    fn new(file_path: String) -> Self {
        Self {
            file_path,
            mod_stack: Vec::new(),
            results: Vec::with_capacity(32),
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

    fn build_full_path(&self, name: &str) -> String {
        if self.mod_stack.is_empty() {
            name.to_string()
        } else {
            format!("{}::{}", self.mod_stack.join("::"), name)
        }
    }

    fn push_fn(&mut self, name: &str, vis: &Visibility, is_method: bool, parent_type: Option<String>) {
        let full_path = self.build_full_path(name);
        self.results.push(FunctionDef {
            name: name.to_string(),
            full_path,
            file: self.file_path.clone(),
            is_method,
            parent_type,
            visibility: Self::visibility_str(vis).to_string(),
        });
    }
}

impl<'ast> Visit<'ast> for FunctionExtractor {
    fn visit_item(&mut self, item: &'ast Item) {
        match item {
            // Free functions
            Item::Fn(ItemFn { sig, vis, .. }) => {
                self.push_fn(&sig.ident.to_string(), vis, false, None);
            }

            // Impl blocks
            Item::Impl(ItemImpl {
                self_ty,
                items,
                trait_,
                ..
            }) => {
                let type_name = extract_type_name(self_ty);
                let parent_name = if let Some((_, path, _)) = trait_ {
                    // impl Trait for Type - use trait name
                    path.segments
                        .last()
                        .map(|s| s.ident.to_string())
                        .unwrap_or_else(|| type_name.clone())
                } else {
                    type_name.clone()
                };

                self.mod_stack.push(parent_name.clone());

                for impl_item in items {
                    if let ImplItem::Fn(method) = impl_item {
                        let is_method = method.sig.inputs.iter().any(|arg| {
                            matches!(arg, syn::FnArg::Receiver(_))
                        });
                        self.push_fn(
                            &method.sig.ident.to_string(),
                            &method.vis,
                            is_method,
                            Some(type_name.clone()),
                        );
                    }
                }

                self.mod_stack.pop();
            }

            // Trait definitions
            Item::Trait(ItemTrait { ident, items, vis, .. }) => {
                self.mod_stack.push(ident.to_string());

                for trait_item in items {
                    if let TraitItem::Fn(method) = trait_item {
                        self.push_fn(&method.sig.ident.to_string(), vis, true, None);
                    }
                }

                self.mod_stack.pop();
            }

            // Nested modules
            Item::Mod(ItemMod {
                ident,
                content: Some((_, items)),
                ..
            }) => {
                self.mod_stack.push(ident.to_string());
                for i in items {
                    self.visit_item(i);
                }
                self.mod_stack.pop();
                return; // Don't call default visitor
            }

            _ => {}
        }

        syn::visit::visit_item(self, item);
    }
}

/// Extract a readable type name from a syn::Type.
fn extract_type_name(ty: &syn::Type) -> String {
    match ty {
        syn::Type::Path(type_path) => type_path
            .path
            .segments
            .last()
            .map(|s| s.ident.to_string())
            .unwrap_or_else(|| "<unknown>".to_string()),
        syn::Type::Reference(r) => extract_type_name(&r.elem),
        _ => "<unknown>".to_string(),
    }
}

/// Extract all function definitions from file content.
///
/// Returns a list of FunctionDef for each function found.
/// On parse error, returns an empty list (resilient behavior).
pub fn extract_callgraph_functions(path: &Path, content: &str) -> Vec<FunctionDef> {
    let ast: File = match syn::parse_file(content) {
        Ok(ast) => ast,
        Err(e) => {
            eprintln!("[WARN] AST parse failed for {}: {}", path.display(), e);
            return Vec::new();
        }
    };

    let mut extractor = FunctionExtractor::new(path.display().to_string());
    extractor.visit_file(&ast);
    extractor.results
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn test_extract_free_function() {
        let content = r#"
fn my_function() {}
pub fn public_fn() {}
"#;
        let result = extract_callgraph_functions(&PathBuf::from("test.rs"), content);
        assert_eq!(result.len(), 2);
        assert_eq!(result[0].name, "my_function");
        assert_eq!(result[0].visibility, "private");
        assert_eq!(result[1].name, "public_fn");
        assert_eq!(result[1].visibility, "pub");
    }

    #[test]
    fn test_extract_impl_methods() {
        let content = r#"
struct Foo;

impl Foo {
    fn new() -> Self { Foo }
    pub fn method(&self) {}
}
"#;
        let result = extract_callgraph_functions(&PathBuf::from("test.rs"), content);
        assert_eq!(result.len(), 2);
        assert_eq!(result[0].name, "new");
        assert!(!result[0].is_method); // no self receiver
        assert_eq!(result[1].name, "method");
        assert!(result[1].is_method);
        assert_eq!(result[1].parent_type, Some("Foo".to_string()));
    }

    #[test]
    fn test_extract_trait_methods() {
        let content = r#"
pub trait MyTrait {
    fn required(&self);
    fn provided(&self) {}
}
"#;
        let result = extract_callgraph_functions(&PathBuf::from("test.rs"), content);
        assert_eq!(result.len(), 2);
        assert!(result.iter().any(|f| f.name == "required"));
        assert!(result.iter().any(|f| f.name == "provided"));
    }

    #[test]
    fn test_extract_nested_module() {
        let content = r#"
mod inner {
    fn nested_fn() {}
}
"#;
        let result = extract_callgraph_functions(&PathBuf::from("test.rs"), content);
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].full_path, "inner::nested_fn");
    }

    #[test]
    fn test_malformed_resilient() {
        let content = "fn broken(";
        let result = extract_callgraph_functions(&PathBuf::from("broken.rs"), content);
        assert!(result.is_empty());
    }
}
