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
