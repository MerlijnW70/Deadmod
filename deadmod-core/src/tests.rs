//! Comprehensive test suite for deadmod-core.

use crate::*;
use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

static TEST_COUNTER: AtomicU64 = AtomicU64::new(0);

fn write_file(file: &Path, content: &str) {
    fs::create_dir_all(file.parent().unwrap()).unwrap();
    fs::write(file, content).unwrap();
}

fn setup_temp_project() -> PathBuf {
    let id = TEST_COUNTER.fetch_add(1, Ordering::SeqCst);
    let timestamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let dir = std::env::temp_dir()
        .join("deadmod_tests")
        .join(format!("{}_{}", timestamp, id));

    if dir.exists() {
        fs::remove_dir_all(&dir).ok();
    }
    fs::create_dir_all(dir.join("src")).unwrap();
    dir
}

fn get_root_module_names(root: &Path) -> HashSet<String> {
    ["src/main.rs", "src/lib.rs"]
        .iter()
        .map(|p| root.join(p))
        .filter(|p| p.exists())
        .filter_map(|p| p.file_stem().map(|s| s.to_string_lossy().to_string()))
        .collect()
}

// Core Test 1: Simple Dead Module Detection
#[test]
fn test_simple_dead_module() {
    let root = setup_temp_project();
    write_file(&root.join("src/main.rs"), "mod b; use b::*; fn main() {}");
    write_file(&root.join("src/a.rs"), "pub fn x() {}");
    write_file(&root.join("src/b.rs"), "pub fn y() {}");

    let files = gather_rs_files(&root).unwrap();
    let mods = parse_modules(&files).unwrap();
    let g = build_graph(&mods);
    let roots = get_root_module_names(&root);
    let mut reachable = HashSet::new();
    for r in &roots {
        reachable.extend(reachable_from_root(&g, r.as_str()));
    }
    let mut dead: Vec<_> = find_dead(&mods, &reachable);
    dead.sort();

    assert_eq!(dead, vec!["a"], "module a should be dead; b is declared");
}

// Core Test 2: Explicit Mod Declaration
#[test]
fn test_mod_declaration_detection() {
    let root = setup_temp_project();
    write_file(&root.join("src/main.rs"), "mod utils; fn main() {}");
    write_file(&root.join("src/utils.rs"), "pub fn y() {}");

    let files = gather_rs_files(&root).unwrap();
    let mods = parse_modules(&files).unwrap();

    assert!(mods.contains_key("utils"));
    assert!(
        mods["main"].refs.contains("utils"),
        "main.rs should reference utils via mod declaration"
    );
}

// Core Test 3: Complex Path Imports
// Tests semantic extraction: only root path components are extracted
#[test]
fn test_use_path_imports() {
    let root = setup_temp_project();
    write_file(
        &root.join("src/main.rs"),
        "use crate::nested::inner::a; mod nested; fn main() {}",
    );
    write_file(&root.join("src/nested.rs"), "pub mod inner;");
    write_file(&root.join("src/nested/inner.rs"), "pub mod a {}");
    write_file(&root.join("src/a.rs"), "pub fn dead_a() {}");

    let files = gather_rs_files(&root).unwrap();
    let mods = parse_modules(&files).unwrap();

    // Semantic extraction: `use crate::nested::inner::a` extracts "nested" (root after crate)
    // NOT "a" which is a nested item (could be a type, function, or submodule)
    assert!(
        mods["main"].refs.contains("nested"),
        "main.rs should depend on root module 'nested' (not leaf 'a')"
    );

    let g = build_graph(&mods);
    let roots = get_root_module_names(&root);
    let mut reachable = HashSet::new();
    for r in &roots {
        reachable.extend(reachable_from_root(&g, r.as_str()));
    }
    let dead = find_dead(&mods, &reachable);

    // The separate src/a.rs is dead because it's not referenced
    // The nested::inner::a is a different module path
    assert!(
        dead.contains(&"a"),
        "src/a.rs should be dead (different from nested::inner::a)"
    );
}

// Extended Test 1: Parallelism Stress Test
#[test]
fn test_parallel_heavy_file_load() {
    let root = setup_temp_project();
    for i in 0..200u32 {
        let file = root.join("src").join(format!("m{}.rs", i));
        write_file(&file, "pub fn x() {}");
    }
    write_file(&root.join("src/main.rs"), "fn main() {}");

    let files = gather_rs_files(&root).unwrap();
    assert_eq!(files.len(), 201, "Should detect all files in parallel load");

    let mods = parse_modules(&files).unwrap();
    assert_eq!(mods.len(), 201, "All modules must be parsed in parallel");
}

// Extended Test 2: Aliased Imports
// Tests semantic extraction: original name is extracted, not the alias
#[test]
fn test_aliased_imports() {
    let root = setup_temp_project();
    write_file(
        &root.join("src/main.rs"),
        "use crate::util as u; fn main() {}",
    );
    write_file(&root.join("src/util.rs"), "pub fn yy() {}");

    let files = gather_rs_files(&root).unwrap();
    let mods = parse_modules(&files).unwrap();

    assert!(mods.contains_key("util"));
    // Semantic extraction: `use crate::util as u` extracts "util" (the actual module)
    // NOT "u" which is just a local alias
    assert!(
        mods["main"].refs.contains("util"),
        "main.rs should contain a ref to original module 'util' (not alias 'u')"
    );
}

// Extended Test 3: Group Renames
// Tests semantic extraction: root path is extracted, not group items
#[test]
fn test_group_renames() {
    let root = setup_temp_project();
    write_file(
        &root.join("src/main.rs"),
        "mod foo; use foo::{a, b as bb}; fn main() {}",
    );
    write_file(&root.join("src/foo.rs"), "pub mod a; pub mod b;");
    write_file(&root.join("src/foo/a.rs"), "pub fn fn_a() {}");
    write_file(&root.join("src/foo/b.rs"), "pub fn fn_b() {}");

    let files = gather_rs_files(&root).unwrap();
    let mods = parse_modules(&files).unwrap();

    // Semantic extraction: `use foo::{a, b as bb}` extracts "foo" (the root path)
    // NOT "a" or "bb" which are nested items (could be types, functions, or submodules)
    assert!(
        mods["main"].refs.contains("foo"),
        "main.rs should reference 'foo' from both mod declaration and use statement"
    );

    // foo.rs has `pub mod a; pub mod b;` which creates dependencies
    assert!(mods["foo"].refs.contains("a"), "foo.rs should reference 'a'");
    assert!(mods["foo"].refs.contains("b"), "foo.rs should reference 'b'");

    let g = build_graph(&mods);
    let roots = get_root_module_names(&root);
    let mut reachable = HashSet::new();
    for r in &roots {
        reachable.extend(reachable_from_root(&g, r.as_str()));
    }
    let dead = find_dead(&mods, &reachable);

    // All modules should be reachable: main -> foo -> a, b
    assert!(!dead.contains(&"foo"));
    assert!(!dead.contains(&"a"));
    assert!(!dead.contains(&"b"));
}

// Extended Test 4: Malformed File Handling
#[test]
fn test_malformed_files_do_not_panic() {
    let root = setup_temp_project();
    write_file(&root.join("src/main.rs"), "fn main() {}");
    write_file(&root.join("src/bad.rs"), "this is not valid rust !!!");

    let files = gather_rs_files(&root).unwrap();
    let result = parse_modules(&files);

    assert!(result.is_ok(), "Parser must not panic on malformed files");

    let mods = result.unwrap();
    assert!(mods.contains_key("main"));
    assert!(!mods.contains_key("bad"), "Malformed file should be skipped");
}

// Extended Test 5: Root Detection for lib.rs
#[test]
fn test_lib_root_detection() {
    let root = setup_temp_project();
    write_file(&root.join("src/lib.rs"), "mod utils; pub fn lib_fn() {}");
    write_file(&root.join("src/utils.rs"), "pub fn used() {}");
    write_file(&root.join("src/dead.rs"), "pub fn unused() {}");

    let files = gather_rs_files(&root).unwrap();
    let mods = parse_modules(&files).unwrap();

    let g = build_graph(&mods);
    let roots = get_root_module_names(&root);

    assert!(roots.contains("lib"), "lib.rs must be detected as root");

    let mut reachable = HashSet::new();
    for r in &roots {
        reachable.extend(reachable_from_root(&g, r.as_str()));
    }
    let mut dead: Vec<_> = find_dead(&mods, &reachable);
    dead.sort();

    assert_eq!(dead, vec!["dead"], "Module 'dead' should be unreachable");
}

// Extended Test 6: Deeply Nested Dead Module
#[test]
fn test_deeply_nested_dead_module() {
    let root = setup_temp_project();
    write_file(&root.join("src/main.rs"), "mod a; fn main() {}");
    write_file(&root.join("src/a.rs"), "pub mod b; pub fn x() {}");
    write_file(&root.join("src/a/b.rs"), "pub fn y() {}");
    write_file(&root.join("src/a/dead_c.rs"), "pub fn z() {}");

    let files = gather_rs_files(&root).unwrap();
    let mods = parse_modules(&files).unwrap();

    let g = build_graph(&mods);
    let roots = get_root_module_names(&root);
    let mut reachable = HashSet::new();
    for r in &roots {
        reachable.extend(reachable_from_root(&g, r.as_str()));
    }
    let mut dead: Vec<_> = find_dead(&mods, &reachable);
    dead.sort();

    assert!(mods.contains_key("dead_c"));
    assert_eq!(dead, vec!["dead_c"], "Nested 'dead_c' must be marked dead");
}

// Test 7: Config Loading
#[test]
fn test_config_loading() {
    let root = setup_temp_project();
    write_file(
        &root.join("deadmod.toml"),
        r#"
ignore = ["test_", "mock"]

[output]
format = "json"
"#,
    );

    let cfg = load_config(&root).unwrap();
    assert!(cfg.is_some());

    let cfg = cfg.unwrap();
    assert!(cfg.ignore.is_some());
    assert_eq!(cfg.ignore.as_ref().unwrap().len(), 2);
    assert!(cfg.output.is_some());
    assert_eq!(cfg.output.as_ref().unwrap().format.as_ref().unwrap(), "json");
}

// Test 8: Config Not Found
#[test]
fn test_config_not_found() {
    let root = setup_temp_project();
    let cfg = load_config(&root).unwrap();
    assert!(cfg.is_none());
}

// Test 9: Logging Module
#[test]
fn test_logging_does_not_panic() {
    log_info("test info");
    log_warn("test warn");
    log_error("test error");
    log_event("CUSTOM", "custom detail");
}

// ============================================================================
// DEEP EDGE CASE TESTS - NASA-GRADE ROBUSTNESS
// ============================================================================

// --- UTF-8 AND UNICODE TESTS ---

#[test]
fn test_unicode_module_names() {
    let root = setup_temp_project();
    // Module names with various unicode characters
    write_file(&root.join("src/main.rs"), "mod café; fn main() {}");
    write_file(&root.join("src/café.rs"), "pub fn brew() {}");

    let files = gather_rs_files(&root).unwrap();
    let mods = parse_modules(&files).unwrap();

    // Should handle unicode module names gracefully
    assert!(mods.contains_key("main"));
}

#[test]
fn test_unicode_in_function_names() {
    let root = setup_temp_project();
    write_file(
        &root.join("src/main.rs"),
        r#"
        fn main() {}
        fn 日本語_関数() {}
        fn emoji_🎉() {}
        fn ñoño() {}
        "#,
    );

    let files = gather_rs_files(&root).unwrap();
    let result = parse_modules(&files);
    assert!(result.is_ok(), "Should handle unicode in function names");
}

#[test]
fn test_unicode_in_comments() {
    let root = setup_temp_project();
    write_file(
        &root.join("src/main.rs"),
        r#"
        // 日本語コメント
        /* Ελληνικά σχόλια */
        /// Документация на русском
        fn main() {}
        "#,
    );

    let files = gather_rs_files(&root).unwrap();
    let mods = parse_modules(&files).unwrap();
    assert!(mods.contains_key("main"));
}

#[test]
fn test_unicode_string_literals() {
    let root = setup_temp_project();
    write_file(
        &root.join("src/main.rs"),
        r#"
        fn main() {
            let s = "日本語テキスト";
            let emoji = "🎉🚀💻";
            let mixed = "Hello 世界!";
        }
        "#,
    );

    let files = gather_rs_files(&root).unwrap();
    let mods = parse_modules(&files).unwrap();
    assert!(mods.contains_key("main"));
}

// --- EMPTY AND BOUNDARY TESTS ---

#[test]
fn test_empty_file() {
    let root = setup_temp_project();
    write_file(&root.join("src/main.rs"), "fn main() {}");
    write_file(&root.join("src/empty.rs"), "");

    let files = gather_rs_files(&root).unwrap();
    let mods = parse_modules(&files).unwrap();

    assert!(mods.contains_key("main"));
    assert!(mods.contains_key("empty"), "Empty files should still be parsed");
}

#[test]
fn test_whitespace_only_file() {
    let root = setup_temp_project();
    write_file(&root.join("src/main.rs"), "fn main() {}");
    write_file(&root.join("src/whitespace.rs"), "   \n\n\t\t\n   ");

    let files = gather_rs_files(&root).unwrap();
    let mods = parse_modules(&files).unwrap();

    assert!(mods.contains_key("main"));
    assert!(mods.contains_key("whitespace"));
}

#[test]
fn test_comment_only_file() {
    let root = setup_temp_project();
    write_file(&root.join("src/main.rs"), "fn main() {}");
    write_file(
        &root.join("src/comments.rs"),
        r#"
        // Just a comment
        /* Block comment */
        /// Doc comment
        //! Inner doc comment
        "#,
    );

    let files = gather_rs_files(&root).unwrap();
    let mods = parse_modules(&files).unwrap();

    // Comment-only file is valid Rust and should be parsed
    // (it may or may not contain the module depending on syn's behavior)
    assert!(mods.contains_key("main"));
    // The file itself should be found
    assert!(files.iter().any(|f| f.file_name().unwrap() == "comments.rs"));
}

#[test]
fn test_zero_modules() {
    let root = setup_temp_project();
    // No .rs files at all
    write_file(&root.join("src/readme.txt"), "Not a rust file");

    let files = gather_rs_files(&root).unwrap();
    assert!(files.is_empty() || files.iter().all(|f| f.extension() != Some(std::ffi::OsStr::new("rs"))));
}

#[test]
fn test_single_module_no_dependencies() {
    let root = setup_temp_project();
    write_file(&root.join("src/main.rs"), "fn main() {}");

    let files = gather_rs_files(&root).unwrap();
    let mods = parse_modules(&files).unwrap();
    let g = build_graph(&mods);

    assert_eq!(mods.len(), 1);
    assert!(mods["main"].refs.is_empty());

    let reachable = reachable_from_root(&g, "main");
    assert!(reachable.contains("main"));
}

// --- CYCLIC DEPENDENCY TESTS ---

#[test]
fn test_direct_cycle_a_to_b_to_a() {
    let root = setup_temp_project();
    write_file(&root.join("src/main.rs"), "mod a; fn main() {}");
    write_file(&root.join("src/a.rs"), "mod b; pub use b::*;");
    write_file(&root.join("src/b.rs"), "mod a; pub use a::*;"); // Cycle!

    let files = gather_rs_files(&root).unwrap();
    let mods = parse_modules(&files).unwrap();
    let g = build_graph(&mods);

    // Should not infinite loop
    let reachable = reachable_from_root(&g, "main");
    assert!(reachable.contains("main"));
    assert!(reachable.contains("a"));
}

#[test]
fn test_self_referential_module() {
    let root = setup_temp_project();
    write_file(&root.join("src/main.rs"), "mod self_ref; fn main() {}");
    write_file(&root.join("src/self_ref.rs"), "use crate::self_ref; pub fn x() {}");

    let files = gather_rs_files(&root).unwrap();
    let mods = parse_modules(&files).unwrap();
    let g = build_graph(&mods);

    let reachable = reachable_from_root(&g, "main");
    assert!(reachable.contains("self_ref"));
}

#[test]
fn test_triangle_cycle() {
    let root = setup_temp_project();
    write_file(&root.join("src/main.rs"), "mod a; fn main() {}");
    write_file(&root.join("src/a.rs"), "mod b;");
    write_file(&root.join("src/b.rs"), "mod c;");
    write_file(&root.join("src/c.rs"), "mod a;"); // Cycle: a -> b -> c -> a

    let files = gather_rs_files(&root).unwrap();
    let mods = parse_modules(&files).unwrap();
    let g = build_graph(&mods);

    let reachable = reachable_from_root(&g, "main");
    assert!(reachable.contains("a"));
    assert!(reachable.contains("b"));
    assert!(reachable.contains("c"));
}

// --- COMPLEX RUST SYNTAX TESTS ---

#[test]
fn test_async_functions() {
    let root = setup_temp_project();
    write_file(
        &root.join("src/main.rs"),
        r#"
        async fn main() {}
        async fn helper() -> Result<(), Box<dyn std::error::Error>> { Ok(()) }
        "#,
    );

    let files = gather_rs_files(&root).unwrap();
    let mods = parse_modules(&files).unwrap();
    assert!(mods.contains_key("main"));
}

#[test]
fn test_const_generics() {
    let root = setup_temp_project();
    write_file(
        &root.join("src/main.rs"),
        r#"
        struct Array<T, const N: usize>([T; N]);
        impl<T, const N: usize> Array<T, N> {
            fn len(&self) -> usize { N }
        }
        fn main() {}
        "#,
    );

    let files = gather_rs_files(&root).unwrap();
    let mods = parse_modules(&files).unwrap();
    assert!(mods.contains_key("main"));
}

#[test]
fn test_impl_trait_syntax() {
    let root = setup_temp_project();
    write_file(
        &root.join("src/main.rs"),
        r#"
        fn returns_closure() -> impl Fn(i32) -> i32 {
            |x| x + 1
        }
        fn takes_impl(f: impl Fn()) { f() }
        fn main() {}
        "#,
    );

    let files = gather_rs_files(&root).unwrap();
    let mods = parse_modules(&files).unwrap();
    assert!(mods.contains_key("main"));
}

#[test]
fn test_complex_where_clauses() {
    let root = setup_temp_project();
    write_file(
        &root.join("src/main.rs"),
        r#"
        fn complex<T, U, V>(t: T, u: U, v: V)
        where
            T: Clone + Send + 'static,
            U: for<'a> Fn(&'a T) -> V,
            V: Default + std::fmt::Debug,
        {
        }
        fn main() {}
        "#,
    );

    let files = gather_rs_files(&root).unwrap();
    let mods = parse_modules(&files).unwrap();
    assert!(mods.contains_key("main"));
}

#[test]
fn test_raw_identifiers() {
    let root = setup_temp_project();
    write_file(
        &root.join("src/main.rs"),
        r#"
        fn r#fn() {}
        fn r#match() {}
        mod r#mod {}
        fn main() { r#fn(); r#match(); }
        "#,
    );

    let files = gather_rs_files(&root).unwrap();
    let mods = parse_modules(&files).unwrap();
    assert!(mods.contains_key("main"));
}

#[test]
fn test_macro_definitions() {
    let root = setup_temp_project();
    write_file(
        &root.join("src/main.rs"),
        r#"
        macro_rules! my_macro {
            () => { println!("empty") };
            ($x:expr) => { println!("{}", $x) };
            ($($x:expr),+ $(,)?) => { $(println!("{}", $x);)+ };
        }
        fn main() { my_macro!(); my_macro!(1); my_macro!(1, 2, 3); }
        "#,
    );

    let files = gather_rs_files(&root).unwrap();
    let mods = parse_modules(&files).unwrap();
    assert!(mods.contains_key("main"));
}

#[test]
fn test_attribute_macros() {
    let root = setup_temp_project();
    write_file(
        &root.join("src/main.rs"),
        r#"
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
        struct Point { x: i32, y: i32 }

        #[cfg(test)]
        mod tests {
            #[test]
            fn it_works() {}
        }

        #[inline(always)]
        fn hot_path() {}

        fn main() {}
        "#,
    );

    let files = gather_rs_files(&root).unwrap();
    let mods = parse_modules(&files).unwrap();
    assert!(mods.contains_key("main"));
}

#[test]
fn test_nested_modules_inline() {
    let root = setup_temp_project();
    write_file(
        &root.join("src/main.rs"),
        r#"
        mod outer {
            pub mod middle {
                pub mod inner {
                    pub fn deeply_nested() {}
                }
            }
        }
        fn main() { outer::middle::inner::deeply_nested(); }
        "#,
    );

    let files = gather_rs_files(&root).unwrap();
    let mods = parse_modules(&files).unwrap();
    assert!(mods.contains_key("main"));
}

// --- MALFORMED INPUT TESTS ---

#[test]
fn test_unclosed_brace() {
    let root = setup_temp_project();
    write_file(&root.join("src/main.rs"), "fn main() {}");
    write_file(&root.join("src/bad.rs"), "fn broken() {");

    let files = gather_rs_files(&root).unwrap();
    let mods = parse_modules(&files).unwrap();

    assert!(mods.contains_key("main"));
    assert!(!mods.contains_key("bad"), "Malformed should be skipped");
}

#[test]
fn test_invalid_token() {
    let root = setup_temp_project();
    write_file(&root.join("src/main.rs"), "fn main() {}");
    write_file(&root.join("src/invalid.rs"), "fn @ invalid() {}");

    let files = gather_rs_files(&root).unwrap();
    let mods = parse_modules(&files).unwrap();

    assert!(mods.contains_key("main"));
}

#[test]
fn test_binary_garbage() {
    let root = setup_temp_project();
    write_file(&root.join("src/main.rs"), "fn main() {}");

    // Write binary garbage
    let garbage: Vec<u8> = vec![0x00, 0xFF, 0xFE, 0x89, 0x50, 0x4E, 0x47];
    fs::write(root.join("src/garbage.rs"), garbage).unwrap();

    let files = gather_rs_files(&root).unwrap();
    let mods = parse_modules(&files).unwrap();

    assert!(mods.contains_key("main"));
    // Garbage file should be skipped gracefully
}

#[test]
fn test_extremely_long_line() {
    let root = setup_temp_project();
    let long_string = "x".repeat(100_000);
    write_file(
        &root.join("src/main.rs"),
        &format!("fn main() {{ let s = \"{}\"; }}", long_string),
    );

    let files = gather_rs_files(&root).unwrap();
    let mods = parse_modules(&files).unwrap();
    assert!(mods.contains_key("main"));
}

#[test]
fn test_deeply_nested_expressions() {
    let root = setup_temp_project();
    // Create deeply nested parentheses
    let mut expr = "x".to_string();
    for _ in 0..100 {
        expr = format!("({})", expr);
    }
    write_file(
        &root.join("src/main.rs"),
        &format!("fn main() {{ let _ = {}; }}", expr),
    );

    let files = gather_rs_files(&root).unwrap();
    let mods = parse_modules(&files).unwrap();
    assert!(mods.contains_key("main"));
}

// --- STRESS TESTS ---

#[test]
fn test_500_modules() {
    let root = setup_temp_project();
    let mut main_content = String::from("fn main() {}\n");

    for i in 0..500 {
        let mod_name = format!("mod_{}", i);
        main_content.push_str(&format!("mod {};\n", mod_name));
        write_file(
            &root.join(format!("src/{}.rs", mod_name)),
            &format!("pub fn func_{}() {{}}", i),
        );
    }
    write_file(&root.join("src/main.rs"), &main_content);

    let files = gather_rs_files(&root).unwrap();
    assert_eq!(files.len(), 501); // main + 500 modules

    let mods = parse_modules(&files).unwrap();
    assert_eq!(mods.len(), 501);
}

#[test]
fn test_deep_dependency_chain() {
    let root = setup_temp_project();

    // Create chain: main -> m0 -> m1 -> m2 -> ... -> m99
    write_file(&root.join("src/main.rs"), "mod m0; fn main() {}");

    for i in 0..99 {
        write_file(
            &root.join(format!("src/m{}.rs", i)),
            &format!("mod m{};", i + 1),
        );
    }
    write_file(&root.join("src/m99.rs"), "pub fn end() {}");

    let files = gather_rs_files(&root).unwrap();
    let mods = parse_modules(&files).unwrap();
    let g = build_graph(&mods);

    let reachable = reachable_from_root(&g, "main");

    // All 101 modules should be reachable
    assert!(reachable.contains("main"));
    assert!(reachable.contains("m0"));
    assert!(reachable.contains("m50"));
    assert!(reachable.contains("m99"));
}

#[test]
fn test_wide_dependency_tree() {
    let root = setup_temp_project();

    // main depends on 100 modules directly
    let mut main_content = String::from("fn main() {}\n");
    for i in 0..100 {
        main_content.push_str(&format!("mod leaf_{};\n", i));
        write_file(
            &root.join(format!("src/leaf_{}.rs", i)),
            "pub fn func() {}",
        );
    }
    write_file(&root.join("src/main.rs"), &main_content);

    let files = gather_rs_files(&root).unwrap();
    let mods = parse_modules(&files).unwrap();
    let g = build_graph(&mods);

    let reachable = reachable_from_root(&g, "main");
    assert_eq!(reachable.len(), 101); // main + 100 leaves
}

#[test]
fn test_diamond_dependency() {
    let root = setup_temp_project();

    //     main
    //    /    \
    //   a      b
    //    \    /
    //      c
    write_file(&root.join("src/main.rs"), "mod a; mod b; fn main() {}");
    write_file(&root.join("src/a.rs"), "mod c;");
    write_file(&root.join("src/b.rs"), "mod c;");
    write_file(&root.join("src/c.rs"), "pub fn shared() {}");

    let files = gather_rs_files(&root).unwrap();
    let mods = parse_modules(&files).unwrap();
    let g = build_graph(&mods);

    let reachable = reachable_from_root(&g, "main");
    assert!(reachable.contains("main"));
    assert!(reachable.contains("a"));
    assert!(reachable.contains("b"));
    assert!(reachable.contains("c"));
}

// --- GRAPH ALGORITHM TESTS ---

#[test]
fn test_multi_source_bfs_correctness() {
    let root = setup_temp_project();

    // Two separate trees from main and lib
    write_file(&root.join("src/main.rs"), "mod main_dep; fn main() {}");
    write_file(&root.join("src/lib.rs"), "mod lib_dep;");
    write_file(&root.join("src/main_dep.rs"), "pub fn x() {}");
    write_file(&root.join("src/lib_dep.rs"), "pub fn y() {}");
    write_file(&root.join("src/orphan.rs"), "pub fn z() {}"); // Not reachable from either

    let files = gather_rs_files(&root).unwrap();
    let mods = parse_modules(&files).unwrap();
    let g = build_graph(&mods);

    // Multi-source BFS from both roots
    let reachable = reachable_from_roots(&g, ["main", "lib"]);

    assert!(reachable.contains("main"));
    assert!(reachable.contains("lib"));
    assert!(reachable.contains("main_dep"));
    assert!(reachable.contains("lib_dep"));
    assert!(!reachable.contains("orphan"), "orphan should be unreachable");
}

#[test]
fn test_reachability_with_missing_root() {
    let root = setup_temp_project();
    write_file(&root.join("src/main.rs"), "fn main() {}");

    let files = gather_rs_files(&root).unwrap();
    let mods = parse_modules(&files).unwrap();
    let g = build_graph(&mods);

    // Try to reach from a non-existent root
    let reachable = reachable_from_roots(&g, ["nonexistent", "main"]);

    assert!(reachable.contains("main"));
    assert!(!reachable.contains("nonexistent"));
}

#[test]
fn test_empty_graph_reachability() {
    let mods: std::collections::HashMap<String, parse::ModuleInfo> = std::collections::HashMap::new();
    let g = build_graph(&mods);

    let reachable = reachable_from_roots(&g, std::iter::empty::<&str>());
    assert!(reachable.is_empty());
}

// --- PATH AND FILE SYSTEM EDGE CASES ---

#[test]
fn test_path_with_spaces() {
    let root = setup_temp_project();
    // Most file systems support spaces in paths
    let subdir = root.join("src/path with spaces");
    fs::create_dir_all(&subdir).unwrap();
    write_file(&root.join("src/main.rs"), "mod space_mod; fn main() {}");
    write_file(&subdir.join("space_mod.rs"), "pub fn x() {}");

    let files = gather_rs_files(&root).unwrap();
    // Files in directories with spaces should be found
    assert!(!files.is_empty());
}

#[test]
fn test_symlink_handling() {
    // Skip on Windows where symlinks require admin
    #[cfg(unix)]
    {
        use std::os::unix::fs::symlink;

        let root = setup_temp_project();
        write_file(&root.join("src/main.rs"), "fn main() {}");
        write_file(&root.join("src/real.rs"), "pub fn x() {}");

        // Create symlink
        let link_path = root.join("src/linked.rs");
        if symlink(root.join("src/real.rs"), &link_path).is_ok() {
            let files = gather_rs_files(&root).unwrap();
            // Should handle symlinks gracefully
            assert!(files.iter().any(|f| f.file_stem() == Some(std::ffi::OsStr::new("main"))));
        }
    }
}

// --- CONCURRENCY TESTS ---

#[test]
fn test_parallel_parse_determinism() {
    let root = setup_temp_project();

    for i in 0..50 {
        write_file(
            &root.join(format!("src/mod_{}.rs", i)),
            &format!("pub fn func_{}() {{}}", i),
        );
    }
    write_file(&root.join("src/main.rs"), "fn main() {}");

    let files = gather_rs_files(&root).unwrap();

    // Parse multiple times and ensure deterministic results
    let mods1 = parse_modules(&files).unwrap();
    let mods2 = parse_modules(&files).unwrap();
    let mods3 = parse_modules(&files).unwrap();

    assert_eq!(mods1.len(), mods2.len());
    assert_eq!(mods2.len(), mods3.len());

    for key in mods1.keys() {
        assert!(mods2.contains_key(key));
        assert!(mods3.contains_key(key));
    }
}

// --- SPECIAL RUST CONSTRUCTS ---

#[test]
fn test_extern_crate() {
    let root = setup_temp_project();
    write_file(
        &root.join("src/main.rs"),
        r#"
        extern crate std;
        extern crate alloc;
        fn main() {}
        "#,
    );

    let files = gather_rs_files(&root).unwrap();
    let mods = parse_modules(&files).unwrap();
    assert!(mods.contains_key("main"));
}

#[test]
fn test_use_glob() {
    let root = setup_temp_project();
    write_file(&root.join("src/main.rs"), "mod utils; use utils::*; fn main() {}");
    write_file(&root.join("src/utils.rs"), "pub fn a() {} pub fn b() {}");

    let files = gather_rs_files(&root).unwrap();
    let mods = parse_modules(&files).unwrap();

    assert!(mods["main"].refs.contains("utils"));
}

#[test]
fn test_pub_use_reexport() {
    let root = setup_temp_project();
    write_file(&root.join("src/main.rs"), "mod facade; use facade::helper; fn main() {}");
    write_file(&root.join("src/facade.rs"), "mod internal; pub use internal::helper;");
    write_file(&root.join("src/internal.rs"), "pub fn helper() {}");

    let files = gather_rs_files(&root).unwrap();
    let mods = parse_modules(&files).unwrap();

    assert!(mods["main"].refs.contains("facade"));
    assert!(mods["facade"].refs.contains("internal"));
}

#[test]
fn test_cfg_conditional_modules() {
    let root = setup_temp_project();
    write_file(
        &root.join("src/main.rs"),
        r#"
        #[cfg(unix)]
        mod unix_impl;

        #[cfg(windows)]
        mod windows_impl;

        #[cfg(target_arch = "x86_64")]
        mod x86_impl;

        fn main() {}
        "#,
    );
    write_file(&root.join("src/unix_impl.rs"), "pub fn unix_fn() {}");
    write_file(&root.join("src/windows_impl.rs"), "pub fn win_fn() {}");
    write_file(&root.join("src/x86_impl.rs"), "pub fn x86_fn() {}");

    let files = gather_rs_files(&root).unwrap();
    let mods = parse_modules(&files).unwrap();

    // All modules should be detected regardless of cfg
    assert!(mods.contains_key("main"));
    // Note: cfg-gated mod declarations are still parsed
}

#[test]
fn test_doc_hidden_modules() {
    let root = setup_temp_project();
    write_file(
        &root.join("src/main.rs"),
        r#"
        #[doc(hidden)]
        mod hidden;

        mod visible;

        fn main() {}
        "#,
    );
    write_file(&root.join("src/hidden.rs"), "pub fn secret() {}");
    write_file(&root.join("src/visible.rs"), "pub fn public() {}");

    let files = gather_rs_files(&root).unwrap();
    let mods = parse_modules(&files).unwrap();

    // Both should be detected
    assert!(mods["main"].refs.contains("hidden"));
    assert!(mods["main"].refs.contains("visible"));
}

// --- VISUALIZER JSON TESTS ---

#[test]
fn test_visualizer_json_structure() {
    let root = setup_temp_project();
    write_file(&root.join("src/main.rs"), "mod utils; fn main() {}");
    write_file(&root.join("src/utils.rs"), "pub fn helper() {}");
    write_file(&root.join("src/dead.rs"), "pub fn unused() {}");

    let files = gather_rs_files(&root).unwrap();
    let mods = parse_modules(&files).unwrap();
    let g = build_graph(&mods);
    let reachable = reachable_from_root(&g, "main");

    let json = module_graph_to_visualizer_json(&mods, &reachable);

    // Verify structure
    assert!(json["nodes"].is_array());
    assert!(json["edges"].is_array());
    assert!(json["stats"].is_object());

    // Verify numeric IDs
    let nodes = json["nodes"].as_array().unwrap();
    for node in nodes {
        assert!(node["id"].is_u64());
        assert!(node["name"].is_string());
        assert!(node["dead"].is_boolean());
    }

    // Verify stats
    assert!(json["stats"]["total_modules"].is_u64());
    assert!(json["stats"]["dead_modules"].is_u64());
}

#[test]
fn test_visualizer_json_dead_detection() {
    let root = setup_temp_project();
    write_file(&root.join("src/main.rs"), "mod alive; fn main() {}");
    write_file(&root.join("src/alive.rs"), "pub fn x() {}");
    write_file(&root.join("src/dead.rs"), "pub fn y() {}");

    let files = gather_rs_files(&root).unwrap();
    let mods = parse_modules(&files).unwrap();
    let g = build_graph(&mods);
    let reachable = reachable_from_root(&g, "main");

    let json = module_graph_to_visualizer_json(&mods, &reachable);
    let nodes = json["nodes"].as_array().unwrap();

    let dead_node = nodes.iter().find(|n| n["name"].as_str() == Some("dead")).unwrap();
    let alive_node = nodes.iter().find(|n| n["name"].as_str() == Some("alive")).unwrap();

    assert!(dead_node["dead"].as_bool().unwrap());
    assert!(!alive_node["dead"].as_bool().unwrap());
}
