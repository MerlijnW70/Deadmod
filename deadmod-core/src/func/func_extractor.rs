//! Function extraction from Rust AST.
//!
//! Extracts all function declarations including:
//! - Free functions (fn foo())
//! - Impl methods (impl Foo { fn bar() })
//! - Trait impl methods
//! - Associated functions
//!
//! NASA-grade resilience: handles malformed AST gracefully.

use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::path::Path;
use syn::{visit::Visit, Attribute, File, ImplItem, ImplItemFn, Item, ItemFn, ItemImpl, ItemMod, Visibility};

/// Information about a single function.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FunctionInfo {
    /// Simple function name (e.g., "foo")
    pub name: String,
    /// Full path including module (e.g., "utils::helpers::foo")
    pub full_path: String,
    /// Visibility: "pub", "pub(crate)", "pub(super)", or "private"
    pub visibility: String,
    /// Source file path
    pub file: String,
    /// Whether this is a method (inside impl block)
    pub is_method: bool,
    /// The type this method belongs to (if is_method)
    pub impl_type: Option<String>,
    /// Whether this function has #[test] attribute
    pub is_test: bool,
    /// Whether this function has #[no_mangle] attribute (FFI entry point)
    pub is_no_mangle: bool,
}

/// AST visitor that extracts all function declarations.
struct FunctionExtractor {
    file_path: String,
    results: Vec<FunctionInfo>,
    current_mod: Vec<String>,
    current_impl: Option<String>,
}

impl FunctionExtractor {
    fn new(file_path: String) -> Self {
        Self {
            file_path,
            results: Vec::with_capacity(32), // Pre-allocate for typical file
            current_mod: Vec::new(),
            current_impl: None,
        }
    }

    fn visibility_str(v: &Visibility) -> &'static str {
        match v {
            Visibility::Public(_) => "pub",
            Visibility::Restricted(r) => {
                // Check for pub(crate), pub(super), etc.
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
        let mut parts = self.current_mod.clone();
        if let Some(ref impl_type) = self.current_impl {
            parts.push(impl_type.clone());
        }
        parts.push(name.to_string());
        parts.join("::")
    }

    /// Check if attributes contain a specific attribute name.
    fn has_attribute(attrs: &[Attribute], name: &str) -> bool {
        attrs.iter().any(|attr| {
            attr.path().is_ident(name)
        })
    }

    fn record_function(&mut self, name: &str, vis: &Visibility, is_method: bool, attrs: &[Attribute]) {
        let is_test = Self::has_attribute(attrs, "test");
        let is_no_mangle = Self::has_attribute(attrs, "no_mangle");

        self.results.push(FunctionInfo {
            name: name.to_string(),
            full_path: self.build_full_path(name),
            visibility: Self::visibility_str(vis).to_string(),
            file: self.file_path.clone(),
            is_method,
            impl_type: self.current_impl.clone(),
            is_test,
            is_no_mangle,
        });
    }
}

impl<'ast> Visit<'ast> for FunctionExtractor {
    fn visit_item(&mut self, item: &'ast Item) {
        match item {
            // Handle inline modules: mod foo { ... }
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

            // Free functions: fn foo() { ... }
            Item::Fn(ItemFn { sig, vis, attrs, .. }) => {
                self.record_function(&sig.ident.to_string(), vis, false, attrs);
            }

            // Impl blocks: impl Foo { ... } or impl Trait for Foo { ... }
            Item::Impl(ItemImpl {
                self_ty, items, ..
            }) => {
                // Extract type name for the impl block
                let type_name = extract_type_name(self_ty);
                self.current_impl = Some(type_name);

                for impl_item in items {
                    if let ImplItem::Fn(ImplItemFn { sig, vis, attrs, .. }) = impl_item {
                        self.record_function(&sig.ident.to_string(), vis, true, attrs);
                    }
                }

                self.current_impl = None;
            }

            _ => {
                // Continue visiting other items
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
        _ => "<unknown>".to_string(),
    }
}

/// Extract all functions from a file's content.
///
/// Returns a list of FunctionInfo for each function found.
/// On parse error, returns an empty list (resilient behavior).
pub fn extract_functions(path: &Path, content: &str) -> Vec<FunctionInfo> {
    let ast: File = match syn::parse_file(content) {
        Ok(ast) => ast,
        Err(e) => {
            eprintln!(
                "[WARN] AST parse failed for {}: {}",
                path.display(),
                e
            );
            return Vec::new();
        }
    };

    let mut extractor = FunctionExtractor::new(path.display().to_string());
    extractor.visit_file(&ast);
    extractor.results
}

/// Extract functions from file content with error reporting.
pub fn extract_functions_strict(path: &Path, content: &str) -> Result<Vec<FunctionInfo>> {
    let ast: File = syn::parse_file(content)
        .map_err(|e| anyhow::anyhow!("Parse error in {}: {}", path.display(), e))?;

    let mut extractor = FunctionExtractor::new(path.display().to_string());
    extractor.visit_file(&ast);
    Ok(extractor.results)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn test_extract_free_function() {
        let content = r#"
fn private_func() {}
pub fn public_func() {}
"#;
        let funcs = extract_functions(&PathBuf::from("test.rs"), content);
        assert_eq!(funcs.len(), 2);

        assert_eq!(funcs[0].name, "private_func");
        assert_eq!(funcs[0].visibility, "private");
        assert!(!funcs[0].is_method);

        assert_eq!(funcs[1].name, "public_func");
        assert_eq!(funcs[1].visibility, "pub");
    }

    #[test]
    fn test_extract_impl_methods() {
        let content = r#"
struct Foo;

impl Foo {
    fn private_method(&self) {}
    pub fn public_method(&self) {}
}
"#;
        let funcs = extract_functions(&PathBuf::from("test.rs"), content);
        assert_eq!(funcs.len(), 2);

        assert_eq!(funcs[0].name, "private_method");
        assert!(funcs[0].is_method);
        assert_eq!(funcs[0].impl_type, Some("Foo".to_string()));
        assert_eq!(funcs[0].full_path, "Foo::private_method");

        assert_eq!(funcs[1].name, "public_method");
        assert_eq!(funcs[1].full_path, "Foo::public_method");
    }

    #[test]
    fn test_extract_nested_mod() {
        let content = r#"
mod inner {
    fn nested_func() {}

    mod deep {
        fn deeply_nested() {}
    }
}
"#;
        let funcs = extract_functions(&PathBuf::from("test.rs"), content);
        assert_eq!(funcs.len(), 2);

        assert_eq!(funcs[0].full_path, "inner::nested_func");
        assert_eq!(funcs[1].full_path, "inner::deep::deeply_nested");
    }

    #[test]
    fn test_extract_pub_crate() {
        let content = r#"
pub(crate) fn crate_visible() {}
pub(super) fn super_visible() {}
"#;
        let funcs = extract_functions(&PathBuf::from("test.rs"), content);
        assert_eq!(funcs.len(), 2);

        assert_eq!(funcs[0].visibility, "pub(crate)");
        assert_eq!(funcs[1].visibility, "pub(super)");
    }

    #[test]
    fn test_malformed_file_resilient() {
        let content = "fn broken( { }"; // Invalid syntax
        let funcs = extract_functions(&PathBuf::from("broken.rs"), content);
        assert!(funcs.is_empty()); // Should not panic
    }

    #[test]
    fn test_extract_test_attribute() {
        let content = r#"
#[test]
fn test_something() {}

fn regular_fn() {}
"#;
        let funcs = extract_functions(&PathBuf::from("test.rs"), content);
        assert_eq!(funcs.len(), 2);

        let test_fn = funcs.iter().find(|f| f.name == "test_something").unwrap();
        assert!(test_fn.is_test);
        assert!(!test_fn.is_no_mangle);

        let regular = funcs.iter().find(|f| f.name == "regular_fn").unwrap();
        assert!(!regular.is_test);
    }

    #[test]
    fn test_extract_no_mangle_attribute() {
        let content = r#"
#[no_mangle]
pub extern "C" fn ffi_function() {}

fn regular_fn() {}
"#;
        let funcs = extract_functions(&PathBuf::from("test.rs"), content);
        assert_eq!(funcs.len(), 2);

        let ffi_fn = funcs.iter().find(|f| f.name == "ffi_function").unwrap();
        assert!(ffi_fn.is_no_mangle);
        assert!(!ffi_fn.is_test);

        let regular = funcs.iter().find(|f| f.name == "regular_fn").unwrap();
        assert!(!regular.is_no_mangle);
    }

    #[test]
    fn test_multiple_attributes() {
        let content = r#"
#[inline]
#[test]
fn inlined_test() {}

#[cfg(test)]
mod tests {
    #[test]
    fn nested_test() {}
}
"#;
        let funcs = extract_functions(&PathBuf::from("test.rs"), content);

        let inlined_test = funcs.iter().find(|f| f.name == "inlined_test").unwrap();
        assert!(inlined_test.is_test);

        let nested_test = funcs.iter().find(|f| f.name == "nested_test").unwrap();
        assert!(nested_test.is_test);
    }
}
