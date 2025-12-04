//! Macro definition extraction from Rust AST.
//!
//! Extracts all macro definitions including:
//! - `macro_rules!` definitions
//! - `#[macro_export]` exported macros
//! - Local macros inside modules
//!
//! NASA-grade resilience: handles malformed AST gracefully.

use serde::{Deserialize, Serialize};
use std::path::Path;
use syn::{visit::Visit, Attribute, File, Item, ItemMacro, ItemMod};

/// Information about a macro definition.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MacroDef {
    /// Name of the macro
    pub name: String,
    /// Whether the macro has #[macro_export]
    pub exported: bool,
    /// Source file path
    pub file: String,
    /// Module path (for nested macros)
    pub module_path: String,
}

/// AST visitor that extracts all macro definitions.
struct MacroExtractor {
    file_path: String,
    results: Vec<MacroDef>,
    current_mod: Vec<String>,
}

impl MacroExtractor {
    fn new(file_path: String) -> Self {
        Self {
            file_path,
            results: Vec::with_capacity(8),
            current_mod: Vec::new(),
        }
    }

    fn is_exported(attrs: &[Attribute]) -> bool {
        attrs.iter().any(|a| a.path().is_ident("macro_export"))
    }

    fn build_module_path(&self) -> String {
        self.current_mod.join("::")
    }

    fn record(&mut self, name: String, exported: bool) {
        self.results.push(MacroDef {
            name,
            exported,
            file: self.file_path.clone(),
            module_path: self.build_module_path(),
        });
    }
}

impl<'ast> Visit<'ast> for MacroExtractor {
    fn visit_item(&mut self, item: &'ast Item) {
        match item {
            // Handle macro_rules! definitions
            Item::Macro(ItemMacro {
                ident: Some(id),
                attrs,
                ..
            }) => {
                self.record(id.to_string(), Self::is_exported(attrs));
            }

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
                return; // Don't call default visitor
            }

            _ => {}
        }

        syn::visit::visit_item(self, item);
    }
}

/// Extract all macro definitions from file content.
///
/// Returns a list of MacroDef for each macro found.
/// On parse error, returns an empty list (resilient behavior).
pub fn extract_macros(path: &Path, content: &str) -> Vec<MacroDef> {
    let ast: File = match syn::parse_file(content) {
        Ok(ast) => ast,
        Err(e) => {
            eprintln!("[WARN] AST parse failed for {}: {}", path.display(), e);
            return Vec::new();
        }
    };

    let mut extractor = MacroExtractor::new(path.display().to_string());
    extractor.visit_file(&ast);
    extractor.results
}

/// Result of macro extraction from multiple files.
#[derive(Debug, Clone, Default)]
pub struct MacroExtractionResult {
    /// All declared macros
    pub declared: Vec<MacroDef>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn test_extract_simple_macro() {
        let content = r#"
macro_rules! my_macro {
    () => {};
}
"#;
        let result = extract_macros(&PathBuf::from("test.rs"), content);
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].name, "my_macro");
        assert!(!result[0].exported);
    }

    #[test]
    fn test_extract_exported_macro() {
        let content = r#"
#[macro_export]
macro_rules! exported_macro {
    ($x:expr) => { $x };
}
"#;
        let result = extract_macros(&PathBuf::from("test.rs"), content);
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].name, "exported_macro");
        assert!(result[0].exported);
    }

    #[test]
    fn test_extract_nested_macro() {
        let content = r#"
mod inner {
    macro_rules! nested_macro {
        () => {};
    }
}
"#;
        let result = extract_macros(&PathBuf::from("test.rs"), content);
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].name, "nested_macro");
        assert_eq!(result[0].module_path, "inner");
    }

    #[test]
    fn test_multiple_macros() {
        let content = r#"
macro_rules! foo {
    () => {};
}

#[macro_export]
macro_rules! bar {
    () => {};
}

macro_rules! baz {
    () => {};
}
"#;
        let result = extract_macros(&PathBuf::from("test.rs"), content);
        assert_eq!(result.len(), 3);
    }

    #[test]
    fn test_malformed_resilient() {
        let content = "macro_rules! { broken";
        let result = extract_macros(&PathBuf::from("broken.rs"), content);
        assert!(result.is_empty());
    }
}
