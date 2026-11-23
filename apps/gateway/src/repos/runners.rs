use crate::entities::runner;
use sea_orm::{
    ColumnTrait, ConnectionTrait, DatabaseConnection, EntityTrait, QueryFilter, Statement,
};

pub async fn register(
    conn: &DatabaseConnection,
    runner_id: &str,
    token_hash: &str,
    callback_url: Option<&str>,
    callback_secret: Option<&str>,
    ts: i64,
) -> Result<(), sea_orm::DbErr> {
    conn.execute_raw(Statement::from_sql_and_values(
        sea_orm::DatabaseBackend::Sqlite,
        "INSERT INTO runners (id, token_hash, created_at, rotated_at, revoked_at, callback_url, callback_secret)
         VALUES (?, ?, ?, NULL, NULL, ?, ?)
         ON CONFLICT(id) DO UPDATE SET token_hash=excluded.token_hash, rotated_at=excluded.created_at, revoked_at=NULL, callback_url=excluded.callback_url, callback_secret=excluded.callback_secret",
        [
            runner_id.into(),
            token_hash.into(),
            ts.into(),
            callback_url.map(|s| s.to_string()).into(),
            callback_secret.map(|s| s.to_string()).into(),
        ],
    ))
    .await?;
    Ok(())
}

pub struct CallbackTarget {
    pub runner_id: String,
    pub url: String,
    pub secret: String,
}

pub async fn callback_targets(
    conn: &DatabaseConnection,
) -> Result<Vec<CallbackTarget>, sea_orm::DbErr> {
    #[derive(sea_orm::FromQueryResult)]
    struct Row {
        id: String,
        callback_url: Option<String>,
        callback_secret: Option<String>,
    }
    let rows = runner::Entity::find()
        .filter(runner::Column::RevokedAt.is_null())
        .into_model::<Row>()
        .all(conn)
        .await?;
    Ok(rows
        .into_iter()
        .filter_map(|r| {
            let url = r.callback_url?.trim().to_string();
            let secret = r.callback_secret?.trim().to_string();
            if url.is_empty() || secret.is_empty() {
                None
            } else {
                Some(CallbackTarget {
                    runner_id: r.id,
                    url,
                    secret,
                })
            }
        })
        .collect())
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
