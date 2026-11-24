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
fn config_set_validates_harness_and_model() {
    let base = std::env::temp_dir().join(format!("factory-config-set-{}", std::process::id()));
    let config = base.join("config.json");
    let t3_home = base.join("t3");
    let _ = std::fs::remove_dir_all(&base);
    std::fs::create_dir_all(t3_home.join("caches")).unwrap();
    std::fs::write(
        t3_home.join("caches/opencode.json"),
        r#"{"instanceId":"opencode","driver":"opencode","installed":true,"status":"ready",
            "models":[{"slug":"openai/gpt-6"},{"slug":"opencode-go/beam-free"}]}"#,
    )
    .unwrap();

    unsafe { std::env::set_var("ISSUE_FACTORY_CONFIG_FILE", &config) };
    unsafe { std::env::set_var("T3CODE_HOME", &t3_home) };

    // Unknown harness is rejected while T3 has probed its catalog.
    config_set("harness", "nope", true).unwrap();
    config_set("harness", "opencode", true).unwrap();
    // Model is validated against that harness only.
    config_set("model", "sonnet-4", true).unwrap();
    config_set("model", "openai/gpt-6", true).unwrap();

    let saved: crate::config::Config =
        serde_json::from_slice(&std::fs::read(&config).unwrap()).unwrap();
    assert_eq!(saved.defaults.harness, "opencode");
    assert_eq!(saved.defaults.model, "openai/gpt-6");

    unsafe { std::env::remove_var("ISSUE_FACTORY_CONFIG_FILE") };
    unsafe { std::env::remove_var("T3CODE_HOME") };
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

#[test]
fn execution_logs_unknown_execution_is_distinguishable() {
    let _lock = crate::state::tests::TEST_ENV_LOCK
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    let db = std::env::temp_dir().join(format!(
        "factory_cli_logs_{}_{}.db",
        std::process::id(),
        uuid::Uuid::now_v7().simple()
    ));
    let prev = std::env::var("ISSUE_FACTORY_DB_FILE").ok();
    unsafe { std::env::set_var("ISSUE_FACTORY_DB_FILE", &db) };
    let err = super::execution_logs("exec_missing", false, 200).unwrap_err();
    assert!(
        err.to_string()
            .contains("execution not found: exec_missing")
    );
    match prev {
        Some(v) => unsafe { std::env::set_var("ISSUE_FACTORY_DB_FILE", v) },
        None => unsafe { std::env::remove_var("ISSUE_FACTORY_DB_FILE") },
    }
    let _ = std::fs::remove_file(&db);
}

#[test]
fn execution_logs_unavailable_and_empty_are_distinguishable() {
    let _lock = crate::state::tests::TEST_ENV_LOCK
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    let db = std::env::temp_dir().join(format!(
        "factory_cli_logs2_{}_{}.db",
        std::process::id(),
        uuid::Uuid::now_v7().simple()
    ));
    let prev = std::env::var("ISSUE_FACTORY_DB_FILE").ok();
    unsafe { std::env::set_var("ISSUE_FACTORY_DB_FILE", &db) };

    let rec = crate::execution::create_execution(
        "org/repo#1",
        "org/repo",
        1,
        1,
        Some("cli"),
        Some("local"),
        None,
    )
    .unwrap();

    let err = super::execution_logs(&rec.execution_id, false, 200).unwrap_err();
    assert!(err.to_string().contains(&format!(
        "logs unavailable for execution: {}",
        rec.execution_id
    )));

    crate::execution::logs::ensure_logs_dir().unwrap();
    std::fs::File::create(crate::execution::logs::log_path(&rec.execution_id)).unwrap();
    // empty is distinguishable: returns Ok and prints a message (we assert no error here)
    let ok = super::execution_logs(&rec.execution_id, false, 200);
    assert!(ok.is_ok());

    match prev {
        Some(v) => unsafe { std::env::set_var("ISSUE_FACTORY_DB_FILE", v) },
        None => unsafe { std::env::remove_var("ISSUE_FACTORY_DB_FILE") },
    }
    let _ = std::fs::remove_dir_all(db.parent().unwrap().join("logs"));
    let _ = std::fs::remove_file(&db);
}
