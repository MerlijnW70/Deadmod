//! Dead module detection logic.

use crate::parse::ModuleInfo;
use std::collections::{HashMap, HashSet};

/// Finds modules present in the system but not present in the reachable set.
pub fn find_dead<'a>(
    mods: &'a HashMap<String, ModuleInfo>,
    reachable: &HashSet<&str>,
) -> Vec<&'a str> {
    mods.keys()
        .map(|s| s.as_str())
        .filter(|m| !reachable.contains(m))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn make_module(name: &str) -> ModuleInfo {
        ModuleInfo {
            name: name.to_string(),
            path: PathBuf::from(format!("src/{}.rs", name)),
            refs: HashSet::new(),
            visibility: crate::parse::Visibility::Public,
            doc_hidden: false,
            mod_decls: HashMap::new(),
            reexports: HashSet::new(),
        }
    }

    #[test]
    fn test_find_dead_empty_inputs() {
        let mods: HashMap<String, ModuleInfo> = HashMap::new();
        let reachable: HashSet<&str> = HashSet::new();
        let dead = find_dead(&mods, &reachable);
        assert!(dead.is_empty());
    }

    #[test]
    fn test_find_dead_all_reachable() {
        let mut mods = HashMap::new();
        mods.insert("lib".to_string(), make_module("lib"));
        mods.insert("api".to_string(), make_module("api"));
        mods.insert("utils".to_string(), make_module("utils"));

        let reachable: HashSet<&str> = ["lib", "api", "utils"].into_iter().collect();
        let dead = find_dead(&mods, &reachable);
        assert!(dead.is_empty());
    }

    #[test]
    fn test_find_dead_some_unreachable() {
        let mut mods = HashMap::new();
        mods.insert("lib".to_string(), make_module("lib"));
        mods.insert("api".to_string(), make_module("api"));
        mods.insert("dead_module".to_string(), make_module("dead_module"));
        mods.insert("unused".to_string(), make_module("unused"));

        let reachable: HashSet<&str> = ["lib", "api"].into_iter().collect();
        let mut dead = find_dead(&mods, &reachable);
        dead.sort();
        assert_eq!(dead, vec!["dead_module", "unused"]);
    }

    #[test]
    fn test_find_dead_none_reachable() {
        let mut mods = HashMap::new();
        mods.insert("orphan1".to_string(), make_module("orphan1"));
        mods.insert("orphan2".to_string(), make_module("orphan2"));

        let reachable: HashSet<&str> = HashSet::new();
        let mut dead = find_dead(&mods, &reachable);
        dead.sort();
        assert_eq!(dead, vec!["orphan1", "orphan2"]);
    }

    #[test]
    fn test_find_dead_reachable_not_in_mods() {
        // Edge case: reachable set contains module not in mods
        let mut mods = HashMap::new();
        mods.insert("lib".to_string(), make_module("lib"));

        let reachable: HashSet<&str> = ["lib", "nonexistent"].into_iter().collect();
        let dead = find_dead(&mods, &reachable);
        assert!(dead.is_empty());
    }
}
