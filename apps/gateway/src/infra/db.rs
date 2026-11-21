use sea_orm::{ConnectionTrait, DatabaseConnection, Statement};
use sea_orm_migration::{MigratorTrait, SchemaManager};
use worker::{D1Database, Env};

use crate::migration::Migrator;

pub async fn d1(_env: &Env) -> worker::Result<D1Database> {
    _env.d1("DB")
}

pub async fn connect(env: &Env) -> Result<DatabaseConnection, sea_orm::DbErr> {
    let d1 = env
        .d1("DB")
        .map_err(|e| sea_orm::DbErr::Conn(sea_orm::RuntimeErr::Internal(e.to_string())))?;
    crate::infra::sea::connect(d1).await
}

pub async fn ensure_schema(conn: &DatabaseConnection) -> Result<(), sea_orm::DbErr> {
    Migrator::install(conn).await?;
    let pending: std::collections::BTreeSet<String> = Migrator::get_pending_migrations(conn)
        .await?
        .into_iter()
        .map(|m| m.name().to_string())
        .collect();
    let manager = SchemaManager::new(conn);
    for migration in Migrator::migrations() {
        if !pending.contains(migration.name()) {
            continue;
        }
        migration.up(&manager).await?;
        let applied_at = super::time::now_ts();
        conn.execute_raw(Statement::from_sql_and_values(
            sea_orm::DatabaseBackend::Sqlite,
            "INSERT INTO seaql_migrations (version, applied_at) VALUES (?, ?)",
            [migration.name().into(), applied_at.into()],
        ))
        .await?;
    }
    Ok(())
}
