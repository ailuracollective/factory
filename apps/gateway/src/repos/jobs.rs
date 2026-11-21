use sea_orm::{
    ColumnTrait, ConnectionTrait, DatabaseConnection, EntityTrait, FromQueryResult, PaginatorTrait,
    QueryFilter, QueryOrder, QuerySelect, Statement,
};
use serde_json::Value;

use crate::entities::job;

pub async fn count_concurrent(
    conn: &DatabaseConnection,
    repo: &str,
) -> Result<i64, sea_orm::DbErr> {
    let count = job::Entity::find()
        .filter(job::Column::Repo.eq(repo))
        .filter(job::Column::Status.is_in(["pending", "claimed", "running"]))
        .count(conn)
        .await?;
    Ok(count as i64)
}

pub async fn count_daily(
    conn: &DatabaseConnection,
    repo: &str,
    day_start: i64,
) -> Result<i64, sea_orm::DbErr> {
    let count = job::Entity::find()
        .filter(job::Column::Repo.eq(repo))
        .filter(job::Column::CreatedAt.gte(day_start))
        .count(conn)
        .await?;
    Ok(count as i64)
}

pub async fn find_pending(conn: &DatabaseConnection) -> Result<Option<String>, sea_orm::DbErr> {
    #[derive(FromQueryResult)]
    struct IdRow {
        id: String,
    }
    let row = job::Entity::find()
        .filter(job::Column::Status.eq("pending"))
        .order_by_asc(job::Column::CreatedAt)
        .into_model::<IdRow>()
        .one(conn)
        .await?;
    Ok(row.map(|r| r.id))
}

pub async fn claim(
    conn: &DatabaseConnection,
    jid: &str,
    runner_id: &str,
    lease: i64,
    ts: i64,
) -> Result<u64, sea_orm::DbErr> {
    let res = conn
        .execute_raw(Statement::from_sql_and_values(
            sea_orm::DatabaseBackend::Sqlite,
            "UPDATE jobs SET status='claimed', runner_id=?, lease_until=?, attempts=attempts+1, updated_at=? WHERE id=? AND status='pending'",
            [runner_id.into(), lease.into(), ts.into(), jid.into()],
        ))
        .await?;
    Ok(res.rows_affected())
}

pub async fn get(conn: &DatabaseConnection, id: &str) -> Result<Option<Value>, sea_orm::DbErr> {
    let model = job::Entity::find_by_id(id.to_string()).one(conn).await?;
    Ok(model.map(model_to_json))
}

pub async fn create(
    conn: &DatabaseConnection,
    id: &str,
    repo: &str,
    number: i64,
    title: Option<&str>,
    url: Option<&str>,
    ts: i64,
) -> Result<(), sea_orm::DbErr> {
    let res = conn
        .execute_raw(Statement::from_sql_and_values(
            sea_orm::DatabaseBackend::Sqlite,
            "INSERT OR IGNORE INTO jobs (id, repo, issue_number, title, url, status, attempts, created_at, updated_at) VALUES (?, ?, ?, ?, ?, 'pending', 0, ?, ?)",
            [
                id.into(),
                repo.into(),
                number.into(),
                title.map(|s| s.to_string()).into(),
                url.map(|s| s.to_string()).into(),
                ts.into(),
                ts.into(),
            ],
        ))
        .await?;
    let _ = res;
    Ok(())
}

pub async fn heartbeat(
    conn: &DatabaseConnection,
    id: &str,
    runner_id: &str,
    status: &str,
    lease: i64,
    ts: i64,
) -> Result<u64, sea_orm::DbErr> {
    let res = conn
        .execute_raw(Statement::from_sql_and_values(
            sea_orm::DatabaseBackend::Sqlite,
            "UPDATE jobs SET status=?, lease_until=?, updated_at=? WHERE id=? AND runner_id=?",
            [
                status.into(),
                lease.into(),
                ts.into(),
                id.into(),
                runner_id.into(),
            ],
        ))
        .await?;
    Ok(res.rows_affected())
}

pub async fn complete(
    conn: &DatabaseConnection,
    id: &str,
    runner_id: &str,
    thread_id: Option<&str>,
    ts: i64,
) -> Result<u64, sea_orm::DbErr> {
    let res = conn
        .execute_raw(Statement::from_sql_and_values(
            sea_orm::DatabaseBackend::Sqlite,
            "UPDATE jobs SET status='completed', thread_id=?, updated_at=?, last_error=NULL WHERE id=? AND runner_id=?",
            [
                thread_id.map(|s| s.to_string()).into(),
                ts.into(),
                id.into(),
                runner_id.into(),
            ],
        ))
        .await?;
    Ok(res.rows_affected())
}

pub async fn fail(
    conn: &DatabaseConnection,
    id: &str,
    runner_id: &str,
    error: &str,
    ts: i64,
) -> Result<u64, sea_orm::DbErr> {
    let res = conn
        .execute_raw(Statement::from_sql_and_values(
            sea_orm::DatabaseBackend::Sqlite,
            "UPDATE jobs SET status='failed', last_error=?, updated_at=? WHERE id=? AND runner_id=?",
            [error.into(), ts.into(), id.into(), runner_id.into()],
        ))
        .await?;
    Ok(res.rows_affected())
}

pub async fn cancel(conn: &DatabaseConnection, id: &str, ts: i64) -> Result<u64, sea_orm::DbErr> {
    let res = conn
        .execute_raw(Statement::from_sql_and_values(
            sea_orm::DatabaseBackend::Sqlite,
            "UPDATE jobs SET status='cancelled', updated_at=? WHERE id=?",
            [ts.into(), id.into()],
        ))
        .await?;
    Ok(res.rows_affected())
}

pub async fn list(conn: &DatabaseConnection) -> Result<Vec<Value>, sea_orm::DbErr> {
    let rows = job::Entity::find()
        .order_by_desc(job::Column::CreatedAt)
        .limit(100)
        .all(conn)
        .await?;
    Ok(rows.into_iter().map(model_to_json).collect())
}

fn model_to_json(m: job::Model) -> Value {
    serde_json::json!({
        "id": m.id,
        "repo": m.repo,
        "issue_number": m.issue_number,
        "title": m.title,
        "url": m.url,
        "status": m.status,
        "runner_id": m.runner_id,
        "lease_until": m.lease_until,
        "attempts": m.attempts,
        "last_error": m.last_error,
        "thread_id": m.thread_id,
        "created_at": m.created_at,
        "updated_at": m.updated_at,
    })
}
