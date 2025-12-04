//! Parallel, safe, deterministic file discovery with efficient directory pruning.
//!
//! Performance optimizations:
//! - Early directory pruning via `WalkDir::filter_entry` (O(1) subtree skip)
//! - Parallel file processing via Rayon's `par_bridge`
//! - Minimal work in parallel threads (only .rs extension check)
//!
//! ## Filesystem-based Module Discovery
//!
//! Rust modules can be discovered via filesystem conventions:
//! - `src/lib.rs` and `src/main.rs` are crate roots
//! - `src/bin/*.rs` are binary crate roots
//! - Directories with `mod.rs` define module hierarchies
//! - `.rs` files as siblings of `mod.rs` are submodules

use anyhow::{Context, Result};
use rayon::prelude::*;
use std::collections::{HashMap, HashSet};
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

// ============================================================================
// Filesystem-based Module Discovery
// ============================================================================

/// A discovered module cluster representing a directory with Rust modules.
#[derive(Debug, Clone)]
pub struct ModuleCluster {
    /// Name of the cluster (directory path as :: separated)
    pub name: String,
    /// Full path to the directory
    pub path: PathBuf,
    /// Path relative to src/
    pub relative_path: String,
    /// The mod.rs file if present
    pub mod_file: Option<PathBuf>,
    /// All .rs files in this directory (excluding mod.rs)
    pub modules: Vec<DiscoveredModule>,
    /// Child clusters (subdirectories with modules)
    pub children: Vec<String>,
    /// Parent cluster name (None for root)
    pub parent: Option<String>,
    /// Depth from src/ root (0 = src/, 1 = src/foo/, etc.)
    pub depth: usize,
}

/// A discovered module from filesystem scanning.
#[derive(Debug, Clone)]
pub struct DiscoveredModule {
    /// Module name (filename without .rs)
    pub name: String,
    /// Full path to the .rs file
    pub path: PathBuf,
    /// The cluster this module belongs to
    pub cluster: String,
    /// Whether this is a crate root (lib.rs, main.rs)
    pub is_crate_root: bool,
    /// Whether this is a mod.rs file
    pub is_mod_file: bool,
    /// Depth in the directory hierarchy
    pub depth: usize,
}

/// Result of filesystem-based module discovery.
#[derive(Debug, Clone)]
pub struct ModuleDiscovery {
    /// All discovered clusters (directories)
    pub clusters: HashMap<String, ModuleCluster>,
    /// All discovered modules
    pub modules: Vec<DiscoveredModule>,
    /// Crate root files (lib.rs, main.rs, bin/*.rs)
    pub crate_roots: Vec<PathBuf>,
    /// Total .rs file count
    pub file_count: usize,
}

/// Discover all modules in a Rust project using filesystem conventions.
///
/// This works even without `mod` declarations by scanning:
/// - `src/lib.rs`, `src/main.rs` as crate roots
/// - `src/bin/*.rs` as binary roots
/// - Directories with `mod.rs` as module clusters
/// - `.rs` files as modules within their parent cluster
pub fn discover_modules(root: &Path) -> Result<ModuleDiscovery> {
    let src_path = root.join("src");
    if !src_path.exists() {
        return Ok(ModuleDiscovery {
            clusters: HashMap::new(),
            modules: Vec::new(),
            crate_roots: Vec::new(),
            file_count: 0,
        });
    }

    let mut clusters: HashMap<String, ModuleCluster> = HashMap::new();
    let mut modules: Vec<DiscoveredModule> = Vec::new();
    let mut crate_roots: Vec<PathBuf> = Vec::new();
    let excludes: HashSet<&str> = EXCLUDED_DIRS.iter().copied().collect();

    // First pass: collect all directories with .rs files
    let mut dir_files: HashMap<PathBuf, Vec<PathBuf>> = HashMap::new();

    for entry in WalkDir::new(&src_path)
        .into_iter()
        .filter_entry(|e| !is_excluded_dir(e, &excludes))
        .filter_map(|e| e.ok())
    {
        let path = entry.path();
        if path.is_file() && path.extension().is_some_and(|ext| ext == "rs") {
            if let Some(parent) = path.parent() {
                dir_files
                    .entry(parent.to_path_buf())
                    .or_default()
                    .push(path.to_path_buf());
            }
        }
    }

    // Second pass: create clusters from directories
    for (dir_path, files) in &dir_files {
        let relative = dir_path
            .strip_prefix(&src_path)
            .unwrap_or(dir_path)
            .to_string_lossy()
            .replace('\\', "/");

        let cluster_name = if relative.is_empty() {
            "root".to_string()
        } else {
            relative.replace('/', "::")
        };

        let depth = if relative.is_empty() {
            0
        } else {
            relative.matches('/').count() + 1
        };

        // Find mod.rs if present
        let mod_file = files
            .iter()
            .find(|f| f.file_name().is_some_and(|name| name == "mod.rs"))
            .cloned();

        // Create discovered modules for each file
        let mut cluster_modules = Vec::new();
        for file in files {
            let file_name = file.file_name().unwrap_or_default().to_string_lossy();
            let module_name = file_name.trim_end_matches(".rs").to_string();

            let is_crate_root = matches!(module_name.as_str(), "lib" | "main");
            let is_mod_file = module_name == "mod";

            if is_crate_root {
                crate_roots.push(file.clone());
            }

            let discovered = DiscoveredModule {
                name: module_name,
                path: file.clone(),
                cluster: cluster_name.clone(),
                is_crate_root,
                is_mod_file,
                depth,
            };

            if !is_mod_file {
                cluster_modules.push(discovered.clone());
            }
            modules.push(discovered);
        }

        // Determine parent cluster
        let parent = if relative.is_empty() {
            None
        } else if let Some(parent_path) = dir_path.parent() {
            let parent_rel = parent_path
                .strip_prefix(&src_path)
                .unwrap_or(parent_path)
                .to_string_lossy()
                .replace('\\', "/");
            Some(if parent_rel.is_empty() {
                "root".to_string()
            } else {
                parent_rel.replace('/', "::")
            })
        } else {
            Some("root".to_string())
        };

        // Find immediate child clusters
        let children: Vec<String> = dir_files
            .keys()
            .filter(|child_dir| {
                if let Ok(child_rel) = child_dir.strip_prefix(dir_path) {
                    // Only immediate children (one level deep)
                    child_rel.components().count() == 1 && **child_dir != *dir_path
                } else {
                    false
                }
            })
            .map(|child_dir| {
                child_dir
                    .strip_prefix(&src_path)
                    .unwrap_or(child_dir)
                    .to_string_lossy()
                    .replace('\\', "/")
                    .replace('/', "::")
            })
            .collect();

        let cluster = ModuleCluster {
            name: cluster_name.clone(),
            path: dir_path.clone(),
            relative_path: relative,
            mod_file,
            modules: cluster_modules,
            children,
            parent,
            depth,
        };

        clusters.insert(cluster_name, cluster);
    }

    // Check for bin/ directory
    let bin_path = src_path.join("bin");
    if bin_path.exists() {
        for entry in WalkDir::new(&bin_path)
            .max_depth(1)
            .into_iter()
            .filter_map(|e| e.ok())
        {
            let path = entry.path();
            if path.is_file() && path.extension().is_some_and(|ext| ext == "rs") {
                crate_roots.push(path.to_path_buf());
                modules.push(DiscoveredModule {
                    name: path
                        .file_stem()
                        .unwrap_or_default()
                        .to_string_lossy()
                        .to_string(),
                    path: path.to_path_buf(),
                    cluster: "bin".to_string(),
                    is_crate_root: true,
                    is_mod_file: false,
                    depth: 1,
                });
            }
        }
    }

    let file_count = modules.len();

    Ok(ModuleDiscovery {
        clusters,
        modules,
        crate_roots,
        file_count,
    })
}

/// Get cluster hierarchy as a tree structure for visualization.
pub fn get_cluster_tree(discovery: &ModuleDiscovery) -> Vec<(String, Vec<String>)> {
    let mut tree = Vec::new();

    // Start with root cluster
    if let Some(root) = discovery.clusters.get("root") {
        tree.push((root.name.clone(), root.children.clone()));

        // Add all child clusters recursively
        fn add_children(
            cluster_name: &str,
            clusters: &HashMap<String, ModuleCluster>,
            tree: &mut Vec<(String, Vec<String>)>,
        ) {
            if let Some(cluster) = clusters.get(cluster_name) {
                for child in &cluster.children {
                    if let Some(child_cluster) = clusters.get(child) {
                        tree.push((child_cluster.name.clone(), child_cluster.children.clone()));
                        add_children(child, clusters, tree);
                    }
                }
            }
        }

        add_children("root", &discovery.clusters, &mut tree);
    }

    tree
}

#[cfg(test)]
mod discovery_tests {
    use super::*;
    use std::fs;

    fn create_test_project() -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("deadmod_discovery_test_{}", std::process::id()));
        if dir.exists() {
            fs::remove_dir_all(&dir).ok();
        }

        // Create structure:
        // src/
        //   lib.rs
        //   api/
        //     mod.rs
        //     routes.rs
        //     handlers.rs
        //   core/
        //     mod.rs
        //     engine.rs
        let src = dir.join("src");
        let api = src.join("api");
        let core = src.join("core");

        fs::create_dir_all(&api).unwrap();
        fs::create_dir_all(&core).unwrap();

        fs::write(src.join("lib.rs"), "mod api;\nmod core;").unwrap();
        fs::write(api.join("mod.rs"), "mod routes;\nmod handlers;").unwrap();
        fs::write(api.join("routes.rs"), "pub fn get() {}").unwrap();
        fs::write(api.join("handlers.rs"), "pub fn handle() {}").unwrap();
        fs::write(core.join("mod.rs"), "mod engine;").unwrap();
        fs::write(core.join("engine.rs"), "pub fn run() {}").unwrap();

        dir
    }

    #[test]
    fn test_discover_modules() {
        let dir = create_test_project();
        let discovery = discover_modules(&dir).unwrap();

        // Should find 3 clusters: root, api, core
        assert_eq!(discovery.clusters.len(), 3);
        assert!(discovery.clusters.contains_key("root"));
        assert!(discovery.clusters.contains_key("api"));
        assert!(discovery.clusters.contains_key("core"));

        // Root should have api and core as children
        let root = discovery.clusters.get("root").unwrap();
        assert!(root.children.contains(&"api".to_string()));
        assert!(root.children.contains(&"core".to_string()));

        // API cluster should have routes and handlers
        let api = discovery.clusters.get("api").unwrap();
        assert_eq!(api.modules.len(), 2); // routes, handlers (not mod.rs)
        assert!(api.mod_file.is_some());

        // Should find lib.rs as crate root
        assert!(!discovery.crate_roots.is_empty());

        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn test_cluster_depth() {
        let dir = create_test_project();
        let discovery = discover_modules(&dir).unwrap();

        let root = discovery.clusters.get("root").unwrap();
        assert_eq!(root.depth, 0);

        let api = discovery.clusters.get("api").unwrap();
        assert_eq!(api.depth, 1);

        fs::remove_dir_all(&dir).ok();
    }
}
