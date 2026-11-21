use sea_orm_migration::prelude::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        let db = manager.get_connection();
        db.execute_unprepared(
            "CREATE TABLE IF NOT EXISTS policies (
                repo TEXT PRIMARY KEY,
                enabled INTEGER NOT NULL DEFAULT 1,
                allowed_users TEXT NOT NULL DEFAULT '[]',
                max_concurrent INTEGER NOT NULL DEFAULT 2,
                daily_quota INTEGER NOT NULL DEFAULT 20,
                updated_at INTEGER NOT NULL
            );",
        )
        .await?;
        db.execute_unprepared(
            "CREATE TABLE IF NOT EXISTS policy_events (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                ts INTEGER NOT NULL,
                repo TEXT NOT NULL,
                actor TEXT,
                decision TEXT NOT NULL,
                reason TEXT
            );",
        )
        .await?;
        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        let db = manager.get_connection();
        db.execute_unprepared("DROP TABLE IF EXISTS policy_events;")
            .await?;
        db.execute_unprepared("DROP TABLE IF EXISTS policies;")
            .await?;
        Ok(())
    }
}
