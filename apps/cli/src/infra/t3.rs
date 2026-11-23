//! T3 Code integration: installed harnesses and their models.
//!
//! T3 Code writes one capability cache per harness under `<T3CODE_HOME>/caches/`.
//! Reading it keeps `factory` in sync with what T3 actually offers without
//! hardcoding harness or model names.
//!
//! shortcut: relies on T3's on-disk cache (refreshed when the T3 server starts),
//! switch to a T3 API when one is published.

use serde_json::Value;
use std::path::PathBuf;

#[derive(Debug, Clone)]
pub struct Harness {
    /// Harness id accepted by `t3 thread start`.
    pub name: String,
    pub driver: String,
    pub display_name: String,
    pub version: Option<String>,
    pub status: String,
    /// Model slugs accepted by the harness.
    pub models: Vec<String>,
}

fn t3_home() -> PathBuf {
    std::env::var("T3CODE_HOME")
        .ok()
        .filter(|p| !p.trim().is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(|| dirs::home_dir().unwrap_or_default().join(".t3"))
}

pub fn harnesses() -> Vec<Harness> {
    let dir = t3_home().join("caches");
    let Ok(entries) = std::fs::read_dir(&dir) else {
        return Vec::new();
    };
    let mut found: Vec<Harness> = entries
        .filter_map(|e| std::fs::read_to_string(e.ok()?.path()).ok())
        .filter_map(|data| parse_cache(&data))
        .collect();
    found.sort_by(|a, b| a.name.cmp(&b.name));
    found
}

pub fn harness(name: &str) -> Option<Harness> {
    harnesses()
        .into_iter()
        .find(|h| h.name == name || h.driver == name)
}

/// Map a configured value to a harness name, leaving it untouched when unknown.
pub fn resolve_harness(name: &str) -> String {
    harness(name)
        .map(|h| h.name)
        .unwrap_or_else(|| name.to_string())
}

fn parse_cache(data: &str) -> Option<Harness> {
    let v: Value = serde_json::from_str(data).ok()?;
    if !v.get("installed").and_then(Value::as_bool).unwrap_or(false) {
        return None;
    }
    let name = v.get("instanceId").and_then(Value::as_str)?.to_string();
    let models = v
        .get("models")
        .and_then(Value::as_array)
        .map(|models| {
            models
                .iter()
                .filter_map(|m| Some(m.get("slug").and_then(Value::as_str)?.to_string()))
                .collect()
        })
        .unwrap_or_default();
    Some(Harness {
        driver: v
            .get("driver")
            .and_then(Value::as_str)
            .unwrap_or(&name)
            .to_string(),
        display_name: v
            .get("displayName")
            .and_then(Value::as_str)
            .unwrap_or(&name)
            .to_string(),
        version: v.get("version").and_then(Value::as_str).map(str::to_string),
        status: v
            .get("status")
            .and_then(Value::as_str)
            .unwrap_or("unknown")
            .to_string(),
        name,
        models,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_installed_harness_and_models() {
        let cache = r#"{
            "instanceId": "opencode",
            "driver": "opencode",
            "displayName": "OpenCode",
            "version": "2.3.2",
            "installed": true,
            "status": "ready",
            "models": [
                {"slug": "opencode-go/beam-free"},
                {"slug": "openai/gpt-6"},
                {"slug": "default"}
            ]
        }"#;
        let h = parse_cache(cache).unwrap();
        assert_eq!(h.name, "opencode");
        assert_eq!(h.display_name, "OpenCode");
        assert_eq!(h.version.as_deref(), Some("2.3.2"));
        assert_eq!(h.status, "ready");
        assert_eq!(
            h.models,
            ["opencode-go/beam-free", "openai/gpt-6", "default"]
        );
    }

    #[test]
    fn ignores_harnesses_not_installed() {
        let cache = r#"{"instanceId": "codex", "installed": false, "models": []}"#;
        assert!(parse_cache(cache).is_none());
    }
}
