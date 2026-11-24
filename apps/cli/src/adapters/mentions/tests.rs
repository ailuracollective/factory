use super::*;
use crate::config::{Config, MentionsConfig, RepoEntry};
use std::collections::HashMap;
use std::path::Path;
use std::sync::Mutex;

fn mentions() -> MentionsConfig {
    MentionsConfig {
        enabled: true,
        usernames: vec!["alice".to_string()],
        command: "factory".to_string(),
        ..MentionsConfig::default()
    }
}

fn config_with(mentions: MentionsConfig) -> Config {
    Config {
        repos: vec![RepoEntry {
            name: "factory".to_string(),
            path: "/tmp/factory".to_string(),
            github: "org/factory".to_string(),
            hook_id: None,
            enabled: true,
        }],
        mentions,
        ..Config::default()
    }
}

fn event(id: &str, body: &str, updated_at: &str) -> TriggerEvent {
    TriggerEvent {
        repo: "org/factory".to_string(),
        kind: EventKind::Comment,
        id: id.to_string(),
        number: 5,
        linked_issue_number: None,
        title: "Issue".to_string(),
        body: body.to_string(),
        url: "https://github.com/org/factory/issues/5".to_string(),
        author: "alice".to_string(),
        author_is_bot: false,
        is_pr: false,
        updated_at: updated_at.to_string(),
    }
}

#[test]
fn parse_self_mention_with_command() {
    let inv = parse_invocation("@alice factory fix the bug", &mentions()).unwrap();
    assert_eq!(inv.username, "alice");
    assert_eq!(inv.instruction, "fix the bug");
}

#[test]
fn parse_is_case_insensitive_and_accepts_slash() {
    let inv = parse_invocation("hi @Alice /Factory: please fix", &mentions()).unwrap();
    assert_eq!(inv.instruction, "please fix");
}

#[test]
fn skips_non_command_mentions_and_finds_a_later_command() {
    let body = "> @alice said hi\n\n@alice factory please fix";
    let inv = parse_invocation(body, &mentions()).unwrap();
    assert_eq!(inv.instruction, "please fix");
}

#[test]
fn ordinary_mention_without_command_is_ignored() {
    assert!(parse_invocation("hey @alice, what do you think?", &mentions()).is_none());
    assert!(parse_invocation("email me at bob@alice.com", &mentions()).is_none());
    assert!(parse_invocation("@alice-factory run", &mentions()).is_none());
}

#[test]
fn empty_command_allows_any_mention() {
    let mut cfg = mentions();
    cfg.command = String::new();
    assert!(parse_invocation("ping @alice", &cfg).is_some());
}

#[test]
fn report_marker_is_never_an_invocation() {
    let body = format!("@alice factory again\n{REPORT_MARKER}");
    assert!(parse_invocation(&body, &mentions()).is_none());
}

#[derive(Default)]
struct MemStore {
    events: Mutex<HashMap<String, (String, String, u64)>>,
}

impl EventStore for MemStore {
    fn seen(&self, key: &str) -> bool {
        self.events.lock().unwrap().contains_key(key)
    }

    fn record(&self, key: &str, status: &str, detail: &str) -> Result<()> {
        self.events
            .lock()
            .unwrap()
            .insert(key.to_string(), (status.to_string(), detail.to_string(), 0));
        Ok(())
    }

    fn count_launched_since(&self, _since: u64) -> usize {
        self.events
            .lock()
            .unwrap()
            .values()
            .filter(|(status, _, _)| status == "accepted" || status == "completed")
            .count()
    }
}

impl MemStore {
    fn status(&self, key: &str) -> Option<String> {
        self.events
            .lock()
            .unwrap()
            .get(key)
            .map(|(status, _, _)| status.clone())
    }
}

#[derive(Default)]
struct FakeDispatcher {
    launched: Mutex<Vec<(String, String)>>,
    reports: Mutex<Vec<String>>,
    fail: bool,
}

impl Dispatcher for FakeDispatcher {
    fn launch(&self, event: &TriggerEvent, instruction: &str) -> Result<String, String> {
        self.launched
            .lock()
            .unwrap()
            .push((event.audit_key(), instruction.to_string()));
        if self.fail {
            Err("launch failed".to_string())
        } else {
            Ok(format!("thread-{}", event.id))
        }
    }

    fn report(&self, event: &TriggerEvent, status: &str) {
        self.reports
            .lock()
            .unwrap()
            .push(format!("{} {}", event.id, status));
    }
}

struct AllowAll;
impl Authorizer for AllowAll {
    fn can_execute(&self, _repo: &str, _user: &str) -> bool {
        true
    }
}

struct DenyAll;
impl Authorizer for DenyAll {
    fn can_execute(&self, _repo: &str, _user: &str) -> bool {
        false
    }
}

#[test]
fn self_mention_launches_without_permission_lookup() {
    let config = config_with(mentions());
    let store = MemStore::default();
    let dispatcher = FakeDispatcher::default();
    let engine = Engine::new(&config, &store, &dispatcher, &DenyAll);
    let ev = event("1", "@alice factory fix it", "2024-01-01T00:00:00Z");
    assert!(matches!(engine.handle(&ev, 100), Outcome::Launched(_)));
    assert_eq!(dispatcher.launched.lock().unwrap().len(), 1);
    assert_eq!(store.status(&ev.audit_key()).as_deref(), Some("completed"));
}

#[test]
fn third_party_mention_rejected_without_permission() {
    let config = config_with(mentions());
    let store = MemStore::default();
    let dispatcher = FakeDispatcher::default();
    let engine = Engine::new(&config, &store, &dispatcher, &DenyAll);
    let mut ev = event("2", "@alice factory fix it", "2024-01-01T00:00:00Z");
    ev.author = "mallory".to_string();
    assert!(matches!(engine.handle(&ev, 100), Outcome::Rejected(_)));
    assert!(dispatcher.launched.lock().unwrap().is_empty());
    assert_eq!(store.status(&ev.audit_key()).as_deref(), Some("rejected"));
}

#[test]
fn third_party_with_write_permission_is_allowed() {
    let config = config_with(mentions());
    let store = MemStore::default();
    let dispatcher = FakeDispatcher::default();
    let engine = Engine::new(&config, &store, &dispatcher, &AllowAll);
    let mut ev = event("3", "@alice factory fix it", "2024-01-01T00:00:00Z");
    ev.author = "bob".to_string();
    assert!(matches!(engine.handle(&ev, 100), Outcome::Launched(_)));
}

#[test]
fn allowlisted_author_without_permission_is_allowed() {
    let mut cfg = mentions();
    cfg.allowed_authors = vec!["bob".to_string()];
    let config = config_with(cfg);
    let store = MemStore::default();
    let dispatcher = FakeDispatcher::default();
    let engine = Engine::new(&config, &store, &dispatcher, &DenyAll);
    let mut ev = event("4", "@alice factory fix it", "2024-01-01T00:00:00Z");
    ev.author = "bob".to_string();
    assert!(matches!(engine.handle(&ev, 100), Outcome::Launched(_)));
}

#[test]
fn repo_outside_allowlist_is_rejected() {
    let config = config_with(mentions());
    let store = MemStore::default();
    let dispatcher = FakeDispatcher::default();
    let engine = Engine::new(&config, &store, &dispatcher, &AllowAll);
    let mut ev = event("5", "@alice factory fix it", "2024-01-01T00:00:00Z");
    ev.repo = "evil/other".to_string();
    assert!(matches!(engine.handle(&ev, 100), Outcome::Rejected(_)));
    assert!(dispatcher.launched.lock().unwrap().is_empty());
}

#[test]
fn bot_authored_mention_is_ignored() {
    let config = config_with(mentions());
    let store = MemStore::default();
    let dispatcher = FakeDispatcher::default();
    let engine = Engine::new(&config, &store, &dispatcher, &AllowAll);
    let mut ev = event("6", "@alice factory fix it", "2024-01-01T00:00:00Z");
    ev.author = "factory[bot]".to_string();
    ev.author_is_bot = true;
    assert_eq!(engine.handle(&ev, 100), Outcome::Ignored("bot"));
    assert!(dispatcher.launched.lock().unwrap().is_empty());
}

#[test]
fn factory_report_comment_does_not_retrigger() {
    let config = config_with(mentions());
    let store = MemStore::default();
    let dispatcher = FakeDispatcher::default();
    let engine = Engine::new(&config, &store, &dispatcher, &AllowAll);
    let body = format!("done\n{REPORT_MARKER}\nFactory: @alice factory again");
    let ev = event("7", &body, "2024-01-01T00:00:00Z");
    assert_eq!(engine.handle(&ev, 100), Outcome::Ignored("no-invocation"));
    assert!(dispatcher.launched.lock().unwrap().is_empty());
}

#[test]
fn duplicate_revision_does_not_launch_twice() {
    let config = config_with(mentions());
    let store = MemStore::default();
    let dispatcher = FakeDispatcher::default();
    let engine = Engine::new(&config, &store, &dispatcher, &AllowAll);
    let ev = event("8", "@alice factory fix it", "2024-01-01T00:00:00Z");
    assert!(matches!(engine.handle(&ev, 100), Outcome::Launched(_)));
    assert_eq!(engine.handle(&ev, 100), Outcome::Ignored("duplicate"));
    assert_eq!(dispatcher.launched.lock().unwrap().len(), 1);
}

#[test]
fn edit_creates_a_new_revision() {
    let config = config_with(mentions());
    let store = MemStore::default();
    let dispatcher = FakeDispatcher::default();
    let engine = Engine::new(&config, &store, &dispatcher, &AllowAll);
    let first = event("9", "@alice factory fix it", "2024-01-01T00:00:00Z");
    let edited = event("9", "@alice factory fix it better", "2024-01-01T00:05:00Z");
    assert!(matches!(engine.handle(&first, 100), Outcome::Launched(_)));
    assert!(matches!(engine.handle(&edited, 100), Outcome::Launched(_)));
    assert_eq!(dispatcher.launched.lock().unwrap().len(), 2);
}

#[test]
fn issue_activity_without_body_change_does_not_launch_again() {
    let config = config_with(mentions());
    let store = MemStore::default();
    let dispatcher = FakeDispatcher::default();
    let engine = Engine::new(&config, &store, &dispatcher, &AllowAll);
    let mut first = event("20", "@alice factory fix it", "2024-01-01T00:00:00Z");
    first.kind = EventKind::Issue;
    let mut commented = first.clone();
    commented.updated_at = "2024-01-01T00:10:00Z".to_string();
    assert!(matches!(engine.handle(&first, 100), Outcome::Launched(_)));
    assert_eq!(
        engine.handle(&commented, 100),
        Outcome::Ignored("duplicate")
    );
    assert_eq!(dispatcher.launched.lock().unwrap().len(), 1);
}

#[test]
fn unauthorized_request_is_audited_once() {
    let config = config_with(mentions());
    let store = MemStore::default();
    let dispatcher = FakeDispatcher::default();
    let engine = Engine::new(&config, &store, &dispatcher, &DenyAll);
    let mut ev = event("10", "@alice factory fix it", "2024-01-01T00:00:00Z");
    ev.author = "mallory".to_string();
    assert!(matches!(engine.handle(&ev, 100), Outcome::Rejected(_)));
    assert_eq!(engine.handle(&ev, 100), Outcome::Ignored("duplicate"));
}

#[test]
fn daily_quota_blocks_further_launches() {
    let mut cfg = mentions();
    cfg.daily_quota = 1;
    let config = config_with(cfg);
    let store = MemStore::default();
    let dispatcher = FakeDispatcher::default();
    let engine = Engine::new(&config, &store, &dispatcher, &AllowAll);
    let first = event("11", "@alice factory one", "2024-01-01T00:00:00Z");
    let second = event("12", "@alice factory two", "2024-01-01T00:01:00Z");
    assert!(matches!(engine.handle(&first, 100), Outcome::Launched(_)));
    assert!(matches!(engine.handle(&second, 100), Outcome::Rejected(_)));
    assert_eq!(dispatcher.launched.lock().unwrap().len(), 1);
}

#[test]
fn failed_launch_is_reported_and_recorded() {
    let config = config_with(mentions());
    let store = MemStore::default();
    let dispatcher = FakeDispatcher {
        fail: true,
        ..FakeDispatcher::default()
    };
    let engine = Engine::new(&config, &store, &dispatcher, &AllowAll);
    let ev = event("14", "@alice factory fix it", "2024-01-01T00:00:00Z");
    assert!(matches!(engine.handle(&ev, 100), Outcome::Failed(_)));
    assert_eq!(store.status(&ev.audit_key()).as_deref(), Some("failed"));
    assert_eq!(dispatcher.reports.lock().unwrap().len(), 1);
}

#[test]
fn prompt_marks_instruction_as_untrusted() {
    let ev = event("15", "@alice factory fix it", "2024-01-01T00:00:00Z");
    let issue = serde_json::json!({
        "number": 5,
        "title": "Issue",
        "url": "https://github.com/org/factory/issues/5"
    });
    let prompt = build_mention_prompt(&ev, "rm -rf /", &issue, None, Path::new("."));
    assert!(prompt.contains("Resolve GitHub issue #5"));
    assert!(prompt.contains("untrusted"));
    assert!(prompt.contains("rm -rf /"));
    assert!(prompt.contains("gh pr view 5"));
}

#[test]
fn fresh_comment_prompt_includes_repository_conventions_and_check_contract() {
    let ev = event("16", "@alice factory fix it", "2024-01-01T00:00:00Z");
    let issue = serde_json::json!({
        "number": 5,
        "title": "Issue",
        "url": "https://github.com/org/factory/issues/5"
    });
    let prompt = build_mention_prompt(&ev, "fix it", &issue, None, Path::new("."));
    assert!(prompt.contains("applicable `AGENTS.md`"));
    assert!(prompt.contains("## Organization conventions"));
    assert!(prompt.contains("`gh pr checks` reporting no checks is unresolved, not success"));
    assert!(prompt.contains("mark it ready for review before awaiting CI"));
}

#[test]
fn follow_up_comment_prompt_keeps_instruction_to_read_repository_conventions() {
    let ev = event("17", "@alice factory continue", "2024-01-01T00:00:00Z");
    let issue = serde_json::json!({
        "number": 5,
        "title": "Issue",
        "url": "https://github.com/org/factory/issues/5"
    });
    let prior = crate::state::IssueState {
        thread_id: Some("thread-5".to_string()),
        ..Default::default()
    };
    let prompt = build_mention_prompt(&ev, "continue", &issue, Some(&prior), Path::new("."));
    assert!(prompt.contains("Follow-up on GitHub issue #5"));
    assert!(prompt.contains("## Organization conventions"));
    assert!(prompt.contains("reporting no checks is unresolved"));
    assert!(prompt.contains("linked organization conventions"));
}

#[test]
fn sqlite_store_persists_dedupe_and_audit() {
    let _lock = crate::state::tests::TEST_ENV_LOCK
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    let tmp = std::env::temp_dir().join("factory_mentions_store.db");
    let _ = std::fs::remove_file(&tmp);
    // FIXME: Audit that the environment access only happens in single-threaded code.
    unsafe { std::env::set_var("ISSUE_FACTORY_DB_FILE", tmp.to_str().unwrap()) };

    let store = SqliteEventStore;
    assert!(!store.seen("k"));
    store.record("k", "accepted", "op=fix").unwrap();
    assert!(store.seen("k"));
    assert_eq!(store.count_launched_since(0), 1);
    store.record("k", "completed", "thread=1").unwrap();
    assert_eq!(store.count_launched_since(0), 1);

    // A fresh store instance (restart) sees the same persisted state.
    let restarted = SqliteEventStore;
    assert!(restarted.seen("k"));

    std::fs::remove_file(&tmp).ok();
    // FIXME: Audit that the environment access only happens in single-threaded code.
    unsafe { std::env::remove_var("ISSUE_FACTORY_DB_FILE") };
}

#[test]
fn parses_github_objects() {
    let issue = serde_json::json!({
        "id": 101,
        "number": 7,
        "title": "Bug",
        "body": "body",
        "html_url": "https://github.com/org/factory/issues/7",
        "updated_at": "2024-05-01T10:00:00Z",
        "user": {"login": "alice", "type": "User"}
    });
    let ev = issue_event("org/factory", &issue).unwrap();
    assert_eq!(ev.kind, EventKind::Issue);
    assert_eq!(ev.number, 7);
    assert!(!ev.is_pr);

    let _ = serde_json::json!({
        "id": 202,
        "body": "@alice factory fix",
        "html_url": "https://github.com/org/factory/issues/7#issuecomment-202",
        "issue_url": "https://api.github.com/repos/org/factory/issues/7",
        "updated_at": "2024-05-01T10:01:00Z",
        "user": {"login": "renovate[bot]", "type": "Bot"}
    });
}

#[test]
fn parse_cancel_mention_with_execution_id() {
    let inv = parse_cancel_invocation("@alice cancel exec_123", &mentions()).unwrap();
    assert_eq!(inv.username, "alice");
    assert_eq!(inv.execution_id.as_deref(), Some("exec_123"));
}

#[test]
fn parse_cancel_mention_without_execution_id() {
    let inv = parse_cancel_invocation("@alice factory cancel", &mentions()).unwrap();
    assert_eq!(inv.execution_id, None);
}

#[test]
fn cancel_mention_rejects_unauthorized_author() {
    let _lock = crate::state::tests::TEST_ENV_LOCK
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    let tmp = std::env::temp_dir().join(format!(
        "factory_cancel_auth_{}.db",
        uuid::Uuid::now_v7().simple()
    ));
    let _ = std::fs::remove_file(&tmp);
    // FIXME: Audit that the environment access only happens in single-threaded code.
    unsafe { std::env::set_var("ISSUE_FACTORY_DB_FILE", tmp.to_str().unwrap()) };
    let rec = crate::execution::create_execution(
        "org/factory#5",
        "org/factory",
        5,
        1,
        Some("github:alice"),
        Some("local"),
        None,
    )
    .unwrap();

    let config = config_with(mentions());
    let store = MemStore::default();
    let dispatcher = FakeDispatcher::default();
    let engine = Engine::new(&config, &store, &dispatcher, &DenyAll);
    let mut ev = event(
        "c1",
        &format!("@alice cancel {}", rec.execution_id),
        "2024-01-01T00:00:00Z",
    );
    ev.author = "mallory".to_string();
    let out = engine.handle(&ev, 100);
    assert!(matches!(out, Outcome::Rejected(_)));
    std::fs::remove_file(&tmp).ok();
    // FIXME: Audit that the environment access only happens in single-threaded code.
    unsafe { std::env::remove_var("ISSUE_FACTORY_DB_FILE") };
}

#[test]
fn cancel_mention_rejects_ambiguous_context() {
    let _lock = crate::state::tests::TEST_ENV_LOCK
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    let tmp = std::env::temp_dir().join(format!(
        "factory_cancel_amb_{}.db",
        uuid::Uuid::now_v7().simple()
    ));
    let _ = std::fs::remove_file(&tmp);
    // FIXME: Audit that the environment access only happens in single-threaded code.
    unsafe { std::env::set_var("ISSUE_FACTORY_DB_FILE", tmp.to_str().unwrap()) };
    let a = crate::execution::create_execution(
        "org/factory#5",
        "org/factory",
        5,
        1,
        Some("github:alice"),
        Some("local"),
        None,
    )
    .unwrap();
    let b = crate::execution::create_execution(
        "org/factory#5",
        "org/factory",
        5,
        2,
        Some("github:alice"),
        Some("local"),
        None,
    )
    .unwrap();
    let _ = crate::execution::transition_execution(
        &a.execution_id,
        Some(crate::execution::ExecutionStatus::Queued),
        crate::execution::ExecutionStatus::Running,
        None,
        None,
    );
    let _ = crate::execution::transition_execution(
        &b.execution_id,
        Some(crate::execution::ExecutionStatus::Queued),
        crate::execution::ExecutionStatus::Running,
        None,
        None,
    );

    let config = config_with(mentions());
    let store = MemStore::default();
    let dispatcher = FakeDispatcher::default();
    let engine = Engine::new(&config, &store, &dispatcher, &AllowAll);
    let ev = event("c2", "@alice factory cancel", "2024-01-01T00:00:00Z");
    let out = engine.handle(&ev, 100);
    match out {
        Outcome::Rejected(msg) => assert!(msg.contains("ambiguous")),
        other => panic!("expected rejected, got {other:?}"),
    }
    std::fs::remove_file(&tmp).ok();
    // FIXME: Audit that the environment access only happens in single-threaded code.
    unsafe { std::env::remove_var("ISSUE_FACTORY_DB_FILE") };
}
