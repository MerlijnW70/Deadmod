//! Generic and lifetime usage detection from Rust AST.
//!
//! Detects all usages of generic type parameters and lifetimes:
//! - Type paths: `x: T`, `Vec<T>`
//! - References: `&'a str`
//! - Generic arguments: `HashMap<K, V>`
//! - Function return types
//! - Field types in structs/enums
//!
//! Tracks usages per parent item for accurate dead generic detection.
//!
//! NASA-grade resilience: handles malformed AST gracefully.

use std::collections::{HashMap, HashSet};
use std::path::Path;

use syn::{
    visit::Visit, AngleBracketedGenericArguments, Expr, Field, File, FnArg, GenericArgument, Item,
    ItemEnum, ItemFn, ItemImpl, ItemStruct, ItemTrait, Pat, PathArguments, ReturnType, Signature,
    Type,
};

/// Information about generic/lifetime usages within a parent item.
#[derive(Debug, Clone, Default)]
pub struct ParentUsages {
    /// Type parameters used (e.g., "T", "U")
    pub used_types: HashSet<String>,
    /// Lifetimes used (e.g., "'a", "'b")
    pub used_lifetimes: HashSet<String>,
}

/// Result of generic usage extraction from a file.
#[derive(Debug, Clone, Default)]
pub struct GenericUsageResult {
    /// Map from parent name to its usages
    pub usages_by_parent: HashMap<String, ParentUsages>,
    /// Global usages across the file (for cross-item analysis)
    pub global_types: HashSet<String>,
    /// Global lifetime usages
    pub global_lifetimes: HashSet<String>,
}

/// AST visitor that extracts generic usages.
struct GenericUsageExtractor {
    current_parent: Option<String>,
    result: GenericUsageResult,
}

impl GenericUsageExtractor {
    fn new() -> Self {
        Self {
            current_parent: None,
            result: GenericUsageResult::default(),
        }
    }

    fn record_type(&mut self, name: &str) {
        self.result.global_types.insert(name.to_string());

        if let Some(ref parent) = self.current_parent {
            self.result
                .usages_by_parent
                .entry(parent.clone())
                .or_default()
                .used_types
                .insert(name.to_string());
        }
    }

    fn record_lifetime(&mut self, name: &str) {
        self.result.global_lifetimes.insert(name.to_string());

        if let Some(ref parent) = self.current_parent {
            self.result
                .usages_by_parent
                .entry(parent.clone())
                .or_default()
                .used_lifetimes
                .insert(name.to_string());
        }
    }

    fn collect_type(&mut self, ty: &Type) {
        match ty {
            Type::Path(tp) => {
                // Single identifier like T, U, V
                if tp.qself.is_none() && tp.path.segments.len() == 1 {
                    let ident = &tp.path.segments[0].ident;
                    let name = ident.to_string();
                    // Heuristic: single uppercase letter or short names are likely generics
                    if name.chars().next().map(|c| c.is_uppercase()).unwrap_or(false) {
                        self.record_type(&name);
                    }
                }

                // Process generic arguments in paths like Vec<T>, HashMap<K, V>
                for seg in &tp.path.segments {
                    if let PathArguments::AngleBracketed(AngleBracketedGenericArguments {
                        args,
                        ..
                    }) = &seg.arguments
                    {
                        for arg in args {
                            match arg {
                                GenericArgument::Type(t) => self.collect_type(t),
                                GenericArgument::Lifetime(lt) => {
                                    self.record_lifetime(&lt.to_string());
                                }
                                _ => {}
                            }
                        }
                    }
                }
            }

            Type::Reference(r) => {
                if let Some(lt) = &r.lifetime {
                    self.record_lifetime(&lt.to_string());
                }
                self.collect_type(&r.elem);
            }

            Type::Slice(s) => {
                self.collect_type(&s.elem);
            }

            Type::Array(a) => {
                self.collect_type(&a.elem);
            }

            Type::Tuple(t) => {
                for elem in &t.elems {
                    self.collect_type(elem);
                }
            }

            Type::Ptr(p) => {
                self.collect_type(&p.elem);
            }

            Type::BareFn(f) => {
                for input in &f.inputs {
                    self.collect_type(&input.ty);
                }
                if let ReturnType::Type(_, ty) = &f.output {
                    self.collect_type(ty);
                }
            }

            Type::ImplTrait(it) => {
                for bound in &it.bounds {
                    if let syn::TypeParamBound::Trait(tb) = bound {
                        for seg in &tb.path.segments {
                            if let PathArguments::AngleBracketed(args) = &seg.arguments {
                                for arg in &args.args {
                                    if let GenericArgument::Type(t) = arg {
                                        self.collect_type(t);
                                    }
                                }
                            }
                        }
                    }
                }
            }

            Type::TraitObject(to) => {
                for bound in &to.bounds {
                    if let syn::TypeParamBound::Trait(tb) = bound {
                        for seg in &tb.path.segments {
                            if let PathArguments::AngleBracketed(args) = &seg.arguments {
                                for arg in &args.args {
                                    if let GenericArgument::Type(t) = arg {
                                        self.collect_type(t);
                                    }
                                }
                            }
                        }
                    }
                }
            }

            Type::Paren(p) => {
                self.collect_type(&p.elem);
            }

            Type::Group(g) => {
                self.collect_type(&g.elem);
            }

            _ => {}
        }
    }

    fn collect_signature(&mut self, sig: &Signature) {
        // Process function arguments
        for arg in &sig.inputs {
            match arg {
                FnArg::Typed(pat_type) => {
                    self.collect_type(&pat_type.ty);
                }
                FnArg::Receiver(recv) => {
                    if let Some((_, Some(lt))) = &recv.reference {
                        self.record_lifetime(&lt.to_string());
                    }
                }
            }
        }

        // Process return type
        if let ReturnType::Type(_, ty) = &sig.output {
            self.collect_type(ty);
        }
    }

    fn collect_fields<'a>(&mut self, fields: impl Iterator<Item = &'a Field>) {
        for field in fields {
            self.collect_type(&field.ty);
        }
    }
}

impl<'ast> Visit<'ast> for GenericUsageExtractor {
    fn visit_item(&mut self, item: &'ast Item) {
        match item {
            Item::Fn(ItemFn { sig, block, .. }) => {
                self.current_parent = Some(sig.ident.to_string());
                self.collect_signature(sig);
                // Visit the function body for type usages
                syn::visit::visit_block(self, block);
                self.current_parent = None;
            }

            Item::Struct(ItemStruct { ident, fields, .. }) => {
                self.current_parent = Some(ident.to_string());
                match fields {
                    syn::Fields::Named(named) => {
                        self.collect_fields(named.named.iter());
                    }
                    syn::Fields::Unnamed(unnamed) => {
                        self.collect_fields(unnamed.unnamed.iter());
                    }
                    syn::Fields::Unit => {}
                }
                self.current_parent = None;
            }

            Item::Enum(ItemEnum { ident, variants, .. }) => {
                self.current_parent = Some(ident.to_string());
                for variant in variants {
                    match &variant.fields {
                        syn::Fields::Named(named) => {
                            self.collect_fields(named.named.iter());
                        }
                        syn::Fields::Unnamed(unnamed) => {
                            self.collect_fields(unnamed.unnamed.iter());
                        }
                        syn::Fields::Unit => {}
                    }
                }
                self.current_parent = None;
            }

            Item::Trait(ItemTrait { ident, items, .. }) => {
                self.current_parent = Some(ident.to_string());
                for item in items {
                    if let syn::TraitItem::Fn(method) = item {
                        self.collect_signature(&method.sig);
                    }
                }
                self.current_parent = None;
            }

            Item::Impl(ItemImpl {
                self_ty,
                trait_,
                items,
                ..
            }) => {
                let parent = if let Some((_, path, _)) = trait_ {
                    path.segments
                        .last()
                        .map(|s| s.ident.to_string())
                        .unwrap_or_else(|| "<unknown>".to_string())
                } else {
                    extract_type_name(self_ty)
                };

                self.current_parent = Some(parent);

                // Collect usages in self_ty
                self.collect_type(self_ty);

                // Collect usages in methods
                for item in items {
                    if let syn::ImplItem::Fn(method) = item {
                        self.collect_signature(&method.sig);
                        syn::visit::visit_block(self, &method.block);
                    }
                }
                self.current_parent = None;
            }

            _ => {
                syn::visit::visit_item(self, item);
            }
        }
    }

    fn visit_type(&mut self, ty: &'ast Type) {
        self.collect_type(ty);
        syn::visit::visit_type(self, ty);
    }

    fn visit_expr(&mut self, expr: &'ast Expr) {
        // Look for type annotations in expressions
        if let Expr::Cast(cast) = expr {
            self.collect_type(&cast.ty);
        }
        syn::visit::visit_expr(self, expr);
    }

    fn visit_local(&mut self, local: &'ast syn::Local) {
        // Check for type annotations in let bindings
        if let Pat::Type(pat_type) = &local.pat {
            self.collect_type(&pat_type.ty);
        }
        syn::visit::visit_local(self, local);
    }
}

/// Extract a readable type name from a syn::Type.
fn extract_type_name(ty: &Type) -> String {
    match ty {
        Type::Path(p) => p
            .path
            .segments
            .last()
            .map(|s| s.ident.to_string())
            .unwrap_or_else(|| "<unknown>".to_string()),
        _ => "<unknown>".to_string(),
    }
}

/// Extract all generic and lifetime usages from file content.
///
/// Returns usage information organized by parent item.
/// On parse error, returns empty result (resilient behavior).
pub fn extract_generic_usages(path: &Path, content: &str) -> GenericUsageResult {
    let ast: File = match syn::parse_file(content) {
        Ok(ast) => ast,
        Err(e) => {
            eprintln!("[WARN] AST parse failed for {}: {}", path.display(), e);
            return GenericUsageResult::default();
        }
    };

    let mut extractor = GenericUsageExtractor::new();
    extractor.visit_file(&ast);
    extractor.result
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn test_extract_fn_type_usage() {
        let content = r#"
fn foo<T>(x: T) -> T {
    x
}
"#;
        let result = extract_generic_usages(&PathBuf::from("test.rs"), content);

        let foo_usages = result.usages_by_parent.get("foo").unwrap();
        assert!(foo_usages.used_types.contains("T"));
    }

    #[test]
    fn test_extract_fn_lifetime_usage() {
        let content = r#"
fn bar<'a>(x: &'a str) -> &'a str {
    x
}
"#;
        let result = extract_generic_usages(&PathBuf::from("test.rs"), content);

        let bar_usages = result.usages_by_parent.get("bar").unwrap();
        assert!(bar_usages.used_lifetimes.contains("'a"));
    }

    #[test]
    fn test_extract_struct_field_usage() {
        let content = r#"
struct Container<T> {
    data: T,
    more: Vec<T>,
}
"#;
        let result = extract_generic_usages(&PathBuf::from("test.rs"), content);

        let container_usages = result.usages_by_parent.get("Container").unwrap();
        assert!(container_usages.used_types.contains("T"));
    }

    #[test]
    fn test_extract_enum_variant_usage() {
        let content = r#"
enum Option<T> {
    Some(T),
    None,
}
"#;
        let result = extract_generic_usages(&PathBuf::from("test.rs"), content);

        let option_usages = result.usages_by_parent.get("Option").unwrap();
        assert!(option_usages.used_types.contains("T"));
    }

    #[test]
    fn test_extract_nested_generic_usage() {
        let content = r#"
fn process<K, V>(map: HashMap<K, V>) {}
"#;
        let result = extract_generic_usages(&PathBuf::from("test.rs"), content);

        let usages = result.usages_by_parent.get("process").unwrap();
        assert!(usages.used_types.contains("K"));
        assert!(usages.used_types.contains("V"));
    }

    #[test]
    fn test_unused_generic_detection() {
        let content = r#"
fn foo<T, U>(x: T) -> T {
    x
}
"#;
        let result = extract_generic_usages(&PathBuf::from("test.rs"), content);

        let usages = result.usages_by_parent.get("foo").unwrap();
        assert!(usages.used_types.contains("T"));
        // U is not used
        assert!(!usages.used_types.contains("U"));
    }

    #[test]
    fn test_unused_lifetime_detection() {
        let content = r#"
fn bar<'a, 'b>(x: &'a str) -> &'a str {
    x
}
"#;
        let result = extract_generic_usages(&PathBuf::from("test.rs"), content);

        let usages = result.usages_by_parent.get("bar").unwrap();
        assert!(usages.used_lifetimes.contains("'a"));
        // 'b is not used
        assert!(!usages.used_lifetimes.contains("'b"));
    }

    #[test]
    fn test_impl_block_usage() {
        let content = r#"
struct Foo<T>(T);

impl<T> Foo<T> {
    fn get(&self) -> &T {
        &self.0
    }
}
"#;
        let result = extract_generic_usages(&PathBuf::from("test.rs"), content);

        let impl_usages = result.usages_by_parent.get("Foo").unwrap();
        assert!(impl_usages.used_types.contains("T"));
    }

    #[test]
    fn test_reference_lifetime_usage() {
        let content = r#"
struct Ref<'a> {
    data: &'a str,
}
"#;
        let result = extract_generic_usages(&PathBuf::from("test.rs"), content);

        let usages = result.usages_by_parent.get("Ref").unwrap();
        assert!(usages.used_lifetimes.contains("'a"));
    }

    #[test]
    fn test_malformed_resilient() {
        let content = "fn foo<T { broken }";
        let result = extract_generic_usages(&PathBuf::from("broken.rs"), content);
        assert!(result.usages_by_parent.is_empty());
    }
}
