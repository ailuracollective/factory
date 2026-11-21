use crate::entities::runner;
use sea_orm::{ConnectionTrait, DatabaseConnection, EntityTrait, Statement};

pub async fn register(
    conn: &DatabaseConnection,
    runner_id: &str,
    token_hash: &str,
    ts: i64,
) -> Result<(), sea_orm::DbErr> {
    conn.execute_raw(Statement::from_sql_and_values(
        sea_orm::DatabaseBackend::Sqlite,
        "INSERT INTO runners (id, token_hash, created_at, rotated_at, revoked_at) VALUES (?, ?, ?, NULL, NULL)
         ON CONFLICT(id) DO UPDATE SET token_hash=excluded.token_hash, rotated_at=excluded.created_at, revoked_at=NULL",
        [runner_id.into(), token_hash.into(), ts.into()],
    ))
    .await?;
    Ok(())
}

pub async fn token_hash(
    conn: &DatabaseConnection,
    runner_id: &str,
) -> Result<Option<String>, sea_orm::DbErr> {
    let model = runner::Entity::find_by_id(runner_id.to_string())
        .one(conn)
        .await?;
    Ok(model.and_then(|r| {
        if r.revoked_at.is_none() {
            Some(r.token_hash)
        } else {
            None
        }
    }))
}

pub async fn revoke(conn: &DatabaseConnection, id: &str, ts: i64) -> Result<u64, sea_orm::DbErr> {
    let res = conn
        .execute_raw(Statement::from_sql_and_values(
            sea_orm::DatabaseBackend::Sqlite,
            "UPDATE runners SET revoked_at=? WHERE id=?",
            [ts.into(), id.into()],
        ))
        .await?;
    Ok(res.rows_affected())
}

pub async fn rotate(
    conn: &DatabaseConnection,
    id: &str,
    token_hash: &str,
    ts: i64,
) -> Result<u64, sea_orm::DbErr> {
    let res = conn
        .execute_raw(Statement::from_sql_and_values(
            sea_orm::DatabaseBackend::Sqlite,
            "UPDATE runners SET token_hash=?, rotated_at=?, revoked_at=NULL WHERE id=?",
            [token_hash.into(), ts.into(), id.into()],
        ))
        .await?;
    Ok(res.rows_affected())
}
