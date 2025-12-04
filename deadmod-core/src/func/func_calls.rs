//! Function call extraction from Rust AST.
//!
//! Detects all call sites including:
//! - Direct calls: foo()
//! - Path calls: module::foo()
//! - Method calls: obj.method()
//! - Associated function calls: Type::func()
//!
//! NASA-grade resilience: handles malformed AST gracefully.

use std::collections::HashSet;
use std::path::Path;

use syn::{visit::Visit, Expr, File};

/// Information about a function call site.
#[derive(Debug, Clone, Hash, Eq, PartialEq)]
pub struct CallSite {
    /// The function name being called
    pub name: String,
    /// Full path if available (e.g., "module::func")
    pub path: Option<String>,
    /// Whether this is a method call (obj.method())
    pub is_method_call: bool,
}

/// AST visitor that extracts all function calls.
struct CallExtractor {
    calls: HashSet<CallSite>,
}

impl CallExtractor {
    fn new() -> Self {
        Self {
            calls: HashSet::with_capacity(64),
        }
    }
}

impl<'ast> Visit<'ast> for CallExtractor {
    fn visit_expr(&mut self, node: &'ast Expr) {
        match node {
            // Direct function calls: foo() or path::foo()
            Expr::Call(call) => {
                if let Expr::Path(expr_path) = &*call.func {
                    let segments: Vec<_> = expr_path
                        .path
                        .segments
                        .iter()
                        .map(|s| s.ident.to_string())
                        .collect();

                    if let Some(name) = segments.last() {
                        let full_path = if segments.len() > 1 {
                            Some(segments.join("::"))
                        } else {
                            None
                        };

                        self.calls.insert(CallSite {
                            name: name.clone(),
                            path: full_path,
                            is_method_call: false,
                        });
                    }
                }
            }

            // Method calls: obj.method() or Type::method()
            Expr::MethodCall(method) => {
                self.calls.insert(CallSite {
                    name: method.method.to_string(),
                    path: None,
                    is_method_call: true,
                });
            }

            _ => {}
        }

        // Continue visiting nested expressions
        syn::visit::visit_expr(self, node);
    }
}

/// Extract all function calls from file content.
///
/// Returns a set of unique call sites found in the file.
/// On parse error, returns an empty set (resilient behavior).
pub fn extract_calls(path: &Path, content: &str) -> HashSet<CallSite> {
    let ast: File = match syn::parse_file(content) {
        Ok(ast) => ast,
        Err(e) => {
            eprintln!("[WARN] AST parse failed for {}: {}", path.display(), e);
            return HashSet::new();
        }
    };

    let mut visitor = CallExtractor::new();
    visitor.visit_file(&ast);
    visitor.calls
}

/// Extract just function names that are called (simplified interface).
///
/// This returns only the function names, useful for simple dead code detection.
pub fn extract_call_names(path: &Path, content: &str) -> HashSet<String> {
    extract_calls(path, content)
        .into_iter()
        .map(|c| c.name)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn test_extract_direct_call() {
        let content = r#"
fn main() {
    foo();
    bar();
}
"#;
        let calls = extract_call_names(&PathBuf::from("test.rs"), content);
        assert!(calls.contains("foo"));
        assert!(calls.contains("bar"));
    }

    #[test]
    fn test_extract_path_call() {
        let content = r#"
fn main() {
    module::helper();
    deep::nested::func();
}
"#;
        let calls = extract_calls(&PathBuf::from("test.rs"), content);

        let helper = calls.iter().find(|c| c.name == "helper").unwrap();
        assert_eq!(helper.path, Some("module::helper".to_string()));

        let func = calls.iter().find(|c| c.name == "func").unwrap();
        assert_eq!(func.path, Some("deep::nested::func".to_string()));
    }

    #[test]
    fn test_extract_method_call() {
        let content = r#"
fn main() {
    let obj = Foo::new();
    obj.process();
    obj.run();
}
"#;
        let calls = extract_calls(&PathBuf::from("test.rs"), content);

        assert!(calls.iter().any(|c| c.name == "new" && !c.is_method_call));
        assert!(calls.iter().any(|c| c.name == "process" && c.is_method_call));
        assert!(calls.iter().any(|c| c.name == "run" && c.is_method_call));
    }

    #[test]
    fn test_extract_chained_calls() {
        let content = r#"
fn main() {
    iter().map(|x| x).filter(|x| true).collect();
}
"#;
        let names = extract_call_names(&PathBuf::from("test.rs"), content);
        assert!(names.contains("iter"));
        assert!(names.contains("map"));
        assert!(names.contains("filter"));
        assert!(names.contains("collect"));
    }

    #[test]
    fn test_extract_nested_calls() {
        let content = r#"
fn main() {
    outer(inner(deep()));
}
"#;
        let names = extract_call_names(&PathBuf::from("test.rs"), content);
        assert!(names.contains("outer"));
        assert!(names.contains("inner"));
        assert!(names.contains("deep"));
    }

    #[test]
    fn test_malformed_file_resilient() {
        let content = "fn main( { broken }";
        let calls = extract_calls(&PathBuf::from("broken.rs"), content);
        assert!(calls.is_empty());
    }
}
