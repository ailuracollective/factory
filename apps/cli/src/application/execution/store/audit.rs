use super::*;
use anyhow::Result;
use serde::Serialize;

#[derive(Debug, Clone, Serialize)]
pub struct ExecutionAuditEvent {
    pub ts: u64,
    pub actor: String,
    pub action: String,
    pub target: String,
    pub outcome: String,
    pub detail: Option<String>,
}

pub fn append_audit(event: ExecutionAuditEvent) -> Result<()> {
    let path = db_path();
    crate::state::run_async(async move {
        let db = connect(&path).await?;
        db.execute_unprepared(
            "CREATE TABLE IF NOT EXISTS execution_audit (id INTEGER PRIMARY KEY AUTOINCREMENT, ts INTEGER NOT NULL, actor TEXT, action TEXT, target TEXT, outcome TEXT, detail TEXT)",
        )
        .await?;
        let stmt = sea_orm::Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "INSERT INTO execution_audit (ts, actor, action, target, outcome, detail) VALUES (?, ?, ?, ?, ?, ?)",
            [
                (event.ts as i64).into(),
                event.actor.into(),
                event.action.into(),
                event.target.into(),
                event.outcome.into(),
                event.detail.into(),
            ],
        );
        db.execute_raw(stmt).await?;
        Ok::<(), anyhow::Error>(())
    })?;
    Ok(())
}

pub fn list_audit(target: &str, limit: u64) -> Result<Vec<ExecutionAuditEvent>> {
    let path = db_path();
    let target = target.to_string();
    crate::state::run_async(async move {
        let db = connect(&path).await?;
        db.execute_unprepared(
            "CREATE TABLE IF NOT EXISTS execution_audit (id INTEGER PRIMARY KEY AUTOINCREMENT, ts INTEGER NOT NULL, actor TEXT, action TEXT, target TEXT, outcome TEXT, detail TEXT)",
        )
        .await?;
        let raw = db
            .query_all_raw(sea_orm::Statement::from_sql_and_values(
                DbBackend::Sqlite,
                "SELECT ts, actor, action, target, outcome, detail FROM execution_audit WHERE target = ? ORDER BY ts DESC LIMIT ?",
                [target.into(), (limit as i64).into()],
            ))
            .await?;
        let mut out = Vec::new();
        for r in raw {
            let ts: i64 = r.try_get("", "ts").unwrap_or(0);
            let actor: String = r.try_get("", "actor").unwrap_or_default();
            let action: String = r.try_get("", "action").unwrap_or_default();
            let tgt: String = r.try_get("", "target").unwrap_or_default();
            let outcome: String = r.try_get("", "outcome").unwrap_or_default();
            let detail: Option<String> = r.try_get("", "detail").ok();
            out.push(ExecutionAuditEvent {
                ts: std::cmp::Ord::max(ts, 0) as u64,
                actor,
                action,
                target: tgt,
                outcome,
                detail,
            });
        }
        Ok(out)
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn with_temp_db<F: FnOnce()>(f: F) {
        let _lock = crate::state::tests::TEST_ENV_LOCK
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let tmp =
            std::env::temp_dir().join(format!("factory_exec_test_{}.db", uuid::Uuid::now_v7()));
        let _ = fs::remove_file(&tmp);
        // FIXME: Audit that the environment access only happens in single-threaded code.
        unsafe { std::env::set_var("ISSUE_FACTORY_DB_FILE", tmp.to_str().unwrap()) };
        f();
        std::fs::remove_file(&tmp).ok();
        // FIXME: Audit that the environment access only happens in single-threaded code.
        unsafe { std::env::remove_var("ISSUE_FACTORY_DB_FILE") };
    }

    #[test]
    fn execution_registration_reports_database_failure() {
        let _lock = crate::state::tests::TEST_ENV_LOCK
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let parent = std::env::temp_dir().join(format!(
            "factory_exec_not_a_dir_{}",
            uuid::Uuid::now_v7().simple()
        ));
        fs::write(&parent, "file").unwrap();
        let db_path = parent.join("factory.db");
        // FIXME: Audit that the environment access only happens in single-threaded code.
        unsafe { std::env::set_var("ISSUE_FACTORY_DB_FILE", db_path.to_str().unwrap()) };
        assert!(create_execution("org/repo#1", "org/repo", 1, 1, None, None, None).is_err());
        fs::remove_file(parent).unwrap();
        // FIXME: Audit that the environment access only happens in single-threaded code.
        unsafe { std::env::remove_var("ISSUE_FACTORY_DB_FILE") };
    }

    #[test]
    fn queued_cancel_is_immediate_and_idempotent() {
        with_temp_db(|| {
            let rec = create_execution(
                "org/repo#1",
                "org/repo",
                1,
                1,
                Some("cli"),
                Some("local"),
                None,
            )
            .unwrap();
            let r1 = request_cancel(&rec.execution_id, "cli:alice", false, Some("stop")).unwrap();
            assert_eq!(r1.status, ExecutionStatus::Cancelled);
            assert!(r1.confirmed);
            let r2 = request_cancel(&rec.execution_id, "cli:alice", false, Some("stop")).unwrap();
            assert_eq!(r2.status, ExecutionStatus::Cancelled);
            assert!(r2.confirmed);
            let stale_start = transition_execution(
                &rec.execution_id,
                Some(ExecutionStatus::Queued),
                ExecutionStatus::Running,
                None,
                None,
            )
            .unwrap();
            assert_eq!(stale_start.status, ExecutionStatus::Cancelled);
        });
    }

    #[test]
    fn running_cancel_moves_to_cancelling_then_confirmed() {
        with_temp_db(|| {
            let rec = create_execution(
                "org/repo#2",
                "org/repo",
                2,
                1,
                Some("cli"),
                Some("local"),
                None,
            )
            .unwrap();
            transition_execution(
                &rec.execution_id,
                Some(ExecutionStatus::Queued),
                ExecutionStatus::Running,
                None,
                None,
            )
            .unwrap();
            let r = request_cancel(&rec.execution_id, "cli:alice", false, None).unwrap();
            assert_eq!(r.status, ExecutionStatus::Cancelling);
            assert!(!r.confirmed);
            let done = mark_cancel_confirmed(&rec.execution_id, "cancelled")
                .unwrap()
                .unwrap();
            assert_eq!(done.status, ExecutionStatus::Cancelled);
            assert!(done.cancel_confirmed_at.is_some());
            let late_completion = complete_execution(&rec.execution_id, Some("late"), None)
                .unwrap()
                .unwrap();
            assert_eq!(late_completion.status, ExecutionStatus::Cancelled);
        });
    }

    #[test]
    fn terminal_states_are_immutable() {
        with_temp_db(|| {
            let rec = create_execution(
                "org/repo#3",
                "org/repo",
                3,
                1,
                Some("cli"),
                Some("local"),
                None,
            )
            .unwrap();
            transition_execution(
                &rec.execution_id,
                Some(ExecutionStatus::Queued),
                ExecutionStatus::Running,
                None,
                None,
            )
            .unwrap();
            complete_execution(&rec.execution_id, Some("t1"), None).unwrap();
            let after = get_execution(&rec.execution_id).unwrap().unwrap();
            assert_eq!(after.status, ExecutionStatus::Completed);
            let r = request_cancel(&rec.execution_id, "cli:alice", false, None).unwrap();
            assert_eq!(r.status, ExecutionStatus::Completed);
            assert!(!r.confirmed);
            let again = get_execution(&rec.execution_id).unwrap().unwrap();
            assert_eq!(again.status, ExecutionStatus::Completed);
        });
    }

    #[test]
    fn cancel_ambiguity_resolution_lists_cancellable() {
        with_temp_db(|| {
            let a = create_execution(
                "org/repo#4",
                "org/repo",
                4,
                1,
                Some("cli"),
                Some("local"),
                None,
            )
            .unwrap();
            let b = create_execution(
                "org/repo#4",
                "org/repo",
                4,
                2,
                Some("cli"),
                Some("local"),
                None,
            )
            .unwrap();
            transition_execution(
                &a.execution_id,
                Some(ExecutionStatus::Queued),
                ExecutionStatus::Running,
                None,
                None,
            )
            .unwrap();
            transition_execution(
                &b.execution_id,
                Some(ExecutionStatus::Queued),
                ExecutionStatus::Running,
                None,
                None,
            )
            .unwrap();
            let matches = list_cancellable_for_issue("org/repo", 4).unwrap();
            assert_eq!(matches.len(), 2);
            let ids: Vec<_> = matches.iter().map(|m| m.execution_id.as_str()).collect();
            assert!(ids.contains(&a.execution_id.as_str()));
            assert!(ids.contains(&b.execution_id.as_str()));
        });
    }
}
