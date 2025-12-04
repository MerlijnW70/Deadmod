//! Trait method usage detection from Rust AST.
//!
//! Detects calls to trait methods via:
//! - `obj.method()` - method calls on trait objects
//! - `Type::method()` - associated function calls
//! - `<Type as Trait>::method()` - qualified path calls
//! - `<Trait>::method()` - direct trait method calls
//!
//! NASA-grade resilience: handles malformed AST gracefully.

use std::collections::HashSet;
use std::path::Path;

use syn::{visit::Visit, Expr, ExprMethodCall, ExprPath, File, QSelf};

/// Information about a trait method usage site.
#[derive(Debug, Clone, Hash, Eq, PartialEq)]
pub struct TraitMethodUsage {
    /// The method name being called
    pub method_name: String,
    /// The trait name if determinable (from qualified paths)
    pub trait_name: Option<String>,
    /// The type name if determinable
    pub type_name: Option<String>,
    /// Kind of usage
    pub usage_kind: UsageKind,
}

/// The kind of trait method usage.
#[derive(Debug, Clone, Hash, Eq, PartialEq)]
pub enum UsageKind {
    /// obj.method() - regular method call
    MethodCall,
    /// Type::method() - associated function call
    AssociatedCall,
    /// <Type as Trait>::method() - fully qualified call
    QualifiedCall,
}

/// AST visitor that extracts trait method usages.
struct TraitUsageExtractor {
    usages: HashSet<TraitMethodUsage>,
}

impl TraitUsageExtractor {
    fn new() -> Self {
        Self {
            usages: HashSet::with_capacity(64),
        }
    }
}

impl<'ast> Visit<'ast> for TraitUsageExtractor {
    fn visit_expr(&mut self, node: &'ast Expr) {
        match node {
            // Method calls: obj.method()
            Expr::MethodCall(ExprMethodCall { method, .. }) => {
                self.usages.insert(TraitMethodUsage {
                    method_name: method.to_string(),
                    trait_name: None,
                    type_name: None,
                    usage_kind: UsageKind::MethodCall,
                });
            }

            // Path-based calls: Type::method() or <Type as Trait>::method()
            Expr::Call(call) => {
                if let Expr::Path(ExprPath { qself, path, .. }) = &*call.func {
                    // Check for qualified self: <Type as Trait>::method()
                    if let Some(QSelf { ty, .. }) = qself {
                        let type_name = extract_type_str(ty);

                        // Extract trait from path if present
                        let segments: Vec<_> = path
                            .segments
                            .iter()
                            .map(|s| s.ident.to_string())
                            .collect();

                        if let Some(method) = segments.last() {
                            let trait_name = if segments.len() > 1 {
                                Some(segments[..segments.len() - 1].join("::"))
                            } else {
                                None
                            };

                            self.usages.insert(TraitMethodUsage {
                                method_name: method.clone(),
                                trait_name,
                                type_name: Some(type_name),
                                usage_kind: UsageKind::QualifiedCall,
                            });
                        }
                    } else {
                        // Regular path call: Type::method() or module::Type::method()
                        let segments: Vec<_> = path
                            .segments
                            .iter()
                            .map(|s| s.ident.to_string())
                            .collect();

                        if segments.len() >= 2 {
                            if let Some(method) = segments.last() {
                                // The part before the method could be Type or Trait
                                let prefix = segments[..segments.len() - 1].join("::");

                                self.usages.insert(TraitMethodUsage {
                                    method_name: method.clone(),
                                    trait_name: None, // Can't determine without type info
                                    type_name: Some(prefix),
                                    usage_kind: UsageKind::AssociatedCall,
                                });
                            }
                        }
                    }
                }
            }

            _ => {}
        }

        // Continue visiting nested expressions
        syn::visit::visit_expr(self, node);
    }
}

/// Extract a string representation from a syn::Type.
fn extract_type_str(ty: &syn::Type) -> String {
    match ty {
        syn::Type::Path(type_path) => type_path
            .path
            .segments
            .iter()
            .map(|s| s.ident.to_string())
            .collect::<Vec<_>>()
            .join("::"),
        syn::Type::Reference(r) => {
            let inner = extract_type_str(&r.elem);
            if r.mutability.is_some() {
                format!("&mut {}", inner)
            } else {
                format!("&{}", inner)
            }
        }
        _ => "<unknown>".to_string(),
    }
}

/// Extract all trait method usages from file content.
///
/// Returns a set of unique usages found in the file.
/// On parse error, returns an empty set (resilient behavior).
pub fn extract_trait_usages(path: &Path, content: &str) -> HashSet<TraitMethodUsage> {
    let ast: File = match syn::parse_file(content) {
        Ok(ast) => ast,
        Err(e) => {
            eprintln!("[WARN] AST parse failed for {}: {}", path.display(), e);
            return HashSet::new();
        }
    };

    let mut visitor = TraitUsageExtractor::new();
    visitor.visit_file(&ast);
    visitor.usages
}

/// Extract just method names that are called (simplified interface).
///
/// This returns only the method names, useful for simple dead code detection.
pub fn extract_called_method_names(path: &Path, content: &str) -> HashSet<String> {
    extract_trait_usages(path, content)
        .into_iter()
        .map(|u| u.method_name)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn test_method_call() {
        let content = r#"
fn main() {
    let obj = Foo::new();
    obj.process();
    obj.run();
}
"#;
        let usages = extract_trait_usages(&PathBuf::from("test.rs"), content);

        assert!(usages.iter().any(|u| u.method_name == "process"
            && matches!(u.usage_kind, UsageKind::MethodCall)));
        assert!(usages.iter().any(|u| u.method_name == "run"
            && matches!(u.usage_kind, UsageKind::MethodCall)));
    }

    #[test]
    fn test_associated_call() {
        let content = r#"
fn main() {
    let x = Foo::new();
    let y = Bar::create();
}
"#;
        let usages = extract_trait_usages(&PathBuf::from("test.rs"), content);

        let new_usage = usages.iter().find(|u| u.method_name == "new").unwrap();
        assert!(matches!(new_usage.usage_kind, UsageKind::AssociatedCall));
        assert_eq!(new_usage.type_name, Some("Foo".to_string()));

        let create_usage = usages.iter().find(|u| u.method_name == "create").unwrap();
        assert!(matches!(create_usage.usage_kind, UsageKind::AssociatedCall));
        assert_eq!(create_usage.type_name, Some("Bar".to_string()));
    }

    #[test]
    fn test_qualified_call() {
        let content = r#"
fn main() {
    <Foo as MyTrait>::required_method(&foo);
    <Bar as OtherTrait>::do_thing();
}
"#;
        let usages = extract_trait_usages(&PathBuf::from("test.rs"), content);

        let req_usage = usages
            .iter()
            .find(|u| u.method_name == "required_method")
            .unwrap();
        assert!(matches!(req_usage.usage_kind, UsageKind::QualifiedCall));
        assert_eq!(req_usage.type_name, Some("Foo".to_string()));
        assert_eq!(req_usage.trait_name, Some("MyTrait".to_string()));
    }

    #[test]
    fn test_chained_method_calls() {
        let content = r#"
fn main() {
    iter().map(|x| x).filter(|x| true).collect();
}
"#;
        let names = extract_called_method_names(&PathBuf::from("test.rs"), content);
        assert!(names.contains("map"));
        assert!(names.contains("filter"));
        assert!(names.contains("collect"));
    }

    #[test]
    fn test_nested_module_type() {
        let content = r#"
fn main() {
    module::inner::Type::method();
}
"#;
        let usages = extract_trait_usages(&PathBuf::from("test.rs"), content);
        let usage = usages.iter().find(|u| u.method_name == "method").unwrap();
        assert_eq!(usage.type_name, Some("module::inner::Type".to_string()));
    }

    #[test]
    fn test_malformed_resilient() {
        let content = "fn main( { obj.broken }";
        let usages = extract_trait_usages(&PathBuf::from("broken.rs"), content);
        assert!(usages.is_empty());
    }
}
