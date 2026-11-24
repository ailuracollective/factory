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

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq, Eq)]
pub struct LabelsConfig {
    #[serde(default = "default_labels_prefix")]
    pub prefix: String,
}

fn default_labels_prefix() -> String {
    crate::infra::execution_labels::DEFAULT_PREFIX.to_string()
}

impl LabelsConfig {
    pub fn normalized_prefix(&self) -> &str {
        self.prefix.as_str()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct FactoryRepoConfig {
    #[serde(default)]
    pub defaults: Option<Defaults>,
    #[serde(default)]
    pub mentions: Option<MentionsConfig>,
    #[serde(default)]
    pub labels: Option<LabelsConfig>,
}

pub fn load_factory_config(repo_path: &std::path::Path) -> Result<Option<FactoryRepoConfig>> {
    let path = repo_path.join("factory.json");
    if !path.exists() {
        return Ok(None);
    }
    let data = std::fs::read_to_string(&path)?;
    let cfg: FactoryRepoConfig = serde_json::from_str(&data)
        .map_err(|e| anyhow::anyhow!("invalid factory.json at {}: {e}", path.display()))?;
    validate_factory_repo_config(&cfg)?;
    Ok(Some(cfg))
}

pub fn validate_factory_repo_config(cfg: &FactoryRepoConfig) -> Result<()> {
    if let Some(labels) = &cfg.labels {
        crate::infra::execution_labels::validate_prefix(&labels.prefix)?;
        // Ensure the full catalog resolves (length limits, etc.).
        crate::infra::execution_labels::resolve_catalog(&labels.prefix)?;
    }
    Ok(())
}

pub fn configured_execution_prefix(cfg: &FactoryRepoConfig) -> &str {
    cfg.labels
        .as_ref()
        .map(|l| l.prefix.as_str())
        .filter(|p| !p.is_empty())
        .unwrap_or(crate::infra::execution_labels::DEFAULT_PREFIX)
}

pub fn ensure_execution_labels_ready(
    repo_key: &str,
    cfg: &FactoryRepoConfig,
) -> Result<crate::infra::execution_labels::ResolvedCatalog> {
    let configured = configured_execution_prefix(cfg);
    let catalog = crate::infra::execution_labels::resolve_catalog(configured)?;
    let state = crate::infra::state::get_execution_label_state(repo_key);
    let has_active = crate::infra::state::has_active_executions_for_repo(repo_key);
    crate::infra::execution_labels::validate_prefix_consistency(
        configured,
        state.installed_prefix.as_deref(),
        has_active,
    )?;
    Ok(catalog)
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
        let cfg = load_factory_config(&dir).unwrap().unwrap();
        assert_eq!(cfg.defaults.unwrap().base_branch, "develop");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn repo_config_defaults_labels_prefix_to_ai() {
        let cfg: FactoryRepoConfig = serde_json::from_str(r#"{}"#).unwrap();
        assert!(cfg.labels.is_none());
        assert_eq!(
            configured_execution_prefix(&cfg),
            crate::infra::execution_labels::DEFAULT_PREFIX
        );
        let catalog =
            crate::infra::execution_labels::resolve_catalog(configured_execution_prefix(&cfg))
                .unwrap();
        assert_eq!(catalog.names()[0], "ai/queued");
    }

    #[test]
    fn repo_config_custom_prefix_resolves() {
        let cfg: FactoryRepoConfig =
            serde_json::from_str(r#"{"labels":{"prefix":"factory/"}}"#).unwrap();
        let prefix = configured_execution_prefix(&cfg);
        assert_eq!(prefix, "factory/");
        let catalog = crate::infra::execution_labels::resolve_catalog(prefix).unwrap();
        assert_eq!(catalog.names()[0], "factory/queued");
        assert_eq!(catalog.names().len(), 8);
    }

    #[test]
    fn repo_config_invalid_prefix_rejected_on_load() {
        let dir = std::env::temp_dir().join("factory_repo_cfg_invalid_prefix");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("factory.json"), r#"{"labels":{"prefix":"AI/"}}"#).unwrap();
        let err = load_factory_config(&dir).unwrap_err().to_string();
        assert!(
            err.contains("labels.prefix") || err.contains("lowercase"),
            "{err}"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn repo_config_invalid_json_rejected_on_load() {
        let dir = std::env::temp_dir().join("factory_repo_cfg_invalid_json");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("factory.json"), "{not json").unwrap();
        let err = load_factory_config(&dir).unwrap_err().to_string();
        assert!(err.contains("invalid factory.json"), "{err}");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn independent_repo_resolution_from_configs() {
        let a: FactoryRepoConfig = serde_json::from_str(r#"{"labels":{"prefix":"ai/"}}"#).unwrap();
        let b: FactoryRepoConfig =
            serde_json::from_str(r#"{"labels":{"prefix":"factory/"}}"#).unwrap();
        let ca = crate::infra::execution_labels::resolve_catalog(configured_execution_prefix(&a))
            .unwrap();
        let cb = crate::infra::execution_labels::resolve_catalog(configured_execution_prefix(&b))
            .unwrap();
        assert_eq!(
            ca.label_for(crate::infra::execution_labels::ExecutionState::Queued)
                .name,
            "ai/queued"
        );
        assert_eq!(
            cb.label_for(crate::infra::execution_labels::ExecutionState::Queued)
                .name,
            "factory/queued"
        );
    }
}
