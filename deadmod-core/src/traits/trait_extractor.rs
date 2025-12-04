//! Trait and trait impl extraction from Rust AST.
//!
//! Extracts:
//! - Trait definitions with their methods (required vs provided)
//! - Trait impl blocks (`impl Trait for Type`)
//! - Method visibility and signatures
//!
//! NASA-grade resilience: handles malformed AST gracefully.

use serde::{Deserialize, Serialize};
use std::path::Path;
use syn::{
    visit::Visit, File, ImplItem, ImplItemFn, Item, ItemImpl, ItemMod, ItemTrait, TraitItem,
    TraitItemFn, Visibility,
};

/// Information about a method defined in a trait.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TraitMethodDef {
    /// The trait this method belongs to
    pub trait_name: String,
    /// Simple method name
    pub method_name: String,
    /// Full path including module (e.g., "module::MyTrait::method")
    pub full_path: String,
    /// Visibility of the trait
    pub visibility: String,
    /// Whether this method requires implementation (no default body)
    pub is_required: bool,
    /// Source file path
    pub file: String,
}

/// Information about a method implemented for a trait.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TraitImplMethod {
    /// The trait being implemented
    pub trait_name: String,
    /// The type implementing the trait
    pub type_name: String,
    /// The method name
    pub method_name: String,
    /// Full identifier: "impl Trait for Type :: method"
    pub full_id: String,
    /// Source file path
    pub file: String,
}

/// Information about an inherent impl method (impl Type { fn method() {} }).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InherentImplMethod {
    /// The type this method is defined on
    pub type_name: String,
    /// The method name
    pub method_name: String,
    /// Full identifier: "Type::method"
    pub full_id: String,
    /// Visibility: "pub", "pub(crate)", etc.
    pub visibility: String,
    /// Whether this is a static method (no self receiver)
    pub is_static: bool,
    /// Source file path
    pub file: String,
    /// Module path
    pub module_path: String,
}

/// Result of trait extraction from a file.
#[derive(Debug, Clone, Default)]
pub struct TraitExtractionResult {
    /// All trait method definitions found
    pub trait_methods: Vec<TraitMethodDef>,
    /// All trait impl methods found
    pub impl_methods: Vec<TraitImplMethod>,
    /// All inherent impl methods found (impl Type { fn method() {} })
    pub inherent_methods: Vec<InherentImplMethod>,
}

/// AST visitor that extracts trait definitions and implementations.
struct TraitExtractor {
    file_path: String,
    current_mod: Vec<String>,
    result: TraitExtractionResult,
}

impl TraitExtractor {
    fn new(file_path: String) -> Self {
        Self {
            file_path,
            current_mod: Vec::new(),
            result: TraitExtractionResult::default(),
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

    fn build_path(&self, components: &[&str]) -> String {
        let mut parts: Vec<String> = self.current_mod.clone();
        parts.extend(components.iter().map(|s| s.to_string()));
        parts.join("::")
    }

    fn record_trait_method(
        &mut self,
        trait_name: &str,
        method_name: &str,
        vis: &Visibility,
        is_required: bool,
    ) {
        let full_path = self.build_path(&[trait_name, method_name]);

        self.result.trait_methods.push(TraitMethodDef {
            trait_name: trait_name.to_string(),
            method_name: method_name.to_string(),
            full_path,
            visibility: Self::visibility_str(vis).to_string(),
            is_required,
            file: self.file_path.clone(),
        });
    }

    fn record_impl_method(&mut self, trait_name: &str, type_name: &str, method_name: &str) {
        let full_id = format!("impl {} for {} :: {}", trait_name, type_name, method_name);

        self.result.impl_methods.push(TraitImplMethod {
            trait_name: trait_name.to_string(),
            type_name: type_name.to_string(),
            method_name: method_name.to_string(),
            full_id,
            file: self.file_path.clone(),
        });
    }

    fn record_inherent_method(
        &mut self,
        type_name: &str,
        method_name: &str,
        vis: &Visibility,
        is_static: bool,
    ) {
        let full_id = format!("{}::{}", type_name, method_name);

        self.result.inherent_methods.push(InherentImplMethod {
            type_name: type_name.to_string(),
            method_name: method_name.to_string(),
            full_id,
            visibility: Self::visibility_str(vis).to_string(),
            is_static,
            file: self.file_path.clone(),
            module_path: self.build_path(&[]),
        });
    }

}

impl<'ast> Visit<'ast> for TraitExtractor {
    fn visit_item(&mut self, item: &'ast Item) {
        match item {
            // Handle inline modules
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
            }

            // Trait definitions: trait Foo { fn bar(); fn baz() {} }
            Item::Trait(ItemTrait {
                ident,
                items,
                vis,
                ..
            }) => {
                let trait_name = ident.to_string();

                for trait_item in items {
                    if let TraitItem::Fn(TraitItemFn { sig, default, .. }) = trait_item {
                        let method_name = sig.ident.to_string();
                        let is_required = default.is_none();
                        self.record_trait_method(&trait_name, &method_name, vis, is_required);
                    }
                }
            }

            // Trait implementations: impl Trait for Type { ... }
            Item::Impl(ItemImpl {
                trait_: Some((_, trait_path, _)),
                self_ty,
                items,
                ..
            }) => {
                // Extract trait name from path
                let trait_name = trait_path
                    .segments
                    .last()
                    .map(|s| s.ident.to_string())
                    .unwrap_or_else(|| "<unknown>".to_string());

                // Extract type name
                let type_name = extract_type_name(self_ty);

                // Record all implemented methods
                for impl_item in items {
                    if let ImplItem::Fn(ImplItemFn { sig, .. }) = impl_item {
                        let method_name = sig.ident.to_string();
                        self.record_impl_method(&trait_name, &type_name, &method_name);
                    }
                }
            }

            // Inherent implementations: impl Type { fn method() {} }
            Item::Impl(ItemImpl {
                trait_: None,
                self_ty,
                items,
                ..
            }) => {
                let type_name = extract_type_name(self_ty);

                for impl_item in items {
                    if let ImplItem::Fn(ImplItemFn { sig, vis, .. }) = impl_item {
                        let method_name = sig.ident.to_string();
                        // Check if method has a self receiver
                        let is_static = !sig.inputs.iter().any(|arg| {
                            matches!(arg, syn::FnArg::Receiver(_))
                        });
                        self.record_inherent_method(&type_name, &method_name, vis, is_static);
                    }
                }
            }

            _ => {
                // Continue visiting for nested items
                syn::visit::visit_item(self, item);
            }
        }
    }
}

/// Extract a readable type name from a syn::Type.
fn extract_type_name(ty: &syn::Type) -> String {
    match ty {
        syn::Type::Path(type_path) => type_path
            .path
            .segments
            .iter()
            .map(|s| s.ident.to_string())
            .collect::<Vec<_>>()
            .join("::"),
        syn::Type::Reference(r) => {
            let inner = extract_type_name(&r.elem);
            if r.mutability.is_some() {
                format!("&mut {}", inner)
            } else {
                format!("&{}", inner)
            }
        }
        _ => "<unknown>".to_string(),
    }
}

/// Extract all traits and trait implementations from file content.
///
/// Returns trait method definitions and impl methods.
/// On parse error, returns empty result (resilient behavior).
pub fn extract_traits(path: &Path, content: &str) -> TraitExtractionResult {
    let ast: File = match syn::parse_file(content) {
        Ok(ast) => ast,
        Err(e) => {
            eprintln!("[WARN] AST parse failed for {}: {}", path.display(), e);
            return TraitExtractionResult::default();
        }
    };

    let mut extractor = TraitExtractor::new(path.display().to_string());
    extractor.visit_file(&ast);
    extractor.result
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn test_extract_trait_definition() {
        let content = r#"
pub trait MyTrait {
    fn required_method(&self);
    fn provided_method(&self) {}
}
"#;
        let result = extract_traits(&PathBuf::from("test.rs"), content);
        assert_eq!(result.trait_methods.len(), 2);

        let required = result
            .trait_methods
            .iter()
            .find(|m| m.method_name == "required_method")
            .unwrap();
        assert!(required.is_required);
        assert_eq!(required.visibility, "pub");

        let provided = result
            .trait_methods
            .iter()
            .find(|m| m.method_name == "provided_method")
            .unwrap();
        assert!(!provided.is_required);
    }

    #[test]
    fn test_extract_trait_impl() {
        let content = r#"
trait Foo {
    fn bar(&self);
}

struct MyStruct;

impl Foo for MyStruct {
    fn bar(&self) {}
}
"#;
        let result = extract_traits(&PathBuf::from("test.rs"), content);

        assert_eq!(result.trait_methods.len(), 1);
        assert_eq!(result.impl_methods.len(), 1);

        let impl_method = &result.impl_methods[0];
        assert_eq!(impl_method.trait_name, "Foo");
        assert_eq!(impl_method.type_name, "MyStruct");
        assert_eq!(impl_method.method_name, "bar");
    }

    #[test]
    fn test_nested_mod_trait() {
        let content = r#"
mod inner {
    pub trait InnerTrait {
        fn inner_method(&self);
    }
}
"#;
        let result = extract_traits(&PathBuf::from("test.rs"), content);
        assert_eq!(result.trait_methods.len(), 1);
        assert_eq!(
            result.trait_methods[0].full_path,
            "inner::InnerTrait::inner_method"
        );
    }

    #[test]
    fn test_multiple_impl_methods() {
        let content = r#"
trait Multi {
    fn a(&self);
    fn b(&self);
    fn c(&self);
}

struct S;

impl Multi for S {
    fn a(&self) {}
    fn b(&self) {}
    fn c(&self) {}
}
"#;
        let result = extract_traits(&PathBuf::from("test.rs"), content);
        assert_eq!(result.trait_methods.len(), 3);
        assert_eq!(result.impl_methods.len(), 3);
    }

    #[test]
    fn test_malformed_resilient() {
        let content = "trait Broken { fn";
        let result = extract_traits(&PathBuf::from("test.rs"), content);
        assert!(result.trait_methods.is_empty());
        assert!(result.impl_methods.is_empty());
    }

    #[test]
    fn test_extract_inherent_impl() {
        let content = r#"
struct MyStruct;

impl MyStruct {
    pub fn public_method(&self) {}
    fn private_method(&mut self) {}
    pub fn static_method() {}
}
"#;
        let result = extract_traits(&PathBuf::from("test.rs"), content);
        assert_eq!(result.inherent_methods.len(), 3);

        let public = result
            .inherent_methods
            .iter()
            .find(|m| m.method_name == "public_method")
            .unwrap();
        assert_eq!(public.visibility, "pub");
        assert!(!public.is_static);

        let private = result
            .inherent_methods
            .iter()
            .find(|m| m.method_name == "private_method")
            .unwrap();
        assert_eq!(private.visibility, "private");

        let static_m = result
            .inherent_methods
            .iter()
            .find(|m| m.method_name == "static_method")
            .unwrap();
        assert!(static_m.is_static);
    }

    #[test]
    fn test_inherent_impl_full_id() {
        let content = r#"
struct Foo;
impl Foo {
    fn bar(&self) {}
}
"#;
        let result = extract_traits(&PathBuf::from("test.rs"), content);
        assert_eq!(result.inherent_methods.len(), 1);
        assert_eq!(result.inherent_methods[0].full_id, "Foo::bar");
    }
}
