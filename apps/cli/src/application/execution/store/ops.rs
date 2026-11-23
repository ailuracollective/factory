use super::*;
use crate::state;
use anyhow::{Result, bail};
use uuid::Uuid;

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
        if let Some(exp) = expected
            && current != exp
        {
            return Ok(ExecutionRecord::from_model(row));
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
