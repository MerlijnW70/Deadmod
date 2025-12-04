# Testing Guide

Comprehensive testing documentation for deadmod.

## Overview

Deadmod has **360+ tests** covering:
- Unit tests for individual functions
- Integration tests for end-to-end workflows
- Edge case tests for corner cases
- Stress tests for performance validation

## Running Tests

### Basic Commands

```bash
# Run all tests
cargo test

# Run tests in release mode (faster)
cargo test --release

# Run with verbose output
cargo test -- --nocapture

# Run specific test
cargo test test_name

# Run tests matching pattern
cargo test parse

# Run tests in specific crate
cargo test -p deadmod-core
cargo test -p deadmod-cli
```

### Test Categories

```bash
# Run only unit tests
cargo test --lib

# Run only integration tests
cargo test --test '*'

# Run only doc tests
cargo test --doc
```

---

## Test Structure

### Directory Layout

```
deadmod/
├── deadmod-core/
│   └── src/
│       ├── lib.rs
│       ├── scan.rs          # + tests
│       ├── parse.rs         # + 27 tests
│       ├── graph.rs         # + tests
│       ├── detect.rs        # + tests
│       ├── fix.rs           # + tests
│       ├── cache.rs         # + 12 tests
│       ├── root.rs          # + tests
│       ├── tests.rs         # Integration tests
│       └── callgraph/
│           ├── extractor.rs # + tests
│           ├── graph.rs     # + tests
│           └── usage.rs     # + tests
├── deadmod-cli/
│   └── src/
│       └── main.rs          # + CLI tests
└── deadmod-lsp/
    └── src/
        └── main.rs          # + LSP tests
```

### Test Module Pattern

Each source file has an inline test module:

```rust
// In parse.rs
pub fn extract_uses_and_decls(content: &str, refs: &mut HashSet<String>) -> Result<()> {
    // Implementation...
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_mod_declaration() {
        let content = "mod utils;";
        let mut refs = HashSet::new();
        extract_uses_and_decls(content, &mut refs).unwrap();
        assert!(refs.contains("utils"));
    }

    #[test]
    fn test_extract_use_statement() {
        let content = "use crate::config;";
        let mut refs = HashSet::new();
        extract_uses_and_decls(content, &mut refs).unwrap();
        assert!(refs.contains("config"));
    }
}
```

---

## Test Categories by Module

### Scanning (`scan.rs`)

| Test | Description |
|------|-------------|
| `test_gather_rs_files_basic` | Finds .rs files in directory |
| `test_gather_rs_files_nested` | Handles nested directories |
| `test_gather_rs_files_excludes_target` | Skips target/ directory |
| `test_gather_rs_files_excludes_git` | Skips .git/ directory |
| `test_gather_rs_files_custom_excludes` | Custom exclusion patterns |

### Parsing (`parse.rs`)

| Test | Description |
|------|-------------|
| `test_extract_mod_declaration` | `mod foo;` extraction |
| `test_extract_pub_mod` | `pub mod foo;` extraction |
| `test_extract_use_crate` | `use crate::foo` extraction |
| `test_extract_use_super` | `use super::foo` extraction |
| `test_extract_nested_use` | `use crate::a::b::c` paths |
| `test_extract_use_alias` | `use foo as bar` handling |
| `test_extract_empty_file` | Empty file handling |
| `test_extract_comment_only_file` | Comments-only file |
| `test_normalize_unix_path` | Path normalization (Unix) |
| `test_normalize_windows_path` | Path normalization (Windows) |
| `test_normalize_mixed_separators` | Mixed path separators |
| `test_normalize_empty_path` | Empty string handling |
| `test_module_info_new` | ModuleInfo construction |
| `test_module_info_refs` | Module reference tracking |

### Graph Building (`graph.rs`)

| Test | Description |
|------|-------------|
| `test_build_graph_basic` | Graph construction |
| `test_reachable_from_root_simple` | Single-source BFS |
| `test_reachable_from_roots_multi_source` | Multi-source BFS |
| `test_reachable_from_roots_missing_root` | Missing root handling |
| `test_reachable_from_roots_empty` | Empty graph |
| `test_module_graph_to_visualizer_json` | JSON export |

### Detection (`detect.rs`)

| Test | Description |
|------|-------------|
| `test_find_dead_basic` | Basic dead detection |
| `test_find_dead_all_reachable` | No dead modules |
| `test_find_dead_empty` | Empty input |

### Root Detection (`root.rs`)

| Test | Description |
|------|-------------|
| `test_find_root_modules_lib_and_main` | lib.rs + main.rs |
| `test_find_root_modules_only_lib` | lib.rs only |
| `test_find_root_modules_with_binaries` | src/bin/*.rs |
| `test_find_root_modules_with_inline_binary` | src/bin/foo/main.rs |
| `test_find_root_modules_no_src` | Missing src/ |
| `test_find_root_modules_mixed` | All entry point types |

### Caching (`cache.rs`)

| Test | Description |
|------|-------------|
| `test_file_hash_deterministic` | Hash consistency |
| `test_file_hash_changes_on_content_change` | Content change detection |
| `test_hash_bytes_deterministic` | In-memory hashing |
| `test_cache_save_load` | Cache persistence |
| `test_load_cache_not_found` | Missing cache handling |
| `test_incremental_parse_fresh` | First-time parsing |
| `test_incremental_parse_cache_hit` | Cache hit path |
| `test_incremental_parse_cache_invalidation` | Cache invalidation |
| `test_incremental_parse_parallel_stress` | 100 file stress test |
| `test_atomic_write_creates_file` | Atomic write creation |
| `test_atomic_write_no_temp_file_left` | Temp file cleanup |
| `test_atomic_write_overwrites_existing` | Atomic overwrite |
| `test_cache_file_is_valid_json` | JSON validity |
| `test_load_cache_corrupted_json` | Corrupted cache handling |
| `test_load_cache_empty_file` | Empty cache file |
| `test_rapid_save_load_cycles` | Concurrent access simulation |
| `test_large_cache_atomic_write` | Large cache handling |
| `test_cache_special_characters_in_module_names` | Special characters |
| `test_hash_bytes_empty` | Empty content hash |
| `test_hash_bytes_unicode` | Unicode content hash |

### Auto-Fix (`fix.rs`)

| Test | Description |
|------|-------------|
| `test_remove_file_exists` | File removal |
| `test_remove_file_dry_run` | Dry-run mode |
| `test_remove_file_not_exists` | Missing file handling |
| `test_remove_mod_declaration_simple` | `mod foo;` removal |
| `test_remove_mod_declaration_pub` | `pub mod foo;` removal |
| `test_remove_mod_declaration_dry_run` | Dry-run mode |
| `test_remove_mod_declaration_not_found` | Missing declaration |
| `test_remove_mod_declaration_with_attributes` | `#[attr] mod foo;` |
| `test_remove_mod_declaration_pub_super` | `pub(super) mod foo;` |
| `test_clean_empty_dirs` | Empty directory cleanup |
| `test_clean_empty_dirs_nested` | Deep nesting |
| `test_clean_empty_dirs_preserves_src` | src/ preservation |
| `test_clean_empty_dirs_max_depth` | Recursion limit |
| `test_fix_dead_modules_integration` | End-to-end fix |
| `test_fix_dead_modules_empty_list` | No dead modules |
| `test_fix_dead_modules_dry_run` | Dry-run mode |
| `test_fix_multiple_dead_modules` | Multiple removals |
| `test_remove_file_symlink_protection` | Symlink security |
| `test_mod_patterns_complex` | Complex patterns |
| `test_mod_patterns_no_match` | No match case |
| `test_mod_patterns_special_chars` | Special characters |
| `test_blank_line_cleanup` | Formatting cleanup |

### Call Graph (`callgraph/`)

| Test | Description |
|------|-------------|
| `test_extract_function_basic` | Function extraction |
| `test_extract_method` | Method extraction |
| `test_extract_impl_method` | impl method extraction |
| `test_extract_call_basic` | Call site extraction |
| `test_extract_method_call` | Method call extraction |
| `test_build_callgraph` | Graph construction |
| `test_find_unreachable` | Dead function detection |
| `test_resolve_call_path` | Path resolution |
| `test_resolve_use_alias` | Alias resolution |

---

## Writing Tests

### Test File Template

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicU64, Ordering};

    // Thread-safe counter for unique test directories
    static TEST_COUNTER: AtomicU64 = AtomicU64::new(0);

    // Create isolated temp directory
    fn create_temp_dir(name: &str) -> PathBuf {
        let id = TEST_COUNTER.fetch_add(1, Ordering::SeqCst);
        let temp_dir = std::env::temp_dir()
            .join("deadmod_test")
            .join(format!("{}_{}", name, id));
        if temp_dir.exists() {
            fs::remove_dir_all(&temp_dir).ok();
        }
        fs::create_dir_all(&temp_dir).unwrap();
        temp_dir
    }

    // Helper to create test files
    fn create_file(path: &Path, content: &str) {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).unwrap();
        }
        fs::write(path, content).unwrap();
    }

    #[test]
    fn test_my_function() {
        // Arrange
        let dir = create_temp_dir("my_test");
        create_file(&dir.join("test.rs"), "fn foo() {}");

        // Act
        let result = my_function(&dir);

        // Assert
        assert!(result.is_ok());

        // Cleanup (optional - temp dirs are periodically cleaned)
        fs::remove_dir_all(&dir).ok();
    }
}
```

### Test Patterns

#### Arrange-Act-Assert

```rust
#[test]
fn test_find_dead() {
    // Arrange
    let mut modules = HashMap::new();
    modules.insert("main".to_string(), ModuleInfo::new(PathBuf::from("src/main.rs")));
    modules.insert("dead".to_string(), ModuleInfo::new(PathBuf::from("src/dead.rs")));
    let reachable: HashSet<&str> = ["main"].into_iter().collect();

    // Act
    let dead = find_dead(&modules, &reachable);

    // Assert
    assert_eq!(dead.len(), 1);
    assert!(dead.contains(&"dead"));
}
```

#### Parameterized Tests

```rust
#[test]
fn test_normalize_paths() {
    let cases = [
        ("src/main.rs", "src/main.rs"),
        ("src\\main.rs", "src/main.rs"),
        ("src\\utils\\mod.rs", "src/utils/mod.rs"),
        ("", ""),
    ];

    for (input, expected) in cases {
        let result = normalize_path_string(input);
        assert_eq!(result, expected, "Failed for input: {}", input);
    }
}
```

#### Error Case Tests

```rust
#[test]
fn test_parse_invalid_syntax() {
    let content = "fn main( { }";  // Invalid syntax
    let mut refs = HashSet::new();

    // Should not panic, should handle gracefully
    let result = extract_uses_and_decls(content, &mut refs);

    // Either Ok with empty refs, or Err - both acceptable
    if result.is_ok() {
        assert!(refs.is_empty());
    }
}
```

---

## Test Infrastructure

### Temp Directory Management

Tests use isolated temporary directories:

```rust
fn create_temp_dir(name: &str) -> PathBuf {
    let id = TEST_COUNTER.fetch_add(1, Ordering::SeqCst);
    let temp_dir = std::env::temp_dir()
        .join("deadmod_test")
        .join(format!("{}_{}", name, id));
    // ...
}
```

**Benefits:**
- Thread-safe (atomic counter)
- Unique directories (no conflicts)
- Isolated (no cross-test interference)

### Test Fixtures

For complex test scenarios, create fixture files:

```rust
// tests/fixtures/simple_crate/
// ├── Cargo.toml
// ├── src/
// │   ├── main.rs
// │   ├── lib.rs
// │   ├── utils.rs
// │   └── dead.rs

#[test]
fn test_simple_crate() {
    let fixture = Path::new("tests/fixtures/simple_crate");
    let result = analyze_crate(fixture);
    // ...
}
```

---

## Coverage

### Running Coverage

```bash
# Install cargo-llvm-cov
cargo install cargo-llvm-cov

# Generate coverage report
cargo llvm-cov

# Generate HTML report
cargo llvm-cov --html

# Open report
open target/llvm-cov/html/index.html
```

### Coverage Goals

| Module | Target | Current |
|--------|--------|---------|
| scan.rs | 90% | 95% |
| parse.rs | 90% | 92% |
| graph.rs | 90% | 94% |
| detect.rs | 90% | 100% |
| fix.rs | 85% | 88% |
| cache.rs | 90% | 93% |
| callgraph/ | 85% | 87% |

---

## Performance Testing

### Stress Tests

```rust
#[test]
fn test_incremental_parse_parallel_stress() {
    let dir = create_temp_dir("parallel_stress");
    fs::create_dir_all(dir.join("src")).unwrap();

    // Create 100 files
    let mut files = Vec::new();
    for i in 0..100 {
        let file = dir.join("src").join(format!("mod_{}.rs", i));
        fs::write(&file, format!("pub fn func_{}() {{}}", i)).unwrap();
        files.push(file);
    }

    // Time first parse (cold cache)
    let start = std::time::Instant::now();
    let result1 = incremental_parse(&dir, &files, None).unwrap();
    let cold_time = start.elapsed();

    // Time second parse (warm cache)
    let cache = load_cache(&dir);
    let start = std::time::Instant::now();
    let result2 = incremental_parse(&dir, &files, cache).unwrap();
    let warm_time = start.elapsed();

    assert_eq!(result1.len(), 100);
    assert_eq!(result2.len(), 100);

    // Warm cache should be faster
    assert!(warm_time < cold_time);
}
```

### Benchmarks

```bash
# Run benchmarks (if configured)
cargo bench
```

---

## CI Integration

### GitHub Actions

```yaml
name: Tests

on: [push, pull_request]

jobs:
  test:
    runs-on: ${{ matrix.os }}
    strategy:
      matrix:
        os: [ubuntu-latest, macos-latest, windows-latest]
        rust: [stable, beta]

    steps:
      - uses: actions/checkout@v4

      - name: Install Rust
        uses: dtolnay/rust-action@stable
        with:
          toolchain: ${{ matrix.rust }}

      - name: Build
        run: cargo build --all

      - name: Test
        run: cargo test --all

      - name: Clippy
        run: cargo clippy -- -D warnings

      - name: Format Check
        run: cargo fmt -- --check
```

---

## Debugging Tests

### Verbose Output

```bash
# Show println! output
cargo test -- --nocapture

# Show test names as they run
cargo test -- --nocapture --test-threads=1
```

### Single Test

```bash
# Run one specific test
cargo test test_parse_mod_declaration -- --nocapture
```

### Debugging

```bash
# Run with debug logging
RUST_LOG=debug cargo test test_name -- --nocapture
```

---

## Test Maintenance

### Flaky Test Prevention

1. **Use atomic counters** for unique resources
2. **Isolate tests** with unique temp directories
3. **Don't rely on global state**
4. **Clean up after tests** (optional but good practice)

### Test Organization

1. **Group related tests** in the same `#[cfg(test)]` module
2. **Use descriptive names** - `test_<function>_<scenario>`
3. **One assertion per test** when possible
4. **Test edge cases** explicitly

---

## Summary

| Metric | Value |
|--------|-------|
| Total Tests | 360+ |
| Test Coverage | ~90% |
| Platforms Tested | Linux, macOS, Windows |
| Rust Versions | Stable, Beta |
| CI Pipeline | GitHub Actions |
