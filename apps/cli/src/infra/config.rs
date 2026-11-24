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

/// Non-model execution defaults shared by global and repository config.
#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq, Eq)]
pub struct Defaults {
    #[serde(default)]
    pub base_branch: String,
    #[serde(default)]
    pub label: String,
    #[serde(default = "default_max_attempts")]
    pub max_attempts: u32,
}

/// A named execution profile. Profiles are the exclusive source of harness and
/// model selection for agent execution.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ExecutionProfile {
    pub harness: String,
    pub model: String,
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
        let raw: serde_json::Value = serde_json::from_str(&data)?;
        let legacy = detect_legacy_global_keys(&raw);
        if !legacy.is_empty() {
            anyhow::bail!(
                "invalid config at {}: {}\n\
                 Migration: remove `defaults.harness` and `defaults.model` from the global config; \
                 configure named profiles in repository `factory.json` and select with `default_profile`.",
                path.display(),
                legacy.join("; ")
            );
        }
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

fn deserialize_profiles<'de, D>(
    deserializer: D,
) -> std::result::Result<Option<std::collections::BTreeMap<String, ExecutionProfile>>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    struct ProfilesVisitor;

    impl<'de> serde::de::Visitor<'de> for ProfilesVisitor {
        type Value = Option<std::collections::BTreeMap<String, ExecutionProfile>>;

        fn expecting(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result {
            formatter.write_str("a map of profile names to execution profiles")
        }

        fn visit_none<E>(self) -> std::result::Result<Self::Value, E>
        where
            E: serde::de::Error,
        {
            Ok(None)
        }

        fn visit_unit<E>(self) -> std::result::Result<Self::Value, E>
        where
            E: serde::de::Error,
        {
            Ok(None)
        }

        fn visit_some<D>(self, deserializer: D) -> std::result::Result<Self::Value, D::Error>
        where
            D: serde::Deserializer<'de>,
        {
            deserializer.deserialize_map(self)
        }

        fn visit_map<A>(self, mut map: A) -> std::result::Result<Self::Value, A::Error>
        where
            A: serde::de::MapAccess<'de>,
        {
            let mut out = std::collections::BTreeMap::new();
            while let Some((key, value)) = map.next_entry::<String, ExecutionProfile>()? {
                if out.contains_key(&key) {
                    return Err(serde::de::Error::custom(format!(
                        "duplicate profile name: {key}"
                    )));
                }
                out.insert(key, value);
            }
            Ok(Some(out))
        }
    }

    deserializer.deserialize_option(ProfilesVisitor)
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct FactoryRepoConfig {
    #[serde(default)]
    pub defaults: Option<Defaults>,
    #[serde(default)]
    pub mentions: Option<MentionsConfig>,
    #[serde(default)]
    pub labels: Option<LabelsConfig>,
    #[serde(default, deserialize_with = "deserialize_profiles")]
    pub profiles: Option<std::collections::BTreeMap<String, ExecutionProfile>>,
    #[serde(default)]
    pub default_profile: Option<String>,
}

#[derive(Debug, Clone, Default)]
pub struct ConfigDiagnostics {
    pub errors: Vec<String>,
    pub warnings: Vec<String>,
}

impl ConfigDiagnostics {
    pub fn ok(&self) -> bool {
        self.errors.is_empty()
    }

    pub fn into_result(self) -> Result<()> {
        if self.errors.is_empty() {
            return Ok(());
        }
        anyhow::bail!("{}", self.errors.join("\n"));
    }
}

pub fn detect_legacy_repo_keys(raw: &serde_json::Value) -> Vec<String> {
    let mut diags = Vec::new();
    let Some(obj) = raw.as_object() else {
        return diags;
    };
    if let Some(defaults) = obj.get("defaults").and_then(|v| v.as_object())
        && (defaults.contains_key("harness") || defaults.contains_key("model"))
    {
        diags.push(
            "legacy factory.json key `defaults.harness`/`defaults.model` is no longer supported; \
             migrate to named `profiles` and `default_profile` (see `apps/cli/README.md`)"
                .to_string(),
        );
    }
    diags
}

pub fn detect_legacy_global_keys(raw: &serde_json::Value) -> Vec<String> {
    let mut diags = Vec::new();
    let Some(obj) = raw.as_object() else {
        return diags;
    };
    if let Some(defaults) = obj.get("defaults").and_then(|v| v.as_object())
        && (defaults.contains_key("harness") || defaults.contains_key("model"))
    {
        diags.push(
            "legacy config key `defaults.harness`/`defaults.model` is no longer supported; \
             harness/model selection comes from repository `factory.json` profiles. \
             Remove the legacy keys from your global config."
                .to_string(),
        );
    }
    diags
}

pub fn legacy_env_overrides() -> Vec<String> {
    let mut diags = Vec::new();
    for key in ["ISSUE_FACTORY_HARNESS", "ISSUE_FACTORY_MODEL"] {
        if std::env::var(key)
            .ok()
            .filter(|v| !v.trim().is_empty())
            .is_some()
        {
            diags.push(format!(
                "legacy environment override `{key}` is no longer supported; \
                 configure named profiles in `factory.json` and select with `default_profile`"
            ));
        }
    }
    diags
}

pub fn load_factory_config(repo_path: &std::path::Path) -> Result<Option<FactoryRepoConfig>> {
    let path = repo_path.join("factory.json");
    if !path.exists() {
        return Ok(None);
    }
    let data = std::fs::read_to_string(&path)?;
    let raw: serde_json::Value = serde_json::from_str(&data)
        .map_err(|e| anyhow::anyhow!("invalid factory.json at {}: {e}", path.display()))?;
    let legacy = detect_legacy_repo_keys(&raw);
    if !legacy.is_empty() {
        anyhow::bail!(
            "invalid factory.json at {}: {}",
            path.display(),
            legacy.join("; ")
        );
    }
    let cfg: FactoryRepoConfig = serde_json::from_str(&data)
        .map_err(|e| anyhow::anyhow!("invalid factory.json at {}: {e}", path.display()))?;
    validate_factory_repo_config(&cfg)?;
    Ok(Some(cfg))
}

pub fn validate_factory_repo_config(cfg: &FactoryRepoConfig) -> Result<()> {
    let mut errors = Vec::new();

    if let Some(labels) = &cfg.labels {
        if let Err(e) = crate::infra::execution_labels::validate_prefix(&labels.prefix) {
            errors.push(e.to_string());
        } else if let Err(e) = crate::infra::execution_labels::resolve_catalog(&labels.prefix) {
            errors.push(e.to_string());
        }
    }

    if let Some(profiles) = &cfg.profiles {
        for (name, profile) in profiles {
            if name.trim().is_empty() {
                errors.push("profile name must not be empty".to_string());
            }
            if profile.harness.trim().is_empty() {
                errors.push(format!("profile `{name}`: harness must not be empty"));
            }
            if profile.model.trim().is_empty() {
                errors.push(format!("profile `{name}`: model must not be empty"));
            }
        }
    }

    if let Some(default_profile) = &cfg.default_profile {
        if default_profile.trim().is_empty() {
            errors.push("default_profile must not be empty".to_string());
        } else if let Some(profiles) = &cfg.profiles {
            if !profiles.contains_key(default_profile) {
                errors.push(format!(
                    "invalid profile reference: default_profile `{default_profile}` does not exist in profiles"
                ));
            }
        } else {
            errors.push(format!(
                "invalid profile reference: default_profile `{default_profile}` but no profiles are defined"
            ));
        }
    }

    if errors.is_empty() {
        Ok(())
    } else {
        anyhow::bail!("{}", errors.join("; "))
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedExecution {
    pub profile_name: String,
    pub harness: String,
    pub model: String,
}

#[derive(Debug, Clone)]
pub struct ProfileValidation {
    pub resolved: ResolvedExecution,
    pub discovery: crate::infra::t3::DiscoveryState,
    pub errors: Vec<String>,
    pub warnings: Vec<String>,
}

impl ProfileValidation {
    pub fn is_temporary_discovery_failure(&self) -> bool {
        matches!(
            self.discovery,
            crate::infra::t3::DiscoveryState::Unavailable(_)
        )
    }

    pub fn into_result(self) -> Result<ResolvedExecution> {
        if let crate::infra::t3::DiscoveryState::Unavailable(msg) = &self.discovery {
            if self.errors.is_empty() {
                anyhow::bail!(
                    "{msg}. Cannot validate profile `{}` against T3 discovery; retry when T3 catalog is available.",
                    self.resolved.profile_name
                );
            }
            anyhow::bail!("{}; {}", msg, self.errors.join("; "));
        }
        if self.errors.is_empty() {
            Ok(self.resolved)
        } else {
            anyhow::bail!("{}", self.errors.join("; "))
        }
    }
}

pub fn select_profile_name<'a>(
    cfg: &'a FactoryRepoConfig,
    requested: Option<&'a str>,
) -> Result<&'a str> {
    if let Some(name) = requested {
        return Ok(name);
    }
    cfg.default_profile.as_deref().ok_or_else(|| {
        anyhow::anyhow!(
            "no execution profile selected: set `default_profile` in factory.json or pass --profile"
        )
    })
}

pub fn resolve_execution_profile(
    cfg: &FactoryRepoConfig,
    requested: Option<&str>,
) -> Result<ProfileValidation> {
    let profile_name = select_profile_name(cfg, requested)?;
    let profiles = cfg.profiles.as_ref().ok_or_else(|| {
        anyhow::anyhow!(
            "invalid profile reference: profile `{profile_name}` requested but no profiles are defined in factory.json"
        )
    })?;
    let profile = profiles.get(profile_name).ok_or_else(|| {
        let available = profiles.keys().cloned().collect::<Vec<_>>().join(", ");
        anyhow::anyhow!(
            "invalid profile reference: profile `{profile_name}` not found; available profiles: {available}"
        )
    })?;

    let discovery = crate::infra::t3::discover_harnesses();
    let mut errors = Vec::new();
    let mut warnings = Vec::new();
    let mut resolved_harness = profile.harness.clone();

    match &discovery {
        crate::infra::t3::DiscoveryState::Available(harnesses) => {
            if let Some(h) = harnesses
                .iter()
                .find(|h| h.name == profile.harness || h.driver == profile.harness)
            {
                resolved_harness = h.name.clone();
                if !h.models.is_empty() && !h.models.iter().any(|m| m == &profile.model) {
                    errors.push(format!(
                        "profile `{profile_name}`: model `{}` is not compatible with harness `{}`; use `factory agent models {}` to list available models",
                        profile.model, h.name, h.name
                    ));
                }
                if h.models.is_empty() {
                    warnings.push(format!(
                        "profile `{profile_name}`: harness `{}` reported no models; skipping model compatibility check",
                        h.name
                    ));
                }
            } else if harnesses.is_empty() {
                errors.push(format!(
                    "temporary discovery failure: T3 reported no installed harnesses; cannot verify profile `{profile_name}`. Start T3 Code so it can probe harnesses, then retry."
                ));
            } else {
                let available = harnesses
                    .iter()
                    .map(|h| h.name.as_str())
                    .collect::<Vec<_>>()
                    .join(", ");
                errors.push(format!(
                    "profile `{profile_name}`: harness `{}` is not installed/available; available harnesses: {available}",
                    profile.harness
                ));
            }
        }
        crate::infra::t3::DiscoveryState::Unavailable(msg) => {
            warnings.push(msg.clone());
        }
    }

    Ok(ProfileValidation {
        resolved: ResolvedExecution {
            profile_name: profile_name.to_string(),
            harness: resolved_harness,
            model: profile.model.clone(),
        },
        discovery,
        errors,
        warnings,
    })
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
    use std::collections::BTreeMap;

    #[test]
    fn config_without_mentions_uses_defaults() {
        let json = r#"{
            "version": 1,
            "projects_base": "/tmp/projects",
            "repo": "",
            "webhook": {"port": 8787, "secret": "", "funnel_url": ""},
            "defaults": {"base_branch": "main", "label": "status/ready", "max_attempts": 3}
        }"#;
        let cfg: Config = serde_json::from_str(json).unwrap();
        assert_eq!(cfg.defaults.base_branch, "main");
        assert_eq!(cfg.defaults.label, "status/ready");
        assert_eq!(cfg.defaults.max_attempts, 3);
        assert!(cfg.mentions.enabled);
        assert_eq!(cfg.mentions.command, "factory");
        assert!(cfg.mentions.report);
    }

    #[test]
    fn defaults_serialize_without_harness_model() {
        let json = serde_json::to_value(Config::default()).unwrap();
        let defaults = json.get("defaults").unwrap().as_object().unwrap();
        assert!(!defaults.contains_key("harness"));
        assert!(!defaults.contains_key("model"));
        assert_eq!(defaults.get("base_branch").unwrap(), "main");
    }

    #[test]
    fn factory_repo_config_loads_profiles_and_defaults() {
        let dir = std::env::temp_dir().join("factory_repo_cfg_profiles");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join("factory.json"),
            r#"{
              "profiles": {
                "default": {"harness": "pi", "model": "openai/gpt-5"}
              },
              "default_profile": "default",
              "defaults": {"base_branch": "develop", "label": "ready", "max_attempts": 5}
            }"#,
        )
        .unwrap();
        let cfg = load_factory_config(&dir).unwrap().unwrap();
        let defaults = cfg.defaults.unwrap();
        assert_eq!(defaults.base_branch, "develop");
        assert_eq!(defaults.label, "ready");
        assert_eq!(defaults.max_attempts, 5);
        let profiles = cfg.profiles.unwrap();
        assert_eq!(profiles.get("default").unwrap().harness, "pi");
        assert_eq!(profiles.get("default").unwrap().model, "openai/gpt-5");
        assert_eq!(cfg.default_profile.as_deref(), Some("default"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn legacy_repo_harness_model_defaults_rejected_with_migration_diagnostic() {
        let dir = std::env::temp_dir().join("factory_repo_cfg_legacy");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join("factory.json"),
            r#"{"defaults":{"harness":"opencode","model":"m","base_branch":"develop"}}"#,
        )
        .unwrap();
        let err = load_factory_config(&dir).unwrap_err().to_string();
        assert!(err.contains("legacy"), "{err}");
        assert!(err.contains("profiles"), "{err}");
        assert!(err.contains("default_profile"), "{err}");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn duplicate_profile_names_rejected() {
        let dir = std::env::temp_dir().join("factory_repo_cfg_dup_profile");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join("factory.json"),
            r#"{
              "profiles": {
                "a": {"harness": "pi", "model": "m1"},
                "a": {"harness": "pi", "model": "m2"}
              },
              "default_profile": "a"
            }"#,
        )
        .unwrap();
        let err = load_factory_config(&dir).unwrap_err().to_string();
        assert!(err.contains("duplicate profile name"), "{err}");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn invalid_default_profile_reference_rejected() {
        let dir = std::env::temp_dir().join("factory_repo_cfg_bad_ref");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join("factory.json"),
            r#"{
              "profiles": {
                "a": {"harness": "pi", "model": "m1"}
              },
              "default_profile": "missing"
            }"#,
        )
        .unwrap();
        let err = load_factory_config(&dir).unwrap_err().to_string();
        assert!(err.contains("invalid profile reference"), "{err}");
        assert!(err.contains("missing"), "{err}");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn malformed_profile_rejected() {
        let dir = std::env::temp_dir().join("factory_repo_cfg_malformed_profile");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join("factory.json"),
            r#"{"profiles": {"a": {"harness": "pi"}}}"#,
        )
        .unwrap();
        let err = load_factory_config(&dir).unwrap_err().to_string();
        assert!(err.contains("invalid factory.json"), "{err}");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn unrelated_defaults_remain_intact_with_profiles() {
        let dir = std::env::temp_dir().join("factory_repo_cfg_unrelated_defaults");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join("factory.json"),
            r#"{
              "profiles": {"default": {"harness": "pi", "model": "m"}},
              "default_profile": "default",
              "defaults": {"base_branch": "main", "label": "status/ready", "max_attempts": 7},
              "mentions": {"enabled": true, "usernames": ["alice"], "command": "factory"},
              "labels": {"prefix": "ai/"}
            }"#,
        )
        .unwrap();
        let cfg = load_factory_config(&dir).unwrap().unwrap();
        let defaults = cfg.defaults.unwrap();
        assert_eq!(defaults.base_branch, "main");
        assert_eq!(defaults.label, "status/ready");
        assert_eq!(defaults.max_attempts, 7);
        let mentions = cfg.mentions.unwrap();
        assert!(mentions.enabled);
        assert_eq!(mentions.usernames, vec!["alice".to_string()]);
        assert_eq!(cfg.labels.unwrap().prefix, "ai/");
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
        let _ = std::fs::remove_dir_all(&dir);
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
        let _ = std::fs::remove_dir_all(&dir);
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

    #[test]
    fn legacy_global_harness_model_rejected() {
        let raw: serde_json::Value =
            serde_json::from_str(r#"{"defaults":{"harness":"pi","model":"m"}}"#).unwrap();
        let diags = detect_legacy_global_keys(&raw);
        assert!(!diags.is_empty());
        assert!(diags[0].contains("defaults.harness"));
    }

    #[test]
    fn validate_rejects_empty_profile_fields() {
        let mut profiles = BTreeMap::new();
        profiles.insert(
            "bad".to_string(),
            ExecutionProfile {
                harness: "".to_string(),
                model: "m".to_string(),
            },
        );
        let cfg = FactoryRepoConfig {
            profiles: Some(profiles),
            default_profile: Some("bad".to_string()),
            ..Default::default()
        };
        let err = validate_factory_repo_config(&cfg).unwrap_err().to_string();
        assert!(err.contains("harness must not be empty"), "{err}");
    }

    #[test]
    fn resolve_profile_reports_temporary_discovery_failure_when_cache_unreadable() {
        let _lock = crate::state::tests::TEST_ENV_LOCK
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let dir = std::env::temp_dir().join(format!("factory_t3_missing_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        unsafe { std::env::set_var("T3CODE_HOME", dir.join("nope").to_str().unwrap()) };

        let mut profiles = BTreeMap::new();
        profiles.insert(
            "default".to_string(),
            ExecutionProfile {
                harness: "pi".to_string(),
                model: "m".to_string(),
            },
        );
        let cfg = FactoryRepoConfig {
            profiles: Some(profiles),
            default_profile: Some("default".to_string()),
            ..Default::default()
        };
        let err = resolve_execution_profile(&cfg, None)
            .unwrap()
            .into_result()
            .unwrap_err()
            .to_string();
        assert!(err.contains("temporary discovery failure"), "{err}");

        unsafe { std::env::remove_var("T3CODE_HOME") };
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn resolve_profile_rejects_incompatible_model() {
        let _lock = crate::state::tests::TEST_ENV_LOCK
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let dir = std::env::temp_dir().join(format!("factory_t3_incompat_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("caches")).unwrap();
        std::fs::write(
            dir.join("caches/pi.json"),
            r#"{"instanceId":"pi","driver":"pi","displayName":"Pi","version":"1.0","installed":true,"status":"ready","models":[{"slug":"ok-model"}]}"#,
        )
        .unwrap();
        unsafe { std::env::set_var("T3CODE_HOME", dir.to_str().unwrap()) };

        let mut profiles = BTreeMap::new();
        profiles.insert(
            "default".to_string(),
            ExecutionProfile {
                harness: "pi".to_string(),
                model: "bad-model".to_string(),
            },
        );
        let cfg = FactoryRepoConfig {
            profiles: Some(profiles),
            default_profile: Some("default".to_string()),
            ..Default::default()
        };
        let err = resolve_execution_profile(&cfg, None)
            .unwrap()
            .into_result()
            .unwrap_err()
            .to_string();
        assert!(err.contains("not compatible"), "{err}");
        assert!(err.contains("bad-model"), "{err}");

        unsafe { std::env::remove_var("T3CODE_HOME") };
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn legacy_env_overrides_reported() {
        let _lock = crate::state::tests::TEST_ENV_LOCK
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        unsafe { std::env::set_var("ISSUE_FACTORY_HARNESS", "pi") };
        let diags = legacy_env_overrides();
        assert!(
            diags.iter().any(|d| d.contains("ISSUE_FACTORY_HARNESS")),
            "{diags:?}"
        );
        unsafe { std::env::remove_var("ISSUE_FACTORY_HARNESS") };
    }
}
