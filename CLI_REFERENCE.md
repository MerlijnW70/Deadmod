# CLI Reference

Complete command-line reference for deadmod.

## Synopsis

```
deadmod [OPTIONS] [PATH]
```

## Arguments

| Argument | Default | Description |
|----------|---------|-------------|
| `PATH` | `.` | Path to Rust crate or workspace root |

## Global Options

| Flag | Description |
|------|-------------|
| `--help` | Print help information |
| `--version` | Print version |

## Output Options

| Flag | Description |
|------|-------------|
| `--json` | Output results in JSON format |
| `--dot` | Generate Graphviz DOT output |
| `--dot-file <FILE>` | Write DOT to file instead of stdout |
| `--html` | Generate interactive HTML Canvas visualization |
| `--html-file <FILE>` | Write HTML Canvas to file |
| `--html-pixi` | Generate PixiJS WebGL visualization |
| `--html-pixi-file <FILE>` | Write PixiJS HTML to file |

## Detection Modes

### Module Detection (Default)

```bash
deadmod .
```

Detects unreachable `mod foo;` declarations.

**Output (plain)**:
```
DEAD MODULES:
  unused_module
  deprecated_feature
```

**Output (JSON)**:
```json
{
  "dead_modules": ["unused_module", "deprecated_feature"]
}
```

---

### Function Detection

```bash
deadmod . --dead-func
```

Detects uncalled functions and methods.

**Output (plain)**:
```
=== Dead Function Analysis ===

Total functions: 150
Reachable:       142
Dead:            8
  - Public:      2
  - Private:     6

DEAD FUNCTIONS:
  [priv] utils::deprecated_helper (src/utils.rs)
  [pub] api::unused_endpoint (src/api.rs)
```

**Output (JSON)**:
```json
{
  "total_functions": 150,
  "reachable_functions": 142,
  "dead_functions": 8,
  "public_dead": 2,
  "private_dead": 6,
  "dead": [
    {
      "name": "deprecated_helper",
      "full_path": "utils::deprecated_helper",
      "visibility": "private",
      "file": "src/utils.rs",
      "is_method": false
    }
  ]
}
```

---

### Trait Method Detection

```bash
deadmod . --dead-traits
```

Detects unused trait methods and impl methods.

**Output (plain)**:
```
=== Dead Trait Method Analysis ===

Total trait methods:  25
  - Required:         15
  - Provided:         10
Total impl methods:   40

Dead trait methods:   3
Dead impl methods:    5

DEAD TRAIT METHODS:
  [provided] MyTrait::unused_method (src/traits.rs)

DEAD IMPL METHODS:
  impl Handler for MyType :: deprecated_handler (src/handlers.rs)
```

---

### Generic Parameter Detection

```bash
deadmod . --dead-generics
```

Detects unused type parameters, lifetimes, and const generics.

**Output (plain)**:
```
=== Dead Generic Parameter Analysis ===

Declared type parameters:     12
Declared lifetimes:           5
Declared const parameters:    2

Dead type parameters:         2
Dead lifetimes:               1
Dead const parameters:        0

DEAD GENERIC PARAMETERS:
  [type] T in MyStruct (src/lib.rs)
  [lifetime] 'unused in process (src/lib.rs)
```

---

### Macro Detection

```bash
deadmod . --dead-macros
```

Detects unused `macro_rules!` definitions.

**Output (plain)**:
```
=== Dead Macro Analysis ===

Total macros declared:  8
  - Exported:           3

Dead macros:            2
  - Exported dead:      1

DEAD MACROS:
  [exported] debug_print (src/macros.rs)
  [local] internal_helper (src/lib.rs)
```

---

### Constant Detection

```bash
deadmod . --dead-constants
```

Detects unused `const` and `static` items.

**Output (plain)**:
```
=== Dead Constants/Statics Analysis ===

Total declared:     20
  - Constants:      15
  - Statics:        5

Dead count:         3
  - Dead consts:    2
  - Dead statics:   1

DEAD CONSTANTS/STATICS:
  [pub] const DEPRECATED_VALUE (src/config.rs)
  [priv] static UNUSED_BUFFER (src/buffer.rs)
```

---

### Enum Variant Detection

```bash
deadmod . --dead-variants
```

Detects unused enum variants.

**Output (plain)**:
```
=== Dead Enum Variant Analysis ===

Total enums:        10
Total variants:     45

Dead variants:      5
Fully dead enums:   1

DEAD ENUM VARIANTS:
  [pub] Status::Deprecated (src/status.rs)
  [priv] Error::LegacyError (src/error.rs)
```

---

### Match Arm Detection

```bash
deadmod . --dead-match-arms
```

Detects unreachable match patterns and wildcard masking.

**Output (plain)**:
```
=== Dead Match Arm Analysis ===

Total match expressions: 25
Total arms:              120
Wildcard arms:           15

Dead/Masked arms:        3

DEAD/MASKED MATCH ARMS:
  [masked] Status::Active (src/handler.rs)
  [non-final-wildcard] _ (src/parser.rs)
```

---

## Call Graph Options

### JSON Call Graph

```bash
deadmod . --callgraph
```

Output function call graph in JSON format.

### DOT Call Graph

```bash
deadmod . --callgraph-dot
```

Output call graph in Graphviz DOT format.

### Visualizer Format

```bash
deadmod . --callgraph-viz
```

Output call graph in visualizer-compatible JSON (numeric IDs, dead flags).

### Module Graph for Visualizer

```bash
deadmod . --modgraph-viz
```

Output module dependency graph in visualizer format.

---

## Export Options

### Export Call Graph

```bash
deadmod . --export-callgraph callgraph.json
```

Export function call graph to JSON file.

### Export Module Graph

```bash
deadmod . --export-modgraph modules.json
```

Export module dependency graph to JSON file.

### Export Combined

```bash
deadmod . --export-combined combined.json
```

Export both graphs in a single JSON file:
```json
{
  "module_graph": { ... },
  "function_graph": { ... }
}
```

---

## Auto-Fix Options

### Fix (Destructive)

```bash
deadmod . --fix
```

**Warning**: This permanently deletes files!

Actions performed:
1. Delete dead module `.rs` files
2. Remove `mod foo;` declarations from parent files
3. Clean up empty directories

### Dry Run

```bash
deadmod . --fix-dry-run
```

Show what would be removed without making changes.

**Output**:
```
[DRY RUN] Would remove: src/deprecated.rs
[DRY RUN] Would remove 'mod deprecated;' from src/lib.rs
```

---

## Workspace Options

### Analyze Workspace

```bash
deadmod . --workspace
```

Analyze all crates in a Cargo workspace.

**Output (plain)**:
```
=== Crate: deadmod-core ===
No dead modules found.

=== Crate: deadmod-cli ===
  - unused_command

=== Crate: deadmod-lsp ===
No dead modules found.
```

---

## Filtering Options

### Ignore Modules

```bash
deadmod . --ignore tests --ignore benches
```

Exclude modules matching patterns from analysis.

Matching rules:
- Exact match: `--ignore foo` matches `foo`
- Suffix match: `--ignore _test` matches `my_test`
- Contains match: `--ignore mock` matches `my_mock_data`

---

## Exit Codes

| Code | Meaning |
|------|---------|
| `0` | Success - no dead code found |
| `1` | Dead code detected |
| `2` | Internal error (panic) |

---

## Examples

### CI Pipeline Check

```bash
#!/bin/bash
deadmod . --json > /tmp/dead.json
if [ $(jq '.dead_modules | length' /tmp/dead.json) -gt 0 ]; then
    echo "Dead modules found!"
    jq '.dead_modules[]' /tmp/dead.json
    exit 1
fi
```

### Generate Full Report

```bash
# All detection modes
deadmod . --json > modules.json
deadmod . --dead-func --json > functions.json
deadmod . --dead-traits --json > traits.json
deadmod . --dead-generics --json > generics.json
deadmod . --dead-macros --json > macros.json
deadmod . --dead-constants --json > constants.json
deadmod . --dead-variants --json > variants.json
```

### Generate Visualizations

```bash
# Module graph
deadmod . --html-file modules.html
deadmod . --dot-file modules.dot

# Call graph
deadmod . --callgraph-dot > callgraph.dot
dot -Tsvg callgraph.dot -o callgraph.svg

# Combined export for external visualizer
deadmod . --export-combined project.json
```

### Clean Up Dead Code

```bash
# Preview changes
deadmod . --fix-dry-run

# Apply changes
deadmod . --fix

# Verify
deadmod .  # Should show no dead modules
```

### Workspace Analysis

```bash
# Analyze entire workspace
deadmod . --workspace --json > workspace-report.json

# Analyze specific crate
deadmod ./crates/my-crate
```

---

## Configuration File

Create `deadmod.toml` in crate root:

```toml
# Modules to ignore during analysis
ignore = [
    "tests",
    "benches",
    "examples",
    "fixtures",
    "_*",  # Underscore-prefixed modules
]
```

Configuration is merged with CLI flags (CLI takes precedence).

---

## Environment Variables

| Variable | Description |
|----------|-------------|
| `RUST_LOG` | Enable structured logging (e.g., `RUST_LOG=info`) |

**Log Output** (JSON to stderr):
```json
{"level":"INFO","message":"Parsing 50 files...","timestamp":"2024-01-15T10:30:00Z"}
```
