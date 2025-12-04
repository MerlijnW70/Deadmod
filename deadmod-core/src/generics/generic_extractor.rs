//! Generic and lifetime parameter extraction from Rust AST.
//!
//! Extracts all declared generic type parameters and lifetimes from:
//! - Functions: `fn foo<T, 'a>()`
//! - Structs: `struct Foo<T, 'a>`
//! - Enums: `enum Bar<T>`
//! - Traits: `trait Baz<T>`
//! - Impl blocks: `impl<T> Foo<T>`
//!
//! Also extracts trait bounds for more precise analysis.
//!
//! NASA-grade resilience: handles malformed AST gracefully.

use serde::{Deserialize, Serialize};
use std::path::Path;
use syn::{
    visit::Visit, File, GenericParam, Item, ItemEnum, ItemFn, ItemImpl, ItemStruct, ItemTrait,
    LifetimeParam, TypeParam, WhereClause, WherePredicate,
};

/// Information about a declared generic parameter.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeclaredGeneric {
    /// The name of the generic (e.g., "T", "'a")
    pub name: String,
    /// Kind: "type", "lifetime", or "const"
    pub kind: GenericKind,
    /// The parent item (e.g., "Foo" for `struct Foo<T>`)
    pub parent: String,
    /// Parent kind for context
    pub parent_kind: ParentKind,
    /// Source file path
    pub file: String,
    /// Trait bounds on this generic (e.g., ["Debug", "Clone"])
    pub bounds: Vec<String>,
}

/// The kind of generic parameter.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum GenericKind {
    Type,
    Lifetime,
    Const,
}

/// The kind of parent item containing the generic.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ParentKind {
    Function,
    Struct,
    Enum,
    Trait,
    Impl,
}

/// Result of generic extraction from a file.
#[derive(Debug, Clone, Default)]
pub struct GenericExtractionResult {
    /// All declared generic parameters found
    pub declared: Vec<DeclaredGeneric>,
}

/// AST visitor that extracts generic parameter declarations.
struct GenericExtractor {
    file_path: String,
    result: GenericExtractionResult,
}

impl GenericExtractor {
    fn new(file_path: String) -> Self {
        Self {
            file_path,
            result: GenericExtractionResult::default(),
        }
    }

    fn record(&mut self, parent: &str, parent_kind: ParentKind, gp: &GenericParam) {
        match gp {
            GenericParam::Type(TypeParam { ident, bounds, .. }) => {
                let bound_names: Vec<String> = bounds
                    .iter()
                    .filter_map(|b| {
                        if let syn::TypeParamBound::Trait(tb) = b {
                            tb.path.segments.last().map(|s| s.ident.to_string())
                        } else {
                            None
                        }
                    })
                    .collect();

                self.result.declared.push(DeclaredGeneric {
                    name: ident.to_string(),
                    kind: GenericKind::Type,
                    parent: parent.to_string(),
                    parent_kind,
                    file: self.file_path.clone(),
                    bounds: bound_names,
                });
            }

            GenericParam::Lifetime(LifetimeParam { lifetime, .. }) => {
                self.result.declared.push(DeclaredGeneric {
                    name: lifetime.to_string(),
                    kind: GenericKind::Lifetime,
                    parent: parent.to_string(),
                    parent_kind,
                    file: self.file_path.clone(),
                    bounds: Vec::new(),
                });
            }

            GenericParam::Const(cp) => {
                self.result.declared.push(DeclaredGeneric {
                    name: cp.ident.to_string(),
                    kind: GenericKind::Const,
                    parent: parent.to_string(),
                    parent_kind,
                    file: self.file_path.clone(),
                    bounds: Vec::new(),
                });
            }
        }
    }

    /// Extract type name from syn::Type for impl blocks.
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

    /// Process where clause to add additional bounds to declared generics.
    ///
    /// Where clauses like `where T: Debug + Clone` add bounds to generics
    /// that may not be declared inline with the generic parameter.
    fn process_where_clause(&mut self, where_clause: Option<&WhereClause>, parent: &str) {
        let Some(where_clause) = where_clause else {
            return;
        };

        for predicate in &where_clause.predicates {
            match predicate {
                WherePredicate::Type(pred_type) => {
                    // Extract the generic name being bounded (e.g., "T" from "T: Debug")
                    let bounded_name = Self::extract_type_name(&syn::Type::Path(syn::TypePath {
                        qself: None,
                        path: match &pred_type.bounded_ty {
                            syn::Type::Path(p) => p.path.clone(),
                            _ => continue,
                        },
                    }));

                    // Extract the bounds being applied
                    let bounds: Vec<String> = pred_type
                        .bounds
                        .iter()
                        .filter_map(|b| {
                            if let syn::TypeParamBound::Trait(tb) = b {
                                tb.path.segments.last().map(|s| s.ident.to_string())
                            } else {
                                None
                            }
                        })
                        .collect();

                    // Add bounds to existing declared generic with this name and parent
                    for decl in &mut self.result.declared {
                        if decl.name == bounded_name && decl.parent == parent {
                            for bound in &bounds {
                                if !decl.bounds.contains(bound) {
                                    decl.bounds.push(bound.clone());
                                }
                            }
                        }
                    }
                }
                WherePredicate::Lifetime(pred_lifetime) => {
                    // Handle lifetime bounds like 'a: 'b
                    let lifetime_name = pred_lifetime.lifetime.to_string();
                    let bounds: Vec<String> = pred_lifetime
                        .bounds
                        .iter()
                        .map(|lt| lt.to_string())
                        .collect();

                    for decl in &mut self.result.declared {
                        if decl.name == lifetime_name && decl.parent == parent {
                            for bound in &bounds {
                                if !decl.bounds.contains(bound) {
                                    decl.bounds.push(bound.clone());
                                }
                            }
                        }
                    }
                }
                _ => {}
            }
        }
    }
}

impl<'ast> Visit<'ast> for GenericExtractor {
    fn visit_item(&mut self, item: &'ast Item) {
        match item {
            Item::Fn(ItemFn { sig, .. }) => {
                let parent = sig.ident.to_string();
                for gp in &sig.generics.params {
                    self.record(&parent, ParentKind::Function, gp);
                }
                // Process where clause bounds
                self.process_where_clause(sig.generics.where_clause.as_ref(), &parent);
            }

            Item::Struct(ItemStruct { ident, generics, .. }) => {
                let parent = ident.to_string();
                for gp in &generics.params {
                    self.record(&parent, ParentKind::Struct, gp);
                }
                // Process where clause bounds
                self.process_where_clause(generics.where_clause.as_ref(), &parent);
            }

            Item::Enum(ItemEnum { ident, generics, .. }) => {
                let parent = ident.to_string();
                for gp in &generics.params {
                    self.record(&parent, ParentKind::Enum, gp);
                }
                // Process where clause bounds
                self.process_where_clause(generics.where_clause.as_ref(), &parent);
            }

            Item::Trait(ItemTrait { ident, generics, .. }) => {
                let parent = ident.to_string();
                for gp in &generics.params {
                    self.record(&parent, ParentKind::Trait, gp);
                }
                // Process where clause bounds
                self.process_where_clause(generics.where_clause.as_ref(), &parent);
            }

            Item::Impl(ItemImpl {
                generics,
                self_ty,
                trait_,
                ..
            }) => {
                // Use trait name if it's a trait impl, otherwise use the type name
                let parent = if let Some((_, path, _)) = trait_ {
                    path.segments
                        .last()
                        .map(|s| s.ident.to_string())
                        .unwrap_or_else(|| "<unknown>".to_string())
                } else {
                    Self::extract_type_name(self_ty)
                };

                for gp in &generics.params {
                    self.record(&parent, ParentKind::Impl, gp);
                }
                // Process where clause bounds
                self.process_where_clause(generics.where_clause.as_ref(), &parent);
            }

            _ => {}
        }

        // Continue visiting nested items
        syn::visit::visit_item(self, item);
    }
}

/// Extract all declared generic parameters from file content.
///
/// Returns extraction result with all generics and lifetimes found.
/// On parse error, returns empty result (resilient behavior).
pub fn extract_declared_generics(path: &Path, content: &str) -> GenericExtractionResult {
    let ast: File = match syn::parse_file(content) {
        Ok(ast) => ast,
        Err(e) => {
            eprintln!("[WARN] AST parse failed for {}: {}", path.display(), e);
            return GenericExtractionResult::default();
        }
    };

    let mut extractor = GenericExtractor::new(path.display().to_string());
    extractor.visit_file(&ast);
    extractor.result
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn test_extract_fn_generics() {
        let content = r#"
fn foo<T, U>(x: T, y: U) {}
"#;
        let result = extract_declared_generics(&PathBuf::from("test.rs"), content);
        assert_eq!(result.declared.len(), 2);
        assert_eq!(result.declared[0].name, "T");
        assert_eq!(result.declared[0].kind, GenericKind::Type);
        assert_eq!(result.declared[0].parent, "foo");
        assert_eq!(result.declared[1].name, "U");
    }

    #[test]
    fn test_extract_fn_lifetimes() {
        let content = r#"
fn bar<'a, 'b>(x: &'a str, y: &'b str) {}
"#;
        let result = extract_declared_generics(&PathBuf::from("test.rs"), content);
        assert_eq!(result.declared.len(), 2);
        assert_eq!(result.declared[0].name, "'a");
        assert_eq!(result.declared[0].kind, GenericKind::Lifetime);
        assert_eq!(result.declared[1].name, "'b");
    }

    #[test]
    fn test_extract_struct_generics() {
        let content = r#"
struct Foo<T, 'a> {
    data: T,
    reference: &'a str,
}
"#;
        let result = extract_declared_generics(&PathBuf::from("test.rs"), content);
        assert_eq!(result.declared.len(), 2);

        let type_param = result.declared.iter().find(|d| d.name == "T").unwrap();
        assert_eq!(type_param.kind, GenericKind::Type);
        assert_eq!(type_param.parent, "Foo");

        let lifetime_param = result.declared.iter().find(|d| d.name == "'a").unwrap();
        assert_eq!(lifetime_param.kind, GenericKind::Lifetime);
    }

    #[test]
    fn test_extract_enum_generics() {
        let content = r#"
enum Result<T, E> {
    Ok(T),
    Err(E),
}
"#;
        let result = extract_declared_generics(&PathBuf::from("test.rs"), content);
        assert_eq!(result.declared.len(), 2);
        assert!(result.declared.iter().any(|d| d.name == "T"));
        assert!(result.declared.iter().any(|d| d.name == "E"));
    }

    #[test]
    fn test_extract_trait_generics() {
        let content = r#"
trait MyTrait<T> {
    fn process(&self, item: T);
}
"#;
        let result = extract_declared_generics(&PathBuf::from("test.rs"), content);
        assert_eq!(result.declared.len(), 1);
        assert_eq!(result.declared[0].name, "T");
        assert_eq!(result.declared[0].parent, "MyTrait");
        assert!(matches!(result.declared[0].parent_kind, ParentKind::Trait));
    }

    #[test]
    fn test_extract_impl_generics() {
        let content = r#"
struct Container<T>(T);

impl<T> Container<T> {
    fn new(value: T) -> Self {
        Container(value)
    }
}
"#;
        let result = extract_declared_generics(&PathBuf::from("test.rs"), content);
        // One from struct, one from impl
        assert_eq!(result.declared.len(), 2);

        let impl_generic = result
            .declared
            .iter()
            .find(|d| matches!(d.parent_kind, ParentKind::Impl))
            .unwrap();
        assert_eq!(impl_generic.name, "T");
    }

    #[test]
    fn test_extract_bounds() {
        let content = r#"
fn foo<T: Debug + Clone, U: Display>(x: T, y: U) {}
"#;
        let result = extract_declared_generics(&PathBuf::from("test.rs"), content);
        assert_eq!(result.declared.len(), 2);

        let t_param = result.declared.iter().find(|d| d.name == "T").unwrap();
        assert!(t_param.bounds.contains(&"Debug".to_string()));
        assert!(t_param.bounds.contains(&"Clone".to_string()));

        let u_param = result.declared.iter().find(|d| d.name == "U").unwrap();
        assert!(u_param.bounds.contains(&"Display".to_string()));
    }

    #[test]
    fn test_extract_const_generic() {
        let content = r#"
struct Array<T, const N: usize> {
    data: [T; N],
}
"#;
        let result = extract_declared_generics(&PathBuf::from("test.rs"), content);
        assert_eq!(result.declared.len(), 2);

        let const_param = result.declared.iter().find(|d| d.name == "N").unwrap();
        assert_eq!(const_param.kind, GenericKind::Const);
    }

    #[test]
    fn test_malformed_resilient() {
        let content = "fn foo<T { broken }";
        let result = extract_declared_generics(&PathBuf::from("broken.rs"), content);
        assert!(result.declared.is_empty());
    }

    #[test]
    fn test_where_clause_bounds() {
        let content = r#"
fn foo<T, U>(x: T, y: U) where T: Debug + Clone, U: Display {}
"#;
        let result = extract_declared_generics(&PathBuf::from("test.rs"), content);
        assert_eq!(result.declared.len(), 2);

        let t_param = result.declared.iter().find(|d| d.name == "T").unwrap();
        assert!(t_param.bounds.contains(&"Debug".to_string()));
        assert!(t_param.bounds.contains(&"Clone".to_string()));

        let u_param = result.declared.iter().find(|d| d.name == "U").unwrap();
        assert!(u_param.bounds.contains(&"Display".to_string()));
    }

    #[test]
    fn test_where_clause_on_struct() {
        let content = r#"
struct Container<T> where T: Clone + Send {
    data: T,
}
"#;
        let result = extract_declared_generics(&PathBuf::from("test.rs"), content);
        assert_eq!(result.declared.len(), 1);

        let t_param = &result.declared[0];
        assert_eq!(t_param.name, "T");
        assert!(t_param.bounds.contains(&"Clone".to_string()));
        assert!(t_param.bounds.contains(&"Send".to_string()));
    }

    #[test]
    fn test_combined_inline_and_where_bounds() {
        let content = r#"
fn process<T: Default>(x: T) where T: Clone + Send {}
"#;
        let result = extract_declared_generics(&PathBuf::from("test.rs"), content);
        assert_eq!(result.declared.len(), 1);

        let t_param = &result.declared[0];
        // Should have both inline (Default) and where clause bounds (Clone, Send)
        assert!(t_param.bounds.contains(&"Default".to_string()));
        assert!(t_param.bounds.contains(&"Clone".to_string()));
        assert!(t_param.bounds.contains(&"Send".to_string()));
    }
}
