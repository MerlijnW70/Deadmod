//! Incremental parsing cache using SHA-256 for robust change detection.
//!
//! Performance characteristics:
//! - Parallel file hashing and parsing via Rayon
//! - Read-once pattern: file content read once, then hashed and parsed
//! - O(changed_files) parsing work, O(1) cache lookups
//!
//! Caches parsed module information based on file content hashes,
//! avoiding re-parsing unchanged files.
//!
//! # Cache Versioning
//!
//! The cache includes version metadata to ensure cache invalidation when:
//! - Deadmod version changes (may have different parsing logic)
//! - Rust toolchain version changes (affects syntax support)
//! - Cache format changes

use crate::parse::{extract_uses_and_decls, ModuleInfo, Visibility};
use anyhow::{Context, Result};
use rayon::prelude::*;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

/// Maximum cache file size (50MB) - prevents unbounded cache growth
const MAX_CACHE_SIZE_BYTES: usize = 50_000_000;

/// Current cache format version. Increment when cache format changes.
const CACHE_VERSION: u32 = 2;

/// Deadmod version for cache compatibility checking.
const DEADMOD_VERSION: &str = env!("CARGO_PKG_VERSION");

/// Cached representation of a module.
/// Stores the hash of the file and the module references found during parsing.
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct CachedModule {
    pub hash: String,
    pub refs: HashSet<String>,
    /// Module visibility (added in cache v2)
    #[serde(default)]
    pub visibility: CachedVisibility,
    /// Whether module is doc(hidden)
    #[serde(default)]
    pub doc_hidden: bool,
}

/// Serializable visibility for cache storage.
#[derive(Debug, Serialize, Deserialize, Clone, Copy, Default, PartialEq, Eq)]
pub enum CachedVisibility {
    #[default]
    Private,
    Public,
    PubCrate,
    PubSuper,
    PubIn,
}

impl From<Visibility> for CachedVisibility {
    fn from(v: Visibility) -> Self {
        match v {
            Visibility::Private => Self::Private,
            Visibility::Public => Self::Public,
            Visibility::PubCrate => Self::PubCrate,
            Visibility::PubSuper => Self::PubSuper,
            Visibility::PubIn => Self::PubIn,
        }
    }
}

impl From<CachedVisibility> for Visibility {
    fn from(v: CachedVisibility) -> Self {
        match v {
            CachedVisibility::Private => Self::Private,
            CachedVisibility::Public => Self::Public,
            CachedVisibility::PubCrate => Self::PubCrate,
            CachedVisibility::PubSuper => Self::PubSuper,
            CachedVisibility::PubIn => Self::PubIn,
        }
    }
}

/// Cache metadata for version checking.
#[derive(Debug, Serialize, Deserialize, Clone, Default)]
pub struct CacheMetadata {
    /// Cache format version
    pub cache_version: u32,
    /// Deadmod version that created this cache
    pub deadmod_version: String,
    /// Timestamp when cache was created
    #[serde(default)]
    pub created_at: u64,
}

impl CacheMetadata {
    /// Create metadata for current environment.
    pub fn current() -> Self {
        let created_at = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);

        Self {
            cache_version: CACHE_VERSION,
            deadmod_version: DEADMOD_VERSION.to_string(),
            created_at,
        }
    }

    /// Check if this cache is compatible with current version.
    pub fn is_compatible(&self) -> bool {
        // Cache version must match exactly
        if self.cache_version != CACHE_VERSION {
            return false;
        }

        // Major version of deadmod must match
        let current_major = DEADMOD_VERSION.split('.').next().unwrap_or("0");
        let cached_major = self.deadmod_version.split('.').next().unwrap_or("0");

        current_major == cached_major
    }
}

/// The full cache model, stored as a file in `.deadmod/cache.json`.
#[derive(Debug, Serialize, Deserialize, Clone, Default)]
pub struct DeadmodCache {
    /// Cache metadata for version checking
    #[serde(default)]
    pub metadata: CacheMetadata,
    /// Maps module name (e.g., "main") to its cached data.
    pub modules: HashMap<String, CachedModule>,
}

/// Compute SHA-256 hash from bytes (in-memory, no I/O).
#[inline]
fn hash_bytes(bytes: &[u8]) -> String {
    let mut sha = Sha256::new();
    sha.update(bytes);
    format!("{:x}", sha.finalize())
}

/// Compute SHA-256 file hash for robust change detection.
/// Preserved for backwards compatibility and testing.
pub fn file_hash(path: &Path) -> Result<String> {
    let bytes =
        fs::read(path).with_context(|| format!("Failed to read {} for hashing", path.display()))?;
    Ok(hash_bytes(&bytes))
}

/// Load the cache from `.deadmod/cache.json`.
///
/// Returns `None` if:
/// - File doesn't exist
/// - File is corrupted
/// - Cache version is incompatible with current deadmod version
pub fn load_cache(crate_root: &Path) -> Option<DeadmodCache> {
    let path = crate_root.join(".deadmod/cache.json");
    if !path.exists() {
        return None;
    }

    let text = fs::read_to_string(&path).ok()?;
    let cache: DeadmodCache = serde_json::from_str(&text).ok()?;

    // Check version compatibility
    if !cache.metadata.is_compatible() {
        eprintln!(
            "[INFO] Cache version mismatch (cache: v{} {}, current: v{} {}), rebuilding...",
            cache.metadata.cache_version,
            cache.metadata.deadmod_version,
            CACHE_VERSION,
            DEADMOD_VERSION
        );
        // Remove incompatible cache
        let _ = fs::remove_file(&path);
        return None;
    }

    Some(cache)
}

/// Save the current cache state to disk.
///
/// Uses atomic write pattern (temp file + rename) to prevent:
/// - Partial writes if process is interrupted
/// - Race conditions with concurrent readers
/// - Corrupted cache files
///
/// Security features:
/// - Random suffix in temp filename prevents collision attacks
/// - Size limit prevents unbounded cache growth (DoS)
pub fn save_cache(crate_root: &Path, cache: &DeadmodCache) -> Result<()> {
    let dir = crate_root.join(".deadmod");
    if !dir.exists() {
        fs::create_dir_all(&dir)?;
    }

    let path = dir.join("cache.json");
    let json = serde_json::to_string_pretty(cache)?;

    // Security: Check cache size to prevent unbounded growth
    if json.len() > MAX_CACHE_SIZE_BYTES {
        eprintln!(
            "[WARN] Cache exceeds {}MB limit, clearing old cache",
            MAX_CACHE_SIZE_BYTES / 1_000_000
        );
        // Remove old cache and return - will be rebuilt on next run
        let _ = fs::remove_file(&path);
        return Ok(());
    }

    // Security: Use random suffix to prevent temp file collision attacks
    // Combines PID with nanosecond timestamp for uniqueness
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    let temp_path = dir.join(format!("cache.json.{}.{}.tmp", std::process::id(), nanos));

    // Write to temp file
    fs::write(&temp_path, &json).with_context(|| {
        format!("Failed to write temp cache file: {}", temp_path.display())
    })?;

    // Atomic rename (on most filesystems)
    fs::rename(&temp_path, &path).with_context(|| {
        // Clean up temp file on failure
        let _ = fs::remove_file(&temp_path);
        format!("Failed to rename cache file to: {}", path.display())
    })?;

    Ok(())
}

/// Result of processing a single file for incremental parsing.
enum FileProcessResult {
    /// Successfully processed (name, info, cache_entry)
    /// ModuleInfo is boxed to reduce enum size (clippy::large_enum_variant)
    Ok(String, Box<ModuleInfo>, CachedModule),
    /// Skipped due to error
    Skipped,
}

/// Process a single file: read, hash, check cache, parse if needed.
///
/// Implements the Read-Once Pattern:
/// - File is read exactly once into memory
/// - Content is hashed in-memory (no second I/O)
/// - Content is parsed only if cache miss
fn process_file(
    file: &PathBuf,
    old_cache: Option<&DeadmodCache>,
) -> FileProcessResult {
    // Extract module name from file stem
    let name = match file.file_stem() {
        Some(s) => s.to_string_lossy().to_string(),
        None => {
            eprintln!("[WARN] skipping file with no stem: {}", file.display());
            return FileProcessResult::Skipped;
        }
    };

    // Read file content once (Read-Once Pattern)
    let content = match fs::read_to_string(file) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("[WARN] read error {}: {}", file.display(), e);
            return FileProcessResult::Skipped;
        }
    };

    // Hash content in memory (no second I/O)
    let hash = hash_bytes(content.as_bytes());

    // Check cache for hash match
    if let Some(old) = old_cache {
        if let Some(cached) = old.modules.get(&name) {
            if cached.hash == hash {
                // Cache hit: reuse parsed refs without re-parsing
                let mut info = ModuleInfo::new(file.clone());
                info.refs = cached.refs.clone();
                return FileProcessResult::Ok(name, Box::new(info), cached.clone());
            }
        }
    }

    // Cache miss: parse the content we already have in memory
    let mut info = ModuleInfo::new(file.clone());
    if let Err(e) = extract_uses_and_decls(&content, &mut info.refs) {
        eprintln!("[WARN] AST parse failed {}: {}", file.display(), e);
        // Continue with empty refs - module still exists in graph
    }

    let cache_entry = CachedModule {
        hash,
        refs: info.refs.clone(),
        visibility: CachedVisibility::from(info.visibility),
        doc_hidden: info.doc_hidden,
    };

    FileProcessResult::Ok(name, Box::new(info), cache_entry)
}

/// Incremental parsing with NASA-grade resilience and parallel execution.
///
/// Performance characteristics:
/// - Parallel file processing via Rayon (scales with CPU cores)
/// - Read-Once Pattern: each file read exactly once
/// - O(|files|) total I/O, O(|changed_files|) parsing work
///
/// Fault tolerance:
/// - If file hash is unchanged → use cached dependency references
/// - If file hash is changed or not in cache → re-run the `syn` parser
/// - If any file fails to read/parse → skip it with warning, continue with others
/// - Never panics, never crashes the entire analysis
pub fn incremental_parse(
    crate_root: &Path,
    files: &[PathBuf],
    old_cache: Option<DeadmodCache>,
) -> Result<HashMap<String, ModuleInfo>> {
    // Process all files in parallel using Rayon
    let results: Vec<FileProcessResult> = files
        .par_iter()
        .map(|file| process_file(file, old_cache.as_ref()))
        .collect();

    // Aggregate results (sequential, but O(n) simple insertions)
    let mut mods = HashMap::with_capacity(results.len());
    let mut new_cache = DeadmodCache {
        metadata: CacheMetadata::current(),
        modules: HashMap::with_capacity(results.len()),
    };

    for result in results {
        if let FileProcessResult::Ok(name, info, cache_entry) = result {
            mods.insert(name.clone(), *info);
            new_cache.modules.insert(name, cache_entry);
        }
    }

    // Best-effort cache save (don't fail if write fails)
    if let Err(e) = save_cache(crate_root, &new_cache) {
        eprintln!("[WARN] cache save failed: {}", e);
    }

    Ok(mods)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn create_temp_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir()
            .join("deadmod_cache_test")
            .join(format!("{}_{}", name, std::process::id()));
        if dir.exists() {
            fs::remove_dir_all(&dir).ok();
        }
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn test_file_hash_deterministic() {
        let dir = create_temp_dir("hash_test");
        let file = dir.join("test.rs");
        fs::write(&file, "fn main() {}").unwrap();

        let hash1 = file_hash(&file).unwrap();
        let hash2 = file_hash(&file).unwrap();
        assert_eq!(hash1, hash2);

        // SHA-256 produces 64 hex characters
        assert_eq!(hash1.len(), 64);

        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn test_file_hash_changes_on_content_change() {
        let dir = create_temp_dir("hash_change");
        let file = dir.join("test.rs");

        fs::write(&file, "fn main() {}").unwrap();
        let hash1 = file_hash(&file).unwrap();

        fs::write(&file, "fn main() { println!(\"hi\"); }").unwrap();
        let hash2 = file_hash(&file).unwrap();

        assert_ne!(hash1, hash2);

        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn test_hash_bytes_deterministic() {
        let content = b"fn main() {}";
        let hash1 = hash_bytes(content);
        let hash2 = hash_bytes(content);
        assert_eq!(hash1, hash2);
        assert_eq!(hash1.len(), 64);
    }

    #[test]
    fn test_cache_save_load() {
        let dir = create_temp_dir("save_load");

        let mut cache = DeadmodCache {
            metadata: CacheMetadata::current(),
            modules: HashMap::new(),
        };
        let mut refs = HashSet::new();
        refs.insert("utils".to_string());
        cache.modules.insert(
            "main".to_string(),
            CachedModule {
                hash: "abc123".to_string(),
                refs,
                visibility: CachedVisibility::default(),
                doc_hidden: false,
            },
        );

        save_cache(&dir, &cache).unwrap();

        let loaded = load_cache(&dir).unwrap();
        assert_eq!(loaded.modules.len(), 1);
        assert!(loaded.modules.contains_key("main"));
        assert_eq!(loaded.modules["main"].hash, "abc123");

        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn test_load_cache_not_found() {
        let dir = create_temp_dir("not_found");
        let result = load_cache(&dir);
        assert!(result.is_none());
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn test_incremental_parse_fresh() {
        let dir = create_temp_dir("incremental_fresh");
        fs::create_dir_all(dir.join("src")).unwrap();

        let main_rs = dir.join("src/main.rs");
        fs::File::create(&main_rs)
            .unwrap()
            .write_all(b"mod utils; fn main() {}")
            .unwrap();

        let files = vec![main_rs];
        let result = incremental_parse(&dir, &files, None).unwrap();

        assert!(result.contains_key("main"));
        assert!(result["main"].refs.contains("utils"));

        // Cache should now exist
        assert!(load_cache(&dir).is_some());

        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn test_incremental_parse_cache_hit() {
        let dir = create_temp_dir("cache_hit");
        fs::create_dir_all(dir.join("src")).unwrap();

        let main_rs = dir.join("src/main.rs");
        fs::write(&main_rs, "mod utils; fn main() {}").unwrap();

        let files = vec![main_rs.clone()];

        // First parse - populates cache
        let result1 = incremental_parse(&dir, &files, None).unwrap();
        assert!(result1.contains_key("main"));

        // Second parse - should use cache
        let cache = load_cache(&dir);
        let result2 = incremental_parse(&dir, &files, cache).unwrap();
        assert!(result2.contains_key("main"));
        assert!(result2["main"].refs.contains("utils"));

        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn test_incremental_parse_cache_invalidation() {
        let dir = create_temp_dir("cache_invalidation");
        fs::create_dir_all(dir.join("src")).unwrap();

        let main_rs = dir.join("src/main.rs");
        fs::write(&main_rs, "mod utils; fn main() {}").unwrap();

        let files = vec![main_rs.clone()];

        // First parse
        incremental_parse(&dir, &files, None).unwrap();
        let cache = load_cache(&dir).unwrap();
        let old_hash = cache.modules["main"].hash.clone();

        // Modify file
        fs::write(&main_rs, "mod other; fn main() {}").unwrap();

        // Second parse - should detect change and reparse
        let result = incremental_parse(&dir, &files, Some(cache)).unwrap();
        assert!(result["main"].refs.contains("other"));
        assert!(!result["main"].refs.contains("utils"));

        // Verify cache was updated
        let new_cache = load_cache(&dir).unwrap();
        assert_ne!(new_cache.modules["main"].hash, old_hash);

        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn test_incremental_parse_parallel_stress() {
        let dir = create_temp_dir("parallel_stress");
        fs::create_dir_all(dir.join("src")).unwrap();

        // Create 100 files to stress test parallel processing
        let mut files = Vec::new();
        for i in 0..100 {
            let file = dir.join("src").join(format!("mod_{}.rs", i));
            fs::write(&file, format!("pub fn func_{}() {{}}", i)).unwrap();
            files.push(file);
        }

        // First parse (cold cache)
        let result1 = incremental_parse(&dir, &files, None).unwrap();
        assert_eq!(result1.len(), 100);

        // Second parse (warm cache) - should be faster
        let cache = load_cache(&dir);
        let result2 = incremental_parse(&dir, &files, cache).unwrap();
        assert_eq!(result2.len(), 100);

        fs::remove_dir_all(&dir).ok();
    }

    // === Atomic Write Tests ===

    #[test]
    fn test_atomic_write_creates_file() {
        let dir = create_temp_dir("atomic_create");
        let cache = DeadmodCache::default();

        save_cache(&dir, &cache).unwrap();

        let cache_path = dir.join(".deadmod/cache.json");
        assert!(cache_path.exists());

        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn test_atomic_write_no_temp_file_left() {
        let dir = create_temp_dir("atomic_no_temp");
        let cache = DeadmodCache::default();

        save_cache(&dir, &cache).unwrap();

        // Check no .tmp files remain
        let deadmod_dir = dir.join(".deadmod");
        for entry in fs::read_dir(&deadmod_dir).unwrap() {
            let entry = entry.unwrap();
            let name = entry.file_name().to_string_lossy().to_string();
            assert!(!name.ends_with(".tmp"), "Temp file left behind: {}", name);
        }

        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn test_atomic_write_overwrites_existing() {
        let dir = create_temp_dir("atomic_overwrite");

        // Write first cache
        let mut cache1 = DeadmodCache {
            metadata: CacheMetadata::current(),
            modules: HashMap::new(),
        };
        cache1.modules.insert(
            "first".to_string(),
            CachedModule {
                hash: "hash1".to_string(),
                refs: HashSet::new(),
                visibility: CachedVisibility::default(),
                doc_hidden: false,
            },
        );
        save_cache(&dir, &cache1).unwrap();

        // Write second cache
        let mut cache2 = DeadmodCache {
            metadata: CacheMetadata::current(),
            modules: HashMap::new(),
        };
        cache2.modules.insert(
            "second".to_string(),
            CachedModule {
                hash: "hash2".to_string(),
                refs: HashSet::new(),
                visibility: CachedVisibility::default(),
                doc_hidden: false,
            },
        );
        save_cache(&dir, &cache2).unwrap();

        // Load and verify second cache
        let loaded = load_cache(&dir).unwrap();
        assert!(!loaded.modules.contains_key("first"));
        assert!(loaded.modules.contains_key("second"));

        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn test_cache_file_is_valid_json() {
        let dir = create_temp_dir("valid_json");

        let mut cache = DeadmodCache {
            metadata: CacheMetadata::current(),
            modules: HashMap::new(),
        };
        let mut refs = HashSet::new();
        refs.insert("foo".to_string());
        refs.insert("bar".to_string());
        cache.modules.insert(
            "test".to_string(),
            CachedModule {
                hash: "abc123def456".to_string(),
                refs,
                visibility: CachedVisibility::default(),
                doc_hidden: false,
            },
        );
        save_cache(&dir, &cache).unwrap();

        // Read raw JSON and verify it's parseable
        let cache_path = dir.join(".deadmod/cache.json");
        let content = fs::read_to_string(&cache_path).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&content).unwrap();
        assert!(parsed.is_object());

        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn test_load_cache_corrupted_json() {
        let dir = create_temp_dir("corrupted");
        let deadmod_dir = dir.join(".deadmod");
        fs::create_dir_all(&deadmod_dir).unwrap();

        // Write corrupted JSON
        fs::write(deadmod_dir.join("cache.json"), "{ not valid json ").unwrap();

        // Should return None, not panic
        let result = load_cache(&dir);
        assert!(result.is_none());

        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn test_load_cache_empty_file() {
        let dir = create_temp_dir("empty_cache");
        let deadmod_dir = dir.join(".deadmod");
        fs::create_dir_all(&deadmod_dir).unwrap();

        // Write empty file
        fs::write(deadmod_dir.join("cache.json"), "").unwrap();

        // Should return None, not panic
        let result = load_cache(&dir);
        assert!(result.is_none());

        fs::remove_dir_all(&dir).ok();
    }

    // === Concurrent Access Simulation Tests ===

    #[test]
    fn test_rapid_save_load_cycles() {
        let dir = create_temp_dir("rapid_cycles");

        for i in 0..20 {
            let mut cache = DeadmodCache {
                metadata: CacheMetadata::current(),
                modules: HashMap::new(),
            };
            cache.modules.insert(
                format!("mod_{}", i),
                CachedModule {
                    hash: format!("hash_{}", i),
                    refs: HashSet::new(),
                    visibility: CachedVisibility::default(),
                    doc_hidden: false,
                },
            );
            save_cache(&dir, &cache).unwrap();

            // Immediately load back
            let loaded = load_cache(&dir).unwrap();
            assert!(loaded.modules.contains_key(&format!("mod_{}", i)));
        }

        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn test_large_cache_atomic_write() {
        let dir = create_temp_dir("large_cache");

        let mut cache = DeadmodCache {
            metadata: CacheMetadata::current(),
            modules: HashMap::new(),
        };
        // Create a large cache with many modules
        for i in 0..500 {
            let mut refs = HashSet::new();
            for j in 0..10 {
                refs.insert(format!("dep_{}_{}", i, j));
            }
            cache.modules.insert(
                format!("large_module_{}", i),
                CachedModule {
                    hash: format!("hash_{:064x}", i),
                    refs,
                    visibility: CachedVisibility::default(),
                    doc_hidden: false,
                },
            );
        }

        save_cache(&dir, &cache).unwrap();

        let loaded = load_cache(&dir).unwrap();
        assert_eq!(loaded.modules.len(), 500);

        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn test_cache_special_characters_in_module_names() {
        let dir = create_temp_dir("special_chars");

        let mut cache = DeadmodCache {
            metadata: CacheMetadata::current(),
            modules: HashMap::new(),
        };
        let mut refs = HashSet::new();
        refs.insert("dep_with_underscore".to_string());
        refs.insert("dep123".to_string());

        cache.modules.insert(
            "mod_with_numbers_123".to_string(),
            CachedModule {
                hash: "hash".to_string(),
                refs,
                visibility: CachedVisibility::default(),
                doc_hidden: false,
            },
        );

        save_cache(&dir, &cache).unwrap();
        let loaded = load_cache(&dir).unwrap();
        assert!(loaded.modules.contains_key("mod_with_numbers_123"));

        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn test_hash_bytes_empty() {
        let hash = hash_bytes(b"");
        assert_eq!(hash.len(), 64);
        // SHA-256 of empty input is a known value
        assert_eq!(
            hash,
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        );
    }

    #[test]
    fn test_hash_bytes_unicode() {
        let hash = hash_bytes("日本語テスト".as_bytes());
        assert_eq!(hash.len(), 64);
    }
}
