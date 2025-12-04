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
