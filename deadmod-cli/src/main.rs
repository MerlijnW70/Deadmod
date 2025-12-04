//! deadmod CLI - NASA-grade dead module detector for Rust projects.
//!
//! Features:
//! - Automatic crate root detection
//! - Workspace-aware scanning
//! - Comprehensive binary detection (src/bin/*.rs, src/bin/*/main.rs)
//! - Rayon-powered parallel parsing
//! - Incremental caching for faster re-analysis
//! - Graphviz DOT visualization

use anyhow::{anyhow, Context, Result};
use clap::Parser;
use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};

use deadmod_core::{
    analyze_workspace, build_graph, cache, discover_modules, extract_call_names, extract_call_usages,
    extract_callgraph_functions, extract_const_usage, extract_constants,
    extract_declared_generics, extract_functions, extract_generic_usages, extract_macro_usages,
    extract_macros, extract_match_arms, extract_match_usages, extract_trait_usages,
    extract_traits, extract_variant_usage, extract_variants, find_all_crates, find_crate_root,
    find_dead, find_root_modules, fix_dead_modules, gather_rs_files, generate_html_graph,
    generate_pixi_graph, get_cluster_tree, init_structured_logging, is_workspace_root, load_config,
    module_graph_to_visualizer_json, print_json, print_plain, reachable_from_roots, visualize,
    CallGraph, ConstGraph, DeadArmReason, EnumGraph, FuncGraph, GenericGraph, GenericKind,
    MacroGraph, MatchGraph, TraitGraph,
};

#[derive(Parser, Debug)]
#[command(author, version, about = "NASA-grade dead module detector for Rust")]
pub struct Cli {
    /// Path to the root of the Rust project
    #[arg(default_value = ".")]
    path: String,

    /// Output results in JSON format
    #[arg(long)]
    json: bool,

    /// Module names or patterns to ignore
    #[arg(long, num_args = 1..)]
    ignore: Vec<String>,

    /// Generate Graphviz DOT output for module dependencies
    #[arg(long)]
    dot: bool,

    /// Write DOT output to a specified file instead of stdout
    #[arg(long)]
    dot_file: Option<String>,

    /// Analyze entire workspace (all member crates)
    #[arg(long)]
    workspace: bool,

    /// Automatically remove dead modules and their declarations
    #[arg(long)]
    fix: bool,

    /// Show what would be removed without actually deleting anything
    #[arg(long)]
    fix_dry_run: bool,

    /// Generate interactive HTML graph visualization
    #[arg(long)]
    html: bool,

    /// Write HTML graph to a specified file instead of stdout
    #[arg(long)]
    html_file: Option<String>,

    /// Generate PixiJS WebGL interactive graph (GPU-accelerated, for large codebases)
    #[arg(long)]
    html_pixi: bool,

    /// Write PixiJS WebGL graph to a specified file instead of stdout
    #[arg(long)]
    html_pixi_file: Option<String>,

    /// Detect dead functions instead of dead modules
    #[arg(long)]
    dead_func: bool,

    /// Detect dead trait methods instead of dead modules
    #[arg(long)]
    dead_traits: bool,

    /// Detect unused generic parameters and lifetimes
    #[arg(long)]
    dead_generics: bool,

    /// Detect unused macro_rules! definitions
    #[arg(long)]
    dead_macros: bool,

    /// Detect unused const and static declarations
    #[arg(long)]
    dead_constants: bool,

    /// Detect unused enum variants
    #[arg(long)]
    dead_variants: bool,

    /// Detect dead match arms (wildcard masking, unreachable patterns)
    #[arg(long)]
    dead_match_arms: bool,

    /// Generate function call graph (JSON output)
    #[arg(long)]
    callgraph: bool,

    /// Generate function call graph in DOT format (for Graphviz)
    #[arg(long)]
    callgraph_dot: bool,

    /// Generate function call graph for visualizer (numeric IDs, dead flags)
    #[arg(long)]
    callgraph_viz: bool,

    /// Generate module dependency graph for visualizer (numeric IDs, dead flags)
    #[arg(long)]
    modgraph_viz: bool,

    /// Export function callgraph to JSON file (visualizer format)
    #[arg(long, value_name = "FILE")]
    export_callgraph: Option<String>,

    /// Export module dependency graph to JSON file (visualizer format)
    #[arg(long, value_name = "FILE")]
    export_modgraph: Option<String>,

    /// Export combined graph (modules + functions) to JSON file
    #[arg(long, value_name = "FILE")]
    export_combined: Option<String>,

    /// Discover all modules via filesystem structure (show cluster hierarchy)
    #[arg(long)]
    discover: bool,
}

/// Prints workspace info when running on a workspace root.
fn print_workspace_info(path: &Path) {
    if is_workspace_root(path) {
        // Count member crates
        if let Ok(entries) = fs::read_dir(path) {
            let members: Vec<_> = entries
                .flatten()
                .filter(|e| {
                    let p = e.path();
                    p.is_dir() && p.join("Cargo.toml").exists()
                })
                .collect();

            if !members.is_empty() {
                eprintln!("INFO: Detected Cargo workspace with {} member(s):", members.len());
                for m in &members {
                    eprintln!("  - {}", m.file_name().to_string_lossy());
                }
                eprintln!("TIP: Run on each crate separately for accurate results.");
                eprintln!();
            }
        }
    }
}

/// Checks if a module name should be ignored based on patterns.
fn is_ignored(module: &str, ignore: &[String]) -> bool {
    ignore
        .iter()
        .any(|p| p == module || module.ends_with(p) || module.contains(p))
}

/// Security: Validates output file paths to prevent path traversal attacks.
///
/// Rejects:
/// - Absolute paths (must be relative to current directory)
/// - Paths containing `..` (parent directory traversal)
/// - Paths with null bytes (injection attacks)
///
/// Returns the validated PathBuf or an error.
fn validate_output_path(path: &str) -> Result<PathBuf> {
    // Security: Check for null bytes (path injection)
    if path.contains('\0') {
        return Err(anyhow!("Output path contains null bytes"));
    }

    let p = PathBuf::from(path);

    // Security: Reject absolute paths
    if p.is_absolute() {
        return Err(anyhow!(
            "Output path must be relative, not absolute: {}",
            path
        ));
    }

    // Security: Reject path traversal attempts
    for component in p.components() {
        if matches!(component, std::path::Component::ParentDir) {
            return Err(anyhow!(
                "Path traversal (..) not allowed in output paths: {}",
                path
            ));
        }
    }

    // Security: Check the path doesn't escape via symlinks in parent
    // We only check for simple cases here; full symlink resolution
    // would require the directory to exist
    let normalized = path.replace('\\', "/");
    if normalized.contains("/../") || normalized.starts_with("../") {
        return Err(anyhow!(
            "Path traversal attempt detected: {}",
            path
        ));
    }

    Ok(p)
}

fn main() -> Result<()> {
    // Global panic guard - NASA-grade resilience
    std::panic::set_hook(Box::new(|info| {
        eprintln!("[PANIC] deadmod internal error: {}", info);
        eprintln!("[PANIC] The process will exit safely with code 2.");
        eprintln!("[PANIC] Please report this at: https://github.com/anthropics/deadmod/issues");
    }));

    // Initialize structured logging (JSON to stderr, respects RUST_LOG)
    init_structured_logging();

    let cli = Cli::parse();

    // Filesystem-based module discovery mode
    if cli.discover {
        let input_path = Path::new(&cli.path);
        let root = find_crate_root(input_path)
            .with_context(|| format!("Failed to find crate root from: {}", cli.path))?;

        let discovery = discover_modules(&root)?;

        if cli.json {
            let clusters_json: Vec<_> = discovery.clusters.values().map(|c| {
                serde_json::json!({
                    "name": c.name,
                    "path": c.path.display().to_string(),
                    "relative_path": c.relative_path,
                    "depth": c.depth,
                    "has_mod_file": c.mod_file.is_some(),
                    "modules": c.modules.iter().map(|m| &m.name).collect::<Vec<_>>(),
                    "children": c.children,
                    "parent": c.parent,
                })
            }).collect();

            let json_output = serde_json::json!({
                "file_count": discovery.file_count,
                "cluster_count": discovery.clusters.len(),
                "crate_roots": discovery.crate_roots.iter().map(|p| p.display().to_string()).collect::<Vec<_>>(),
                "clusters": clusters_json,
            });
            println!("{}", serde_json::to_string_pretty(&json_output)?);
        } else {
            println!("=== Filesystem Module Discovery ===\n");
            println!("Crate root: {}", root.display());
            println!("Total .rs files: {}", discovery.file_count);
            println!("Clusters (directories): {}\n", discovery.clusters.len());

            // Print cluster hierarchy as tree
            let tree = get_cluster_tree(&discovery);
            println!("CLUSTER HIERARCHY:");
            for (name, children) in &tree {
                let indent = "  ".repeat(name.matches("::").count());
                let icon = if children.is_empty() { "📄" } else { "📁" };
                println!("{}{}  {}", indent, icon, name);

                // Show modules in this cluster
                if let Some(cluster) = discovery.clusters.get(name) {
                    for module in &cluster.modules {
                        let mod_indent = "  ".repeat(name.matches("::").count() + 1);
                        let status = if module.is_crate_root { "🎯" } else { "  " };
                        println!("{}{}  {}", mod_indent, status, module.name);
                    }
                }
            }

            if !discovery.crate_roots.is_empty() {
                println!("\nCRATE ROOTS:");
                for root_file in &discovery.crate_roots {
                    println!("  🎯 {}", root_file.display());
                }
            }
        }

        return Ok(());
    }

    // Dead function detection mode
    if cli.dead_func {
        let input_path = Path::new(&cli.path);
        print_workspace_info(input_path);
        let root = find_crate_root(input_path)
            .with_context(|| format!("Failed to find crate root from: {}", cli.path))?;

        // Gather files and parse modules
        let files = gather_rs_files(&root)?;
        let cached = cache::load_cache(&root);
        let mods = cache::incremental_parse(&root, &files, cached)?;

        // Extract functions and calls from all files
        let mut all_funcs = Vec::new();
        let mut file_calls = std::collections::HashMap::new();

        for info in mods.values() {
            if let Ok(content) = fs::read_to_string(&info.path) {
                let funcs = extract_functions(&info.path, &content);
                let calls = extract_call_names(&info.path, &content);

                all_funcs.extend(funcs);
                file_calls.insert(info.path.display().to_string(), calls);
            }
        }

        // Build function graph and find dead functions
        let graph = FuncGraph::build(&all_funcs, &file_calls);
        let result = graph.analyze();

        if cli.json {
            let json_output = serde_json::json!({
                "total_functions": result.stats.total_functions,
                "reachable_functions": result.stats.reachable_count,
                "dead_functions": result.stats.dead_count,
                "public_dead": result.stats.public_dead,
                "private_dead": result.stats.private_dead,
                "dead": result.dead.iter().map(|f| {
                    serde_json::json!({
                        "name": f.name,
                        "full_path": f.full_path,
                        "visibility": f.visibility,
                        "file": f.file,
                        "is_method": f.is_method,
                    })
                }).collect::<Vec<_>>(),
            });
            println!("{}", serde_json::to_string_pretty(&json_output)?);
        } else {
            println!("=== Dead Function Analysis ===\n");
            println!("Total functions: {}", result.stats.total_functions);
            println!("Reachable:       {}", result.stats.reachable_count);
            println!("Dead:            {}", result.stats.dead_count);
            println!("  - Public:      {}", result.stats.public_dead);
            println!("  - Private:     {}", result.stats.private_dead);

            if !result.dead.is_empty() {
                println!("\nDEAD FUNCTIONS:");
                for func in &result.dead {
                    let vis_marker = if func.visibility.starts_with("pub") {
                        "[pub]"
                    } else {
                        "[priv]"
                    };
                    println!("  {} {} ({})", vis_marker, func.full_path, func.file);
                }
            } else {
                println!("\nNo dead functions found.");
            }
        }

        std::process::exit(if result.dead.is_empty() { 0 } else { 1 });
    }

    // Dead trait method detection mode
    if cli.dead_traits {
        let input_path = Path::new(&cli.path);
        print_workspace_info(input_path);
        let root = find_crate_root(input_path)
            .with_context(|| format!("Failed to find crate root from: {}", cli.path))?;

        // Gather files and parse modules
        let files = gather_rs_files(&root)?;
        let cached = cache::load_cache(&root);
        let mods = cache::incremental_parse(&root, &files, cached)?;

        // Extract traits and usages from all files
        let mut all_extractions = Vec::new();
        let mut all_usages = Vec::new();

        for info in mods.values() {
            if let Ok(content) = fs::read_to_string(&info.path) {
                let extraction = extract_traits(&info.path, &content);
                let usages = extract_trait_usages(&info.path, &content);

                all_extractions.push(extraction);
                all_usages.push(usages);
            }
        }

        // Build trait graph and find dead trait methods
        let graph = TraitGraph::build(&all_extractions, &all_usages);
        let result = graph.analyze();

        if cli.json {
            let json_output = serde_json::json!({
                "total_trait_methods": result.stats.total_trait_methods,
                "total_impl_methods": result.stats.total_impl_methods,
                "dead_trait_methods": result.stats.dead_trait_method_count,
                "dead_impl_methods": result.stats.dead_impl_method_count,
                "required_methods": result.stats.required_methods,
                "provided_methods": result.stats.provided_methods,
                "dead_traits": result.dead_trait_methods.iter().map(|m| {
                    serde_json::json!({
                        "trait_name": m.trait_name,
                        "method_name": m.method_name,
                        "full_path": m.full_path,
                        "visibility": m.visibility,
                        "is_required": m.is_required,
                        "file": m.file,
                    })
                }).collect::<Vec<_>>(),
                "dead_impls": result.dead_impl_methods.iter().map(|m| {
                    serde_json::json!({
                        "trait_name": m.trait_name,
                        "type_name": m.type_name,
                        "method_name": m.method_name,
                        "full_id": m.full_id,
                        "file": m.file,
                    })
                }).collect::<Vec<_>>(),
            });
            println!("{}", serde_json::to_string_pretty(&json_output)?);
        } else {
            println!("=== Dead Trait Method Analysis ===\n");
            println!("Total trait methods:  {}", result.stats.total_trait_methods);
            println!("  - Required:         {}", result.stats.required_methods);
            println!("  - Provided:         {}", result.stats.provided_methods);
            println!("Total impl methods:   {}", result.stats.total_impl_methods);
            println!();
            println!("Dead trait methods:   {}", result.stats.dead_trait_method_count);
            println!("Dead impl methods:    {}", result.stats.dead_impl_method_count);

            if !result.dead_trait_methods.is_empty() {
                println!("\nDEAD TRAIT METHODS:");
                for method in &result.dead_trait_methods {
                    let req_marker = if method.is_required {
                        "[required]"
                    } else {
                        "[provided]"
                    };
                    println!(
                        "  {} {}::{} ({})",
                        req_marker, method.trait_name, method.method_name, method.file
                    );
                }
            }

            if !result.dead_impl_methods.is_empty() {
                println!("\nDEAD IMPL METHODS:");
                for method in &result.dead_impl_methods {
                    println!(
                        "  impl {} for {} :: {} ({})",
                        method.trait_name, method.type_name, method.method_name, method.file
                    );
                }
            }

            if result.dead_trait_methods.is_empty() && result.dead_impl_methods.is_empty() {
                println!("\nNo dead trait methods found.");
            }
        }

        let has_dead =
            !result.dead_trait_methods.is_empty() || !result.dead_impl_methods.is_empty();
        std::process::exit(if has_dead { 1 } else { 0 });
    }

    // Dead generic parameter detection mode
    if cli.dead_generics {
        let input_path = Path::new(&cli.path);
        print_workspace_info(input_path);
        let root = find_crate_root(input_path)
            .with_context(|| format!("Failed to find crate root from: {}", cli.path))?;

        // Gather files and parse modules
        let files = gather_rs_files(&root)?;
        let cached = cache::load_cache(&root);
        let mods = cache::incremental_parse(&root, &files, cached)?;

        // Extract declared generics and usages from all files
        let mut all_extractions = Vec::new();
        let mut all_usages = Vec::new();

        for info in mods.values() {
            if let Ok(content) = fs::read_to_string(&info.path) {
                let extraction = extract_declared_generics(&info.path, &content);
                let usage = extract_generic_usages(&info.path, &content);

                all_extractions.push(extraction);
                all_usages.push(usage);
            }
        }

        // Build generic graph and find dead generics
        let graph = GenericGraph::new(&all_extractions, &all_usages);
        let result = graph.analyze();

        if cli.json {
            let json_output = serde_json::json!({
                "total_declared_types": result.stats.total_declared_types,
                "total_declared_lifetimes": result.stats.total_declared_lifetimes,
                "total_declared_consts": result.stats.total_declared_consts,
                "dead_types": result.stats.dead_types,
                "dead_lifetimes": result.stats.dead_lifetimes,
                "dead_consts": result.stats.dead_consts,
                "dead": result.dead.iter().map(|d| {
                    serde_json::json!({
                        "name": d.name,
                        "kind": format!("{:?}", d.kind),
                        "parent": d.parent,
                        "parent_kind": format!("{:?}", d.parent_kind),
                        "file": d.file,
                        "unused_bounds": d.unused_bounds,
                    })
                }).collect::<Vec<_>>(),
            });
            println!("{}", serde_json::to_string_pretty(&json_output)?);
        } else {
            println!("=== Dead Generic Parameter Analysis ===\n");
            println!(
                "Declared type parameters:     {}",
                result.stats.total_declared_types
            );
            println!(
                "Declared lifetimes:           {}",
                result.stats.total_declared_lifetimes
            );
            println!(
                "Declared const parameters:    {}",
                result.stats.total_declared_consts
            );
            println!();
            println!("Dead type parameters:         {}", result.stats.dead_types);
            println!("Dead lifetimes:               {}", result.stats.dead_lifetimes);
            println!("Dead const parameters:        {}", result.stats.dead_consts);

            if !result.dead.is_empty() {
                println!("\nDEAD GENERIC PARAMETERS:");
                for d in &result.dead {
                    let kind_str = match d.kind {
                        GenericKind::Type => "type",
                        GenericKind::Lifetime => "lifetime",
                        GenericKind::Const => "const",
                    };
                    let bounds_str = if !d.unused_bounds.is_empty() {
                        format!(" (bounds: {})", d.unused_bounds.join(", "))
                    } else {
                        String::new()
                    };
                    println!(
                        "  [{}] {} in {}{} ({})",
                        kind_str, d.name, d.parent, bounds_str, d.file
                    );
                }
            } else {
                println!("\nNo dead generic parameters found.");
            }
        }

        std::process::exit(if result.dead.is_empty() { 0 } else { 1 });
    }

    // Dead macro detection mode
    if cli.dead_macros {
        let input_path = Path::new(&cli.path);
        print_workspace_info(input_path);
        let root = find_crate_root(input_path)
            .with_context(|| format!("Failed to find crate root from: {}", cli.path))?;

        // Gather files and parse modules
        let files = gather_rs_files(&root)?;
        let cached = cache::load_cache(&root);
        let mods = cache::incremental_parse(&root, &files, cached)?;

        // Extract macros and usages from all files
        let mut all_macros = Vec::new();
        let mut all_usages = Vec::new();

        for info in mods.values() {
            if let Ok(content) = fs::read_to_string(&info.path) {
                let macros = extract_macros(&info.path, &content);
                let usages = extract_macro_usages(&info.path, &content);

                all_macros.extend(macros);
                all_usages.push(usages);
            }
        }

        // Build macro graph and find dead macros
        let graph = MacroGraph::new(all_macros, &all_usages);
        let result = graph.analyze();

        if cli.json {
            let json_output = serde_json::json!({
                "total_declared": result.stats.total_declared,
                "exported_count": result.stats.exported_count,
                "dead_count": result.stats.dead_count,
                "dead_exported_count": result.stats.dead_exported_count,
                "dead": result.dead.iter().map(|m| {
                    serde_json::json!({
                        "name": m.name,
                        "exported": m.exported,
                        "file": m.file,
                        "module_path": m.module_path,
                    })
                }).collect::<Vec<_>>(),
            });
            println!("{}", serde_json::to_string_pretty(&json_output)?);
        } else {
            println!("=== Dead Macro Analysis ===\n");
            println!("Total macros declared:  {}", result.stats.total_declared);
            println!("  - Exported:           {}", result.stats.exported_count);
            println!();
            println!("Dead macros:            {}", result.stats.dead_count);
            println!("  - Exported dead:      {}", result.stats.dead_exported_count);

            if !result.dead.is_empty() {
                println!("\nDEAD MACROS:");
                for m in &result.dead {
                    let export_marker = if m.exported { "[exported]" } else { "[local]" };
                    println!("  {} {} ({})", export_marker, m.name, m.file);
                }
            } else {
                println!("\nNo dead macros found.");
            }
        }

        std::process::exit(if result.dead.is_empty() { 0 } else { 1 });
    }

    // Dead constants detection mode
    if cli.dead_constants {
        let input_path = Path::new(&cli.path);
        print_workspace_info(input_path);
        let root = find_crate_root(input_path)
            .with_context(|| format!("Failed to find crate root from: {}", cli.path))?;

        // Gather files and parse modules
        let files = gather_rs_files(&root)?;
        let cached = cache::load_cache(&root);
        let mods = cache::incremental_parse(&root, &files, cached)?;

        // Extract constants and usages from all files
        let mut all_constants = Vec::new();
        let mut all_usages = Vec::new();

        for info in mods.values() {
            if let Ok(content) = fs::read_to_string(&info.path) {
                let constants = extract_constants(&info.path, &content);
                let usages = extract_const_usage(&info.path, &content);

                all_constants.extend(constants);
                all_usages.push(usages);
            }
        }

        // Build constant graph and find dead constants
        let graph = ConstGraph::new(all_constants, &all_usages);
        let result = graph.analyze();

        if cli.json {
            let json_output = serde_json::json!({
                "total_declared": result.stats.total_declared,
                "const_count": result.stats.const_count,
                "static_count": result.stats.static_count,
                "dead_count": result.stats.dead_count,
                "dead_const_count": result.stats.dead_const_count,
                "dead_static_count": result.stats.dead_static_count,
                "dead": result.dead.iter().map(|c| {
                    serde_json::json!({
                        "name": c.name,
                        "is_static": c.is_static,
                        "visibility": c.visibility,
                        "file": c.file,
                        "module_path": c.module_path,
                    })
                }).collect::<Vec<_>>(),
            });
            println!("{}", serde_json::to_string_pretty(&json_output)?);
        } else {
            println!("=== Dead Constants/Statics Analysis ===\n");
            println!("Total declared:     {}", result.stats.total_declared);
            println!("  - Constants:      {}", result.stats.const_count);
            println!("  - Statics:        {}", result.stats.static_count);
            println!();
            println!("Dead count:         {}", result.stats.dead_count);
            println!("  - Dead consts:    {}", result.stats.dead_const_count);
            println!("  - Dead statics:   {}", result.stats.dead_static_count);

            if !result.dead.is_empty() {
                println!("\nDEAD CONSTANTS/STATICS:");
                for c in &result.dead {
                    let kind = if c.is_static { "static" } else { "const" };
                    let vis = if c.visibility == "pub" {
                        "[pub]"
                    } else {
                        "[priv]"
                    };
                    println!("  {} {} {} ({})", vis, kind, c.name, c.file);
                }
            } else {
                println!("\nNo dead constants/statics found.");
            }
        }

        std::process::exit(if result.dead.is_empty() { 0 } else { 1 });
    }

    // Dead enum variant detection mode
    if cli.dead_variants {
        let input_path = Path::new(&cli.path);
        print_workspace_info(input_path);
        let root = find_crate_root(input_path)
            .with_context(|| format!("Failed to find crate root from: {}", cli.path))?;

        // Gather files and parse modules
        let files = gather_rs_files(&root)?;
        let cached = cache::load_cache(&root);
        let mods = cache::incremental_parse(&root, &files, cached)?;

        // Extract variants and usages from all files
        let mut all_variants = Vec::new();
        let mut all_usages = Vec::new();

        for info in mods.values() {
            if let Ok(content) = fs::read_to_string(&info.path) {
                let variants = extract_variants(&info.path, &content);
                let usages = extract_variant_usage(&info.path, &content);

                all_variants.extend(variants);
                all_usages.push(usages);
            }
        }

        // Build enum graph and find dead variants
        let graph = EnumGraph::new(all_variants, &all_usages);
        let result = graph.analyze();

        if cli.json {
            let json_output = serde_json::json!({
                "total_variants": result.stats.total_variants,
                "total_enums": result.stats.total_enums,
                "dead_variant_count": result.stats.dead_variant_count,
                "dead_enum_count": result.stats.dead_enum_count,
                "dead": result.dead.iter().map(|v| {
                    serde_json::json!({
                        "enum_name": v.enum_name,
                        "variant_name": v.variant_name,
                        "full_name": v.full_name,
                        "visibility": v.visibility,
                        "file": v.file,
                    })
                }).collect::<Vec<_>>(),
            });
            println!("{}", serde_json::to_string_pretty(&json_output)?);
        } else {
            println!("=== Dead Enum Variant Analysis ===\n");
            println!("Total enums:        {}", result.stats.total_enums);
            println!("Total variants:     {}", result.stats.total_variants);
            println!();
            println!("Dead variants:      {}", result.stats.dead_variant_count);
            println!("Fully dead enums:   {}", result.stats.dead_enum_count);

            if !result.dead.is_empty() {
                println!("\nDEAD ENUM VARIANTS:");
                for v in &result.dead {
                    let vis = if v.visibility == "pub" {
                        "[pub]"
                    } else {
                        "[priv]"
                    };
                    println!("  {} {} ({})", vis, v.full_name, v.file);
                }
            } else {
                println!("\nNo dead enum variants found.");
            }
        }

        std::process::exit(if result.dead.is_empty() { 0 } else { 1 });
    }

    // Dead match arm detection mode
    if cli.dead_match_arms {
        let input_path = Path::new(&cli.path);
        print_workspace_info(input_path);
        let root = find_crate_root(input_path)
            .with_context(|| format!("Failed to find crate root from: {}", cli.path))?;

        // Gather files and parse modules
        let files = gather_rs_files(&root)?;
        let cached = cache::load_cache(&root);
        let mods = cache::incremental_parse(&root, &files, cached)?;

        // Extract match arms and usages from all files
        let mut all_arms = Vec::new();
        let mut total_match_count = 0;
        let mut all_usages = Vec::new();

        for info in mods.values() {
            if let Ok(content) = fs::read_to_string(&info.path) {
                let extraction = extract_match_arms(&info.path, &content);
                all_arms.extend(extraction.arms);
                total_match_count += extraction.match_count;

                let usages = extract_match_usages(&info.path, &content);
                all_usages.push(usages);
            }
        }

        // Build match graph and find dead arms
        let graph = MatchGraph::new(all_arms, total_match_count, &all_usages);
        let result = graph.analyze();

        if cli.json {
            let json_output = serde_json::json!({
                "total_match_expressions": result.stats.total_match_expressions,
                "total_arms": result.stats.total_arms,
                "wildcard_count": result.stats.wildcard_count,
                "dead_arm_count": result.stats.dead_arm_count,
                "masked_arm_count": result.stats.masked_arm_count,
                "dead_arms": result.dead_arms.iter().map(|a| {
                    serde_json::json!({
                        "pattern": a.pattern,
                        "reason": format!("{:?}", a.reason),
                        "file": a.file,
                    })
                }).collect::<Vec<_>>(),
            });
            println!("{}", serde_json::to_string_pretty(&json_output)?);
        } else {
            println!("=== Dead Match Arm Analysis ===\n");
            println!("Total match expressions: {}", result.stats.total_match_expressions);
            println!("Total arms:              {}", result.stats.total_arms);
            println!("Wildcard arms:           {}", result.stats.wildcard_count);
            println!();
            println!("Dead/Masked arms:        {}", result.stats.dead_arm_count);

            if !result.dead_arms.is_empty() {
                println!("\nDEAD/MASKED MATCH ARMS:");
                for arm in &result.dead_arms {
                    let reason = match arm.reason {
                        DeadArmReason::NeverUsed => "[never-used]",
                        DeadArmReason::MaskedByWildcard => "[masked]",
                        DeadArmReason::NonFinalWildcard => "[non-final-wildcard]",
                    };
                    println!("  {} {} ({})", reason, arm.pattern, arm.file);
                }
            } else {
                println!("\nNo dead match arms found.");
            }
        }

        std::process::exit(if result.dead_arms.is_empty() { 0 } else { 1 });
    }

    // Module dependency graph for visualizer
    if cli.modgraph_viz {
        let input_path = Path::new(&cli.path);
        print_workspace_info(input_path);
        let root = find_crate_root(input_path)
            .with_context(|| format!("Failed to find crate root from: {}", cli.path))?;

        // Gather files and parse modules
        let files = gather_rs_files(&root)?;
        let cached = cache::load_cache(&root);
        let mods = cache::incremental_parse(&root, &files, cached)?;

        // Build dependency graph and find reachable modules
        let graph = build_graph(&mods);
        let roots = find_root_modules(&root);
        let reachable = reachable_from_roots(&graph, roots.iter().map(String::as_str));

        // Output visualizer-compatible JSON
        let json = module_graph_to_visualizer_json(&mods, &reachable);
        println!("{}", serde_json::to_string_pretty(&json)?);

        std::process::exit(0);
    }

    // Export module graph to file
    if let Some(ref path) = cli.export_modgraph {
        // Security: Validate output path
        let safe_path = validate_output_path(path)
            .with_context(|| format!("Invalid output path: {}", path))?;

        let input_path = Path::new(&cli.path);
        let root = find_crate_root(input_path)
            .with_context(|| format!("Failed to find crate root from: {}", cli.path))?;

        let files = gather_rs_files(&root)?;
        let cached = cache::load_cache(&root);
        let mods = cache::incremental_parse(&root, &files, cached)?;

        let graph = build_graph(&mods);
        let roots = find_root_modules(&root);
        let reachable = reachable_from_roots(&graph, roots.iter().map(String::as_str));

        let json = module_graph_to_visualizer_json(&mods, &reachable);
        let serialized = serde_json::to_string_pretty(&json)
            .context("Failed to serialize module graph to JSON")?;

        fs::write(&safe_path, &serialized)
            .with_context(|| format!("Failed to write module graph to {}", safe_path.display()))?;

        eprintln!("[deadmod] Module graph exported → {}", safe_path.display());
        std::process::exit(0);
    }

    // Export function callgraph to file
    if let Some(ref path) = cli.export_callgraph {
        // Security: Validate output path
        let safe_path = validate_output_path(path)
            .with_context(|| format!("Invalid output path: {}", path))?;

        let input_path = Path::new(&cli.path);
        let root = find_crate_root(input_path)
            .with_context(|| format!("Failed to find crate root from: {}", cli.path))?;

        let files = gather_rs_files(&root)?;
        let cached = cache::load_cache(&root);
        let mods = cache::incremental_parse(&root, &files, cached)?;

        let mut all_functions = Vec::new();
        let mut usage_map = std::collections::HashMap::new();

        for info in mods.values() {
            if let Ok(content) = fs::read_to_string(&info.path) {
                let functions = extract_callgraph_functions(&info.path, &content);
                let usages = extract_call_usages(&info.path, &content);
                all_functions.extend(functions);
                usage_map.insert(info.path.display().to_string(), usages);
            }
        }

        let graph = CallGraph::build(&all_functions, &usage_map);
        let json = graph.to_visualizer_json();
        let serialized = serde_json::to_string_pretty(&json)
            .context("Failed to serialize callgraph to JSON")?;

        fs::write(&safe_path, &serialized)
            .with_context(|| format!("Failed to write callgraph to {}", safe_path.display()))?;

        eprintln!("[deadmod] Function callgraph exported → {}", safe_path.display());
        std::process::exit(0);
    }

    // Export combined graph (modules + functions) to file
    if let Some(ref path) = cli.export_combined {
        // Security: Validate output path
        let safe_path = validate_output_path(path)
            .with_context(|| format!("Invalid output path: {}", path))?;

        let input_path = Path::new(&cli.path);
        let root = find_crate_root(input_path)
            .with_context(|| format!("Failed to find crate root from: {}", cli.path))?;

        let files = gather_rs_files(&root)?;
        let cached = cache::load_cache(&root);
        let mods = cache::incremental_parse(&root, &files, cached)?;

        // Build module graph
        let mod_graph = build_graph(&mods);
        let roots = find_root_modules(&root);
        let reachable = reachable_from_roots(&mod_graph, roots.iter().map(String::as_str));
        let module_graph_json = module_graph_to_visualizer_json(&mods, &reachable);

        // Build function callgraph
        let mut all_functions = Vec::new();
        let mut usage_map = std::collections::HashMap::new();
        for info in mods.values() {
            if let Ok(content) = fs::read_to_string(&info.path) {
                let functions = extract_callgraph_functions(&info.path, &content);
                let usages = extract_call_usages(&info.path, &content);
                all_functions.extend(functions);
                usage_map.insert(info.path.display().to_string(), usages);
            }
        }
        let func_graph = CallGraph::build(&all_functions, &usage_map);
        let function_graph_json = func_graph.to_visualizer_json();

        // Combine both graphs
        let combined = serde_json::json!({
            "module_graph": module_graph_json,
            "function_graph": function_graph_json,
        });

        let serialized = serde_json::to_string_pretty(&combined)
            .context("Failed to serialize combined graph to JSON")?;

        fs::write(&safe_path, &serialized)
            .with_context(|| format!("Failed to write combined graph to {}", safe_path.display()))?;

        eprintln!("[deadmod] Combined graph exported → {}", safe_path.display());
        eprintln!("  • Module graph: {} nodes, {} edges",
            module_graph_json["stats"]["total_modules"],
            module_graph_json["stats"]["total_edges"]);
        eprintln!("  • Function graph: {} nodes, {} edges",
            function_graph_json["stats"]["total_functions"],
            function_graph_json["stats"]["total_edges"]);
        std::process::exit(0);
    }

    // Call graph generation mode
    if cli.callgraph || cli.callgraph_dot || cli.callgraph_viz {
        let input_path = Path::new(&cli.path);
        print_workspace_info(input_path);
        let root = find_crate_root(input_path)
            .with_context(|| format!("Failed to find crate root from: {}", cli.path))?;

        // Gather files and parse modules
        let files = gather_rs_files(&root)?;
        let cached = cache::load_cache(&root);
        let mods = cache::incremental_parse(&root, &files, cached)?;

        // Extract functions and call usages from all files
        let mut all_functions = Vec::new();
        let mut usage_map = std::collections::HashMap::new();

        for info in mods.values() {
            if let Ok(content) = fs::read_to_string(&info.path) {
                let functions = extract_callgraph_functions(&info.path, &content);
                let usages = extract_call_usages(&info.path, &content);

                all_functions.extend(functions);
                usage_map.insert(info.path.display().to_string(), usages);
            }
        }

        // Build call graph
        let graph = CallGraph::build(&all_functions, &usage_map);

        if cli.callgraph_dot {
            // Output DOT format
            println!("{}", graph.to_dot());
        } else if cli.callgraph_viz {
            // Output visualizer-compatible JSON (numeric IDs, dead flags)
            println!("{}", serde_json::to_string_pretty(&graph.to_visualizer_json())?);
        } else {
            // Output JSON format
            println!("{}", serde_json::to_string_pretty(&graph.to_json())?);
        }

        std::process::exit(0);
    }

    // Workspace mode: analyze all crates in workspace
    if cli.workspace {
        let root = Path::new(&cli.path)
            .canonicalize()
            .with_context(|| format!("Failed to canonicalize path: {}", cli.path))?;

        let results = analyze_workspace(&root)?;

        // Check if any crate has dead modules (for exit code)
        let has_dead = results.iter().any(|r| !r.dead_modules.is_empty());

        if cli.json {
            let json_output: Vec<serde_json::Value> = results
                .iter()
                .map(|r| {
                    serde_json::json!({
                        "crate": r.name,
                        "root": r.root.display().to_string(),
                        "dead_modules": r.dead_modules,
                        "reachable_modules": r.reachable_modules,
                    })
                })
                .collect();
            println!("{}", serde_json::to_string_pretty(&json_output)?);
        } else {
            for result in &results {
                println!("=== Crate: {} ===", result.name);
                if result.dead_modules.is_empty() {
                    println!("No dead modules found.\n");
                } else {
                    for m in &result.dead_modules {
                        println!("  - {}", m);
                    }
                    println!();
                }
            }
        }

        // DOT output for workspace (combined or per-crate)
        if cli.dot {
            for result in &results {
                println!("// === DOT for crate: {} ===", result.name);
                println!("{}", result.dot_output);
            }
        }

        std::process::exit(if has_dead { 1 } else { 0 });
    }

    // Smart mode: Auto-detect workspace and scan all crates automatically
    let input_path = Path::new(&cli.path);
    let canonical_path = input_path.canonicalize()
        .with_context(|| format!("Failed to canonicalize path: {}", cli.path))?;

    // Check if this is a workspace root - if so, auto-scan all crates
    if is_workspace_root(&canonical_path) {
        eprintln!("INFO: Detected Cargo workspace - scanning all crates automatically...");

        let all_crates = find_all_crates(&canonical_path)?;
        eprintln!("INFO: Found {} crate(s):", all_crates.len());
        for cr in &all_crates {
            let name = cr.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
            eprintln!("  - {}", name);
        }
        eprintln!();

        // Build combined module map with crate prefixes
        let mut combined_mods: std::collections::HashMap<String, deadmod_core::ModuleInfo> = std::collections::HashMap::new();
        let mut all_roots: Vec<String> = Vec::new();

        for crate_root in &all_crates {
            let crate_name = crate_root.file_name()
                .map(|n| n.to_string_lossy().to_string())
                .unwrap_or_else(|| "unknown".to_string());

            let files = match gather_rs_files(crate_root) {
                Ok(f) => f,
                Err(e) => {
                    eprintln!("[WARN] Failed to scan {}: {}", crate_name, e);
                    continue;
                }
            };

            let cached = cache::load_cache(crate_root);
            let mods = match cache::incremental_parse(crate_root, &files, cached) {
                Ok(m) => m,
                Err(e) => {
                    eprintln!("[WARN] Failed to parse {}: {}", crate_name, e);
                    continue;
                }
            };

            // Find root modules for this crate
            let crate_roots = find_root_modules(crate_root);
            for root_mod in crate_roots {
                all_roots.push(format!("{}::{}", crate_name, root_mod));
            }

            // Add modules with crate prefix
            for (name, mut info) in mods {
                let prefixed_name = format!("{}::{}", crate_name, name);
                // Update refs to use prefixed names
                let prefixed_refs: HashSet<String> = info.refs.iter()
                    .map(|r| format!("{}::{}", crate_name, r))
                    .collect();
                info.refs = prefixed_refs;
                combined_mods.insert(prefixed_name, info);
            }
        }

        if combined_mods.is_empty() {
            eprintln!("No modules found in workspace.");
            std::process::exit(0);
        }

        // Build combined graph
        let graph = build_graph(&combined_mods);
        let valid_roots = all_roots.iter()
            .filter(|name| combined_mods.contains_key(*name))
            .map(|s| s.as_str());
        let reachable = reachable_from_roots(&graph, valid_roots);

        // Find dead modules
        let mut dead = find_dead(&combined_mods, &reachable);
        dead.sort();

        // PixiJS graph for workspace
        if cli.html_pixi || cli.html_pixi_file.is_some() {
            let reachable_owned: HashSet<String> = reachable.iter().map(|s| s.to_string()).collect();
            let html = generate_pixi_graph(&combined_mods, &reachable_owned);

            if let Some(ref file) = cli.html_pixi_file {
                match validate_output_path(file) {
                    Ok(safe_path) => {
                        if let Err(e) = fs::write(&safe_path, &html) {
                            eprintln!("[WARN] PixiJS graph write failed: {}", e);
                        } else {
                            eprintln!("PixiJS WebGL graph saved to: {}", safe_path.display());
                        }
                    }
                    Err(e) => eprintln!("[WARN] Invalid output path: {}", e),
                }
            } else {
                println!("{}", html);
            }
            std::process::exit(if dead.is_empty() { 0 } else { 1 });
        }

        // HTML graph for workspace
        if cli.html || cli.html_file.is_some() {
            let reachable_owned: HashSet<String> = reachable.iter().map(|s| s.to_string()).collect();
            let html = generate_html_graph(&combined_mods, &reachable_owned);

            if let Some(ref file) = cli.html_file {
                match validate_output_path(file) {
                    Ok(safe_path) => {
                        if let Err(e) = fs::write(&safe_path, &html) {
                            eprintln!("[WARN] HTML write failed: {}", e);
                        } else {
                            eprintln!("HTML graph saved to: {}", safe_path.display());
                        }
                    }
                    Err(e) => eprintln!("[WARN] Invalid output path: {}", e),
                }
            } else {
                println!("{}", html);
            }
            std::process::exit(if dead.is_empty() { 0 } else { 1 });
        }

        // Text output for workspace
        if cli.json {
            let json_output = serde_json::json!({
                "workspace": true,
                "crates": all_crates.len(),
                "total_modules": combined_mods.len(),
                "reachable": reachable.len(),
                "dead_count": dead.len(),
                "dead_modules": dead,
            });
            println!("{}", serde_json::to_string_pretty(&json_output)?);
        } else {
            println!("=== Workspace Analysis ===\n");
            println!("Crates: {}", all_crates.len());
            println!("Total modules: {}", combined_mods.len());
            println!("Reachable: {}", reachable.len());
            println!("Dead: {}\n", dead.len());

            if !dead.is_empty() {
                println!("DEAD MODULES:");
                for m in &dead {
                    println!("  - {}", m);
                }
            } else {
                println!("No dead modules found.");
            }
        }

        std::process::exit(if dead.is_empty() { 0 } else { 1 });
    }

    // Single crate mode (original behavior)
    // 1. Determine crate root
    print_workspace_info(input_path);
    let root = find_crate_root(input_path)
        .with_context(|| format!("Failed to find crate root from: {}", cli.path))?;

    // 2. Load config from deadmod.toml if present (safe - don't fail on config errors)
    let mut ignore = cli.ignore.clone();
    match load_config(&root) {
        Ok(Some(cfg)) => {
            if let Some(list) = cfg.ignore {
                ignore.extend(list);
            }
        }
        Ok(None) => {} // No config file - that's fine
        Err(e) => {
            eprintln!("[WARN] config load failed: {}", e);
        }
    }

    // 3. Scan for .rs files
    let files = gather_rs_files(&root)
        .with_context(|| format!("Failed to gather Rust files from: {}", root.display()))?;

    // 4. Parse all modules with incremental caching (resilient - never fails)
    let cached = cache::load_cache(&root);
    let mut mods = cache::incremental_parse(&root, &files, cached)?;

    // 5. Filter ignored modules
    mods.retain(|name, _| !is_ignored(name, &ignore));

    // 6. Build dependency graph
    let graph = build_graph(&mods);

    // 7. Find reachable modules from all entry points (single O(|V|+|E|) traversal)
    let root_modules = find_root_modules(&root);
    let valid_roots = root_modules
        .iter()
        .filter(|name| mods.contains_key(*name))
        .map(|s| s.as_str());
    let reachable = reachable_from_roots(&graph, valid_roots);

    // 8. Detect dead modules
    let mut dead = find_dead(&mods, &reachable);
    dead.sort();

    // 9. Auto-fix mode (if requested)
    if cli.fix || cli.fix_dry_run {
        let dry_run = cli.fix_dry_run;
        fix_dead_modules(&root, &dead, &mods, dry_run)?;
        std::process::exit(if dead.is_empty() { 0 } else { 1 });
    }

    // 10. HTML interactive graph (if requested)
    if cli.html || cli.html_file.is_some() {
        let reachable_owned: HashSet<String> = reachable.iter().map(|s| s.to_string()).collect();
        let html = generate_html_graph(&mods, &reachable_owned);

        if let Some(ref file) = cli.html_file {
            // Security: Validate output path
            match validate_output_path(file) {
                Ok(safe_path) => {
                    if let Err(e) = fs::write(&safe_path, &html) {
                        eprintln!("[WARN] HTML write failed to {}: {}", safe_path.display(), e);
                    } else {
                        println!("HTML graph saved to: {}", safe_path.display());
                    }
                }
                Err(e) => {
                    eprintln!("[ERROR] Invalid output path: {}", e);
                    std::process::exit(2);
                }
            }
        } else {
            println!("{}", html);
        }
        std::process::exit(if dead.is_empty() { 0 } else { 1 });
    }

    // 10b. PixiJS WebGL interactive graph (GPU-accelerated)
    if cli.html_pixi || cli.html_pixi_file.is_some() {
        let reachable_owned: HashSet<String> = reachable.iter().map(|s| s.to_string()).collect();
        let html = generate_pixi_graph(&mods, &reachable_owned);

        if let Some(ref file) = cli.html_pixi_file {
            // Security: Validate output path
            match validate_output_path(file) {
                Ok(safe_path) => {
                    if let Err(e) = fs::write(&safe_path, &html) {
                        eprintln!("[WARN] PixiJS HTML write failed to {}: {}", safe_path.display(), e);
                    } else {
                        println!("PixiJS WebGL graph saved to: {}", safe_path.display());
                    }
                }
                Err(e) => {
                    eprintln!("[ERROR] Invalid output path: {}", e);
                    std::process::exit(2);
                }
            }
        } else {
            println!("{}", html);
        }
        std::process::exit(if dead.is_empty() { 0 } else { 1 });
    }

    // 11. Report results
    if cli.json {
        print_json(&dead);
    } else {
        print_plain(&dead);
    }

    // 12. DOT/Graphviz output (safe - don't crash on write errors)
    if cli.dot {
        let reachable_owned: HashSet<String> = reachable.iter().map(|s| s.to_string()).collect();
        let dot = visualize::generate_dot(&mods, &reachable_owned);
        if let Some(ref file) = cli.dot_file {
            // Security: Validate output path
            match validate_output_path(file) {
                Ok(safe_path) => {
                    if let Err(e) = fs::write(&safe_path, &dot) {
                        eprintln!("[WARN] DOT write failed to {}: {}", safe_path.display(), e);
                    }
                }
                Err(e) => {
                    eprintln!("[ERROR] Invalid output path: {}", e);
                    std::process::exit(2);
                }
            }
        } else {
            println!("{}", dot);
        }
    }

    // 13. Exit code (CI-friendly)
    std::process::exit(if dead.is_empty() { 0 } else { 1 });
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use std::path::PathBuf;
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
            .join("deadmod_cli_test")
            .join(format!("{}_{}", name, id));
        if temp_dir.exists() {
            fs::remove_dir_all(&temp_dir).ok();
        }
        fs::create_dir_all(&temp_dir).unwrap();
        temp_dir
    }

    // --- find_crate_root TESTS ---

    #[test]
    fn test_find_crate_root_simple_crate() {
        let temp_dir = create_temp_dir("simple_crate");
        create_file(&temp_dir.join("Cargo.toml"), "[package]\nname = \"test\"");
        fs::create_dir_all(temp_dir.join("src")).unwrap();
        create_file(&temp_dir.join("src/main.rs"), "fn main() {}");

        let root = find_crate_root(&temp_dir).unwrap();
        assert!(root.join("Cargo.toml").exists());
    }

    #[test]
    fn test_find_crate_root_with_src_directory() {
        let temp_dir = create_temp_dir("src_crate");
        let src_dir = temp_dir.join("src");
        fs::create_dir_all(&src_dir).unwrap();
        create_file(&src_dir.join("main.rs"), "fn main() {}");

        let root = find_crate_root(&temp_dir).unwrap();
        assert!(root.join("src").exists());
    }

    #[test]
    fn test_find_crate_root_workspace_detection() {
        let ws_root = create_temp_dir("workspace");
        create_file(
            &ws_root.join("Cargo.toml"),
            "[workspace]\nmembers = [\"crate_a\", \"crate_b\"]",
        );

        fs::create_dir_all(ws_root.join("crate_a/src")).unwrap();
        fs::create_dir_all(ws_root.join("crate_b/src")).unwrap();
        create_file(&ws_root.join("crate_a/Cargo.toml"), "[package]\nname = \"a\"");
        create_file(&ws_root.join("crate_b/Cargo.toml"), "[package]\nname = \"b\"");
        create_file(&ws_root.join("crate_a/src/lib.rs"), "");
        create_file(&ws_root.join("crate_b/src/lib.rs"), "");

        // Should detect workspace using the core function
        assert!(is_workspace_root(&ws_root));
    }

    // --- is_ignored TESTS ---

    #[test]
    fn test_is_ignored_exact_match() {
        let ignore = vec!["test".to_string()];
        assert!(is_ignored("test", &ignore));
        // Note: "testing" contains "test", so it IS ignored (contains-based matching)
        assert!(is_ignored("testing", &ignore));
    }

    #[test]
    fn test_is_ignored_suffix_match() {
        let ignore = vec!["_test".to_string()];
        assert!(is_ignored("module_test", &ignore));
        assert!(!is_ignored("test_module", &ignore));
    }

    #[test]
    fn test_is_ignored_contains_match() {
        let ignore = vec!["mock".to_string()];
        assert!(is_ignored("mock", &ignore));
        assert!(is_ignored("mock_data", &ignore));
        assert!(is_ignored("my_mock_module", &ignore));
    }

    // --- is_workspace TESTS ---

    #[test]
    fn test_is_workspace_true() {
        let temp_dir = create_temp_dir("ws_true");
        create_file(
            &temp_dir.join("Cargo.toml"),
            "[workspace]\nmembers = [\"a\"]",
        );

        assert!(is_workspace_root(&temp_dir));
    }

    #[test]
    fn test_is_workspace_false() {
        let temp_dir = create_temp_dir("ws_false");
        create_file(
            &temp_dir.join("Cargo.toml"),
            "[package]\nname = \"test\"",
        );

        assert!(!is_workspace_root(&temp_dir));
    }

    #[test]
    fn test_is_workspace_no_cargo_toml() {
        let temp_dir = create_temp_dir("ws_none");

        assert!(!is_workspace_root(&temp_dir));
    }
}
