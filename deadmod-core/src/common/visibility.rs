//! Shared visibility string conversion for AST extraction.

use syn::Visibility;

/// Converts a syn Visibility type to a human-readable string.
///
/// # Returns
/// - `"pub"` for public items
/// - `"pub(crate)"` for crate-visible items
/// - `"pub(super)"` for parent-module-visible items
/// - `"pub(restricted)"` for other restricted visibility
/// - `"private"` for inherited (default private) visibility
pub fn visibility_str(v: &Visibility) -> &'static str {
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

#[cfg(test)]
mod tests {
    use super::*;
    use syn::parse_quote;

    #[test]
    fn test_visibility_public() {
        let vis: Visibility = parse_quote!(pub);
        assert_eq!(visibility_str(&vis), "pub");
    }

    #[test]
    fn test_visibility_pub_crate() {
        let vis: Visibility = parse_quote!(pub(crate));
        assert_eq!(visibility_str(&vis), "pub(crate)");
    }

    #[test]
    fn test_visibility_pub_super() {
        let vis: Visibility = parse_quote!(pub(super));
        assert_eq!(visibility_str(&vis), "pub(super)");
    }

    #[test]
    fn test_visibility_private() {
        let vis: Visibility = Visibility::Inherited;
        assert_eq!(visibility_str(&vis), "private");
    }
}
