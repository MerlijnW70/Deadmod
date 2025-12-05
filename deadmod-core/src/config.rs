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

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn test_load_config_missing_file() {
        let dir = std::env::temp_dir().join(format!("deadmod_config_test_{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();

        let result = load_config(&dir);
        assert!(result.is_ok());
        assert!(result.unwrap().is_none());

        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn test_load_config_empty_file() {
        let dir = std::env::temp_dir().join(format!("deadmod_config_empty_{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join("deadmod.toml"), "").unwrap();

        let result = load_config(&dir);
        assert!(result.is_ok());
        let cfg = result.unwrap().unwrap();
        assert!(cfg.ignore.is_none());
        assert!(cfg.output.is_none());

        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn test_load_config_with_ignore() {
        let dir = std::env::temp_dir().join(format!("deadmod_config_ignore_{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        fs::write(
            dir.join("deadmod.toml"),
            r#"
ignore = ["tests", "benches", "examples"]
"#,
        )
        .unwrap();

        let result = load_config(&dir);
        assert!(result.is_ok());
        let cfg = result.unwrap().unwrap();
        let ignore = cfg.ignore.unwrap();
        assert_eq!(ignore.len(), 3);
        assert!(ignore.contains(&"tests".to_string()));
        assert!(ignore.contains(&"benches".to_string()));

        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn test_load_config_with_output() {
        let dir = std::env::temp_dir().join(format!("deadmod_config_output_{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        fs::write(
            dir.join("deadmod.toml"),
            r#"
[output]
format = "json"
"#,
        )
        .unwrap();

        let result = load_config(&dir);
        assert!(result.is_ok());
        let cfg = result.unwrap().unwrap();
        let output = cfg.output.unwrap();
        assert_eq!(output.format, Some("json".to_string()));

        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn test_load_config_full() {
        let dir = std::env::temp_dir().join(format!("deadmod_config_full_{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        fs::write(
            dir.join("deadmod.toml"),
            r#"
ignore = ["test_utils", "mocks"]

[output]
format = "plain"
"#,
        )
        .unwrap();

        let result = load_config(&dir);
        assert!(result.is_ok());
        let cfg = result.unwrap().unwrap();
        assert_eq!(cfg.ignore.as_ref().unwrap().len(), 2);
        assert_eq!(cfg.output.as_ref().unwrap().format, Some("plain".to_string()));

        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn test_load_config_invalid_toml() {
        let dir = std::env::temp_dir().join(format!("deadmod_config_invalid_{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join("deadmod.toml"), "this is not valid toml {{{").unwrap();

        let result = load_config(&dir);
        assert!(result.is_err());

        fs::remove_dir_all(&dir).ok();
    }
}
