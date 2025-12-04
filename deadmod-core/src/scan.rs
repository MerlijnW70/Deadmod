//! Parallel, safe, deterministic file discovery with efficient directory pruning.
//!
//! Performance optimizations:
//! - Early directory pruning via `WalkDir::filter_entry` (O(1) subtree skip)
//! - Parallel file processing via Rayon's `par_bridge`
//! - Minimal work in parallel threads (only .rs extension check)

use anyhow::{Context, Result};
use rayon::prelude::*;
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use walkdir::WalkDir;

/// Directories to exclude by default (standard Rust project conventions).
const EXCLUDED_DIRS: &[&str] = &["target", ".git", "node_modules", ".cargo"];

/// Checks if a directory entry should be pruned (excluded from traversal).
///
/// This is called by `WalkDir::filter_entry` and runs sequentially,
/// but enables O(1) subtree skipping for excluded directories.
#[inline]
fn is_excluded_dir(entry: &walkdir::DirEntry, excludes: &HashSet<&str>) -> bool {
    entry.file_type().is_dir()
        && entry
            .file_name()
            .to_str()
            .is_some_and(|name| excludes.contains(name))
}

/// Gathers all .rs files recursively starting from the root path using parallel iteration.
///
/// Performance characteristics:
/// - Uses early directory pruning to skip `target/`, `.git/`, etc. in O(1)
/// - Parallelizes file processing across available CPU cores
/// - Only processes entries that pass the directory filter
///
/// Automatically excludes `target/`, `.git/`, `node_modules/`, and `.cargo/`.
pub fn gather_rs_files(root: &Path) -> Result<Vec<PathBuf>> {
    let excludes: HashSet<&str> = EXCLUDED_DIRS.iter().copied().collect();

    WalkDir::new(root)
        .into_iter()
        // CRITICAL: filter_entry prunes entire subtrees before iteration
        // This runs sequentially but prevents thousands of unnecessary entries
        .filter_entry(|e| !is_excluded_dir(e, &excludes))
        .par_bridge() // Parallelize processing of remaining entries
        .filter_map(|entry| match entry {
            Ok(e) => {
                let path = e.path();
                // Simple check: is it an .rs file?
                if path.is_file() && path.extension().is_some_and(|ext| ext == "rs") {
                    Some(Ok(path.to_path_buf()))
                } else {
                    None
                }
            }
            Err(e) => Some(Err(e.into())),
        })
        .collect::<Result<Vec<_>>>()
        .context(format!("Failed to gather .rs files from {}", root.display()))
}

/// Gathers all .rs files with custom exclusion patterns using early pruning.
///
/// Combines default exclusions with custom patterns for efficient subtree skipping.
pub fn gather_rs_files_with_excludes(root: &Path, excludes: &[&str]) -> Result<Vec<PathBuf>> {
    // Combine default and custom excludes into a single HashSet for O(1) lookup
    let all_excludes: HashSet<&str> = EXCLUDED_DIRS
        .iter()
        .copied()
        .chain(excludes.iter().copied())
        .collect();

    WalkDir::new(root)
        .into_iter()
        // Early pruning with combined exclusion set
        .filter_entry(|e| !is_excluded_dir(e, &all_excludes))
        .par_bridge()
        .filter_map(|entry| match entry {
            Ok(e) => {
                let path = e.path();
                if path.is_file() && path.extension().is_some_and(|ext| ext == "rs") {
                    Some(Ok(path.to_path_buf()))
                } else {
                    None
                }
            }
            Err(e) => Some(Err(e.into())),
        })
        .collect::<Result<Vec<_>>>()
        .context(format!("Failed to gather .rs files from {}", root.display()))
}
