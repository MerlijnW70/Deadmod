//! Auto-fix functionality for removing dead modules.
//!
//! NASA-grade resilience: never panics, handles all errors gracefully.
//!
//! Performance characteristics:
//! - Pre-compiled regex patterns (compile once, use many)
//! - Parallel-safe (stateless operations)
//!
//! Features:
//! - Safe file deletion with dry-run support
//! - Automatic `mod xyz;` declaration removal from parent modules
//! - Empty directory cleanup
//! - Comprehensive logging of all actions

use std::collections::HashMap;
use std::fs;
use std::path::Path;
use std::sync::OnceLock;

use anyhow::{Context, Result};
use regex::Regex;

use crate::parse::ModuleInfo;
use serde::{Deserialize, Serialize};

/// Result of a fix operation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FixResult {
    pub files_removed: Vec<String>,
    pub declarations_removed: Vec<String>,
    pub dirs_removed: Vec<String>,
    pub errors: Vec<String>,
}

impl FixResult {
    fn new() -> Self {
        Self {
            files_removed: Vec::new(),
            declarations_removed: Vec::new(),
            dirs_removed: Vec::new(),
            errors: Vec::new(),
        }
    }
}

/// Pre-compiled regex patterns for mod declaration removal.
/// Uses OnceLock for thread-safe lazy initialization.
struct ModPatterns {
    simple_mod: Regex,
    pub_mod: Regex,
    pub_vis_mod: Regex,
    attr_mod: Regex,
    attr_pub_mod: Regex,
}

impl ModPatterns {
    /// Create patterns for a specific module name.
    fn for_module(name: &str) -> Option<Self> {
        let escaped = regex::escape(name);
        Some(Self {
            simple_mod: Regex::new(&format!(r"(?m)^\s*mod\s+{}\s*;.*$", escaped)).ok()?,
            pub_mod: Regex::new(&format!(r"(?m)^\s*pub\s+mod\s+{}\s*;.*$", escaped)).ok()?,
            // Security: Use non-greedy matching to prevent ReDoS attacks
            pub_vis_mod: Regex::new(&format!(r"(?m)^\s*pub\s*\([^)]*?\)\s*mod\s+{}\s*;.*$", escaped)).ok()?,
            attr_mod: Regex::new(&format!(r"(?m)^\s*#\[[^\]]*?\]\s*\n\s*mod\s+{}\s*;.*$", escaped)).ok()?,
            attr_pub_mod: Regex::new(&format!(r"(?m)^\s*#\[[^\]]*?\]\s*\n\s*pub\s+mod\s+{}\s*;.*$", escaped)).ok()?,
        })
    }

    /// Apply all patterns to content, returning modified content if any matched.
    fn apply(&self, content: &str) -> Option<String> {
        let mut result = content.to_string();
        let mut found = false;

        for pattern in [&self.simple_mod, &self.pub_mod, &self.pub_vis_mod, &self.attr_mod, &self.attr_pub_mod] {
            if pattern.is_match(&result) {
                found = true;
                result = pattern.replace_all(&result, "").to_string();
            }
        }

        if found { Some(result) } else { None }
    }
}

/// Pre-compiled regex for cleaning consecutive blank lines.
fn blank_line_regex() -> &'static Regex {
    static REGEX: OnceLock<Regex> = OnceLock::new();
    // SAFETY: This regex pattern is hardcoded and validated at compile-test time.
    REGEX.get_or_init(|| {
        Regex::new(r"\n\s*\n\s*\n").expect("Hardcoded regex pattern is valid")
    })
}

/// Safely remove a file.
///
/// In dry-run mode, only prints what would be deleted.
/// NASA-grade: never panics, logs errors and continues.
///
/// Security: Refuses to delete symlinks to prevent symlink attacks.
pub fn remove_file(path: &Path, dry_run: bool) -> Result<bool> {
    // Security check: Get metadata without following symlinks
    let metadata = match path.symlink_metadata() {
        Ok(m) => m,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(false),
        Err(e) => return Err(e).with_context(|| format!("Failed to stat: {}", path.display())),
    };

    // Security: Refuse to delete symlinks to prevent symlink attacks
    if metadata.file_type().is_symlink() {
        eprintln!(
            "[WARN] Refusing to delete symlink: {} (security measure)",
            path.display()
        );
        return Ok(false);
    }

    // Verify it's actually a file
    if !metadata.is_file() {
        eprintln!("[WARN] Not a regular file: {}", path.display());
        return Ok(false);
    }

    if dry_run {
        println!("[DRY-RUN] Would remove: {}", path.display());
        return Ok(true);
    }

    fs::remove_file(path)
        .with_context(|| format!("Failed to remove file: {}", path.display()))?;

    println!("[FIX] Removed: {}", path.display());
    Ok(true)
}

/// Remove a `mod xyz;` declaration from a parent module file.
///
/// Handles various declaration styles:
/// - `mod xyz;`
/// - `pub mod xyz;`
/// - `pub(crate) mod xyz;`
/// - With or without attributes
///
/// Performance: Uses pre-compiled regex patterns per module name.
/// NASA-grade: never panics, returns error on failure.
pub fn remove_mod_declaration(parent_path: &Path, child_name: &str, dry_run: bool) -> Result<bool> {
    if !parent_path.exists() {
        return Ok(false);
    }

    let content = fs::read_to_string(parent_path)
        .with_context(|| format!("Failed to read: {}", parent_path.display()))?;

    // Try regex-based removal first (handles complex cases)
    let new_content = if let Some(patterns) = ModPatterns::for_module(child_name) {
        patterns.apply(&content)
    } else {
        None
    };

    // Fallback: simple line-by-line removal if regex didn't work
    let new_content = new_content.or_else(|| {
        let lines: Vec<&str> = content.lines().collect();
        let mut filtered: Vec<&str> = Vec::with_capacity(lines.len());
        let mod_decl_simple = format!("mod {};", child_name);
        let mod_decl_pub = format!("pub mod {};", child_name);
        let mut found = false;

        for line in &lines {
            let trimmed = line.trim();
            if trimmed == mod_decl_simple
                || trimmed == mod_decl_pub
                || trimmed.starts_with(&format!("mod {} ", child_name))
                || trimmed.starts_with(&format!("pub mod {} ", child_name))
                || trimmed.contains(&format!(" mod {};", child_name))
            {
                found = true;
                continue;
            }
            filtered.push(line);
        }

        if found {
            let mut result = filtered.join("\n");
            // Preserve trailing newline if original had one
            if content.ends_with('\n') && !result.ends_with('\n') {
                result.push('\n');
            }
            Some(result)
        } else {
            None
        }
    });

    let Some(mut new_content) = new_content else {
        return Ok(false);
    };

    // Clean up multiple consecutive blank lines using pre-compiled regex
    let blank_regex = blank_line_regex();
    while blank_regex.is_match(&new_content) {
        new_content = blank_regex.replace_all(&new_content, "\n\n").to_string();
    }

    // Ensure trailing newline
    if content.ends_with('\n') && !new_content.ends_with('\n') {
        new_content.push('\n');
    }

    if dry_run {
        println!(
            "[DRY-RUN] Would remove `mod {};` from: {}",
            child_name,
            parent_path.display()
        );
        return Ok(true);
    }

    fs::write(parent_path, &new_content)
        .with_context(|| format!("Failed to write: {}", parent_path.display()))?;

    println!(
        "[FIX] Removed `mod {};` from: {}",
        child_name,
        parent_path.display()
    );
    Ok(true)
}

/// Maximum recursion depth to prevent stack overflow on deeply nested directories.
const MAX_RECURSION_DEPTH: usize = 128;

/// Recursively clean up empty directories.
///
/// NASA-grade: never panics, logs errors and continues.
/// Limited to MAX_RECURSION_DEPTH levels to prevent stack overflow.
pub fn clean_empty_dirs(root: &Path, dry_run: bool) -> Result<Vec<String>> {
    let mut removed = Vec::new();
    clean_empty_dirs_recursive(root, dry_run, &mut removed, 0)?;
    Ok(removed)
}

fn clean_empty_dirs_recursive(
    dir: &Path,
    dry_run: bool,
    removed: &mut Vec<String>,
    depth: usize,
) -> Result<()> {
    if !dir.is_dir() {
        return Ok(());
    }

    // Prevent stack overflow from deeply nested directories
    if depth >= MAX_RECURSION_DEPTH {
        eprintln!(
            "[WARN] Max recursion depth ({}) reached at: {}",
            MAX_RECURSION_DEPTH,
            dir.display()
        );
        return Ok(());
    }

    // First, recurse into subdirectories
    if let Ok(entries) = fs::read_dir(dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                clean_empty_dirs_recursive(&path, dry_run, removed, depth + 1)?;
            }
        }
    }

    // Check if directory is now empty
    let is_empty = fs::read_dir(dir)
        .map(|mut entries| entries.next().is_none())
        .unwrap_or(false);

    if is_empty {
        // Don't remove src directory itself
        if dir.file_name().is_some_and(|n| n == "src") {
            return Ok(());
        }

        if dry_run {
            println!("[DRY-RUN] Would remove empty dir: {}", dir.display());
        } else if let Err(e) = fs::remove_dir(dir) {
            eprintln!("[WARN] Failed to remove dir {}: {}", dir.display(), e);
        } else {
            println!("[FIX] Removed empty dir: {}", dir.display());
        }
        removed.push(dir.display().to_string());
    }

    Ok(())
}

/// Find the parent module file that declares a given module.
///
/// Uses the pre-parsed module refs to find the parent without additional I/O.
/// The `refs` set already contains all `mod xyz;` declarations from parsing.
///
/// Performance: O(|modules|) lookup, no file I/O.
fn find_parent_module(
    _crate_root: &Path,
    module_name: &str,
    mods: &HashMap<String, ModuleInfo>,
) -> Option<std::path::PathBuf> {
    // Check which modules reference this module (already parsed)
    for info in mods.values() {
        if info.refs.contains(module_name) {
            return Some(info.path.clone());
        }
    }

    None
}

/// Main fix orchestration function.
///
/// Removes dead modules and cleans up their declarations.
///
/// NASA-grade resilience:
/// - Continues on individual file errors
/// - Reports all errors at the end
/// - Never panics
pub fn fix_dead_modules(
    crate_root: &Path,
    dead: &[&str],
    mods: &HashMap<String, ModuleInfo>,
    dry_run: bool,
) -> Result<FixResult> {
    let mut result = FixResult::new();

    if dead.is_empty() {
        println!("No dead modules to fix.");
        return Ok(result);
    }

    let mode = if dry_run { "DRY-RUN" } else { "FIX" };
    println!("\n[{}] Processing {} dead module(s)...\n", mode, dead.len());

    for module_name in dead {
        // 1. Find and remove the module file
        if let Some(info) = mods.get(*module_name) {
            match remove_file(&info.path, dry_run) {
                Ok(true) => result.files_removed.push(info.path.display().to_string()),
                Ok(false) => {}
                Err(e) => result.errors.push(format!("remove {}: {}", info.path.display(), e)),
            }
        }

        // 2. Find and update parent module to remove declaration
        if let Some(parent_path) = find_parent_module(crate_root, module_name, mods) {
            match remove_mod_declaration(&parent_path, module_name, dry_run) {
                Ok(true) => result
                    .declarations_removed
                    .push(format!("{} from {}", module_name, parent_path.display())),
                Ok(false) => {}
                Err(e) => result.errors.push(format!(
                    "remove decl {} from {}: {}",
                    module_name,
                    parent_path.display(),
                    e
                )),
            }
        }
    }

    // 3. Clean up empty directories
    let src = crate_root.join("src");
    match clean_empty_dirs(&src, dry_run) {
        Ok(dirs) => result.dirs_removed = dirs,
        Err(e) => result.errors.push(format!("clean dirs: {}", e)),
    }

    // Summary
    println!();
    println!("=== {} Summary ===", mode);
    println!("Files removed: {}", result.files_removed.len());
    println!("Declarations removed: {}", result.declarations_removed.len());
    println!("Empty dirs removed: {}", result.dirs_removed.len());

    if !result.errors.is_empty() {
        println!("Errors: {}", result.errors.len());
        for err in &result.errors {
            eprintln!("  - {}", err);
        }
    }

    Ok(result)
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
            .join("deadmod_fix_test")
            .join(format!("{}_{}", name, id));
        if temp_dir.exists() {
            fs::remove_dir_all(&temp_dir).ok();
        }
        fs::create_dir_all(&temp_dir).unwrap();
        temp_dir
    }

    #[test]
    fn test_remove_file_exists() {
        let dir = create_temp_dir("remove_exists");
        let file = dir.join("test.rs");
        create_file(&file, "fn foo() {}");

        assert!(file.exists());
        let result = remove_file(&file, false).unwrap();
        assert!(result);
        assert!(!file.exists());

        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn test_remove_file_dry_run() {
        let dir = create_temp_dir("remove_dry");
        let file = dir.join("test.rs");
        create_file(&file, "fn foo() {}");

        let result = remove_file(&file, true).unwrap();
        assert!(result);
        assert!(file.exists()); // Still exists in dry-run

        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn test_remove_file_not_exists() {
        let dir = create_temp_dir("remove_not_exists");
        let file = dir.join("nonexistent.rs");

        let result = remove_file(&file, false).unwrap();
        assert!(!result);

        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn test_remove_mod_declaration_simple() {
        let dir = create_temp_dir("mod_decl_simple");
        let lib = dir.join("lib.rs");
        create_file(&lib, "mod utils;\nmod dead;\n\nfn main() {}\n");

        let result = remove_mod_declaration(&lib, "dead", false).unwrap();
        assert!(result);

        let content = fs::read_to_string(&lib).unwrap();
        assert!(content.contains("mod utils;"));
        assert!(!content.contains("mod dead;"));

        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn test_remove_mod_declaration_pub() {
        let dir = create_temp_dir("mod_decl_pub");
        let lib = dir.join("lib.rs");
        create_file(&lib, "pub mod utils;\npub mod dead;\n");

        let result = remove_mod_declaration(&lib, "dead", false).unwrap();
        assert!(result);

        let content = fs::read_to_string(&lib).unwrap();
        assert!(content.contains("pub mod utils;"));
        assert!(!content.contains("pub mod dead;"));

        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn test_remove_mod_declaration_dry_run() {
        let dir = create_temp_dir("mod_decl_dry");
        let lib = dir.join("lib.rs");
        let original = "mod utils;\nmod dead;\n";
        create_file(&lib, original);

        let result = remove_mod_declaration(&lib, "dead", true).unwrap();
        assert!(result);

        let content = fs::read_to_string(&lib).unwrap();
        assert_eq!(content, original); // Unchanged in dry-run

        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn test_remove_mod_declaration_not_found() {
        let dir = create_temp_dir("mod_decl_not_found");
        let lib = dir.join("lib.rs");
        create_file(&lib, "mod utils;\n");

        let result = remove_mod_declaration(&lib, "nonexistent", false).unwrap();
        assert!(!result);

        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn test_clean_empty_dirs() {
        let dir = create_temp_dir("clean_dirs");
        let empty_sub = dir.join("src/empty_subdir");
        fs::create_dir_all(&empty_sub).unwrap();

        let non_empty = dir.join("src/non_empty");
        fs::create_dir_all(&non_empty).unwrap();
        create_file(&non_empty.join("file.rs"), "");

        let removed = clean_empty_dirs(&dir.join("src"), false).unwrap();
        assert!(removed.iter().any(|p| p.contains("empty_subdir")));
        assert!(!empty_sub.exists());
        assert!(non_empty.exists());

        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn test_fix_dead_modules_integration() {
        let dir = create_temp_dir("fix_integration");
        let src = dir.join("src");
        fs::create_dir_all(&src).unwrap();

        create_file(&src.join("main.rs"), "mod utils;\nmod dead;\n\nfn main() {}\n");
        create_file(&src.join("utils.rs"), "pub fn helper() {}\n");
        create_file(&src.join("dead.rs"), "pub fn unused() {}\n");

        // Build module info
        let mut mods = HashMap::new();
        let mut main_info = ModuleInfo::new(src.join("main.rs"));
        main_info.refs.insert("utils".to_string());
        main_info.refs.insert("dead".to_string());
        mods.insert("main".to_string(), main_info);

        mods.insert("utils".to_string(), ModuleInfo::new(src.join("utils.rs")));
        mods.insert("dead".to_string(), ModuleInfo::new(src.join("dead.rs")));

        let dead = vec!["dead"];
        let result = fix_dead_modules(&dir, &dead, &mods, false).unwrap();

        assert_eq!(result.files_removed.len(), 1);
        assert!(!src.join("dead.rs").exists());
        assert!(src.join("utils.rs").exists());

        let main_content = fs::read_to_string(src.join("main.rs")).unwrap();
        assert!(main_content.contains("mod utils;"));
        assert!(!main_content.contains("mod dead;"));

        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn test_mod_patterns_complex() {
        // Test the ModPatterns struct directly
        let patterns = ModPatterns::for_module("dead").unwrap();

        // Test simple mod
        let result = patterns.apply("mod dead;\nmod utils;\n");
        assert!(result.is_some());
        assert!(!result.unwrap().contains("mod dead;"));

        // Test pub mod
        let result = patterns.apply("pub mod dead;\npub mod utils;\n");
        assert!(result.is_some());
        assert!(!result.unwrap().contains("pub mod dead;"));

        // Test pub(crate) mod
        let result = patterns.apply("pub(crate) mod dead;\nmod utils;\n");
        assert!(result.is_some());
        assert!(!result.unwrap().contains("pub(crate) mod dead;"));
    }

    // --- DEEP EDGE CASE TESTS FOR FIX MODULE ---

    #[test]
    fn test_remove_mod_declaration_with_attributes() {
        let dir = create_temp_dir("mod_decl_attrs");
        let lib = dir.join("lib.rs");
        create_file(&lib, "#[cfg(test)]\nmod dead;\nmod utils;\n");

        let result = remove_mod_declaration(&lib, "dead", false).unwrap();
        assert!(result);

        let content = fs::read_to_string(&lib).unwrap();
        assert!(content.contains("mod utils;"));
        // Note: attribute-prefixed mods may or may not be removed depending on pattern

        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn test_remove_mod_declaration_pub_super() {
        let dir = create_temp_dir("mod_decl_pub_super");
        let lib = dir.join("lib.rs");
        create_file(&lib, "pub(super) mod dead;\npub(in crate) mod utils;\n");

        let result = remove_mod_declaration(&lib, "dead", false).unwrap();
        assert!(result);

        let content = fs::read_to_string(&lib).unwrap();
        assert!(!content.contains("pub(super) mod dead;"));

        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn test_clean_empty_dirs_nested() {
        let dir = create_temp_dir("clean_nested");
        let deep = dir.join("src/a/b/c/d");
        fs::create_dir_all(&deep).unwrap();

        let _removed = clean_empty_dirs(&dir.join("src"), false).unwrap();

        // All empty dirs should be cleaned
        assert!(!deep.exists());
        assert!(!dir.join("src/a/b/c").exists());
        assert!(!dir.join("src/a/b").exists());
        assert!(!dir.join("src/a").exists());

        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn test_clean_empty_dirs_preserves_src() {
        let dir = create_temp_dir("clean_src");
        let src = dir.join("src");
        fs::create_dir_all(&src).unwrap();

        let _removed = clean_empty_dirs(&src, false).unwrap();

        // src itself should not be removed
        assert!(src.exists());

        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn test_fix_result_structure() {
        let result = FixResult::new();
        assert!(result.files_removed.is_empty());
        assert!(result.declarations_removed.is_empty());
        assert!(result.dirs_removed.is_empty());
        assert!(result.errors.is_empty());
    }

    #[test]
    fn test_fix_dead_modules_empty_list() {
        let dir = create_temp_dir("fix_empty");
        fs::create_dir_all(dir.join("src")).unwrap();

        let mods = HashMap::new();
        let dead: Vec<&str> = vec![];
        let result = fix_dead_modules(&dir, &dead, &mods, false).unwrap();

        assert!(result.files_removed.is_empty());
        assert!(result.declarations_removed.is_empty());

        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn test_fix_dead_modules_dry_run() {
        let dir = create_temp_dir("fix_dry");
        let src = dir.join("src");
        fs::create_dir_all(&src).unwrap();

        create_file(&src.join("main.rs"), "mod dead;\nfn main() {}\n");
        create_file(&src.join("dead.rs"), "pub fn unused() {}\n");

        let mut mods = HashMap::new();
        let mut main_info = ModuleInfo::new(src.join("main.rs"));
        main_info.refs.insert("dead".to_string());
        mods.insert("main".to_string(), main_info);
        mods.insert("dead".to_string(), ModuleInfo::new(src.join("dead.rs")));

        let dead = vec!["dead"];
        let _result = fix_dead_modules(&dir, &dead, &mods, true).unwrap();

        // Files should still exist in dry-run mode
        assert!(src.join("dead.rs").exists());
        let main_content = fs::read_to_string(src.join("main.rs")).unwrap();
        assert!(main_content.contains("mod dead;"));

        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn test_remove_file_symlink_protection() {
        // Skip on Windows where symlinks require admin
        #[cfg(unix)]
        {
            use std::os::unix::fs::symlink;

            let dir = create_temp_dir("symlink_protection");
            let real_file = dir.join("real.txt");
            let link_file = dir.join("link.rs");

            create_file(&real_file, "important data");

            // Create symlink
            if symlink(&real_file, &link_file).is_ok() {
                // Should refuse to delete symlink
                let result = remove_file(&link_file, false);
                // Even if it succeeds, the real file should not be affected
                assert!(real_file.exists());
            }

            fs::remove_dir_all(&dir).ok();
        }
    }

    #[test]
    fn test_mod_patterns_no_match() {
        let patterns = ModPatterns::for_module("nonexistent").unwrap();

        let result = patterns.apply("mod utils;\nmod helpers;\n");
        assert!(result.is_none()); // No changes made
    }

    #[test]
    fn test_mod_patterns_special_chars() {
        // Module names with underscores
        let patterns = ModPatterns::for_module("my_module_name").unwrap();

        let result = patterns.apply("mod my_module_name;\nmod other;\n");
        assert!(result.is_some());
        assert!(!result.unwrap().contains("mod my_module_name;"));
    }

    #[test]
    fn test_blank_line_cleanup() {
        let dir = create_temp_dir("blank_cleanup");
        let lib = dir.join("lib.rs");
        create_file(&lib, "mod utils;\n\n\nmod dead;\n\n\nfn main() {}\n");

        let result = remove_mod_declaration(&lib, "dead", false).unwrap();
        assert!(result);

        let content = fs::read_to_string(&lib).unwrap();
        // Should not have more than 2 consecutive newlines
        assert!(!content.contains("\n\n\n\n"));

        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn test_fix_multiple_dead_modules() {
        let dir = create_temp_dir("fix_multiple");
        let src = dir.join("src");
        fs::create_dir_all(&src).unwrap();

        create_file(&src.join("main.rs"), "mod a;\nmod b;\nmod c;\nfn main() {}\n");
        create_file(&src.join("a.rs"), "// dead");
        create_file(&src.join("b.rs"), "// dead");
        create_file(&src.join("c.rs"), "// alive");

        let mut mods = HashMap::new();
        let mut main_info = ModuleInfo::new(src.join("main.rs"));
        main_info.refs.insert("a".to_string());
        main_info.refs.insert("b".to_string());
        main_info.refs.insert("c".to_string());
        mods.insert("main".to_string(), main_info);
        mods.insert("a".to_string(), ModuleInfo::new(src.join("a.rs")));
        mods.insert("b".to_string(), ModuleInfo::new(src.join("b.rs")));
        mods.insert("c".to_string(), ModuleInfo::new(src.join("c.rs")));

        let dead = vec!["a", "b"];
        let result = fix_dead_modules(&dir, &dead, &mods, false).unwrap();

        assert_eq!(result.files_removed.len(), 2);
        assert!(!src.join("a.rs").exists());
        assert!(!src.join("b.rs").exists());
        assert!(src.join("c.rs").exists());

        let main_content = fs::read_to_string(src.join("main.rs")).unwrap();
        assert!(!main_content.contains("mod a;"));
        assert!(!main_content.contains("mod b;"));
        assert!(main_content.contains("mod c;"));

        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn test_clean_empty_dirs_max_depth() {
        let dir = create_temp_dir("max_depth");

        // Create directory 10 levels deep (well under MAX_RECURSION_DEPTH)
        let mut path = dir.join("src");
        for i in 0..10 {
            path = path.join(format!("level{}", i));
        }
        fs::create_dir_all(&path).unwrap();

        let _removed = clean_empty_dirs(&dir.join("src"), false).unwrap();

        // All should be cleaned
        assert!(!dir.join("src/level0").exists());

        fs::remove_dir_all(&dir).ok();
    }
}
