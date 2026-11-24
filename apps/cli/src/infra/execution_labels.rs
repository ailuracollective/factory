use anyhow::{Result, bail};
use serde::{Deserialize, Serialize};

pub const DEFAULT_PREFIX: &str = "ai/";
pub const STATE_COUNT: usize = 8;

/// Canonical execution state identifiers. These are independent of label names.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ExecutionState {
    Queued,
    Processing,
    Waiting,
    Blocked,
    NeedsInput,
    Failed,
    Cancelled,
    Completed,
}

impl ExecutionState {
    pub const ALL: [ExecutionState; STATE_COUNT] = [
        ExecutionState::Queued,
        ExecutionState::Processing,
        ExecutionState::Waiting,
        ExecutionState::Blocked,
        ExecutionState::NeedsInput,
        ExecutionState::Failed,
        ExecutionState::Cancelled,
        ExecutionState::Completed,
    ];

    pub const fn id(self) -> &'static str {
        match self {
            ExecutionState::Queued => "queued",
            ExecutionState::Processing => "processing",
            ExecutionState::Waiting => "waiting",
            ExecutionState::Blocked => "blocked",
            ExecutionState::NeedsInput => "needs-input",
            ExecutionState::Failed => "failed",
            ExecutionState::Cancelled => "cancelled",
            ExecutionState::Completed => "completed",
        }
    }

    pub const fn color(self) -> &'static str {
        match self {
            ExecutionState::Queued => "CFD3D7",
            ExecutionState::Processing => "1D76DB",
            ExecutionState::Waiting => "FBCA04",
            ExecutionState::Blocked => "B60205",
            ExecutionState::NeedsInput => "D93F0B",
            ExecutionState::Failed => "B60205",
            ExecutionState::Cancelled => "6E7781",
            ExecutionState::Completed => "0E8A16",
        }
    }

    pub const fn description(self) -> &'static str {
        match self {
            ExecutionState::Queued => {
                "Accepted work awaiting execution, including scheduled retries."
            }
            ExecutionState::Processing => "Active execution.",
            ExecutionState::Waiting => {
                "Waiting for an external condition without human intervention."
            }
            ExecutionState::Blocked => "An unmet prerequisite prevents progress.",
            ExecutionState::NeedsInput => "Human information or a decision is required.",
            ExecutionState::Failed => {
                "Unsuccessful terminal outcome with no automatic retry pending."
            }
            ExecutionState::Cancelled => "Confirmed cancellation.",
            ExecutionState::Completed => {
                "Successful automated work, independent of merge or issue closure."
            }
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedLabel {
    pub state: ExecutionState,
    pub name: String,
    pub color: &'static str,
    pub description: &'static str,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedCatalog {
    pub prefix: String,
    pub labels: Vec<ResolvedLabel>,
}

impl ResolvedCatalog {
    pub fn label_for(&self, state: ExecutionState) -> &ResolvedLabel {
        self.labels
            .iter()
            .find(|l| l.state == state)
            .expect("catalog contains all states")
    }

    pub fn names(&self) -> Vec<&str> {
        self.labels.iter().map(|l| l.name.as_str()).collect()
    }
}

/// Reserved organization/core label families that cannot be selected as the
/// execution prefix namespace. "type/" is a frozen core family; the full set of
/// core families is protected for Standards adopters. "complexity" is protected
/// when adopted as a core family in Standards adopters.
const RESERVED_FAMILIES: &[&str] = &[
    "type/",
    "status/",
    "scope/",
    "meta/",
    "github_actions/",
    "release/",
    "complexity/",
];

/// Maximum GitHub label name length.
const MAX_LABEL_NAME_LEN: usize = 50;

pub fn validate_prefix(prefix: &str) -> Result<()> {
    if prefix.is_empty() {
        bail!("labels.prefix must not be empty");
    }
    if !prefix.ends_with('/') {
        bail!(
            "labels.prefix must be a single lowercase ASCII namespace followed by '/': {prefix:?}"
        );
    }
    let namespace = &prefix[..prefix.len() - 1];
    if namespace.is_empty() {
        bail!("labels.prefix must not be empty before '/'");
    }
    if namespace.contains('/') {
        bail!(
            "labels.prefix must be a single namespace followed by '/'; nested namespaces are not allowed: {prefix:?}"
        );
    }
    if !namespace
        .bytes()
        .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-' || b == b'_')
    {
        bail!(
            "labels.prefix must use a single lowercase ASCII namespace followed by '/'; invalid characters: {prefix:?}"
        );
    }
    let lower = prefix.to_ascii_lowercase();
    for reserved in RESERVED_FAMILIES {
        if lower == *reserved {
            bail!("labels.prefix {prefix:?} conflicts with reserved label family {reserved:?}");
        }
    }
    // Also reject if the prefix would be inside a reserved family namespace
    // (e.g. "type/something/" is nested, already rejected; "status/" exact rejected).
    Ok(())
}

pub fn resolve_catalog(prefix: &str) -> Result<ResolvedCatalog> {
    validate_prefix(prefix)?;
    let mut labels = Vec::with_capacity(STATE_COUNT);
    for state in ExecutionState::ALL {
        let name = format!("{prefix}{}", state.id());
        if name.len() > MAX_LABEL_NAME_LEN {
            bail!(
                "effective label name {name:?} exceeds GitHub's {MAX_LABEL_NAME_LEN}-character limit"
            );
        }
        labels.push(ResolvedLabel {
            state,
            name,
            color: state.color(),
            description: state.description(),
        });
    }
    Ok(ResolvedCatalog {
        prefix: prefix.to_string(),
        labels,
    })
}

pub fn default_catalog() -> ResolvedCatalog {
    resolve_catalog(DEFAULT_PREFIX).expect("default prefix is valid")
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LabelCollision {
    pub effective_name: String,
    pub existing_name: String,
    pub reason: String,
}

/// Check resolved catalog names against existing repository labels.
/// Returns collisions/conflicts that must be reported rather than silently overwritten.
/// `existing_labels` are the label names currently present in the repository.
pub fn detect_collisions(
    catalog: &ResolvedCatalog,
    existing_labels: &[String],
) -> Vec<LabelCollision> {
    let mut collisions = Vec::new();
    for resolved in &catalog.labels {
        let resolved_lower = resolved.name.to_ascii_lowercase();
        for existing in existing_labels {
            let existing_lower = existing.to_ascii_lowercase();
            if existing_lower == resolved_lower && *existing != resolved.name {
                collisions.push(LabelCollision {
                    effective_name: resolved.name.clone(),
                    existing_name: existing.clone(),
                    reason: format!(
                        "case-insensitive collision: existing label {existing:?} conflicts with resolved catalog label {:?}",
                        resolved.name
                    ),
                });
            }
        }
    }
    collisions
}

/// Validate that the installed/active catalog prefix is consistent with the
/// configured prefix. A prefix change requires explicit migration.
pub fn validate_prefix_consistency(
    configured_prefix: &str,
    installed_prefix: Option<&str>,
    has_active_execution: bool,
) -> Result<()> {
    let Some(installed) = installed_prefix else {
        return Ok(());
    };
    if installed == configured_prefix {
        return Ok(());
    }
    if has_active_execution {
        bail!(
            "labels.prefix changed from {installed:?} to {configured_prefix:?} while executions are active; explicit migration is required"
        );
    }
    bail!(
        "labels.prefix changed from {installed:?} to {configured_prefix:?}; installed catalog requires explicit migration"
    )
}

/// Detect inconsistency between effective configurations from different components.
/// Returns a human-readable error if they differ.
pub fn detect_config_inconsistency(
    component_a: &str,
    prefix_a: &str,
    component_b: &str,
    prefix_b: &str,
) -> Result<()> {
    if prefix_a == prefix_b {
        return Ok(());
    }
    bail!(
        "inconsistent execution label configuration between {component_a} ({prefix_a:?}) and {component_b} ({prefix_b:?}); resolve before label mutation"
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_catalog_resolves_ai_prefix() {
        let catalog = default_catalog();
        assert_eq!(catalog.prefix, "ai/");
        assert_eq!(catalog.labels.len(), 8);
        let names: Vec<&str> = catalog.names();
        assert_eq!(
            names,
            vec![
                "ai/queued",
                "ai/processing",
                "ai/waiting",
                "ai/blocked",
                "ai/needs-input",
                "ai/failed",
                "ai/cancelled",
                "ai/completed",
            ]
        );
    }

    #[test]
    fn custom_prefix_resolves_same_states() {
        let catalog = resolve_catalog("factory/").unwrap();
        assert_eq!(catalog.prefix, "factory/");
        let names: Vec<&str> = catalog.names();
        assert_eq!(
            names,
            vec![
                "factory/queued",
                "factory/processing",
                "factory/waiting",
                "factory/blocked",
                "factory/needs-input",
                "factory/failed",
                "factory/cancelled",
                "factory/completed",
            ]
        );
        for (resolved, state) in catalog.labels.iter().zip(ExecutionState::ALL.iter()) {
            assert_eq!(resolved.state, *state);
            assert_eq!(resolved.color, state.color());
            assert_eq!(resolved.description, state.description());
        }
    }

    #[test]
    fn invalid_prefixes_are_rejected() {
        for bad in [
            "", "AI/", "ai", "ai//", "ai/sub/", "ai /", "ai./", "ai*/", "/ai/", "a i/",
        ] {
            let err = validate_prefix(bad).unwrap_err().to_string();
            assert!(!err.is_empty(), "expected error for {bad:?}");
        }
    }

    #[test]
    fn reserved_families_are_rejected() {
        for reserved in [
            "type/",
            "status/",
            "scope/",
            "meta/",
            "github_actions/",
            "release/",
            "complexity/",
        ] {
            let err = validate_prefix(reserved).unwrap_err().to_string();
            assert!(
                err.contains("reserved"),
                "expected reserved error for {reserved:?}, got {err}"
            );
        }
    }

    #[test]
    fn effective_name_length_limit_is_enforced() {
        // "a-very-long-namespace-prefix-that-makes-names-too-long/" + "processing" (10 chars) = 66
        let long = "a-very-long-namespace-prefix-that-makes-names-too-long/";
        let err = resolve_catalog(long).unwrap_err().to_string();
        assert!(err.contains("50-character"), "{err}");
    }

    #[test]
    fn collisions_are_reported_case_insensitively() {
        let catalog = resolve_catalog("factory/").unwrap();
        let existing = vec![
            "factory/Queued".to_string(),
            "factory/processing".to_string(),
            "unrelated/label".to_string(),
        ];
        let collisions = detect_collisions(&catalog, &existing);
        assert_eq!(collisions.len(), 1);
        assert_eq!(collisions[0].existing_name, "factory/Queued");
        assert_eq!(collisions[0].effective_name, "factory/queued");
    }

    #[test]
    fn independent_repo_resolution() {
        let a = resolve_catalog("ai/").unwrap();
        let b = resolve_catalog("factory/").unwrap();
        assert_ne!(a.names(), b.names());
        assert_eq!(a.labels.len(), 8);
        assert_eq!(b.labels.len(), 8);
        assert_eq!(a.label_for(ExecutionState::Queued).color, "CFD3D7");
        assert_eq!(b.label_for(ExecutionState::Queued).color, "CFD3D7");
    }

    #[test]
    fn prefix_consistency_requires_migration() {
        assert!(validate_prefix_consistency("ai/", None, false).is_ok());
        assert!(validate_prefix_consistency("ai/", Some("ai/"), true).is_ok());
        let err = validate_prefix_consistency("factory/", Some("ai/"), false)
            .unwrap_err()
            .to_string();
        assert!(err.contains("requires explicit migration"), "{err}");
        let err = validate_prefix_consistency("factory/", Some("ai/"), true)
            .unwrap_err()
            .to_string();
        assert!(err.contains("active"), "{err}");
    }

    #[test]
    fn config_inconsistency_is_detectable() {
        assert!(detect_config_inconsistency("cli", "ai/", "gateway", "ai/").is_ok());
        let err = detect_config_inconsistency("cli", "ai/", "gateway", "factory/")
            .unwrap_err()
            .to_string();
        assert!(err.contains("inconsistent"), "{err}");
    }
}
