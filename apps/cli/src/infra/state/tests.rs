use super::*;
use std::sync::Mutex;
pub static TEST_ENV_LOCK: Mutex<()> = Mutex::new(());

#[test]
fn roundtrip_state() {
    let _lock = TEST_ENV_LOCK.lock().unwrap();
    let mut m = HashMap::new();
    m.insert(
        "repo#1".to_string(),
        IssueState {
            status: IssueStatus::Completed,
            attempts: 2,
            thread_id: Some("t-1".to_string()),
            ..Default::default()
        },
    );
    let tmp = std::env::temp_dir().join("factory_state_test.db");
    // FIXME: Audit that the environment access only happens in single-threaded code.
    unsafe { std::env::set_var("ISSUE_FACTORY_DB_FILE", tmp.to_str().unwrap()) };
    let _ = fs::remove_file(&tmp);
    save_state(&m).unwrap();
    let loaded = load_state();
    assert_eq!(loaded.get("repo#1").unwrap().status, IssueStatus::Completed);
    assert_eq!(
        loaded.get("repo#1").unwrap().thread_id.as_deref(),
        Some("t-1")
    );
    std::fs::remove_file(&tmp).ok();
    // FIXME: Audit that the environment access only happens in single-threaded code.
    unsafe { std::env::remove_var("ISSUE_FACTORY_DB_FILE") };
}

#[test]
fn reconcile_running_to_failed() {
    let _lock = TEST_ENV_LOCK.lock().unwrap();
    let tmp = std::env::temp_dir().join("factory_state_running.db");
    // FIXME: Audit that the environment access only happens in single-threaded code.
    unsafe { std::env::set_var("ISSUE_FACTORY_DB_FILE", tmp.to_str().unwrap()) };
    let _ = fs::remove_file(&tmp);
    let mut m = HashMap::new();
    m.insert(
        "r#1".to_string(),
        IssueState {
            status: IssueStatus::Running,
            attempts: 1,
            ..Default::default()
        },
    );
    save_state(&m).unwrap();
    let loaded = load_state();
    assert_eq!(loaded.get("r#1").unwrap().status, IssueStatus::Failed);
    std::fs::remove_file(&tmp).ok();
    // FIXME: Audit that the environment access only happens in single-threaded code.
    unsafe { std::env::remove_var("ISSUE_FACTORY_DB_FILE") };
}

#[test]
fn pull_request_links_back_to_issue_state() {
    let _lock = TEST_ENV_LOCK.lock().unwrap();
    let tmp = std::env::temp_dir().join(format!(
        "factory_state_pr_link_{}.db",
        Uuid::now_v7().simple()
    ));
    // FIXME: Audit that the environment access only happens in single-threaded code.
    unsafe { std::env::set_var("ISSUE_FACTORY_DB_FILE", tmp.to_str().unwrap()) };
    let key = "org/repo#37";
    let state = IssueState {
        status: IssueStatus::Completed,
        thread_id: Some("thread-37".to_string()),
        pr_url: Some("https://github.com/org/repo/pull/38".to_string()),
        ..Default::default()
    };
    save_state(&HashMap::from([(key.to_string(), state)])).unwrap();
    link_pull_request(
        key,
        38,
        "https://github.com/org/repo/pull/38",
        Some("thread-37"),
    )
    .unwrap();

    assert_eq!(
        linked_pull_requests(key)
            .unwrap()
            .iter()
            .map(|pr| pr.number)
            .collect::<Vec<_>>(),
        vec![38]
    );
    let linked = pull_request_for_event("org/repo", 38).unwrap();
    assert_eq!(linked.issue_key, key);
    assert_eq!(linked.thread_id.as_deref(), Some("thread-37"));
    link_pull_request(
        "org/repo#37",
        40,
        "https://github.com/org/repo/pull/40",
        None,
    )
    .unwrap();
    assert_eq!(
        pull_request_for_event("org/repo", 40).map(|pr| pr.issue_key),
        Some("org/repo#37".to_string())
    );
    assert_eq!(
        pull_request_for_event("org/repo", 40).and_then(|pr| pr.thread_id),
        None,
        "PR lookup requires a persisted thread id"
    );

    link_pull_request(
        "org/repo#39",
        39,
        "https://github.com/org/repo/pull/39",
        Some("thread-39"),
    )
    .unwrap();
    link_pull_request(
        "org/repo#39",
        39,
        "https://github.com/org/repo/pull/39",
        None,
    )
    .unwrap();
    std::fs::remove_file(&tmp).ok();
    // FIXME: Audit that the environment access only happens in single-threaded code.
    unsafe { std::env::remove_var("ISSUE_FACTORY_DB_FILE") };
}
