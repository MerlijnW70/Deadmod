//! Workspace-level analysis for multi-crate Rust projects.
//!
//! NASA-grade resilience: never panics, handles all errors gracefully.
//!
//! Supports:
//! - Automatic workspace detection via `[workspace]` in Cargo.toml
//! - Crate discovery via `cargo metadata` or fallback directory scan
//! - Per-crate analysis with fault tolerance
//! - Combined reporting across all workspace members

use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use rayon::prelude::*;
use serde::Deserialize;

use crate::{
    build_graph, cache, find_dead, find_root_modules, gather_rs_files, reachable_from_roots,
    visualize,
};

/// Minimal subset of `cargo metadata` output we need.
#[derive(Debug, Deserialize)]
struct CargoMetadata {
    #[allow(dead_code)]
    workspace_root: String,
    packages: Vec<CargoPackage>,
}

#[derive(Debug, Deserialize)]
struct CargoPackage {
    #[allow(dead_code)]
    id: String,
    manifest_path: String,
}

/// Result of analyzing a single crate.
#[derive(Debug, Clone)]
pub struct CrateAnalysis {
    pub name: String,
    pub root: PathBuf,
    pub dead_modules: Vec<String>,
    pub reachable_modules: Vec<String>,
    pub dot_output: String,
}

/// Try using `cargo metadata` for workspace discovery.
/// This is the most reliable method as it respects Cargo.toml workspace config.
fn try_cargo_metadata(path: &Path) -> Option<CargoMetadata> {
    let output = std::process::Command::new("cargo")
        .args(["metadata", "--no-deps", "--format-version", "1"])
        .current_dir(path)
        .output()
        .ok()?;

    if !output.status.success() {
        return None;
    }

    serde_json::from_slice(&output.stdout).ok()
}

/// Detect if a root is a Cargo workspace.
pub fn is_workspace_root(root: &Path) -> bool {
    let cargo_toml = root.join("Cargo.toml");
    if !cargo_toml.exists() {
        return false;
    }

    let text = match fs::read_to_string(&cargo_toml) {
        Ok(content) => content,
        Err(e) => {
            eprintln!(
                "[WARN] Failed to read {}: {} (assuming not a workspace)",
                cargo_toml.display(),
                e
            );
            return false;
        }
    };
    text.contains("[workspace]")
}

/// Find the crate root from a given path.
///
/// Search strategy:
/// 1. If path has Cargo.toml + src/, it's a crate root
/// 2. If path has just src/, treat as crate root
/// 3. For workspaces, find first subdirectory with Cargo.toml
/// 4. Walk up parent directories looking for Cargo.toml + src/
///
/// Returns `None` if no crate root can be found.
pub fn find_crate_root(path: &Path) -> Option<PathBuf> {
    let canonical = path.canonicalize().ok()?;

    // Check if this path itself is a crate root
    if canonical.join("Cargo.toml").exists() && canonical.join("src").exists() {
        return Some(canonical);
    }

    // Check for just src directory
    if canonical.join("src").exists() {
        return Some(canonical);
    }

    // For workspace: find first subdirectory with Cargo.toml
    if let Ok(entries) = fs::read_dir(&canonical) {
        for entry in entries.flatten() {
            let entry_path = entry.path();
            if entry_path.is_dir() && entry_path.join("Cargo.toml").exists() {
                return Some(entry_path);
            }
        }
    }

    // Walk up parent directories
    let mut current = canonical.as_path();
    while let Some(parent) = current.parent() {
        if parent.join("Cargo.toml").exists() && parent.join("src").exists() {
            return Some(parent.to_path_buf());
        }
        current = parent;
    }

    // Fallback: return the original path
    Some(canonical)
}

/// Find all crate roots in a workspace.
/// Prefers `cargo metadata` when available, falls back to directory scan.
pub fn find_all_crates(root: &Path) -> Result<Vec<PathBuf>> {
    // Try cargo metadata first (most reliable)
    if let Some(meta) = try_cargo_metadata(root) {
        let mut crates = Vec::new();
        for pkg in meta.packages {
            let manifest = PathBuf::from(&pkg.manifest_path);
            if let Some(parent) = manifest.parent() {
                crates.push(parent.to_path_buf());
            }
        }
        if !crates.is_empty() {
            return Ok(crates);
        }
    }

    // Fallback: manual directory scan
    let mut crates = Vec::new();

    // Check if root itself is a crate (has src/)
    if root.join("src").exists() && root.join("Cargo.toml").exists() {
        crates.push(root.to_path_buf());
    }

    // Scan subdirectories for crates
    if let Ok(entries) = fs::read_dir(root) {
        for entry in entries.flatten() {
            let path = entry.path();
            let name = path.file_name().unwrap_or_default().to_string_lossy();

            // Skip common non-crate directories
            if name == "target" || name == ".git" || name == "node_modules" {
                continue;
            }

            if path.is_dir() && path.join("Cargo.toml").exists() {
                crates.push(path);
            }
        }
    }

    Ok(crates)
}

/// Extract crate name from Cargo.toml content.
fn parse_crate_name(cargo_toml: &str) -> String {
    for line in cargo_toml.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with("name") {
            if let Some((_, value)) = trimmed.split_once('=') {
                return value.trim().trim_matches('"').trim_matches('\'').to_string();
            }
        }
    }
    "unknown".to_string()
}

/// Analyze a single crate and return the analysis result.
///
/// This is fault-tolerant: parse errors in individual files are logged but don't
/// cause the entire analysis to fail.
pub fn analyze_crate(crate_root: &Path) -> Result<CrateAnalysis> {
    let manifest = crate_root.join("Cargo.toml");
    let cargo_toml = fs::read_to_string(&manifest)
        .with_context(|| format!("Failed to read Cargo.toml at {}", manifest.display()))?;
    let crate_name = parse_crate_name(&cargo_toml);

    // 1. Gather all .rs files
    let files = gather_rs_files(crate_root)
        .with_context(|| format!("Failed to gather files for crate {}", crate_name))?;

    // 2. Load cache
    let cached = cache::load_cache(crate_root);

    // 3. Parse modules (incremental)
    let mods = cache::incremental_parse(crate_root, &files, cached)
        .with_context(|| format!("Failed to parse modules for crate {}", crate_name))?;

    // 4. Find root modules (entry points)
    let root_mods = find_root_modules(crate_root);

    // 5. Build graph and find reachable modules (single O(|V|+|E|) traversal)
    let graph = build_graph(&mods);
    let valid_roots = root_mods
        .iter()
        .filter(|name| mods.contains_key(*name))
        .map(|s| s.as_str());
    let reachable: HashSet<&str> = reachable_from_roots(&graph, valid_roots);

    // 6. Find dead modules
    let mut dead = find_dead(&mods, &reachable);
    dead.sort();

    // 7. Generate DOT visualization
    let reachable_owned: HashSet<String> = reachable.iter().map(|s| s.to_string()).collect();
    let dot = visualize::generate_dot(&mods, &reachable_owned);

    Ok(CrateAnalysis {
        name: crate_name,
        root: crate_root.to_path_buf(),
        dead_modules: dead.into_iter().map(String::from).collect(),
        reachable_modules: reachable_owned.into_iter().collect(),
        dot_output: dot,
    })
}

/// Analyze an entire workspace, returning results for each crate.
///
/// NASA-grade resilience:
/// - If workspace scan fails → returns empty vec with warning
/// - If one crate fails to analyze → continues with others
/// - Never panics, never crashes
///
/// Performance optimization:
/// - Uses Rayon for parallel crate analysis (Fork-Join pattern)
/// - Runtime: O(T_longest_crate) instead of O(sum of all crates)
/// - Scales horizontally with available CPU cores
pub fn analyze_workspace(root: &Path) -> Result<Vec<CrateAnalysis>> {
    // 1. Safe workspace scanning (Sequential - I/O bound)
    let crates = match find_all_crates(root) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("[WARN] cannot scan workspace {}: {}", root.display(), e);
            return Ok(vec![]);
        }
    };

    if crates.is_empty() {
        eprintln!("[WARN] No crates found in workspace at {}", root.display());
        return Ok(vec![]);
    }

    // Informative logging (Sequential)
    eprintln!(
        "INFO: Analyzing workspace with {} crate(s) in parallel...",
        crates.len()
    );
    for cr in &crates {
        let name = cr
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_else(|| "<unknown>".to_string());
        eprintln!("  - {}", name);
    }
    eprintln!();

    // 2. Parallel Crate Analysis (Compute-bound)
    // Uses Rayon's work-stealing thread pool for optimal CPU utilization
    let results: Vec<CrateAnalysis> = crates
        .into_par_iter()
        .filter_map(|crate_root| {
            match analyze_crate(&crate_root) {
                Ok(analysis) => Some(analysis),
                Err(e) => {
                    // Failure: Log error but continue (Bulkhead Pattern)
                    eprintln!("[WARN] crate {} failed: {}", crate_root.display(), e);
                    None
                }
            }
        })
        .collect();

    Ok(results)
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

    fn create_temp_dir(name: &str) -> PathBuf {
        let id = TEST_COUNTER.fetch_add(1, Ordering::SeqCst);
        let temp_dir = std::env::temp_dir()
            .join("deadmod_workspace_test")
            .join(format!("{}_{}", name, id));
        if temp_dir.exists() {
            fs::remove_dir_all(&temp_dir).ok();
        }
        fs::create_dir_all(&temp_dir).unwrap();
        temp_dir
    }

    #[test]
    fn test_is_workspace_root_true() {
        let dir = create_temp_dir("ws_root_true");
        create_file(
            &dir.join("Cargo.toml"),
            "[workspace]\nmembers = [\"core\", \"cli\"]",
        );

        assert!(is_workspace_root(&dir));
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn test_is_workspace_root_false() {
        let dir = create_temp_dir("ws_root_false");
        create_file(&dir.join("Cargo.toml"), "[package]\nname = \"test\"");

        assert!(!is_workspace_root(&dir));
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn test_is_workspace_root_no_file() {
        let dir = create_temp_dir("ws_root_none");
        assert!(!is_workspace_root(&dir));
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn test_parse_crate_name() {
        let toml = r#"
[package]
name = "my-awesome-crate"
version = "0.1.0"
"#;
        assert_eq!(parse_crate_name(toml), "my-awesome-crate");
    }

    #[test]
    fn test_parse_crate_name_single_quotes() {
        let toml = "[package]\nname = 'test-crate'";
        assert_eq!(parse_crate_name(toml), "test-crate");
    }

    #[test]
    fn test_parse_crate_name_missing() {
        let toml = "[package]\nversion = \"1.0\"";
        assert_eq!(parse_crate_name(toml), "unknown");
    }

    #[test]
    fn test_find_all_crates_fallback() {
        let ws = create_temp_dir("find_crates");

        // Create workspace structure
        create_file(
            &ws.join("Cargo.toml"),
            "[workspace]\nmembers = [\"core\", \"cli\"]",
        );

        // Create member crates
        fs::create_dir_all(ws.join("core/src")).unwrap();
        fs::create_dir_all(ws.join("cli/src")).unwrap();
        create_file(&ws.join("core/Cargo.toml"), "[package]\nname = \"core\"");
        create_file(&ws.join("cli/Cargo.toml"), "[package]\nname = \"cli\"");
        create_file(&ws.join("core/src/lib.rs"), "");
        create_file(&ws.join("cli/src/main.rs"), "fn main() {}");

        // This will use fallback since cargo metadata won't work in temp dir
        let crates = find_all_crates(&ws).unwrap();
        assert_eq!(crates.len(), 2);

        fs::remove_dir_all(&ws).ok();
    }

    #[test]
    fn test_analyze_crate_simple() {
        let dir = create_temp_dir("analyze_simple");

        create_file(&dir.join("Cargo.toml"), "[package]\nname = \"test-crate\"");
        fs::create_dir_all(dir.join("src")).unwrap();
        create_file(&dir.join("src/main.rs"), "mod utils; fn main() {}");
        create_file(&dir.join("src/utils.rs"), "pub fn helper() {}");
        create_file(&dir.join("src/dead.rs"), "pub fn unused() {}");

        let result = analyze_crate(&dir).unwrap();

        assert_eq!(result.name, "test-crate");
        assert!(result.dead_modules.contains(&"dead".to_string()));
        assert!(result.reachable_modules.contains(&"main".to_string()));
        assert!(result.reachable_modules.contains(&"utils".to_string()));
        assert!(result.dot_output.contains("digraph"));

        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn test_analyze_workspace_multiple_crates() {
        let ws = create_temp_dir("analyze_ws");

        // Workspace root
        create_file(
            &ws.join("Cargo.toml"),
            "[workspace]\nmembers = [\"core\", \"cli\"]",
        );

        // Core crate
        fs::create_dir_all(ws.join("core/src")).unwrap();
        create_file(&ws.join("core/Cargo.toml"), "[package]\nname = \"core\"");
        create_file(&ws.join("core/src/lib.rs"), "pub mod utils;");
        create_file(&ws.join("core/src/utils.rs"), "pub fn x() {}");

        // CLI crate
        fs::create_dir_all(ws.join("cli/src")).unwrap();
        create_file(&ws.join("cli/Cargo.toml"), "[package]\nname = \"cli\"");
        create_file(&ws.join("cli/src/main.rs"), "fn main() {}");

        let results = analyze_workspace(&ws).unwrap();

        assert_eq!(results.len(), 2);

        let names: Vec<&str> = results.iter().map(|r| r.name.as_str()).collect();
        assert!(names.contains(&"core"));
        assert!(names.contains(&"cli"));

        fs::remove_dir_all(&ws).ok();
    }
}
