use crate::entities::policy;
use sea_orm::{ConnectionTrait, DatabaseConnection, EntityTrait, Statement};

pub struct Policy {
    pub enabled: i64,
    pub allowed_users: String,
    pub max_concurrent: i64,
    pub daily_quota: i64,
}

pub async fn get_or_create(
    conn: &DatabaseConnection,
    repo: &str,
    ts: i64,
) -> Result<Policy, sea_orm::DbErr> {
    let row = policy::Entity::find_by_id(repo.to_string())
        .one(conn)
        .await?;
    if let Some(r) = row {
        return Ok(Policy {
            enabled: r.enabled,
            allowed_users: r.allowed_users,
            max_concurrent: r.max_concurrent,
            daily_quota: r.daily_quota,
        });
    }
    conn.execute_raw(Statement::from_sql_and_values(
        sea_orm::DatabaseBackend::Sqlite,
        "INSERT INTO policies (repo, enabled, allowed_users, max_concurrent, daily_quota, updated_at) VALUES (?,1,'[]',2,20,?)",
        [repo.into(), ts.into()],
    ))
    .await?;
    Ok(Policy {
        enabled: 1,
        allowed_users: "[]".to_string(),
        max_concurrent: 2,
        daily_quota: 20,
    })
}
