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

fn default_max_attempts() -> u32 {
    3
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Defaults {
    #[serde(default)]
    pub harness: String,
    #[serde(default)]
    pub model: String,
    #[serde(default)]
    pub base_branch: String,
    #[serde(default)]
    pub label: String,
    #[serde(default = "default_max_attempts")]
    pub max_attempts: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct GatewayConfig {
    #[serde(default)]
    pub url: String,
    #[serde(default)]
    pub runner_id: String,
    #[serde(default)]
    pub token: String,
    #[serde(default)]
    pub callback_secret: String,
    #[serde(default)]
    pub callback_url: String,
}

/// Mention handling config. Mentions are processed from webhooks in real time.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MentionsConfig {
    #[serde(default)]
    pub enabled: bool,
    /// GitHub logins (without `@`) that act as trigger identities.
    #[serde(default)]
    pub usernames: Vec<String>,
    /// Additional trigger usernames merged on top of the global list.
    #[serde(default)]
    pub usernames_extra: Vec<String>,
    /// Explicit command required after the mention, e.g. `@alice factory fix it`.
    /// Empty means any mention triggers.
    #[serde(default = "default_mention_command")]
    pub command: String,
    /// Extra authors allowed to trigger, beyond the usernames above. Others need
    /// write permission on the repository (checked through `gh`).
    #[serde(default)]
    pub allowed_authors: Vec<String>,
    #[serde(default = "default_daily_quota")]
    pub daily_quota: u32,
    /// Comment the outcome back on the issue/PR.
    #[serde(default = "default_true")]
    pub report: bool,
}

fn default_mention_command() -> String {
    "factory".to_string()
}

fn default_daily_quota() -> u32 {
    20
}

fn default_true() -> bool {
    true
}

impl Default for MentionsConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            usernames: Vec::new(),
            usernames_extra: Vec::new(),
            command: default_mention_command(),
            allowed_authors: Vec::new(),
            daily_quota: default_daily_quota(),
            report: true,
        }
    }
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
    #[serde(skip, default)]
    pub repos: Vec<RepoEntry>,
    #[serde(default)]
    pub gateway: GatewayConfig,
    #[serde(default)]
    pub mentions: MentionsConfig,
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
                harness: "opencode".to_string(),
                model: "opencode-go/iceman-contributor-free".to_string(),
                base_branch: "main".to_string(),
                label: "status/ready".to_string(),
                max_attempts: 3,
            },
            repos: Vec::new(),
            gateway: GatewayConfig::default(),
            mentions: MentionsConfig::default(),
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
    if let Ok(p) = std::env::var("ISSUE_FACTORY_CONFIG_FILE")
        && !p.trim().is_empty()
    {
        return PathBuf::from(p);
    }
    config_dir().join("config.json")
}

pub fn load_config() -> Result<Config> {
    let path = config_file();
    let mut cfg = if path.exists() {
        let data = fs::read_to_string(&path)?;
        serde_json::from_str::<Config>(&data)?
    } else {
        let cfg = Config::default();
        save_config(&cfg)?;
        cfg
    };
    cfg.repos = crate::state::load_projects();
    Ok(cfg)
}

pub fn save_config(cfg: &Config) -> Result<()> {
    let path = config_file();
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let data = serde_json::to_string_pretty(cfg)?;
    let tmp = path.with_extension("json.tmp");
    fs::write(&tmp, data)?;
    fs::rename(&tmp, &path)?;
    sync_projects_to_db(&cfg.repos);
    Ok(())
}

fn sync_projects_to_db(repos: &[RepoEntry]) {
    crate::state::sync_projects(repos);
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct FactoryRepoConfig {
    #[serde(default)]
    pub defaults: Option<Defaults>,
    #[serde(default)]
    pub mentions: Option<MentionsConfig>,
}

pub fn load_factory_config(repo_path: &std::path::Path) -> Option<FactoryRepoConfig> {
    let path = repo_path.join("factory.json");
    let data = std::fs::read_to_string(&path).ok()?;
    serde_json::from_str(&data).ok()
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn config_without_mentions_uses_defaults() {
        let json = r#"{
            "version": 1,
            "projects_base": "/tmp/projects",
            "repo": "",
            "webhook": {"port": 8787, "secret": "", "funnel_url": ""},
            "defaults": {"harness": "opencode", "model": "m", "base_branch": "main", "label": "status/ready"}
        }"#;
        let cfg: Config = serde_json::from_str(json).unwrap();
        assert_eq!(cfg.defaults.harness, "opencode");
        assert!(cfg.mentions.enabled);
        assert_eq!(cfg.mentions.command, "factory");
        assert!(cfg.mentions.report);
    }

    #[test]
    fn defaults_serialize_as_harness() {
        let json = serde_json::to_value(Config::default()).unwrap();
        let defaults = json.get("defaults").unwrap();
        assert_eq!(defaults.get("harness").unwrap(), "opencode");
        assert!(defaults.get("provider").is_none());
    }

    #[test]
    fn factory_repo_config_loads() {
        let dir = std::env::temp_dir().join("factory_repo_cfg_test");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join("factory.json"),
            r#"{"defaults":{"harness":"opencode","model":"m","base_branch":"develop","label":"ready","max_attempts":5}}"#,
        )
        .unwrap();
        let cfg = load_factory_config(&dir).unwrap();
        assert_eq!(cfg.defaults.unwrap().base_branch, "develop");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
