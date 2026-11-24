use super::*;
use crate::config::RepoEntry;
use std::process::Command;

fn git(dir: &std::path::Path, args: &[&str]) {
    let status = Command::new("git")
        .args(args)
        .current_dir(dir)
        .status()
        .unwrap();
    assert!(status.success(), "git {args:?} failed");
}

#[test]
fn repo_check_reports_ok_for_valid_repo() {
    let dir = std::env::temp_dir().join(format!("factory-repo-check-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    git(&dir, &["init"]);
    git(
        &dir,
        &["remote", "add", "origin", "git@github.com:org/repo.git"],
    );

    let repo = RepoEntry {
        name: "repo".to_string(),
        path: dir.to_string_lossy().to_string(),
        github: "org/repo".to_string(),
        hook_id: Some(1),
        enabled: true,
    };
    let results = repo_check_results(&repo, true);
    assert!(results.iter().all(|(l, _)| *l != "FAIL"));

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn repo_check_reports_missing_path_and_hook() {
    let repo = RepoEntry {
        name: "repo".to_string(),
        path: "/tmp/factory-repo-check-missing-path".to_string(),
        github: "org/repo".to_string(),
        hook_id: None,
        enabled: true,
    };
    let results = repo_check_results(&repo, true);
    assert!(
        results
            .iter()
            .any(|(l, m)| *l == "FAIL" && m.contains("path missing"))
    );
    assert!(
        results
            .iter()
            .any(|(l, m)| *l == "FAIL" && m.contains("hook_id missing"))
    );
}

#[test]
fn config_set_rejects_legacy_harness_and_model_with_migration_diagnostic() {
    let _lock = crate::state::tests::TEST_ENV_LOCK
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    let base =
        std::env::temp_dir().join(format!("factory-config-set-legacy-{}", std::process::id()));
    let config = base.join("config.json");
    let _ = std::fs::remove_dir_all(&base);
    std::fs::create_dir_all(&base).unwrap();

    unsafe { std::env::set_var("ISSUE_FACTORY_CONFIG_FILE", &config) };

    let err = config_set("harness", "opencode", true)
        .unwrap_err()
        .to_string();
    assert!(err.contains("legacy"), "{err}");
    assert!(err.contains("profiles"), "{err}");

    let err = config_set("model", "openai/gpt-6", true)
        .unwrap_err()
        .to_string();
    assert!(err.contains("legacy"), "{err}");
    assert!(err.contains("profiles"), "{err}");

    unsafe { std::env::remove_var("ISSUE_FACTORY_CONFIG_FILE") };
    let _ = std::fs::remove_dir_all(base);
}

struct MockManager {
    start_res: Result<(), String>,
    stop_res: Result<(), String>,
    restart_res: Result<(), String>,
}

impl super::ServiceManager for MockManager {
    fn start(&self) -> Result<(), String> {
        self.start_res.clone()
    }
    fn stop(&self) -> Result<(), String> {
        self.stop_res.clone()
    }
    fn restart(&self) -> Result<(), String> {
        self.restart_res.clone()
    }
}

#[test]
fn start_with_prints_success_on_ok() {
    let m = MockManager {
        start_res: Ok(()),
        stop_res: Ok(()),
        restart_res: Ok(()),
    };
    assert!(super::start_with(&m).is_ok());
}

#[test]
fn start_with_propagates_failure() {
    let m = MockManager {
        start_res: Err("boom (exit code 1)".to_string()),
        stop_res: Ok(()),
        restart_res: Ok(()),
    };
    let err = super::start_with(&m).unwrap_err();
    assert!(err.to_string().contains("boom"));
}

#[test]
fn stop_with_propagates_unsupported() {
    let m = MockManager {
        start_res: Ok(()),
        stop_res: Err("unsupported service manager: systemctl not found".to_string()),
        restart_res: Ok(()),
    };
    let err = super::stop_with(&m).unwrap_err();
    assert!(err.to_string().contains("unsupported"));
}

#[test]
fn restart_with_propagates_failure() {
    let m = MockManager {
        start_res: Ok(()),
        stop_res: Ok(()),
        restart_res: Err("failed to restart (exit code 3)".to_string()),
    };
    let err = super::restart_with(&m).unwrap_err();
    assert!(err.to_string().contains("failed to restart"));
}
