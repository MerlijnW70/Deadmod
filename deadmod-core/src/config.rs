//! Configuration loading from deadmod.toml.

use anyhow::{Context, Result};
use serde::Deserialize;
use std::{fs, path::Path};

/// Main configuration structure for deadmod.toml.
#[derive(Debug, Deserialize, Default)]
pub struct DeadmodConfig {
    /// List of module names or patterns to ignore.
    pub ignore: Option<Vec<String>>,
    /// Output configuration.
    pub output: Option<OutputConfig>,
}

/// Output format configuration.
#[derive(Debug, Deserialize, Default)]
pub struct OutputConfig {
    /// Output format: "plain" or "json".
    pub format: Option<String>,
}

/// Loads configuration from deadmod.toml if it exists.
pub fn load_config(root: &Path) -> Result<Option<DeadmodConfig>> {
    let path = root.join("deadmod.toml");
    if !path.exists() {
        return Ok(None);
    }

    let content = fs::read_to_string(&path)?;
    let cfg = toml::from_str(&content).context("Invalid deadmod.toml")?;
    Ok(Some(cfg))
}
