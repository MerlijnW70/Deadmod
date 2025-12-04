# Contributing to Deadmod

Thank you for your interest in contributing to deadmod! This document provides guidelines and workflows for contributing.

## Code of Conduct

- Be respectful and inclusive
- Focus on constructive feedback
- Help others learn and grow

## Getting Started

### Prerequisites

- Rust 1.70+ (2021 edition)
- Cargo
- Git

### Setup

```bash
# Fork and clone
git clone https://github.com/YOUR_USERNAME/deadmod.git
cd deadmod

# Add upstream remote
git remote add upstream https://github.com/anthropics/deadmod.git

# Build
cargo build

# Run tests
cargo test

# Run clippy
cargo clippy
```

---

## Ways to Contribute

### 1. Report Bugs

Open an issue with:
- Clear title describing the bug
- Steps to reproduce
- Expected vs actual behavior
- Environment (OS, Rust version)
- Minimal reproduction case if possible

### 2. Suggest Features

Open an issue with:
- Clear description of the feature
- Use case / motivation
- Proposed implementation (optional)

### 3. Improve Documentation

- Fix typos or unclear explanations
- Add examples
- Improve API documentation
- Translate documentation

### 4. Submit Code

- Bug fixes
- New detection modes
- Performance improvements
- Test coverage

---

## Development Workflow

### 1. Create a Branch

```bash
# Sync with upstream
git fetch upstream
git checkout main
git merge upstream/main

# Create feature branch
git checkout -b feature/my-feature
# Or for bugs: fix/issue-123
```

### 2. Make Changes

Follow the coding conventions (see below).

### 3. Test Your Changes

```bash
# Run all tests
cargo test

# Run specific test
cargo test test_name

# Run with verbose output
cargo test -- --nocapture

# Run clippy
cargo clippy -- -D warnings

# Check formatting
cargo fmt -- --check
```

### 4. Commit

Use conventional commits:

```
feat: add dead lifetime detection
fix: handle unicode in module names
docs: improve API documentation
test: add edge cases for parser
refactor: simplify graph traversal
perf: parallelize file scanning
```

Example:
```bash
git add .
git commit -m "feat: add dead lifetime detection

- Extract lifetime parameters from functions and structs
- Track lifetime usage in type annotations
- Report unused lifetimes with source location

Closes #42"
```

### 5. Push and Create PR

```bash
git push origin feature/my-feature
```

Then open a Pull Request on GitHub with:
- Clear title
- Description of changes
- Link to related issue
- Test plan

---

## Coding Conventions

### Rust Style

Follow the [Rust API Guidelines](https://rust-lang.github.io/api-guidelines/).

```rust
// Good: descriptive names, clear purpose
pub fn find_dead_modules(
    modules: &HashMap<String, ModuleInfo>,
    reachable: &HashSet<&str>,
) -> Vec<&str>

// Bad: unclear abbreviations
pub fn fd(m: &HashMap<String, ModuleInfo>, r: &HashSet<&str>) -> Vec<&str>
```

### Error Handling

Use `anyhow` for error propagation with context:

```rust
use anyhow::{Context, Result};

fn parse_file(path: &Path) -> Result<Module> {
    let content = fs::read_to_string(path)
        .with_context(|| format!("Failed to read: {}", path.display()))?;

    // Parse content...
    Ok(module)
}
```

Never use `.unwrap()` in library code:

```rust
// Bad
let value = option.unwrap();

// Good
let value = option.context("Missing required value")?;
// Or
let value = option.unwrap_or_default();
```

### Documentation

Document public APIs:

```rust
/// Finds modules not reachable from any entry point.
///
/// # Arguments
///
/// * `modules` - All parsed modules in the crate
/// * `reachable` - Set of module names reachable from entry points
///
/// # Returns
///
/// List of dead module names.
///
/// # Example
///
/// ```
/// let dead = find_dead(&modules, &reachable);
/// for name in dead {
///     println!("Dead: {}", name);
/// }
/// ```
pub fn find_dead<'a>(
    modules: &'a HashMap<String, ModuleInfo>,
    reachable: &HashSet<&str>,
) -> Vec<&'a str>
```

### Testing

Write tests for new functionality:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_find_dead_basic() {
        // Arrange
        let mut modules = HashMap::new();
        modules.insert("main".to_string(), ModuleInfo::new(/* ... */));
        modules.insert("dead".to_string(), ModuleInfo::new(/* ... */));

        let reachable: HashSet<&str> = ["main"].into_iter().collect();

        // Act
        let dead = find_dead(&modules, &reachable);

        // Assert
        assert_eq!(dead.len(), 1);
        assert!(dead.contains(&"dead"));
    }

    #[test]
    fn test_find_dead_empty() {
        let modules = HashMap::new();
        let reachable = HashSet::new();

        let dead = find_dead(&modules, &reachable);

        assert!(dead.is_empty());
    }
}
```

---

## Adding a New Detection Mode

### 1. Create Module Structure

```bash
mkdir -p deadmod-core/src/mytype
```

### 2. Implement Extractor

`deadmod-core/src/mytype/mytype_extractor.rs`:

```rust
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
    // Extract items from AST...
    items
}
```

### 3. Implement Usage Tracker

`deadmod-core/src/mytype/mytype_usage.rs`:

```rust
use std::collections::HashSet;
use std::path::Path;

pub fn extract_mytype_usages(path: &Path, content: &str) -> HashSet<String> {
    let mut usages = HashSet::new();
    // Extract usage sites from AST...
    usages
}
```

### 4. Implement Graph Builder

`deadmod-core/src/mytype/mytype_graph.rs`:

```rust
use std::collections::HashSet;

pub struct MyTypeGraph {
    items: Vec<MyTypeItem>,
    usages: HashSet<String>,
}

impl MyTypeGraph {
    pub fn new(items: Vec<MyTypeItem>, usages: HashSet<String>) -> Self {
        Self { items, usages }
    }

    pub fn find_dead(&self) -> Vec<&MyTypeItem> {
        self.items
            .iter()
            .filter(|item| !self.usages.contains(&item.name))
            .collect()
    }
}
```

### 5. Export from mod.rs

`deadmod-core/src/mytype/mod.rs`:

```rust
mod mytype_extractor;
mod mytype_graph;
mod mytype_usage;

pub use mytype_extractor::*;
pub use mytype_graph::*;
pub use mytype_usage::*;
```

### 6. Add to lib.rs

```rust
pub mod mytype;
pub use mytype::*;
```

### 7. Add CLI Flag

`deadmod-cli/src/main.rs`:

```rust
#[derive(Parser, Debug)]
pub struct Cli {
    // ... existing flags ...

    /// Detect dead mytypes
    #[arg(long)]
    dead_mytypes: bool,
}
```

### 8. Add Handler

```rust
if cli.dead_mytypes {
    // Run mytype detection
}
```

### 9. Add Tests

Add comprehensive tests for all edge cases.

### 10. Update Documentation

- Update README.md feature table
- Add CLI_REFERENCE.md section
- Add API_REFERENCE.md section

---

## Pull Request Checklist

Before submitting a PR, ensure:

- [ ] Code compiles without warnings (`cargo build`)
- [ ] All tests pass (`cargo test`)
- [ ] Clippy passes (`cargo clippy -- -D warnings`)
- [ ] Code is formatted (`cargo fmt`)
- [ ] New code has tests
- [ ] Public APIs are documented
- [ ] CHANGELOG is updated (for user-facing changes)
- [ ] PR description explains what and why

---

## Review Process

1. **CI checks** - All tests and lints must pass
2. **Code review** - At least one maintainer approval
3. **Address feedback** - Respond to comments, make changes
4. **Merge** - Squash and merge to main

---

## Release Process

Releases follow semantic versioning:
- **MAJOR**: Breaking API changes
- **MINOR**: New features, backwards compatible
- **PATCH**: Bug fixes, backwards compatible

---

## Getting Help

- Open a GitHub issue for questions
- Check existing issues and PRs
- Read the documentation

---

## Recognition

Contributors are recognized in:
- Git commit history
- Release notes
- README acknowledgments (for significant contributions)

Thank you for contributing to deadmod!
