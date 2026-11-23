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
