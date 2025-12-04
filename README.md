# Deadmod

**NASA-grade dead code detection for Rust projects**

[![Tests](https://img.shields.io/badge/tests-360%20passing-brightgreen)]()
[![Rust](https://img.shields.io/badge/rust-2021%20edition-orange)]()
[![License](https://img.shields.io/badge/license-MIT-blue)]()

Deadmod is a comprehensive static analysis tool that detects unreachable code in Rust projects. It goes beyond simple unused code warnings to find modules, functions, traits, generics, macros, constants, enum variants, and match arms that are truly dead—never called from any entry point.

## Why Deadmod?

| Problem | Deadmod Solution |
|---------|------------------|
| `#[allow(dead_code)]` hides real issues | Graph-based reachability from entry points |
| Compiler warnings miss module-level dead code | Full module dependency analysis |
| No visibility into call graphs | Interactive visualizers (Canvas 2D, WebGL) |
| Manual dead code hunting is tedious | Automated detection with auto-fix |
| CI pipelines lack dead code checks | Exit code 1 on dead code found |

## Architecture Overview

```
┌─────────────────────────────────────────────────────────────────┐
│                         deadmod-cli                              │
│  ┌─────────────┐  ┌─────────────┐  ┌─────────────┐              │
│  │   Scanner   │  │   Parser    │  │  Analyzer   │              │
│  │  (Rayon)    │──│   (Syn)     │──│  (Graphs)   │              │
│  └─────────────┘  └─────────────┘  └─────────────┘              │
│         │                │                │                      │
│         ▼                ▼                ▼                      │
│  ┌─────────────────────────────────────────────────────────┐    │
│  │                    deadmod-core                          │    │
│  │  ┌────────┐ ┌────────┐ ┌────────┐ ┌────────┐ ┌────────┐ │    │
│  │  │Modules │ │Functions│ │Traits  │ │Generics│ │ Macros │ │    │
│  │  └────────┘ └────────┘ └────────┘ └────────┘ └────────┘ │    │
│  │  ┌────────┐ ┌────────┐ ┌────────┐ ┌────────┐ ┌────────┐ │    │
│  │  │Constants│ │ Enums  │ │ Match  │ │Callgraph│ │  Fix   │ │    │
│  │  └────────┘ └────────┘ └────────┘ └────────┘ └────────┘ │    │
│  └─────────────────────────────────────────────────────────┘    │
│                              │                                   │
│         ┌────────────────────┼────────────────────┐             │
│         ▼                    ▼                    ▼             │
│  ┌─────────────┐     ┌─────────────┐     ┌─────────────┐       │
│  │    JSON     │     │     DOT     │     │    HTML     │       │
│  │   Export    │     │  (Graphviz) │     │ Visualizer  │       │
│  └─────────────┘     └─────────────┘     └─────────────┘       │
└─────────────────────────────────────────────────────────────────┘
```

## Repository Structure

```
deadmod/
├── deadmod-cli/          # Command-line interface
│   └── src/main.rs       # CLI entry point (1200+ lines)
├── deadmod-core/         # Core analysis library
│   └── src/
│       ├── lib.rs        # Public API exports
│       ├── scan.rs       # File discovery (Rayon parallel)
│       ├── parse.rs      # AST parsing (Syn)
│       ├── graph.rs      # Module dependency graph
│       ├── detect.rs     # Dead code detection
│       ├── fix.rs        # Auto-removal of dead code
│       ├── cache.rs      # Incremental parsing cache
│       ├── callgraph/    # Function call graph analysis
│       ├── func/         # Dead function detection
│       ├── traits/       # Dead trait method detection
│       ├── generics/     # Unused generic parameter detection
│       ├── macros/       # Dead macro detection
│       ├── constants/    # Dead const/static detection
│       ├── enums/        # Dead enum variant detection
│       ├── matcharms/    # Dead match arm detection
│       ├── visualize*.rs # Graph visualizers
│       └── workspace.rs  # Cargo workspace support
├── deadmod-lsp/          # Language Server Protocol (experimental)
└── Cargo.toml            # Workspace manifest
```

## Features

### Detection Modes

| Mode | Flag | Detects |
|------|------|---------|
| Modules | (default) | Unreachable `mod foo;` declarations |
| Functions | `--dead-func` | Uncalled functions and methods |
| Traits | `--dead-traits` | Unused trait methods |
| Generics | `--dead-generics` | Unused type parameters and lifetimes |
| Macros | `--dead-macros` | Unused `macro_rules!` definitions |
| Constants | `--dead-constants` | Unused `const` and `static` items |
| Variants | `--dead-variants` | Unused enum variants |
| Match Arms | `--dead-match-arms` | Unreachable match patterns |

### Output Formats

| Format | Flag | Use Case |
|--------|------|----------|
| Plain text | (default) | Human readable |
| JSON | `--json` | CI/CD pipelines, tooling |
| DOT | `--dot` | Graphviz visualization |
| HTML Canvas | `--html` | Interactive web visualizer |
| HTML WebGL | `--html-pixi` | GPU-accelerated large graphs |

## Quick Start

### Installation

```bash
# Clone and build
git clone https://github.com/anthropics/deadmod
cd deadmod
cargo build --release

# Add to PATH
export PATH="$PATH:$(pwd)/target/release"
```

### Basic Usage

```bash
# Analyze current directory
deadmod .

# Analyze specific crate
deadmod ./my-crate

# JSON output for CI
deadmod . --json

# Auto-fix (remove dead modules)
deadmod . --fix

# Dry-run (show what would be removed)
deadmod . --fix-dry-run
```

### Detection Examples

```bash
# Find dead functions
deadmod . --dead-func

# Find unused trait methods
deadmod . --dead-traits

# Find unused generics
deadmod . --dead-generics

# Find unused macros
deadmod . --dead-macros

# Find unused constants
deadmod . --dead-constants

# Find unused enum variants
deadmod . --dead-variants

# Find dead match arms
deadmod . --dead-match-arms
```

### Visualization

```bash
# Generate interactive HTML graph
deadmod . --html-file graph.html

# Generate WebGL graph (for large codebases)
deadmod . --html-pixi-file graph_webgl.html

# Generate Graphviz DOT
deadmod . --dot --dot-file deps.dot
dot -Tpng deps.dot -o deps.png
```

### Call Graph Analysis

```bash
# Generate function call graph (JSON)
deadmod . --callgraph

# Generate call graph (DOT)
deadmod . --callgraph-dot

# Export for visualizer
deadmod . --export-callgraph callgraph.json
deadmod . --export-modgraph modules.json
deadmod . --export-combined combined.json
```

### Workspace Support

```bash
# Analyze entire Cargo workspace
deadmod . --workspace
```

## Configuration

Create `deadmod.toml` in your crate root:

```toml
# Modules to ignore
ignore = [
    "tests",
    "benches",
    "examples",
    "_hidden",
]
```

Or use CLI flags:

```bash
deadmod . --ignore tests --ignore benches
```

## CI/CD Integration

### GitHub Actions

```yaml
- name: Check for dead code
  run: |
    cargo install --path .
    deadmod . --json > dead-code.json
    if [ -s dead-code.json ]; then
      echo "Dead code found!"
      cat dead-code.json
      exit 1
    fi
```

### Exit Codes

| Code | Meaning |
|------|---------|
| 0 | No dead code found |
| 1 | Dead code detected |
| 2 | Internal error (panic) |

## Performance

| Metric | Value |
|--------|-------|
| Parallel parsing | Rayon work-stealing |
| Incremental caching | SHA-256 file hashing |
| Graph traversal | O(V + E) BFS |
| Memory | ~50MB for 100K LOC |

### Benchmarks (deadmod analyzing itself)

```
Files scanned:     51
Modules parsed:    48
Cache hit rate:    95% (warm)
Analysis time:     0.4s
```

## API Overview

```rust
use deadmod_core::{
    // Scanning
    gather_rs_files,

    // Parsing
    parse_modules, ModuleInfo,

    // Graph building
    build_graph, reachable_from_roots,

    // Detection
    find_dead,

    // Auto-fix
    fix_dead_modules,

    // Call graph
    CallGraph, extract_callgraph_functions, extract_call_usages,

    // Visualization
    generate_html_graph, generate_pixi_graph, visualize,
};
```

## Documentation

- [Architecture](ARCHITECTURE.md) - System design and internals
- [Developer Guide](DEVELOPER_GUIDE.md) - Setup and contribution
- [CLI Reference](CLI_REFERENCE.md) - Complete command documentation
- [API Reference](API_REFERENCE.md) - Library documentation
- [Testing](TESTING.md) - Test strategy and coverage
- [Security](SECURITY.md) - Security considerations
- [Contributing](CONTRIBUTING.md) - How to contribute

## License

MIT License - see [LICENSE](LICENSE) for details.

## Acknowledgments

Built with:
- [syn](https://crates.io/crates/syn) - Rust AST parsing
- [rayon](https://crates.io/crates/rayon) - Parallel iteration
- [serde](https://crates.io/crates/serde) - Serialization
- [clap](https://crates.io/crates/clap) - CLI argument parsing
- [anyhow](https://crates.io/crates/anyhow) - Error handling
