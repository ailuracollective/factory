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
    pub provider: String,
    pub model: String,
    pub base_branch: String,
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
}

/// Standalone (Gateway-free) mention polling. Disabled until usernames are set.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MentionsConfig {
    #[serde(default)]
    pub enabled: bool,
    /// GitHub logins (without `@`) that act as trigger identities. Self-mentions work.
    #[serde(default)]
    pub usernames: Vec<String>,
    /// Optional trigger word between the mention and the command, e.g.
    /// `@alice factory plan`. Empty means the command directly follows the mention.
    #[serde(default = "default_mention_command")]
    pub command: String,
    /// Extra authors allowed to trigger, beyond the usernames above. Others need
    /// write permission on the repository (checked through `gh`).
    #[serde(default)]
    pub allowed_authors: Vec<String>,
    /// Authors treated as Factory administrators (may cancel any execution).
    #[serde(default)]
    pub admin_users: Vec<String>,
    #[serde(default = "default_poll_interval")]
    pub poll_interval_secs: u64,
    /// How far back the first poll looks before a cursor exists.
    #[serde(default = "default_initial_lookback")]
    pub initial_lookback_secs: u64,
    #[serde(default = "default_daily_quota")]
    pub daily_quota: u32,
    /// Commands allowed for the `test` command, run in the project directory
    /// without a shell. Empty means `test` is rejected (fail closed).
    #[serde(default)]
    pub test_allowlist: Vec<String>,
    /// Queued/running executions older than this are failed or cancelled on the
    /// next poll. `0` disables timeout recovery.
    #[serde(default = "default_execution_timeout")]
    pub execution_timeout_secs: u64,
    /// Explicit mapping of repository roles to Factory capabilities.
    #[serde(default)]
    pub command_policy: crate::commands::CommandPolicy,
    /// Comment the outcome back on the issue/PR.
    #[serde(default = "default_true")]
    pub report: bool,
}

fn default_execution_timeout() -> u64 {
    3600
}

fn default_mention_command() -> String {
    "factory".to_string()
}

fn default_poll_interval() -> u64 {
    60
}

fn default_initial_lookback() -> u64 {
    300
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
            enabled: false,
            usernames: Vec::new(),
            command: default_mention_command(),
            allowed_authors: Vec::new(),
            admin_users: Vec::new(),
            poll_interval_secs: default_poll_interval(),
            initial_lookback_secs: default_initial_lookback(),
            daily_quota: default_daily_quota(),
            test_allowlist: Vec::new(),
            execution_timeout_secs: default_execution_timeout(),
            command_policy: crate::commands::CommandPolicy::default(),
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
                provider: "opencode".to_string(),
                model: "opencode/beam-free".to_string(),
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
    if let Ok(p) = std::env::var("ISSUE_FACTORY_CONFIG_FILE") {
        if !p.trim().is_empty() {
            return PathBuf::from(p);
        }
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
            "defaults": {"provider": "opencode", "model": "m", "base_branch": "main", "label": "status/ready"}
        }"#;
        let cfg: Config = serde_json::from_str(json).unwrap();
        assert!(!cfg.mentions.enabled);
        assert_eq!(cfg.mentions.command, "factory");
        assert_eq!(cfg.mentions.poll_interval_secs, 60);
        assert_eq!(cfg.mentions.execution_timeout_secs, 3600);
        assert!(cfg.mentions.test_allowlist.is_empty());
        assert!(cfg.mentions.command_policy.allows(
            crate::commands::Role::Owner,
            crate::commands::Capability::ExecutionsCancel
        ));
        assert!(!cfg.mentions.command_policy.allows(
            crate::commands::Role::Write,
            crate::commands::Capability::ExecutionsCancel
        ));
        assert!(cfg.mentions.report);
    }
}
