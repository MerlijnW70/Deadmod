//! Shared module path building utilities for AST extraction.

/// Trait for building module paths from the current module context.
///
/// Implement this trait on extractor structs that track the current
/// module hierarchy during AST traversal.
pub trait ModulePathBuilder {
    /// Returns the current module path components.
    fn current_mod(&self) -> &[String];

    /// Builds the current module path as a `::` separated string.
    ///
    /// # Example
    /// If `current_mod()` returns `["api", "v1", "handlers"]`,
    /// this returns `"api::v1::handlers"`.
    fn build_module_path(&self) -> String {
        self.current_mod().join("::")
    }

    /// Builds a full path by appending a name to the current module path.
    ///
    /// # Example
    /// If `current_mod()` returns `["api", "handlers"]` and `name` is `"process"`,
    /// this returns `"api::handlers::process"`.
    fn build_full_path(&self, name: &str) -> String {
        let mut parts = self.current_mod().to_vec();
        parts.push(name.to_string());
        parts.join("::")
    }

    /// Builds a full path with an optional intermediate component (e.g., impl type).
    ///
    /// # Example
    /// If `current_mod()` returns `["api"]`, `intermediate` is `Some("Handler")`,
    /// and `name` is `"new"`, this returns `"api::Handler::new"`.
    fn build_full_path_with_intermediate(&self, intermediate: Option<&str>, name: &str) -> String {
        let mut parts = self.current_mod().to_vec();
        if let Some(inter) = intermediate {
            parts.push(inter.to_string());
        }
        parts.push(name.to_string());
        parts.join("::")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct TestExtractor {
        current_mod: Vec<String>,
    }

    impl ModulePathBuilder for TestExtractor {
        fn current_mod(&self) -> &[String] {
            &self.current_mod
        }
    }

    #[test]
    fn test_build_module_path_empty() {
        let ext = TestExtractor { current_mod: vec![] };
        assert_eq!(ext.build_module_path(), "");
    }

    #[test]
    fn test_build_module_path_single() {
        let ext = TestExtractor {
            current_mod: vec!["api".to_string()],
        };
        assert_eq!(ext.build_module_path(), "api");
    }

    #[test]
    fn test_build_module_path_nested() {
        let ext = TestExtractor {
            current_mod: vec!["api".to_string(), "v1".to_string(), "handlers".to_string()],
        };
        assert_eq!(ext.build_module_path(), "api::v1::handlers");
    }

    #[test]
    fn test_build_full_path() {
        let ext = TestExtractor {
            current_mod: vec!["api".to_string(), "handlers".to_string()],
        };
        assert_eq!(ext.build_full_path("process"), "api::handlers::process");
    }

    #[test]
    fn test_build_full_path_with_intermediate() {
        let ext = TestExtractor {
            current_mod: vec!["api".to_string()],
        };
        assert_eq!(
            ext.build_full_path_with_intermediate(Some("Handler"), "new"),
            "api::Handler::new"
        );
    }

    #[test]
    fn test_build_full_path_without_intermediate() {
        let ext = TestExtractor {
            current_mod: vec!["api".to_string()],
        };
        assert_eq!(
            ext.build_full_path_with_intermediate(None, "process"),
            "api::process"
        );
    }
}
