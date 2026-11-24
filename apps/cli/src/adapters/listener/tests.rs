use super::*;
use serde_json::json;

#[test]
fn verify_signature_empty_secret() {
    assert!(!verify_signature("", b"hello", ""));
}

#[test]
fn verify_signature_rejects_bad() {
    assert!(!verify_signature("secret", b"hello", "sha256=deadbeef"));
}

#[test]
fn issue_matches_label_labeled() {
    let issue = json!({"labels": []});
    let payload = json!({"label": {"name": "status/ready"}});
    assert!(issue_matches_label(
        "status/ready",
        &issue,
        "labeled",
        &payload
    ));
}

#[test]
fn issue_matches_label_opened() {
    let issue = json!({"labels": [{"name": "status/ready"}, {"name": "ai"}]});
    let payload = json!({});
    assert!(issue_matches_label(
        "status/ready",
        &issue,
        "opened",
        &payload
    ));
    assert!(!issue_matches_label("other", &issue, "opened", &payload));
}

#[allow(clippy::await_holding_lock)]
mod lifecycle_tests {
    use super::*;
    use crate::config::{Config, RepoEntry};
    use crate::state::{IssueStatus, load_state};
    use serde_json::json;
    use std::sync::Mutex;

    fn make_config(tmp: &std::path::Path) -> Config {
        Config {
            repos: vec![RepoEntry {
                name: "repo".to_string(),
                path: tmp.to_string_lossy().to_string(),
                github: "org/repo".to_string(),
                hook_id: None,
                enabled: true,
            }],
            ..Config::default()
        }
    }

    fn write_script(name: &str, body: &str) -> std::path::PathBuf {
        let p = std::env::temp_dir().join(name);
        std::fs::write(&p, body).unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o755)).unwrap();
        }
        p
    }

    #[tokio::test]
    async fn failed_launch_allows_retry() {
        let _lock = crate::state::tests::TEST_ENV_LOCK
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let tmp = std::env::temp_dir().join("factory_lc_failed");
        let _ = std::fs::remove_dir_all(&tmp);
        std::fs::create_dir_all(&tmp).unwrap();
        let state_file = std::env::temp_dir().join("factory_lc_failed_state.json");
        let _ = std::fs::remove_file(&state_file);
        // FIXME: Audit that the environment access only happens in single-threaded code.
        unsafe { std::env::set_var("ISSUE_FACTORY_STATE_FILE", state_file.to_str().unwrap()) };

        let t3_fail = write_script("t3_fail.sh", "#!/bin/sh\nexit 1\n");
        // FIXME: Audit that the environment access only happens in single-threaded code.
        unsafe { std::env::set_var("ISSUE_FACTORY_T3", t3_fail.to_str().unwrap()) };

        let state = AppState {
            config: Arc::new(make_config(&tmp)),
            issues: Arc::new(Mutex::new(load_state())),
        };

        let payload = json!({
            "action": "opened",
            "issue": {"number": 1, "title": "t", "labels": [{"name": "status/ready"}], "url": "u"},
            "repository": {"full_name": "org/repo"}
        });

        let r = handle_payload(state.clone(), "issues".to_string(), payload.clone()).await;
        assert!(r.starts_with("failed"), "{}", r);

        let t3_ok = write_script(
            "t3_ok.sh",
            "#!/bin/sh\necho '{\"threadId\":\"thread-123\"}'\n",
        );
        // FIXME: Audit that the environment access only happens in single-threaded code.
        unsafe { std::env::set_var("ISSUE_FACTORY_T3", t3_ok.to_str().unwrap()) };

        let r2 = handle_payload(state.clone(), "issues".to_string(), payload).await;
        assert!(r2.starts_with("launched"), "{}", r2);

        let issues = state.issues.lock().unwrap();
        let rec = issues.get("org/repo#1").unwrap();
        assert_eq!(rec.status, IssueStatus::Completed);
        assert_eq!(rec.thread_id.as_deref(), Some("thread-123"));

        // FIXME: Audit that the environment access only happens in single-threaded code.
        unsafe { std::env::remove_var("ISSUE_FACTORY_T3") };
        // FIXME: Audit that the environment access only happens in single-threaded code.
        unsafe { std::env::remove_var("ISSUE_FACTORY_STATE_FILE") };
    }

    #[tokio::test]
    async fn duplicate_delivery_is_idempotent() {
        let _lock = crate::state::tests::TEST_ENV_LOCK
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let tmp = std::env::temp_dir().join("factory_lc_dup");
        let _ = std::fs::remove_dir_all(&tmp);
        std::fs::create_dir_all(&tmp).unwrap();
        let state_file = std::env::temp_dir().join("factory_lc_dup_state.json");
        let _ = std::fs::remove_file(&state_file);
        // FIXME: Audit that the environment access only happens in single-threaded code.
        unsafe { std::env::set_var("ISSUE_FACTORY_STATE_FILE", state_file.to_str().unwrap()) };

        let t3_ok = write_script(
            "t3_ok2.sh",
            "#!/bin/sh\necho '{\"threadId\":\"thread-dup\"}'\n",
        );
        // FIXME: Audit that the environment access only happens in single-threaded code.
        unsafe { std::env::set_var("ISSUE_FACTORY_T3", t3_ok.to_str().unwrap()) };

        let state = AppState {
            config: Arc::new(make_config(&tmp)),
            issues: Arc::new(Mutex::new(load_state())),
        };
        let payload = json!({
            "action": "opened",
            "issue": {"number": 2, "title": "t", "labels": [{"name": "status/ready"}], "url": "u"},
            "repository": {"full_name": "org/repo"}
        });

        let r1 = handle_payload(state.clone(), "issues".to_string(), payload.clone()).await;
        assert!(r1.starts_with("launched"), "{}", r1);
        let r2 = handle_payload(state.clone(), "issues".to_string(), payload).await;
        assert!(r2.contains("already"), "{}", r2);

        // FIXME: Audit that the environment access only happens in single-threaded code.
        unsafe { std::env::remove_var("ISSUE_FACTORY_T3") };
        // FIXME: Audit that the environment access only happens in single-threaded code.
        unsafe { std::env::remove_var("ISSUE_FACTORY_STATE_FILE") };
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn concurrent_deliveries_single_launch() {
        let _lock = crate::state::tests::TEST_ENV_LOCK
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let tmp = std::env::temp_dir().join("factory_lc_conc");
        let _ = std::fs::remove_dir_all(&tmp);
        std::fs::create_dir_all(&tmp).unwrap();
        let state_file = std::env::temp_dir().join("factory_lc_conc_state.json");
        let _ = std::fs::remove_file(&state_file);
        // FIXME: Audit that the environment access only happens in single-threaded code.
        unsafe { std::env::set_var("ISSUE_FACTORY_STATE_FILE", state_file.to_str().unwrap()) };

        let t3_slow = write_script(
            "t3_slow.sh",
            "#!/bin/sh\nsleep 1\necho '{\"threadId\":\"thread-conc\"}'\n",
        );
        // FIXME: Audit that the environment access only happens in single-threaded code.
        unsafe { std::env::set_var("ISSUE_FACTORY_T3", t3_slow.to_str().unwrap()) };

        let state = AppState {
            config: Arc::new(make_config(&tmp)),
            issues: Arc::new(Mutex::new(load_state())),
        };
        let payload = json!({
            "action": "opened",
            "issue": {"number": 3, "title": "t", "labels": [{"name": "status/ready"}], "url": "u"},
            "repository": {"full_name": "org/repo"}
        });

        let h1 = tokio::spawn(handle_payload(
            state.clone(),
            "issues".to_string(),
            payload.clone(),
        ));
        let h2 = tokio::spawn(handle_payload(state.clone(), "issues".to_string(), payload));
        let a = h1.await.unwrap();
        let b = h2.await.unwrap();
        // one launched, one ignored as already running
        let vals = [a, b];
        assert!(vals.iter().any(|v| v.starts_with("launched")), "{:?}", vals);
        assert!(
            vals.iter().any(|v| v.contains("already running")),
            "{:?}",
            vals
        );

        // FIXME: Audit that the environment access only happens in single-threaded code.
        unsafe { std::env::remove_var("ISSUE_FACTORY_T3") };
        // FIXME: Audit that the environment access only happens in single-threaded code.
        unsafe { std::env::remove_var("ISSUE_FACTORY_STATE_FILE") };
    }

    #[tokio::test]
    async fn restart_recovery_marks_running_failed() {
        let _lock = crate::state::tests::TEST_ENV_LOCK
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let state_file = std::env::temp_dir().join("factory_lc_restart_state.json");
        let _ = std::fs::remove_file(&state_file);
        // FIXME: Audit that the environment access only happens in single-threaded code.
        unsafe { std::env::set_var("ISSUE_FACTORY_STATE_FILE", state_file.to_str().unwrap()) };

        // persist a running record
        let mut m = std::collections::HashMap::new();
        m.insert(
            "org/repo#9".to_string(),
            IssueState {
                status: IssueStatus::Running,
                attempts: 1,
                ..IssueState::default()
            },
        );
        crate::state::save_state(&m).unwrap();

        let loaded = load_state();
        assert_eq!(
            loaded.get("org/repo#9").unwrap().status,
            IssueStatus::Failed
        );

        // FIXME: Audit that the environment access only happens in single-threaded code.
        unsafe { std::env::remove_var("ISSUE_FACTORY_STATE_FILE") };
    }

    #[tokio::test]
    async fn issue_comment_mention_launches_via_webhook() {
        let _lock = crate::state::tests::TEST_ENV_LOCK
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let tmp = std::env::temp_dir().join("factory_lc_mention_wh");
        let _ = std::fs::remove_dir_all(&tmp);
        std::fs::create_dir_all(&tmp).unwrap();
        let state_file = std::env::temp_dir().join("factory_lc_mention_wh_state.json");
        let _ = std::fs::remove_file(&state_file);
        // FIXME: Audit that the environment access only happens in single-threaded code.
        unsafe { std::env::set_var("ISSUE_FACTORY_STATE_FILE", state_file.to_str().unwrap()) };

        let t3_ok = write_script(
            "t3_mention_wh.sh",
            "#!/bin/sh\necho '{\"threadId\":\"thread-mention-wh\"}'\n",
        );
        // FIXME: Audit that the environment access only happens in single-threaded code.
        unsafe { std::env::set_var("ISSUE_FACTORY_T3", t3_ok.to_str().unwrap()) };

        let mut config = make_config(&tmp);
        config.mentions.enabled = true;
        config.mentions.usernames = vec!["alice".to_string()];
        config.mentions.command = "factory".to_string();

        let state = AppState {
            config: Arc::new(config),
            issues: Arc::new(Mutex::new(load_state())),
        };

        let payload = json!({
            "action": "created",
            "issue": {"number": 7, "title": "Bug", "html_url": "u", "pull_request": null},
            "comment": {
                "id": 1001,
                "body": "@alice factory fix it",
                "updated_at": "2026-01-01T00:00:00Z",
                "html_url": "c",
                "user": {"login": "alice", "type": "User"}
            },
            "repository": {"full_name": "org/repo"}
        });

        let r = handle_payload(state.clone(), "issue_comment".to_string(), payload).await;
        assert!(r.starts_with("mention launched"), "{}", r);

        // FIXME: Audit that the environment access only happens in single-threaded code.
        unsafe { std::env::remove_var("ISSUE_FACTORY_T3") };
        // FIXME: Audit that the environment access only happens in single-threaded code.
        unsafe { std::env::remove_var("ISSUE_FACTORY_STATE_FILE") };
    }
}
