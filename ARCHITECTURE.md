# Architecture

This document describes the internal architecture of deadmod, a NASA-grade dead code detection system for Rust.

## System Overview

```
┌─────────────────────────────────────────────────────────────────────────┐
│                              USER INTERFACE                              │
├─────────────────────────────────────────────────────────────────────────┤
│  deadmod-cli          │  deadmod-lsp (experimental)                      │
│  - Command parsing    │  - LSP protocol                                  │
│  - Mode dispatch      │  - Real-time diagnostics                         │
│  - Output formatting  │  - Editor integration                            │
└───────────┬───────────┴──────────────────────────────────────────────────┘
            │
            ▼
┌─────────────────────────────────────────────────────────────────────────┐
│                             deadmod-core                                 │
├─────────────────────────────────────────────────────────────────────────┤
│                                                                          │
│  ┌──────────────┐    ┌──────────────┐    ┌──────────────┐               │
│  │   SCANNING   │───▶│   PARSING    │───▶│   ANALYSIS   │               │
│  │   scan.rs    │    │   parse.rs   │    │   detect.rs  │               │
│  │   (Rayon)    │    │   (Syn AST)  │    │   (Graphs)   │               │
│  └──────────────┘    └──────────────┘    └──────────────┘               │
│         │                   │                   │                        │
│         │                   │                   │                        │
│         ▼                   ▼                   ▼                        │
│  ┌──────────────┐    ┌──────────────┐    ┌──────────────┐               │
│  │    CACHE     │    │  EXTRACTORS  │    │    GRAPHS    │               │
│  │   cache.rs   │    │  (8 types)   │    │   graph.rs   │               │
│  │  (SHA-256)   │    │              │    │  callgraph/  │               │
│  └──────────────┘    └──────────────┘    └──────────────┘               │
│                                                 │                        │
│                                                 ▼                        │
│                           ┌─────────────────────────────────────┐       │
│                           │            OUTPUT                    │       │
│                           │  report.rs │ visualize*.rs │ fix.rs │       │
│                           └─────────────────────────────────────┘       │
│                                                                          │
└─────────────────────────────────────────────────────────────────────────┘
```

## Core Subsystems

### 1. File Scanner (`scan.rs`)

**Purpose**: Discover all `.rs` files in a crate.

**Algorithm**:
```
gather_rs_files(root: Path) -> Vec<PathBuf>
  1. Use walkdir to traverse directory tree
  2. Filter for .rs extension
  3. Parallelize with Rayon par_bridge()
  4. Return sorted file list
```

**Complexity**: O(n) where n = total files in directory tree

**Key Functions**:
- `gather_rs_files(root: &Path) -> Result<Vec<PathBuf>>`

---

### 2. AST Parser (`parse.rs`)

**Purpose**: Parse Rust source files into module dependency information.

**Data Flow**:
```
┌─────────────┐    ┌─────────────┐    ┌─────────────┐
│  .rs file   │───▶│  syn::File  │───▶│ ModuleInfo  │
│  (String)   │    │    (AST)    │    │  (refs)     │
└─────────────┘    └─────────────┘    └─────────────┘
```

**ModuleInfo Structure**:
```rust
pub struct ModuleInfo {
    pub path: PathBuf,     // Source file path
    pub name: String,      // Module name (file stem)
    pub refs: HashSet<String>,  // Referenced modules
}
```

**Extraction Strategy**:
1. Parse `mod foo;` declarations → external module refs
2. Parse `use crate::foo` statements → internal refs
3. Skip inline modules `mod foo { }` (not external)
4. Skip path keywords: `self`, `super`, `crate`

**Parallelism**: Files parsed in parallel via Rayon

---

### 3. Incremental Cache (`cache.rs`)

**Purpose**: Avoid re-parsing unchanged files.

**Cache Structure**:
```rust
pub struct DeadmodCache {
    pub modules: HashMap<String, CachedModule>,
}

pub struct CachedModule {
    pub hash: String,           // SHA-256 of file content
    pub refs: HashSet<String>,  // Cached module references
}
```

**Cache Location**: `.deadmod/cache.json`

**Algorithm**:
```
incremental_parse(files, cached):
  FOR EACH file IN files (parallel):
    hash = SHA256(file_content)
    IF cached[file].hash == hash:
      USE cached[file].refs
    ELSE:
      PARSE file
      UPDATE cache
  SAVE cache atomically
```

**Atomic Writes**: Uses temp file + rename pattern to prevent corruption.

---

### 4. Dependency Graph (`graph.rs`)

**Purpose**: Build directed graph of module dependencies.

**Graph Structure**:
```
Nodes: Module names (strings)
Edges: A → B means "A depends on B" (A has `mod B;` or `use B`)
```

**Adjacency Representation**:
```rust
HashMap<String, HashSet<String>>  // module → dependencies
```

**Key Operations**:
- `build_graph(mods)` - O(|M| + |E|) construction
- `reachable_from_roots(graph, roots)` - O(|V| + |E|) BFS

---

### 5. Root Module Detection (`root.rs`)

**Purpose**: Find entry points for reachability analysis.

**Entry Points**:
| File | Module Name |
|------|-------------|
| `src/main.rs` | `main` |
| `src/lib.rs` | `lib` |
| `src/bin/*.rs` | `<filename>` |
| `src/bin/*/main.rs` | `<dirname>` |

**Algorithm**:
```
find_root_modules(crate_root):
  roots = []
  IF exists(src/main.rs): roots.push("main")
  IF exists(src/lib.rs): roots.push("lib")
  FOR file IN src/bin/*.rs: roots.push(file.stem)
  FOR dir IN src/bin/*/:
    IF exists(dir/main.rs): roots.push(dir.name)
  RETURN roots
```

---

### 6. Dead Code Detection (`detect.rs`)

**Purpose**: Find modules unreachable from entry points.

**Algorithm**:
```
find_dead(all_modules, reachable):
  dead = []
  FOR module IN all_modules:
    IF module NOT IN reachable:
      dead.push(module)
  RETURN dead
```

**Complexity**: O(|M|) where M = modules

---

### 7. Call Graph Engine (`callgraph/`)

**Purpose**: Build function-level call graph for dead function detection.

**Module Structure**:
```
callgraph/
├── mod.rs           # Public API
├── extractor.rs     # Function definition extraction
├── usage.rs         # Call site extraction
├── graph.rs         # Graph building and analysis
└── path_resolver.rs # Semantic path resolution
```

**Function Definition Extraction** (`extractor.rs`):
```rust
pub struct FunctionDef {
    pub name: String,        // Simple name
    pub full_path: String,   // Qualified path
    pub file: String,        // Source file
    pub is_method: bool,     // Has self receiver
    pub parent_type: Option<String>,
    pub visibility: String,
}
```

**Call Extraction** (`usage.rs`):
- `Expr::Call` - Direct calls: `foo()`
- `Expr::MethodCall` - Method calls: `x.method()`
- `Expr::Path` - Function references

**Path Resolution** (`path_resolver.rs`):
```
use crate::db::query;
query()  →  resolves to "db::query"
```

**Graph Building** (`graph.rs`):
```rust
pub struct CallGraph {
    pub nodes: HashMap<String, FunctionDef>,
    pub edges: HashSet<(String, String)>,
    pub adjacency: HashMap<String, Vec<String>>,
    pub reverse_edges: HashMap<String, HashSet<String>>,
}
```

---

### 8. Detection Subsystems

Each detection type follows the same pattern:

```
┌─────────────┐    ┌─────────────┐    ┌─────────────┐
│  Extractor  │───▶│    Graph    │───▶│   Analyze   │
│  (AST→Data) │    │  (Build)    │    │  (Find Dead)│
└─────────────┘    └─────────────┘    └─────────────┘
```

| Subsystem | Directory | Detects |
|-----------|-----------|---------|
| Functions | `func/` | Uncalled functions |
| Traits | `traits/` | Unused trait methods |
| Generics | `generics/` | Unused type params |
| Macros | `macros/` | Unused macro_rules! |
| Constants | `constants/` | Unused const/static |
| Enums | `enums/` | Unused variants |
| Match Arms | `matcharms/` | Unreachable patterns |

---

### 9. Auto-Fix System (`fix.rs`)

**Purpose**: Safely remove dead code.

**Operations**:
1. Remove `.rs` files for dead modules
2. Remove `mod foo;` declarations from parent files
3. Clean up empty directories
4. Collapse excessive blank lines

**Safety Measures**:
- Symlink detection (refuses to delete symlinks)
- Dry-run mode (`--fix-dry-run`)
- Regex-based declaration removal
- Atomic file operations

**Recursion Protection**: Maximum depth of 100 for directory cleanup.

---

### 10. Visualization (`visualize*.rs`)

**Three Output Formats**:

| Format | File | Technology |
|--------|------|------------|
| DOT | `visualize.rs` | Graphviz |
| Canvas 2D | `visualize_html.rs` | HTML5 Canvas |
| WebGL | `visualize_pixi.rs` | PixiJS |

**HTML Visualizer Features**:
- Force-directed layout
- Module clustering
- Edge bundling (Bézier curves)
- Inspector panel
- Zoom/pan/drag
- Dead module highlighting (red)

---

## Data Flow Diagrams

### Module Analysis Flow

```
┌──────────┐    ┌──────────┐    ┌──────────┐    ┌──────────┐
│  Crate   │───▶│  Scan    │───▶│  Parse   │───▶│  Build   │
│  Root    │    │  Files   │    │  ASTs    │    │  Graph   │
└──────────┘    └──────────┘    └──────────┘    └──────────┘
                                                      │
┌──────────┐    ┌──────────┐    ┌──────────┐         │
│  Output  │◀───│  Find    │◀───│  BFS     │◀────────┘
│  Report  │    │  Dead    │    │  Reach   │
└──────────┘    └──────────┘    └──────────┘
```

### Function Analysis Flow

```
┌──────────┐    ┌──────────┐    ┌──────────┐    ┌──────────┐
│  Files   │───▶│ Extract  │───▶│ Extract  │───▶│  Build   │
│          │    │ FuncDefs │    │  Calls   │    │CallGraph │
└──────────┘    └──────────┘    └──────────┘    └──────────┘
                                                      │
┌──────────┐    ┌──────────┐    ┌──────────┐         │
│  Report  │◀───│  Find    │◀───│  Find    │◀────────┘
│          │    │  Dead    │    │  Entry   │
└──────────┘    └──────────┘    └──────────┘
```

---

## Memory Model

### Heap Allocations

| Structure | Size Estimate | Lifetime |
|-----------|---------------|----------|
| `Vec<PathBuf>` files | O(F) | Scan phase |
| `HashMap<String, ModuleInfo>` | O(M × R) | Full analysis |
| Graph adjacency | O(M + E) | Analysis phase |
| Cache | O(M) | Persistent |

Where:
- F = number of files
- M = number of modules
- R = average refs per module
- E = number of edges

### Stack Usage

- Recursive AST visitors: bounded by `syn` visitor depth
- BFS queue: O(M) worst case
- Directory cleanup: max depth 100 (hardcoded limit)

---

## Error Handling Strategy

### Resilience Levels

| Level | Strategy | Example |
|-------|----------|---------|
| Fatal | Return `Err` | Can't find crate root |
| Recoverable | Skip + warn | Malformed .rs file |
| Silent | Use default | Cache load failure |

### Error Propagation

```rust
// NASA-grade: explicit context at every level
fn find_crate_root(path: &Path) -> Result<PathBuf> {
    path.canonicalize()
        .with_context(|| format!("Failed to canonicalize: {}", path.display()))?
}
```

### Panic Prevention

- Global panic hook in CLI captures panics
- Exit code 2 on panic (distinguishes from dead code found)
- No `.unwrap()` in production paths (use `.expect()` with message)

---

## Performance Characteristics

### Time Complexity

| Operation | Complexity | Notes |
|-----------|------------|-------|
| File scan | O(F) | Parallel |
| Parse all | O(F × L) | Parallel, L=lines |
| Build graph | O(M + E) | Linear |
| BFS reachability | O(M + E) | Single traversal |
| Find dead | O(M) | Set difference |

### Space Complexity

| Phase | Memory | Notes |
|-------|--------|-------|
| Scanning | O(F) | File paths |
| Parsing | O(M × R) | Module refs |
| Graph | O(M + E) | Adjacency list |
| Output | O(D) | Dead modules |

### Parallelism

```
┌─────────────────────────────────────────────┐
│  Thread Pool (Rayon)                        │
│  ┌─────┐ ┌─────┐ ┌─────┐ ┌─────┐ ┌─────┐   │
│  │ T1  │ │ T2  │ │ T3  │ │ T4  │ │ ... │   │
│  └──┬──┘ └──┬──┘ └──┬──┘ └──┬──┘ └──┬──┘   │
│     │       │       │       │       │       │
│     ▼       ▼       ▼       ▼       ▼       │
│  ┌─────────────────────────────────────┐   │
│  │      Work-Stealing Queue            │   │
│  │  [file1] [file2] [file3] [file4]... │   │
│  └─────────────────────────────────────┘   │
└─────────────────────────────────────────────┘
```

---

## Safety Considerations

### File System Safety

1. **Symlink attacks**: `remove_file()` checks `symlink_metadata()` first
2. **Path traversal**: Operations bounded to crate root
3. **Atomic writes**: Cache uses temp file + rename

### Concurrency Safety

1. **File access**: Read-only during analysis
2. **Cache writes**: Atomic via rename
3. **Thread safety**: All parallel ops are embarrassingly parallel

### Resource Limits

1. **Recursion depth**: 100 max for directory cleanup
2. **File size**: No explicit limit (bounded by available memory)
3. **Graph size**: No limit (tested up to 500 modules)

---

## Extension Points

### Adding a New Detection Type

1. Create directory: `src/<type>/`
2. Implement extractor: `<type>_extractor.rs`
3. Implement graph: `<type>_graph.rs`
4. Implement usage: `<type>_usage.rs`
5. Add to `lib.rs` exports
6. Add CLI flag in `deadmod-cli/src/main.rs`

### Adding a New Output Format

1. Create function: `generate_<format>(mods, reachable) -> String`
2. Add to `lib.rs` exports
3. Add CLI flags: `--<format>`, `--<format>-file`

### Adding a New Visualization

1. Create file: `visualize_<name>.rs`
2. Implement: `generate_<name>_graph(mods, reachable) -> String`
3. Add module and export to `lib.rs`
4. Add CLI flags
