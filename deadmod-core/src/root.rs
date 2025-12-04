//! Root module detection for Rust crates.
//!
//! Implements Cargo's full entrypoint logic to detect all valid root modules.
//! NASA-grade resilience: never panics, handles all I/O errors gracefully.

use std::collections::HashSet;
use std::fs;
use std::path::Path;

/// Detect all valid Cargo root modules for a crate.
///
/// NASA-grade resilience: never panics, returns empty set on any error.
///
/// Returns module names (e.g. "main", "lib", "convert_cli").
///
/// This implements Cargo's full entrypoint logic:
/// - src/main.rs
/// - src/lib.rs
/// - src/bin/*.rs
/// - src/bin/<name>/main.rs
pub fn find_root_modules(crate_root: &Path) -> HashSet<String> {
    let mut out = HashSet::new();

    let src = crate_root.join("src");
    if !src.exists() {
        return out;
    }

    // 1. src/main.rs → "main"
    let main = src.join("main.rs");
    if main.exists() {
        out.insert("main".to_string());
    }

    // 2. src/lib.rs → "lib"
    let lib = src.join("lib.rs");
    if lib.exists() {
        out.insert("lib".to_string());
    }

    // 3. src/bin/*.rs and src/bin/<name>/main.rs
    let bin_dir = src.join("bin");
    if bin_dir.exists() && bin_dir.is_dir() {
        if let Ok(entries) = fs::read_dir(&bin_dir) {
            for entry in entries.flatten() {
                let path = entry.path();

                // src/bin/name.rs (standalone binary)
                if path.is_file() && path.extension().is_some_and(|ext| ext == "rs") {
                    if let Some(stem) = path.file_stem() {
                        out.insert(stem.to_string_lossy().to_string());
                    }
                }

                // src/bin/<name>/main.rs (inline binary folder)
                if path.is_dir() {
                    let inline_main = path.join("main.rs");
                    if inline_main.exists() {
                        if let Some(bin_name) = path.file_name() {
                            out.insert(bin_name.to_string_lossy().to_string());
                        }
                    }
                }
            }
        }
    }

    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use std::sync::atomic::{AtomicU64, Ordering};

    static TEST_COUNTER: AtomicU64 = AtomicU64::new(0);

    fn create_file(path: &Path, content: &str) {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).unwrap();
        }
        fs::File::create(path)
            .unwrap()
            .write_all(content.as_bytes())
            .unwrap();
    }

    fn create_temp_dir(name: &str) -> std::path::PathBuf {
        let id = TEST_COUNTER.fetch_add(1, Ordering::SeqCst);
        let temp_dir = std::env::temp_dir()
            .join("deadmod_root_test")
            .join(format!("{}_{}", name, id));
        if temp_dir.exists() {
            fs::remove_dir_all(&temp_dir).ok();
        }
        fs::create_dir_all(&temp_dir).unwrap();
        temp_dir
    }

    #[test]
    fn test_find_root_modules_lib_and_main() {
        let temp_dir = create_temp_dir("lib_main");
        let src_dir = temp_dir.join("src");
        fs::create_dir_all(&src_dir).unwrap();

        create_file(&src_dir.join("lib.rs"), "");
        create_file(&src_dir.join("main.rs"), "fn main() {}");

        let roots = find_root_modules(&temp_dir);
        assert_eq!(roots.len(), 2);
        assert!(roots.contains("lib"));
        assert!(roots.contains("main"));
    }

    #[test]
    fn test_find_root_modules_only_lib() {
        let temp_dir = create_temp_dir("lib_only");
        let src_dir = temp_dir.join("src");
        fs::create_dir_all(&src_dir).unwrap();

        create_file(&src_dir.join("lib.rs"), "");

        let roots = find_root_modules(&temp_dir);
        assert_eq!(roots.len(), 1);
        assert!(roots.contains("lib"));
    }

    #[test]
    fn test_find_root_modules_with_binaries() {
        let temp_dir = create_temp_dir("with_bins");
        let src_dir = temp_dir.join("src");
        let bin_dir = src_dir.join("bin");
        fs::create_dir_all(&bin_dir).unwrap();

        create_file(&src_dir.join("main.rs"), "fn main() {}");
        create_file(&bin_dir.join("cli.rs"), "fn main() {}");
        create_file(&bin_dir.join("server.rs"), "fn main() {}");

        let roots = find_root_modules(&temp_dir);
        assert_eq!(roots.len(), 3);
        assert!(roots.contains("main"));
        assert!(roots.contains("cli"));
        assert!(roots.contains("server"));
    }

    #[test]
    fn test_find_root_modules_with_inline_binary() {
        let temp_dir = create_temp_dir("inline_bin");
        let src_dir = temp_dir.join("src");
        let bin_dir = src_dir.join("bin").join("myapp");
        fs::create_dir_all(&bin_dir).unwrap();

        create_file(&src_dir.join("lib.rs"), "");
        create_file(&bin_dir.join("main.rs"), "fn main() {}");

        let roots = find_root_modules(&temp_dir);
        assert_eq!(roots.len(), 2);
        assert!(roots.contains("lib"));
        assert!(roots.contains("myapp"));
    }

    #[test]
    fn test_find_root_modules_no_src() {
        let temp_dir = create_temp_dir("no_src");

        let roots = find_root_modules(&temp_dir);
        assert!(roots.is_empty());
    }

    #[test]
    fn test_find_root_modules_mixed() {
        let temp_dir = create_temp_dir("mixed");
        let src_dir = temp_dir.join("src");
        let bin_dir = src_dir.join("bin");
        fs::create_dir_all(bin_dir.join("tool_folder")).unwrap();

        // lib + standalone bin + inline bin
        create_file(&src_dir.join("lib.rs"), "");
        create_file(&bin_dir.join("fast_tool.rs"), "fn main() {}");
        create_file(&bin_dir.join("tool_folder").join("main.rs"), "fn main() {}");

        let roots = find_root_modules(&temp_dir);
        assert_eq!(roots.len(), 3);
        assert!(roots.contains("lib"));
        assert!(roots.contains("fast_tool"));
        assert!(roots.contains("tool_folder"));
    }
}
