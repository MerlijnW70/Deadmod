# API Reference

Complete API documentation for `deadmod-core`.

## Overview

```rust
use deadmod_core::{
    // Scanning
    gather_rs_files, gather_rs_files_with_excludes,

    // Parsing
    parse_modules, ModuleInfo, extract_uses_and_decls,
    normalize_path_string, path_to_normalized_string,

    // Graph Building
    build_graph, reachable_from_roots, reachable_from_root,
    module_graph_to_visualizer_json,

    // Detection
    find_dead,

    // Root Detection
    find_root_modules,

    // Auto-Fix
    fix_dead_modules, remove_file, remove_mod_declaration,
    clean_empty_dirs, FixResult,

    // Caching
    incremental_parse, load_cache, save_cache, file_hash,
    DeadmodCache, CachedModule,

    // Call Graph
    CallGraph, CallGraphAnalysis, CallGraphStats,
    FunctionDef, extract_callgraph_functions,
    extract_call_usages, extract_call_usages_resolved,
    CallUsageResult,

    // Path Resolution
    resolve_call_path, resolve_call_full, collect_use_statements,
    UseMap, ModulePathContext, ResolvedCall,

    // Configuration
    Config, load_config,

    // Logging
    setup_logging,

    // Workspace
    is_workspace_root, find_workspace_members,

    // Visualization
    generate_html_graph, generate_pixi_graph, visualize,
};
```

---

## File Scanning (`scan.rs`)

### `gather_rs_files`

Recursively discovers all `.rs` files in a directory.

```rust
pub fn gather_rs_files(root: &Path) -> Result<Vec<PathBuf>>
```

**Arguments:**
- `root` - The root directory to scan

**Returns:**
- `Result<Vec<PathBuf>>` - List of `.rs` file paths

**Performance:**
- Parallel processing via Rayon
- Early directory pruning (skips `target/`, `.git/`, `node_modules/`, `.cargo/`)
- O(n) where n = total directory entries

**Example:**
```rust
let files = gather_rs_files(Path::new("./my-crate"))?;
for file in &files {
    println!("Found: {}", file.display());
}
```

---

### `gather_rs_files_with_excludes`

Discovers `.rs` files with custom exclusion patterns.

```rust
pub fn gather_rs_files_with_excludes(
    root: &Path,
    excludes: &[&str]
) -> Result<Vec<PathBuf>>
```

**Arguments:**
- `root` - The root directory to scan
- `excludes` - Additional directory names to exclude

**Example:**
```rust
let files = gather_rs_files_with_excludes(
    Path::new("./my-crate"),
    &["benches", "examples"]
)?;
```

---

## Parsing (`parse.rs`)

### `ModuleInfo`

Represents a parsed module with its dependencies.

```rust
pub struct ModuleInfo {
    pub path: PathBuf,           // Source file path
    pub name: String,            // Module name (file stem)
    pub refs: HashSet<String>,   // Referenced module names
}
```

**Methods:**

```rust
impl ModuleInfo {
    pub fn new(path: PathBuf) -> Self;
}
```

---

### `extract_uses_and_decls`

Extracts module references from source content.

```rust
pub fn extract_uses_and_decls(
    content: &str,
    refs: &mut HashSet<String>
) -> Result<()>
```

**Arguments:**
- `content` - Rust source code as string
- `refs` - Set to populate with found module references

**Extracted patterns:**
- `mod foo;` declarations
- `use crate::foo` statements
- `use super::foo` statements

**Example:**
```rust
let content = "mod utils; use crate::config;";
let mut refs = HashSet::new();
extract_uses_and_decls(content, &mut refs)?;
assert!(refs.contains("utils"));
assert!(refs.contains("config"));
```

---

### Path Normalization

Cross-platform path utilities.

```rust
/// Converts backslashes to forward slashes
pub fn normalize_path_string(path: &str) -> String

/// Converts Path to normalized string
pub fn path_to_normalized_string(path: &Path) -> String
```

**Example:**
```rust
assert_eq!(normalize_path_string("src\\utils\\mod.rs"), "src/utils/mod.rs");
```

---

## Graph Building (`graph.rs`)

### `build_graph`

Constructs a directed dependency graph from parsed modules.

```rust
pub fn build_graph(mods: &HashMap<String, ModuleInfo>) -> DiGraphMap<&str, ()>
```

**Arguments:**
- `mods` - Map of module name to ModuleInfo

**Returns:**
- `DiGraphMap<&str, ()>` - Directed graph (petgraph)

**Complexity:** O(|V| + |E|) where V = modules, E = dependencies

---

### `reachable_from_roots`

Multi-source BFS to find all reachable modules.

```rust
pub fn reachable_from_roots<'a>(
    g: &DiGraphMap<&'a str, ()>,
    roots: impl IntoIterator<Item = &'a str>,
) -> HashSet<&'a str>
```

**Arguments:**
- `g` - The dependency graph
- `roots` - Iterator of root module names (entry points)

**Returns:**
- Set of all module names reachable from any root

**Complexity:** O(|V| + |E|) - single traversal regardless of root count

**Example:**
```rust
let graph = build_graph(&modules);
let reachable = reachable_from_roots(&graph, ["main", "lib"]);
```

---

### `reachable_from_root`

Single-source BFS (convenience wrapper).

```rust
pub fn reachable_from_root<'a>(
    g: &DiGraphMap<&'a str, ()>,
    root: &'a str
) -> HashSet<&'a str>
```

---

### `module_graph_to_visualizer_json`

Export graph in visualizer-compatible JSON format.

```rust
pub fn module_graph_to_visualizer_json(
    mods: &HashMap<String, ModuleInfo>,
    reachable: &HashSet<&str>,
) -> serde_json::Value
```

**Output format:**
```json
{
  "nodes": [{ "id": 0, "name": "main", "file": "src/main.rs", "dead": false }],
  "edges": [{ "from": 0, "to": 1 }],
  "stats": { "total_modules": 10, "total_edges": 15, "dead_modules": 2 }
}
```

---

## Detection (`detect.rs`)

### `find_dead`

Finds modules not in the reachable set.

```rust
pub fn find_dead<'a>(
    mods: &'a HashMap<String, ModuleInfo>,
    reachable: &HashSet<&str>,
) -> Vec<&'a str>
```

**Arguments:**
- `mods` - All parsed modules
- `reachable` - Set of reachable module names

**Returns:**
- List of dead (unreachable) module names

**Complexity:** O(|M|)

**Example:**
```rust
let dead = find_dead(&modules, &reachable);
for module in dead {
    println!("Dead: {}", module);
}
```

---

## Root Detection (`root.rs`)

### `find_root_modules`

Detects all Cargo entry points for a crate.

```rust
pub fn find_root_modules(crate_root: &Path) -> HashSet<String>
```

**Detected entry points:**
| File | Module Name |
|------|-------------|
| `src/main.rs` | `"main"` |
| `src/lib.rs` | `"lib"` |
| `src/bin/foo.rs` | `"foo"` |
| `src/bin/bar/main.rs` | `"bar"` |

**Returns:**
- Set of root module names (never panics, returns empty on error)

**Example:**
```rust
let roots = find_root_modules(Path::new("./my-crate"));
// roots = {"main", "lib"}
```

---

## Caching (`cache.rs`)

### `DeadmodCache`

Persistent cache structure.

```rust
pub struct DeadmodCache {
    pub modules: HashMap<String, CachedModule>,
}

pub struct CachedModule {
    pub hash: String,           // SHA-256 of file content
    pub refs: HashSet<String>,  // Cached module references
}
```

---

### `incremental_parse`

Incremental parsing with cache support.

```rust
pub fn incremental_parse(
    crate_root: &Path,
    files: &[PathBuf],
    old_cache: Option<DeadmodCache>,
) -> Result<HashMap<String, ModuleInfo>>
```

**Arguments:**
- `crate_root` - Root directory (for cache storage)
- `files` - Files to parse
- `old_cache` - Previously loaded cache (optional)

**Performance:**
- Parallel file processing via Rayon
- Read-Once Pattern: each file read exactly once
- O(|files|) I/O, O(|changed_files|) parsing

**Example:**
```rust
let cache = load_cache(Path::new("./my-crate"));
let modules = incremental_parse(
    Path::new("./my-crate"),
    &files,
    cache
)?;
```

---

### `load_cache` / `save_cache`

Cache persistence functions.

```rust
pub fn load_cache(crate_root: &Path) -> Option<DeadmodCache>
pub fn save_cache(crate_root: &Path, cache: &DeadmodCache) -> Result<()>
```

**Cache location:** `.deadmod/cache.json`

**Atomic writes:** Uses temp file + rename to prevent corruption.

---

### `file_hash`

Compute SHA-256 hash of file content.

```rust
pub fn file_hash(path: &Path) -> Result<String>
```

---

## Auto-Fix (`fix.rs`)

### `fix_dead_modules`

Main fix orchestration function.

```rust
pub fn fix_dead_modules(
    crate_root: &Path,
    dead: &[&str],
    mods: &HashMap<String, ModuleInfo>,
    dry_run: bool,
) -> Result<FixResult>
```

**Arguments:**
- `crate_root` - Crate root directory
- `dead` - List of dead module names
- `mods` - All parsed modules (for parent lookup)
- `dry_run` - If true, only print what would be done

**Returns:**
```rust
pub struct FixResult {
    pub files_removed: Vec<String>,
    pub declarations_removed: Vec<String>,
    pub dirs_removed: Vec<String>,
    pub errors: Vec<String>,
}
```

**Example:**
```rust
// Dry run first
let result = fix_dead_modules(&root, &dead, &modules, true)?;
println!("Would remove {} files", result.files_removed.len());

// Actually fix
let result = fix_dead_modules(&root, &dead, &modules, false)?;
```

---

### `remove_file`

Safely remove a file with symlink protection.

```rust
pub fn remove_file(path: &Path, dry_run: bool) -> Result<bool>
```

**Security:** Refuses to delete symlinks to prevent symlink attacks.

---

### `remove_mod_declaration`

Remove `mod foo;` declaration from a parent file.

```rust
pub fn remove_mod_declaration(
    parent_path: &Path,
    child_name: &str,
    dry_run: bool
) -> Result<bool>
```

**Handles:**
- `mod foo;`
- `pub mod foo;`
- `pub(crate) mod foo;`
- `#[attr] mod foo;`

---

### `clean_empty_dirs`

Recursively clean up empty directories.

```rust
pub fn clean_empty_dirs(root: &Path, dry_run: bool) -> Result<Vec<String>>
```

**Safety:** Limited to 128 levels of recursion depth.

---

## Call Graph (`callgraph/`)

### `FunctionDef`

Represents a function definition.

```rust
pub struct FunctionDef {
    pub name: String,              // Simple name
    pub full_path: String,         // Qualified path (module::func)
    pub file: String,              // Source file path
    pub is_method: bool,           // Has self receiver
    pub parent_type: Option<String>, // Type for impl methods
    pub visibility: String,        // "pub", "pub(crate)", "private"
}
```

---

### `extract_callgraph_functions`

Extract function definitions from source.

```rust
pub fn extract_callgraph_functions(
    path: &Path,
    content: &str
) -> Vec<FunctionDef>
```

---

### `extract_call_usages`

Extract function call sites from source.

```rust
pub fn extract_call_usages(
    path: &Path,
    content: &str
) -> CallUsageResult
```

---

### `CallGraph`

Main call graph structure.

```rust
pub struct CallGraph {
    pub nodes: HashMap<String, FunctionDef>,
    pub edges: HashSet<(String, String)>,
    pub adjacency: HashMap<String, Vec<String>>,
    pub reverse_edges: HashMap<String, HashSet<String>>,
}

impl CallGraph {
    pub fn build(
        functions: &[FunctionDef],
        usage_map: &HashMap<String, CallUsageResult>
    ) -> Self;

    pub fn analyze(&self) -> CallGraphAnalysis;

    pub fn to_dot(&self) -> String;

    pub fn to_json(&self) -> serde_json::Value;
}
```

---

### `CallGraphAnalysis`

Analysis results from call graph.

```rust
pub struct CallGraphAnalysis {
    pub stats: CallGraphStats,
    pub unreachable: Vec<FunctionDef>,
    pub entry_points: Vec<String>,
}

pub struct CallGraphStats {
    pub total_functions: usize,
    pub reachable_functions: usize,
    pub dead_functions: usize,
    pub public_dead: usize,
    pub private_dead: usize,
}
```

---

## Path Resolution (`callgraph/path_resolver.rs`)

### `UseMap`

Maps import aliases to full paths.

```rust
pub type UseMap = HashMap<String, String>;

pub fn collect_use_statements(content: &str) -> UseMap
```

**Example:**
```rust
// For: use crate::db::query;
// UseMap: {"query" => "db::query"}
```

---

### `resolve_call_path`

Resolve a call site to possible function paths.

```rust
pub fn resolve_call_path(
    call: &str,
    usemap: &UseMap,
    ctx: &ModulePathContext,
) -> Vec<String>
```

---

### `ModulePathContext`

Context for path resolution.

```rust
pub struct ModulePathContext {
    pub module_path: String,  // Current module's path
    pub crate_name: String,   // Crate name
}
```

---

## Visualization

### `generate_html_graph`

Generate Canvas 2D interactive visualizer.

```rust
pub fn generate_html_graph(
    mods: &HashMap<String, ModuleInfo>,
    reachable: &HashSet<&str>,
) -> String
```

**Features:**
- Force-directed layout
- Module clustering
- Edge bundling (Bézier curves)
- Zoom/pan/drag
- Dead module highlighting (red)

---

### `generate_pixi_graph`

Generate WebGL visualizer using PixiJS.

```rust
pub fn generate_pixi_graph(
    mods: &HashMap<String, ModuleInfo>,
    reachable: &HashSet<&str>,
) -> String
```

**Features:**
- GPU-accelerated rendering
- Handles large graphs (1000+ nodes)
- Same interaction model as Canvas 2D

---

### `visualize`

Generate Graphviz DOT output.

```rust
pub fn visualize(
    mods: &HashMap<String, ModuleInfo>,
    reachable: &HashSet<&str>,
) -> String
```

---

## Workspace Support (`workspace.rs`)

### `is_workspace_root`

Check if directory is a Cargo workspace root.

```rust
pub fn is_workspace_root(root: &Path) -> bool
```

---

### `find_workspace_members`

Find all crate paths in a workspace.

```rust
pub fn find_workspace_members(workspace_root: &Path) -> Result<Vec<PathBuf>>
```

---

## Configuration (`config.rs`)

### `Config`

Configuration loaded from `deadmod.toml`.

```rust
pub struct Config {
    pub ignore: Vec<String>,
}

pub fn load_config(crate_root: &Path) -> Option<Config>
```

**Config file format:**
```toml
ignore = ["tests", "benches", "examples"]
```

---

## Logging (`logging.rs`)

### `setup_logging`

Initialize structured JSON logging.

```rust
pub fn setup_logging() -> Result<()>
```

**Environment:** Set `RUST_LOG=info` (or `debug`, `trace`)

**Output format:**
```json
{"level":"INFO","message":"Parsing 50 files...","timestamp":"2024-01-15T10:30:00Z"}
```

---

## Complete Example

```rust
use deadmod_core::*;
use std::path::Path;

fn main() -> anyhow::Result<()> {
    let crate_root = Path::new("./my-crate");

    // 1. Scan for files
    let files = gather_rs_files(crate_root)?;
    println!("Found {} .rs files", files.len());

    // 2. Parse with caching
    let cache = load_cache(crate_root);
    let modules = incremental_parse(crate_root, &files, cache)?;
    println!("Parsed {} modules", modules.len());

    // 3. Build dependency graph
    let graph = build_graph(&modules);

    // 4. Find entry points
    let roots = find_root_modules(crate_root);
    println!("Entry points: {:?}", roots);

    // 5. Find reachable modules
    let reachable = reachable_from_roots(
        &graph,
        roots.iter().map(String::as_str)
    );

    // 6. Detect dead modules
    let dead = find_dead(&modules, &reachable);

    if dead.is_empty() {
        println!("No dead modules found!");
    } else {
        println!("Dead modules:");
        for module in &dead {
            println!("  - {}", module);
        }

        // 7. Optionally fix
        // fix_dead_modules(crate_root, &dead, &modules, false)?;
    }

    // 8. Generate visualization
    let html = generate_html_graph(&modules, &reachable);
    std::fs::write("graph.html", html)?;

    Ok(())
}
```

---

## Error Handling

All functions use `anyhow::Result` for error propagation:

```rust
use anyhow::{Context, Result};

fn example() -> Result<()> {
    let files = gather_rs_files(Path::new("./crate"))
        .context("Failed to scan crate")?;
    Ok(())
}
```

**Error categories:**
| Category | Handling |
|----------|----------|
| File not found | `Result::Err` with context |
| Parse error | Warning printed, module skipped |
| Cache corruption | Cache ignored, fresh parse |
| Fix error | Logged, continues with other files |

---

## Thread Safety

All public APIs are thread-safe:
- Parallel scanning and parsing via Rayon
- Atomic cache writes
- No global mutable state

---

## Performance Tips

1. **Use caching** - `incremental_parse` is 10-100x faster on unchanged files
2. **Use multi-source BFS** - `reachable_from_roots` vs calling `reachable_from_root` N times
3. **Exclude directories** - Use `gather_rs_files_with_excludes` to skip test fixtures
4. **Release builds** - `cargo build --release` for production analysis

---

## Version Compatibility

- Rust edition: 2021
- MSRV: 1.70+
- Tested on: Linux, macOS, Windows
