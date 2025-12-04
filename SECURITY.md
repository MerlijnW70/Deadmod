# Security

This document describes the security considerations, threat model, and safeguards implemented in deadmod.

## Threat Model

Deadmod is a static analysis tool that:
- Reads Rust source files
- Parses them using the `syn` crate
- Builds dependency graphs
- Optionally modifies files (auto-fix mode)

### Trust Boundaries

| Input | Trust Level | Validation |
|-------|-------------|------------|
| Crate root path | User-provided | Path canonicalization |
| Rust source files | Untrusted | Parsed via `syn`, errors handled |
| Cache files | Semi-trusted | JSON validation, graceful fallback |
| Configuration | User-provided | TOML parsing with defaults |

---

## Security Measures

### 1. Symlink Attack Prevention

The auto-fix system refuses to delete symlinks:

```rust
// fix.rs
pub fn remove_file(path: &Path, dry_run: bool) -> Result<bool> {
    let metadata = path.symlink_metadata()?;

    // Security: Refuse to delete symlinks
    if metadata.file_type().is_symlink() {
        eprintln!("[WARN] Refusing to delete symlink: {}", path.display());
        return Ok(false);
    }

    // Proceed with deletion...
}
```

**Rationale:** An attacker could create a symlink `dead_module.rs -> /etc/passwd`. Without this check, running `deadmod --fix` could delete critical system files.

---

### 2. Path Traversal Prevention

Operations are bounded to the crate root:

```rust
// All paths are validated to be within the crate directory
let crate_root = crate_root.canonicalize()?;

// File operations only occur within crate_root
let file_path = crate_root.join("src").join(&module_name);
```

---

### 3. Recursion Depth Limits

Directory cleanup has a maximum recursion depth:

```rust
// fix.rs
const MAX_RECURSION_DEPTH: usize = 128;

fn clean_empty_dirs_recursive(dir: &Path, depth: usize) -> Result<()> {
    if depth >= MAX_RECURSION_DEPTH {
        eprintln!("[WARN] Max recursion depth reached");
        return Ok(());
    }
    // ...
}
```

**Rationale:** Prevents stack overflow attacks via deeply nested directories.

---

### 4. Atomic Cache Writes

Cache writes use the temp-file-then-rename pattern:

```rust
// cache.rs
pub fn save_cache(crate_root: &Path, cache: &DeadmodCache) -> Result<()> {
    let temp_path = dir.join(format!("cache.json.{}.tmp", std::process::id()));

    // Write to temp file
    fs::write(&temp_path, &json)?;

    // Atomic rename
    fs::rename(&temp_path, &path)?;

    Ok(())
}
```

**Rationale:** Prevents:
- Partial writes if process is interrupted
- Race conditions with concurrent readers
- Cache corruption

---

### 5. Parse Error Resilience

Malformed input files don't crash the tool:

```rust
// parse.rs
pub fn extract_uses_and_decls(content: &str, refs: &mut HashSet<String>) -> Result<()> {
    let ast = match syn::parse_file(content) {
        Ok(ast) => ast,
        Err(e) => {
            // Log and continue - don't crash
            eprintln!("[WARN] Parse error: {}", e);
            return Ok(());
        }
    };
    // ...
}
```

**Rationale:** A single malformed file shouldn't prevent analysis of the rest of the codebase.

---

### 6. No Arbitrary Code Execution

Deadmod performs purely static analysis:
- No macro expansion
- No code execution
- No build system integration

The `syn` crate parses Rust syntax without executing it.

---

### 7. Memory Safety

Deadmod is written in 100% safe Rust:
- No `unsafe` blocks
- Memory managed by Rust's ownership system
- Bounds checking on all array accesses

---

### 8. Panic Prevention

Production code paths avoid `.unwrap()`:

```rust
// Bad (can panic)
let value = option.unwrap();

// Good (handles errors)
let value = option.unwrap_or_default();
let value = option.context("Missing value")?;
```

Global panic hook captures any panics:

```rust
// main.rs
std::panic::set_hook(Box::new(|info| {
    eprintln!("[PANIC] {}", info);
    std::process::exit(2);
}));
```

---

## Security Best Practices for Users

### 1. Review Before Fixing

Always use `--fix-dry-run` before `--fix`:

```bash
# Preview changes
deadmod . --fix-dry-run

# Review output carefully

# Then apply
deadmod . --fix
```

### 2. Version Control

Run deadmod in a clean git working directory:

```bash
git status  # Ensure no uncommitted changes
deadmod . --fix
git diff    # Review changes
git commit -m "Remove dead code"
```

### 3. Backup Critical Files

For production codebases, maintain backups before running auto-fix.

### 4. Cache Directory Permissions

The `.deadmod/` cache directory should have appropriate permissions:

```bash
# Unix: owner read/write only
chmod 700 .deadmod
chmod 600 .deadmod/cache.json
```

---

## Vulnerability Disclosure

### Reporting Security Issues

If you discover a security vulnerability:

1. **Do NOT** open a public GitHub issue
2. Email security concerns to the maintainers
3. Include:
   - Description of the vulnerability
   - Steps to reproduce
   - Potential impact
   - Suggested fix (optional)

### Response Timeline

| Phase | Timeline |
|-------|----------|
| Initial response | 48 hours |
| Assessment | 1 week |
| Fix development | 2 weeks |
| Coordinated disclosure | After fix release |

---

## Known Limitations

### 1. Macro Expansion

Deadmod does not expand macros. Code generated by macros is not analyzed:

```rust
// This module reference is NOT detected
macro_rules! use_module {
    ($name:ident) => { mod $name; }
}

use_module!(utils);  // utils will appear dead
```

**Mitigation:** Use `--ignore` flag for macro-generated modules.

### 2. Build Scripts

Code referenced only in `build.rs` may appear dead:

```rust
// build.rs references codegen module
// But deadmod analyzes src/ separately
```

**Mitigation:** Add build script modules to ignore list.

### 3. Feature Flags

Feature-gated code may appear dead:

```rust
#[cfg(feature = "async")]
mod async_support;  // Dead if feature disabled
```

**Mitigation:** Run analysis with all features enabled.

---

## Dependencies

Deadmod's security depends on its dependencies:

| Crate | Purpose | Security Notes |
|-------|---------|----------------|
| `syn` | Rust parsing | Well-audited, no code execution |
| `serde` | Serialization | Well-audited |
| `rayon` | Parallelism | Memory-safe concurrency |
| `sha2` | Hashing | Cryptographic hash, no secrets |
| `walkdir` | Directory traversal | Follows symlinks (we check separately) |
| `regex` | Pattern matching | DoS-resistant implementation |
| `anyhow` | Error handling | No security implications |

### Dependency Auditing

Run `cargo audit` to check for known vulnerabilities:

```bash
cargo install cargo-audit
cargo audit
```

---

## Compliance

### OWASP Considerations

| OWASP Category | Status |
|----------------|--------|
| Injection | N/A (no code execution) |
| Broken Authentication | N/A (no auth) |
| Sensitive Data Exposure | N/A (no secrets) |
| XML External Entities | N/A (no XML) |
| Broken Access Control | Path traversal prevented |
| Security Misconfiguration | Secure defaults |
| XSS | N/A (CLI tool) |
| Insecure Deserialization | JSON only, validated |
| Using Components with Vulnerabilities | Audited dependencies |
| Insufficient Logging | Actions logged to stderr |

---

## Changelog

### Security Fixes

| Version | Fix |
|---------|-----|
| 0.2.0 | Added symlink attack prevention |
| 0.2.0 | Added recursion depth limits |
| 0.2.0 | Added atomic cache writes |
| 0.1.0 | Initial release |

---

## Contact

For security concerns, contact the maintainers directly rather than opening public issues.
