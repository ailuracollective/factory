use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WebhookConfig {
    pub port: u16,
    #[serde(default = "default_bind")]
    pub bind: String,
    pub secret: String,
    pub funnel_url: String,
}

fn default_bind() -> String {
    "0.0.0.0".to_string()
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Defaults {
    pub provider: String,
    pub model: String,
    pub base_branch: String,
    pub label: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RepoEntry {
    pub name: String,
    pub path: String,
    pub github: String,
    pub hook_id: Option<u64>,
    pub enabled: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    pub version: u32,
    pub projects_base: String,
    #[serde(default)]
    pub repo: String,
    pub webhook: WebhookConfig,
    pub defaults: Defaults,
    pub repos: Vec<RepoEntry>,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            version: 1,
            repo: String::new(),
            projects_base: dirs::home_dir()
                .unwrap_or_default()
                .join("projects/ailuracollective")
                .to_string_lossy()
                .to_string(),
            webhook: WebhookConfig {
                port: 8787,
                bind: "0.0.0.0".to_string(),
                secret: String::new(),
                funnel_url: String::new(),
            },
            defaults: Defaults {
                provider: "opencode".to_string(),
                model: "opencode-go/beam-free".to_string(),
                base_branch: "main".to_string(),
                label: "status/ready".to_string(),
            },
            repos: Vec::new(),
        }
    }
}

pub fn config_dir() -> PathBuf {
    dirs::config_dir()
        .unwrap_or_else(|| dirs::home_dir().unwrap().join(".config"))
        .join("ailuracollective")
        .join("factory")
}

pub fn config_file() -> PathBuf {
    if let Ok(p) = std::env::var("ISSUE_FACTORY_CONFIG_FILE") {
        if !p.trim().is_empty() {
            return PathBuf::from(p);
        }
    }
    config_dir().join("config.json")
}

pub fn load_config() -> Result<Config> {
    let path = config_file();
    if !path.exists() {
        let cfg = Config::default();
        save_config(&cfg)?;
        return Ok(cfg);
    }
    let data = fs::read_to_string(&path)?;
    let cfg: Config = serde_json::from_str(&data)?;
    Ok(cfg)
}

pub fn save_config(cfg: &Config) -> Result<()> {
    let dir = config_dir();
    fs::create_dir_all(&dir)?;
    let data = serde_json::to_string_pretty(cfg)?;
    fs::write(config_file(), data)?;
    Ok(())
}

pub fn get_repo<'a>(config: &'a Config, name: &str) -> Option<&'a RepoEntry> {
    config
        .repos
        .iter()
        .find(|r| r.name == name || r.github == name)
}

pub fn get_repo_mut<'a>(config: &'a mut Config, name: &str) -> Option<&'a mut RepoEntry> {
    config
        .repos
        .iter_mut()
        .find(|r| r.name == name || r.github == name)
}
