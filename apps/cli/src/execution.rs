use anyhow::{anyhow, bail, Result};
use sea_orm::{
    sea_query::OnConflict, ActiveModelTrait, ColumnTrait, ConnectionTrait, Database,
    DatabaseConnection, DbBackend, EntityTrait, QueryFilter, QueryOrder, QuerySelect, Schema, Set,
};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use uuid::Uuid;

use crate::state;

mod execution_store {
    use sea_orm::entity::prelude::*;

    #[derive(Clone, Debug, PartialEq, Eq, DeriveEntityModel)]
    #[sea_orm(table_name = "executions")]
    pub struct Model {
        #[sea_orm(primary_key, auto_increment = false)]
        pub execution_id: String,
        pub issue_key: String,
        pub repo: String,
        pub issue_number: i64,
        pub attempt: i32,
        pub status: String,
        pub requester: Option<String>,
        pub runner: Option<String>,
        pub thread_id: Option<String>,
        pub pr_url: Option<String>,
        pub reason: Option<String>,
        pub cancel_requested: i32,
        pub cancel_requested_by: Option<String>,
        pub cancel_requested_at: Option<i64>,
        pub cancel_reason: Option<String>,
        pub cancel_confirmed_at: Option<i64>,
        pub cancel_outcome: Option<String>,
        pub started_at: i64,
        pub ended_at: Option<i64>,
        pub updated_at: i64,
        pub audit: Option<String>,
    }

    #[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
    pub enum Relation {}

    impl ActiveModelBehavior for ActiveModel {}
}

use execution_store::{
    ActiveModel as ExecutionActiveModel, Column as ExecutionColumn, Entity as ExecutionEntity,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ExecutionStatus {
    Queued,
    Running,
    Cancelling,
    Cancelled,
    Completed,
    Failed,
}

impl ExecutionStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Queued => "queued",
            Self::Running => "running",
            Self::Cancelling => "cancelling",
            Self::Cancelled => "cancelled",
            Self::Completed => "completed",
            Self::Failed => "failed",
        }
    }

    pub fn parse_status(s: &str) -> Self {
        match s {
            "running" => Self::Running,
            "cancelling" => Self::Cancelling,
            "cancelled" => Self::Cancelled,
            "completed" => Self::Completed,
            "failed" => Self::Failed,
            _ => Self::Queued,
        }
    }

    pub fn is_terminal(self) -> bool {
        matches!(self, Self::Completed | Self::Failed | Self::Cancelled)
    }

    pub fn is_cancellable(self) -> bool {
        matches!(self, Self::Queued | Self::Running | Self::Cancelling)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExecutionRecord {
    pub execution_id: String,
    pub issue_key: String,
    pub repo: String,
    pub issue_number: i64,
    pub attempt: u32,
    pub status: ExecutionStatus,
    pub requester: Option<String>,
    pub runner: Option<String>,
    pub thread_id: Option<String>,
    pub pr_url: Option<String>,
    pub reason: Option<String>,
    pub cancel_requested: bool,
    pub cancel_requested_by: Option<String>,
    pub cancel_requested_at: Option<u64>,
    pub cancel_reason: Option<String>,
    pub cancel_confirmed_at: Option<u64>,
    pub cancel_outcome: Option<String>,
    pub started_at: u64,
    pub ended_at: Option<u64>,
    pub updated_at: u64,
}

impl ExecutionRecord {
    fn from_model(m: execution_store::Model) -> Self {
        Self {
            execution_id: m.execution_id,
            issue_key: m.issue_key,
            repo: m.repo,
            issue_number: m.issue_number,
            attempt: m.attempt.max(0) as u32,
            status: ExecutionStatus::parse_status(&m.status),
            requester: m.requester,
            runner: m.runner,
            thread_id: m.thread_id,
            pr_url: m.pr_url,
            reason: m.reason,
            cancel_requested: m.cancel_requested != 0,
            cancel_requested_by: m.cancel_requested_by,
            cancel_requested_at: m.cancel_requested_at.map(|v| v.max(0) as u64),
            cancel_reason: m.cancel_reason,
            cancel_confirmed_at: m.cancel_confirmed_at.map(|v| v.max(0) as u64),
            cancel_outcome: m.cancel_outcome,
            started_at: m.started_at.max(0) as u64,
            ended_at: m.ended_at.map(|v| v.max(0) as u64),
            updated_at: m.updated_at.max(0) as u64,
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct CancelResult {
    pub execution_id: String,
    pub status: ExecutionStatus,
    pub requested: bool,
    pub confirmed: bool,
    pub message: String,
}

fn db_path() -> PathBuf {
    state::state_file()
}

fn run_async<F, T>(fut: F) -> T
where
    F: std::future::Future<Output = T> + Send,
    T: Send,
{
    std::thread::scope(|s| {
        let handle = s.spawn(|| {
            let rt = tokio::runtime::Runtime::new().expect("tokio runtime");
            rt.block_on(fut)
        });
        handle.join().expect("worker thread panicked")
    })
}

async fn connect(path: &std::path::Path) -> Result<DatabaseConnection> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let db = Database::connect(format!("sqlite://{}?mode=rwc", path.display())).await?;
    let schema = Schema::new(DbBackend::Sqlite);
    let mut stmt = schema.create_table_from_entity(ExecutionEntity);
    stmt.if_not_exists();
    db.execute(&stmt).await?;
    // Lightweight migrations for existing DBs.
    let _ = db
        .execute_unprepared("ALTER TABLE executions ADD COLUMN cancel_confirmed_at INTEGER")
        .await;
    let _ = db
        .execute_unprepared("ALTER TABLE executions ADD COLUMN cancel_outcome TEXT")
        .await;
    let _ = db
        .execute_unprepared("ALTER TABLE executions ADD COLUMN pr_url TEXT")
        .await;
    Ok(db)
}

pub fn new_execution_id() -> String {
    format!("exec_{}", Uuid::now_v7().simple())
}

pub fn create_execution(
    issue_key: &str,
    repo: &str,
    issue_number: i64,
    attempt: u32,
    requester: Option<&str>,
    runner: Option<&str>,
    reason: Option<&str>,
) -> Result<ExecutionRecord> {
    let path = db_path();
    let rec = ExecutionRecord {
        execution_id: new_execution_id(),
        issue_key: issue_key.to_string(),
        repo: repo.to_string(),
        issue_number,
        attempt,
        status: ExecutionStatus::Queued,
        requester: requester.map(|s| s.to_string()),
        runner: runner.map(|s| s.to_string()),
        thread_id: None,
        pr_url: None,
        reason: reason.map(|s| s.to_string()),
        cancel_requested: false,
        cancel_requested_by: None,
        cancel_requested_at: None,
        cancel_reason: None,
        cancel_confirmed_at: None,
        cancel_outcome: None,
        started_at: state::now(),
        ended_at: None,
        updated_at: state::now(),
    };
    let rec_clone = rec.clone();
    run_async(async move {
        let db = connect(&path).await?;
        let am = ExecutionActiveModel {
            execution_id: Set(rec_clone.execution_id.clone()),
            issue_key: Set(rec_clone.issue_key.clone()),
            repo: Set(rec_clone.repo.clone()),
            issue_number: Set(rec_clone.issue_number),
            attempt: Set(rec_clone.attempt as i32),
            status: Set(rec_clone.status.as_str().to_string()),
            requester: Set(rec_clone.requester.clone()),
            runner: Set(rec_clone.runner.clone()),
            thread_id: Set(rec_clone.thread_id.clone()),
            pr_url: Set(rec_clone.pr_url.clone()),
            reason: Set(rec_clone.reason.clone()),
            cancel_requested: Set(if rec_clone.cancel_requested { 1 } else { 0 }),
            cancel_requested_by: Set(rec_clone.cancel_requested_by.clone()),
            cancel_requested_at: Set(rec_clone.cancel_requested_at.map(|v| v as i64)),
            cancel_reason: Set(rec_clone.cancel_reason.clone()),
            cancel_confirmed_at: Set(rec_clone.cancel_confirmed_at.map(|v| v as i64)),
            cancel_outcome: Set(rec_clone.cancel_outcome.clone()),
            started_at: Set(rec_clone.started_at as i64),
            ended_at: Set(rec_clone.ended_at.map(|v| v as i64)),
            updated_at: Set(rec_clone.updated_at as i64),
            audit: Set(None),
        };
        ExecutionEntity::insert(am)
            .on_conflict(
                OnConflict::column(ExecutionColumn::ExecutionId)
                    .do_nothing()
                    .to_owned(),
            )
            .exec(&db)
            .await?;
        Ok::<(), anyhow::Error>(())
    })?;
    Ok(rec)
}

pub fn get_execution(execution_id: &str) -> Result<Option<ExecutionRecord>> {
    let path = db_path();
    let id = execution_id.to_string();
    run_async(async move {
        let db = connect(&path).await?;
        let row = ExecutionEntity::find_by_id(id).one(&db).await?;
        Ok(row.map(ExecutionRecord::from_model))
    })
}

pub fn list_executions(limit: u64) -> Result<Vec<ExecutionRecord>> {
    let path = db_path();
    run_async(async move {
        let db = connect(&path).await?;
        let rows = ExecutionEntity::find()
            .order_by_desc(ExecutionColumn::StartedAt)
            .limit(limit)
            .all(&db)
            .await?;
        Ok(rows.into_iter().map(ExecutionRecord::from_model).collect())
    })
}

pub fn list_cancellable_for_issue(repo: &str, issue_number: i64) -> Result<Vec<ExecutionRecord>> {
    let path = db_path();
    let repo = repo.to_string();
    run_async(async move {
        let db = connect(&path).await?;
        let rows = ExecutionEntity::find()
            .filter(ExecutionColumn::Repo.eq(repo))
            .filter(ExecutionColumn::IssueNumber.eq(issue_number))
            .filter(ExecutionColumn::Status.is_in(["queued", "running", "cancelling"]))
            .order_by_desc(ExecutionColumn::StartedAt)
            .all(&db)
            .await?;
        Ok(rows.into_iter().map(ExecutionRecord::from_model).collect())
    })
}

pub fn list_cancellable_for_repo(repo: &str) -> Result<Vec<ExecutionRecord>> {
    let path = db_path();
    let repo = repo.to_string();
    run_async(async move {
        let db = connect(&path).await?;
        let rows = ExecutionEntity::find()
            .filter(ExecutionColumn::Repo.eq(repo))
            .filter(ExecutionColumn::Status.is_in(["queued", "running", "cancelling"]))
            .order_by_desc(ExecutionColumn::StartedAt)
            .all(&db)
            .await?;
        Ok(rows.into_iter().map(ExecutionRecord::from_model).collect())
    })
}

fn transition_allowed(from: ExecutionStatus, to: ExecutionStatus) -> bool {
    matches!(
        (from, to),
        (ExecutionStatus::Queued, ExecutionStatus::Running)
            | (ExecutionStatus::Queued, ExecutionStatus::Cancelled)
            | (ExecutionStatus::Queued, ExecutionStatus::Failed)
            | (ExecutionStatus::Running, ExecutionStatus::Cancelling)
            | (ExecutionStatus::Running, ExecutionStatus::Completed)
            | (ExecutionStatus::Running, ExecutionStatus::Failed)
            | (ExecutionStatus::Running, ExecutionStatus::Cancelled)
            | (ExecutionStatus::Cancelling, ExecutionStatus::Cancelled)
            | (ExecutionStatus::Cancelling, ExecutionStatus::Failed)
    )
}

pub fn transition_execution(
    execution_id: &str,
    expected: Option<ExecutionStatus>,
    next: ExecutionStatus,
    reason: Option<&str>,
    thread_id: Option<&str>,
) -> Result<ExecutionRecord> {
    let path = db_path();
    let id = execution_id.to_string();
    let reason = reason.map(|s| s.to_string());
    let thread_id = thread_id.map(|s| s.to_string());
    run_async(async move {
        let db = connect(&path).await?;
        let row = ExecutionEntity::find_by_id(id.clone())
            .one(&db)
            .await?
            .ok_or_else(|| anyhow!("execution not found: {id}"))?;
        let current = ExecutionStatus::parse_status(&row.status);
        if let Some(exp) = expected {
            if current != exp {
                return Ok(ExecutionRecord::from_model(row));
            }
        }
        if current.is_terminal() {
            return Ok(ExecutionRecord::from_model(row));
        }
        if !transition_allowed(current, next) {
            bail!("invalid transition {current:?} -> {next:?} for {id}");
        }
        let now = state::now() as i64;
        db.execute_raw(sea_orm::Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "UPDATE executions SET status=?, updated_at=?, reason=CASE WHEN ? IS NULL THEN reason ELSE ? END, thread_id=COALESCE(?, thread_id), ended_at=CASE WHEN ? THEN ? ELSE ended_at END WHERE execution_id=? AND status=?",
            [
                next.as_str().into(),
                now.into(),
                reason.clone().into(),
                reason.into(),
                thread_id.into(),
                next.is_terminal().into(),
                now.into(),
                id.clone().into(),
                current.as_str().into(),
            ],
        ))
        .await?;
        let updated = ExecutionEntity::find_by_id(id.clone())
            .one(&db)
            .await?
            .ok_or_else(|| anyhow!("execution disappeared during transition"))?;
        Ok(ExecutionRecord::from_model(updated))
    })
}

pub fn request_cancel(
    execution_id: &str,
    requested_by: &str,
    force: bool,
    reason: Option<&str>,
) -> Result<CancelResult> {
    let path = db_path();
    let id = execution_id.to_string();
    let requested_by = requested_by.to_string();
    let reason = reason.map(|s| s.to_string());
    run_async(async move {
        let db = connect(&path).await?;
        let row = ExecutionEntity::find_by_id(id.clone())
            .one(&db)
            .await?
            .ok_or_else(|| anyhow!("execution not found: {id}"))?;
        let current = ExecutionStatus::parse_status(&row.status);
        if current.is_terminal() {
            let confirmed = current == ExecutionStatus::Cancelled;
            return Ok(CancelResult {
                execution_id: id,
                status: current,
                requested: row.cancel_requested != 0,
                confirmed,
                message: if confirmed {
                    "execution already cancelled".to_string()
                } else {
                    format!("execution already {}", current.as_str())
                },
            });
        }
        let now = state::now() as i64;
        let outcome = if force {
            "force_requested"
        } else {
            "cancel_requested"
        };
        db.execute_raw(sea_orm::Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "UPDATE executions SET status=CASE WHEN status='queued' THEN 'cancelled' WHEN status='running' THEN 'cancelling' ELSE status END, cancel_requested=1, cancel_requested_by=COALESCE(cancel_requested_by, ?), cancel_requested_at=COALESCE(cancel_requested_at, ?), cancel_reason=COALESCE(?, cancel_reason), cancel_confirmed_at=CASE WHEN status='queued' THEN ? ELSE cancel_confirmed_at END, cancel_outcome=CASE WHEN status='queued' THEN 'cancelled_before_start' WHEN status='cancelling' AND ?=0 THEN cancel_outcome ELSE ? END, ended_at=CASE WHEN status='queued' THEN ? ELSE ended_at END, updated_at=? WHERE execution_id=? AND status IN ('queued','running','cancelling')",
            [
                requested_by.into(),
                now.into(),
                reason.into(),
                now.into(),
                force.into(),
                outcome.into(),
                now.into(),
                now.into(),
                id.clone().into(),
            ],
        ))
        .await?;
        let updated = ExecutionEntity::find_by_id(id.clone())
            .one(&db)
            .await?
            .ok_or_else(|| anyhow!("execution disappeared during cancellation"))?;
        let rec = ExecutionRecord::from_model(updated);
        let (confirmed, message) = match rec.status {
            ExecutionStatus::Cancelled => (true, "execution cancelled".to_string()),
            ExecutionStatus::Cancelling => (
                false,
                if force {
                    "force cancellation requested; awaiting confirmation".to_string()
                } else {
                    "cancellation requested; awaiting confirmation".to_string()
                },
            ),
            _ => (false, format!("execution is {}", rec.status.as_str())),
        };
        Ok(CancelResult {
            execution_id: rec.execution_id,
            status: rec.status,
            requested: rec.cancel_requested,
            confirmed,
            message,
        })
    })
}

pub fn mark_cancel_confirmed(execution_id: &str, outcome: &str) -> Result<Option<ExecutionRecord>> {
    let path = db_path();
    let id = execution_id.to_string();
    let outcome = outcome.to_string();
    run_async(async move {
        let db = connect(&path).await?;
        let row = match ExecutionEntity::find_by_id(id.clone()).one(&db).await? {
            Some(r) => r,
            None => return Ok(None),
        };
        let current = ExecutionStatus::parse_status(&row.status);
        if current == ExecutionStatus::Completed || current == ExecutionStatus::Failed {
            return Ok(Some(ExecutionRecord::from_model(row)));
        }
        let now = state::now() as i64;
        db.execute_raw(sea_orm::Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "UPDATE executions SET status='cancelled', cancel_confirmed_at=?, cancel_outcome=?, updated_at=?, ended_at=? WHERE execution_id=? AND status IN ('queued','running','cancelling')",
            [
                now.into(),
                outcome.into(),
                now.into(),
                now.into(),
                id.clone().into(),
            ],
        ))
        .await?;
        let updated = ExecutionEntity::find_by_id(id)
            .one(&db)
            .await?
            .ok_or_else(|| anyhow!("execution disappeared while confirming cancellation"))?;
        Ok(Some(ExecutionRecord::from_model(updated)))
    })
}

pub fn mark_cancel_unconfirmed(
    execution_id: &str,
    outcome: &str,
) -> Result<Option<ExecutionRecord>> {
    let path = db_path();
    let id = execution_id.to_string();
    let outcome = outcome.to_string();
    run_async(async move {
        let db = connect(&path).await?;
        let row = match ExecutionEntity::find_by_id(id.clone()).one(&db).await? {
            Some(r) => r,
            None => return Ok(None),
        };
        let current = ExecutionStatus::parse_status(&row.status);
        if current.is_terminal() {
            return Ok(Some(ExecutionRecord::from_model(row)));
        }
        let now = state::now() as i64;
        let mut am: ExecutionActiveModel = row.into();
        am.cancel_outcome = Set(Some(outcome));
        am.updated_at = Set(now);
        let updated = am.update(&db).await?;
        Ok(Some(ExecutionRecord::from_model(updated)))
    })
}

pub fn complete_execution(
    execution_id: &str,
    thread_id: Option<&str>,
    pr_url: Option<&str>,
) -> Result<Option<ExecutionRecord>> {
    let path = db_path();
    let id = execution_id.to_string();
    let thread_id = thread_id.map(|s| s.to_string());
    let pr_url = pr_url.map(|s| s.to_string());
    run_async(async move {
        let db = connect(&path).await?;
        let row = match ExecutionEntity::find_by_id(id.clone()).one(&db).await? {
            Some(r) => r,
            None => return Ok(None),
        };
        let current = ExecutionStatus::parse_status(&row.status);
        if current.is_terminal() {
            return Ok(Some(ExecutionRecord::from_model(row)));
        }
        let now = state::now() as i64;
        db.execute_raw(sea_orm::Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "UPDATE executions SET status='completed', thread_id=COALESCE(?, thread_id), pr_url=COALESCE(?, pr_url), updated_at=?, ended_at=? WHERE execution_id=? AND status='running'",
            [
                thread_id.into(),
                pr_url.into(),
                now.into(),
                now.into(),
                id.clone().into(),
            ],
        ))
        .await?;
        let updated = ExecutionEntity::find_by_id(id)
            .one(&db)
            .await?
            .ok_or_else(|| anyhow!("execution disappeared while completing"))?;
        Ok(Some(ExecutionRecord::from_model(updated)))
    })
}

pub fn fail_execution(execution_id: &str, error: &str) -> Result<Option<ExecutionRecord>> {
    let path = db_path();
    let id = execution_id.to_string();
    let error = error.to_string();
    run_async(async move {
        let db = connect(&path).await?;
        let row = match ExecutionEntity::find_by_id(id.clone()).one(&db).await? {
            Some(r) => r,
            None => return Ok(None),
        };
        let current = ExecutionStatus::parse_status(&row.status);
        if current.is_terminal() {
            return Ok(Some(ExecutionRecord::from_model(row)));
        }
        let now = state::now() as i64;
        db.execute_raw(sea_orm::Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "UPDATE executions SET status='failed', reason=?, updated_at=?, ended_at=? WHERE execution_id=? AND status='running'",
            [error.into(), now.into(), now.into(), id.clone().into()],
        ))
        .await?;
        let updated = ExecutionEntity::find_by_id(id.clone())
            .one(&db)
            .await?
            .ok_or_else(|| anyhow!("execution disappeared while failing"))?;
        Ok(Some(ExecutionRecord::from_model(updated)))
    })
}

pub fn is_cancel_requested(execution_id: &str) -> bool {
    get_execution(execution_id)
        .ok()
        .flatten()
        .map(|r| {
            r.cancel_requested
                || r.status == ExecutionStatus::Cancelling
                || r.status == ExecutionStatus::Cancelled
        })
        .unwrap_or(false)
}

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
    run_async(async move {
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
    run_async(async move {
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
                ts: ts.max(0) as u64,
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
        std::env::set_var("ISSUE_FACTORY_DB_FILE", tmp.to_str().unwrap());
        f();
        std::fs::remove_file(&tmp).ok();
        std::env::remove_var("ISSUE_FACTORY_DB_FILE");
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
        std::env::set_var("ISSUE_FACTORY_DB_FILE", db_path.to_str().unwrap());
        assert!(create_execution("org/repo#1", "org/repo", 1, 1, None, None, None).is_err());
        fs::remove_file(parent).unwrap();
        std::env::remove_var("ISSUE_FACTORY_DB_FILE");
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
