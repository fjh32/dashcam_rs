use std::fs;
use anyhow::Result;

use crate::config::AppConfig;

pub fn load_config(path: &str) -> Result<AppConfig> {
    let text = fs::read_to_string(path)?;
    let cfg: AppConfig = toml::from_str(&text)?;
    Ok(cfg)
}
