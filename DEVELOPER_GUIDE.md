# Developer Guide

Complete guide for developing and extending deadmod.

## Prerequisites

- Rust 1.70+ (2021 edition)
- Cargo
- Git

Optional:
- Graphviz (for DOT visualization)
- A modern browser (for HTML visualizers)

## Quick Setup

```bash
# Clone repository
git clone https://github.com/anthropics/deadmod
cd deadmod

# Build debug
cargo build

# Build release
cargo build --release

# Run tests
cargo test

# Run with logging
RUST_LOG=info cargo run -- .
```

## Project Structure

```
deadmod/
├── Cargo.toml              # Workspace manifest
├── deadmod-cli/            # Command-line interface
│   ├── Cargo.toml
│   └── src/
│       └── main.rs         # CLI entry point (1200+ lines)
├── deadmod-core/           # Core library
│   ├── Cargo.toml
│   └── src/
│       ├── lib.rs          # Public exports
│       ├── scan.rs         # File discovery
│       ├── parse.rs        # AST parsing
│       ├── graph.rs        # Module graph
│       ├── detect.rs       # Dead detection
│       ├── fix.rs          # Auto-fix
│       ├── cache.rs        # Incremental cache
│       ├── config.rs       # Configuration
│       ├── logging.rs      # Structured logging
│       ├── report.rs       # Output formatting
│       ├── root.rs         # Entry point detection
│       ├── workspace.rs    # Workspace support
│       ├── visualize.rs    # DOT output
│       ├── visualize_html.rs   # Canvas visualizer
│       ├── visualize_pixi.rs   # WebGL visualizer
│       ├── tests.rs        # Integration tests
│       ├── callgraph/      # Call graph subsystem
│       ├── func/           # Function detection
│       ├── traits/         # Trait detection
│       ├── generics/       # Generic detection
│       ├── macros/         # Macro detection
│       ├── constants/      # Constant detection
│       ├── enums/          # Enum detection
│       └── matcharms/      # Match arm detection
└── deadmod-lsp/            # Language server (experimental)
    ├── Cargo.toml
    └── src/
        └── main.rs
```

## Module-by-Module Walkthrough

### deadmod-core/src/scan.rs

**Purpose**: Discover all `.rs` files in a crate.

**Key Function**:
```rust
pub fn gather_rs_files(root: &Path) -> Result<Vec<PathBuf>>
```

**Implementation Notes**:
- Uses `walkdir` for directory traversal
- Parallelized with `rayon::par_bridge()`
- Filters for `.rs` extension
- Returns sorted paths for determinism

---

### deadmod-core/src/parse.rs

**Purpose**: Parse Rust files and extract module references.

**Key Types**:
```rust
pub struct ModuleInfo {
    pub path: PathBuf,
    pub name: String,
    pub refs: HashSet<String>,
}

pub enum ParseResult {
    Ok(String, ModuleInfo),
    Skipped(PathBuf, String),
}
```

**Key Functions**:
```rust
pub fn parse_single_module(path: &Path) -> ParseResult
pub fn parse_modules(files: &[PathBuf]) -> Result<HashMap<String, ModuleInfo>>
pub fn extract_uses_and_decls(content: &str, refs: &mut HashSet<String>) -> Result<()>
```

**Path Normalization** (cross-platform):
```rust
pub fn normalize_path_string(path: &str) -> String  // Converts \ to /
pub fn path_to_normalized_string(path: &Path) -> String
```

---

### deadmod-core/src/cache.rs

**Purpose**: Incremental parsing with SHA-256 caching.

**Cache Structure**:
```rust
pub struct DeadmodCache {
    pub modules: HashMap<String, CachedModule>,
}

pub struct CachedModule {
    pub hash: String,
    pub refs: HashSet<String>,
}
```

**Key Functions**:
```rust
pub fn load_cache(crate_root: &Path) -> Option<DeadmodCache>
pub fn save_cache(crate_root: &Path, cache: &DeadmodCache) -> Result<()>
pub fn incremental_parse(
    crate_root: &Path,
    files: &[PathBuf],
    cached: Option<DeadmodCache>,
) -> Result<HashMap<String, ModuleInfo>>
```

**Atomic Write Pattern**:
```rust
// Write to temp file
fs::write(&temp_path, &json)?;
// Atomic rename
fs::rename(&temp_path, &path)?;
```

---

### deadmod-core/src/graph.rs

**Purpose**: Build module dependency graph.

**Key Functions**:
```rust
pub fn build_graph(mods: &HashMap<String, ModuleInfo>) -> HashMap<String, HashSet<String>>
pub fn reachable_from_roots<'a>(
    graph: &HashMap<String, HashSet<String>>,
    roots: impl Iterator<Item = &'a str>,
) -> HashSet<String>
```

**BFS Implementation**:
```rust
// O(V + E) traversal using adjacency list
let mut visited = HashSet::new();
let mut queue = VecDeque::from_iter(roots);
while let Some(node) = queue.pop_front() {
    if visited.insert(node.to_string()) {
        if let Some(deps) = graph.get(node) {
            queue.extend(deps.iter().map(String::as_str));
        }
    }
}
```

---

### deadmod-core/src/callgraph/

**Purpose**: Function-level call graph analysis.

**Module Structure**:
```
callgraph/
├── mod.rs           # Exports
├── extractor.rs     # Function extraction
├── usage.rs         # Call extraction
├── graph.rs         # Graph building
└── path_resolver.rs # Semantic resolution
```

**Path Resolution**:
```rust
// Resolves: query() → "db::query" (via use crate::db::query)
pub fn resolve_call_path(
    call: &str,
    usemap: &UseMap,
    ctx: &ModulePathContext,
) -> Vec<String>
```

---

### deadmod-core/src/fix.rs

**Purpose**: Auto-remove dead code.

**Key Functions**:
```rust
pub fn fix_dead_modules(
    crate_root: &Path,
    dead: &[String],
    mods: &HashMap<String, ModuleInfo>,
    dry_run: bool,
) -> Result<FixResult>

pub fn remove_file(path: &Path, dry_run: bool) -> Result<bool>
pub fn remove_mod_declaration(parent: &Path, module_name: &str, dry_run: bool) -> Result<bool>
pub fn clean_empty_dirs(dir: &Path, dry_run: bool) -> Result<Vec<String>>
```

**Safety Measures**:
```rust
// Symlink check
if metadata.file_type().is_symlink() {
    eprintln!("[WARN] Refusing to delete symlink: {}", path.display());
    return Ok(false);
}
```

---

## Adding a New Detection Type

### Step 1: Create Module Structure

```bash
mkdir -p deadmod-core/src/mytype
touch deadmod-core/src/mytype/mod.rs
touch deadmod-core/src/mytype/mytype_extractor.rs
touch deadmod-core/src/mytype/mytype_graph.rs
touch deadmod-core/src/mytype/mytype_usage.rs
```

### Step 2: Implement Extractor

```rust
// mytype_extractor.rs
use syn::{File, Item};
use std::path::Path;

#[derive(Debug, Clone)]
pub struct MyTypeItem {
    pub name: String,
    pub file: String,
    pub visibility: String,
}

pub fn extract_mytypes(path: &Path, content: &str) -> Vec<MyTypeItem> {
    let ast: File = match syn::parse_file(content) {
        Ok(ast) => ast,
        Err(_) => return Vec::new(),
    };

    let mut items = Vec::new();
    for item in &ast.items {
        // Extract relevant items
    }
    items
}
```

### Step 3: Implement Graph

```rust
// mytype_graph.rs
use std::collections::{HashMap, HashSet};

pub struct MyTypeGraph {
    items: Vec<MyTypeItem>,
    usages: HashSet<String>,
}

impl MyTypeGraph {
    pub fn new(items: Vec<MyTypeItem>, usages: &[HashSet<String>]) -> Self {
        // Build graph
    }

    pub fn analyze(&self) -> MyTypeAnalysis {
        // Find dead items
    }
}
```

### Step 4: Implement Usage Extraction

```rust
// mytype_usage.rs
use syn::File;
use std::collections::HashSet;

pub fn extract_mytype_usages(path: &Path, content: &str) -> HashSet<String> {
    // Extract usage sites
}
```

### Step 5: Export from mod.rs

```rust
// mytype/mod.rs
mod mytype_extractor;
mod mytype_graph;
mod mytype_usage;

pub use mytype_extractor::*;
pub use mytype_graph::*;
pub use mytype_usage::*;
```

### Step 6: Add to lib.rs

```rust
// lib.rs
pub mod mytype;
pub use mytype::*;
```

### Step 7: Add CLI Flag

```rust
// deadmod-cli/src/main.rs

#[derive(Parser, Debug)]
pub struct Cli {
    // ... existing flags ...

    /// Detect dead mytypes
    #[arg(long)]
    dead_mytypes: bool,
}
```

### Step 8: Add Handler in main()

```rust
if cli.dead_mytypes {
    // Handle detection mode
}
```

---

## Coding Conventions

### Error Handling

```rust
// Use anyhow for error propagation
use anyhow::{Context, Result};

fn my_function(path: &Path) -> Result<Data> {
    let content = fs::read_to_string(path)
        .with_context(|| format!("Failed to read: {}", path.display()))?;
    // ...
}
```

### Logging

```rust
// Use eprintln! for warnings (captured by CLI)
eprintln!("[WARN] Skipping {}: {}", path.display(), reason);

// Use structured logging for verbose output
tracing::info!("Processing {} files", count);
```

### Testing

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_my_function() {
        // Arrange
        let input = "...";

        // Act
        let result = my_function(input);

        // Assert
        assert!(result.is_ok());
    }
}
```

### Documentation

```rust
/// Brief description.
///
/// Detailed explanation of what this function does.
///
/// # Arguments
///
/// * `path` - Path to the file
///
/// # Returns
///
/// Returns `Ok(Data)` on success.
///
/// # Errors
///
/// Returns an error if the file cannot be read.
pub fn my_function(path: &Path) -> Result<Data> {
    // ...
}
```

---

## Common Patterns

### AST Visitor Pattern

```rust
use syn::visit::Visit;

struct MyVisitor {
    results: Vec<Item>,
}

impl<'ast> Visit<'ast> for MyVisitor {
    fn visit_item(&mut self, item: &'ast syn::Item) {
        // Process item
        syn::visit::visit_item(self, item);
    }
}

fn extract(content: &str) -> Vec<Item> {
    let ast = syn::parse_file(content).ok()?;
    let mut visitor = MyVisitor { results: Vec::new() };
    visitor.visit_file(&ast);
    visitor.results
}
```

### Parallel Processing

```rust
use rayon::prelude::*;

fn process_files(files: &[PathBuf]) -> Vec<Result> {
    files.par_iter()
        .map(|file| process_single(file))
        .collect()
}
```

### Graph Traversal (BFS)

```rust
use std::collections::{HashSet, VecDeque};

fn bfs_reachable(graph: &Graph, start: &str) -> HashSet<String> {
    let mut visited = HashSet::new();
    let mut queue = VecDeque::new();
    queue.push_back(start.to_string());

    while let Some(node) = queue.pop_front() {
        if visited.insert(node.clone()) {
            if let Some(neighbors) = graph.get(&node) {
                queue.extend(neighbors.iter().cloned());
            }
        }
    }
    visited
}
```

---

## Development Workflow

### 1. Create Feature Branch

```bash
git checkout -b feature/my-feature
```

### 2. Make Changes

```bash
# Edit files
vim deadmod-core/src/mymodule.rs

# Check compilation
cargo check

# Run specific test
cargo test test_name

# Run all tests
cargo test
```

### 3. Format and Lint

```bash
cargo fmt
cargo clippy
```

### 4. Commit

```bash
git add .
git commit -m "feat: add my feature

- Description of change
- Another point"
```

### 5. Push and PR

```bash
git push origin feature/my-feature
# Create PR on GitHub
```

---

## Debugging Tips

### Enable Verbose Output

```bash
RUST_LOG=debug cargo run -- .
```

### Print AST

```rust
let ast = syn::parse_file(content)?;
println!("{:#?}", ast);
```

### Trace Call Graph

```bash
deadmod . --callgraph | jq '.'
```

### Check Cache State

```bash
cat .deadmod/cache.json | jq '.'
```

### Profile Performance

```bash
cargo build --release
time ./target/release/deadmod .
```

---

## Common Pitfalls

### 1. Forgetting to Export

```rust
// Wrong: function not exported
fn my_function() {}

// Right: add pub and export in lib.rs
pub fn my_function() {}
```

### 2. Using .unwrap() in Production

```rust
// Wrong: can panic
let value = option.unwrap();

// Right: handle gracefully
let value = option.unwrap_or_default();
// Or
let value = option.context("Missing value")?;
```

### 3. Not Handling Parse Errors

```rust
// Wrong: panics on malformed input
let ast = syn::parse_file(content).unwrap();

// Right: return empty/skip
let ast = match syn::parse_file(content) {
    Ok(ast) => ast,
    Err(e) => {
        eprintln!("[WARN] Parse error: {}", e);
        return Vec::new();
    }
};
```

### 4. Windows Path Issues

```rust
// Wrong: assumes Unix paths
let path = format!("{}/{}", dir, file);

// Right: use Path APIs
let path = dir.join(file);
```

### 5. Non-Atomic File Writes

```rust
// Wrong: can corrupt on crash
fs::write(path, content)?;

// Right: atomic write
fs::write(&temp_path, content)?;
fs::rename(&temp_path, path)?;
```
