use sea_orm_migration::prelude::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        let db = manager.get_connection();
        db.execute_unprepared(
            "CREATE TABLE IF NOT EXISTS jobs (
                id TEXT PRIMARY KEY,
                repo TEXT NOT NULL,
                issue_number INTEGER NOT NULL,
                title TEXT,
                url TEXT,
                status TEXT NOT NULL,
                runner_id TEXT,
                lease_until INTEGER,
                attempts INTEGER NOT NULL DEFAULT 0,
                last_error TEXT,
                thread_id TEXT,
                created_at INTEGER NOT NULL,
                updated_at INTEGER NOT NULL,
                UNIQUE(repo, issue_number)
            );",
        )
        .await?;
        db.execute_unprepared(
            "CREATE TABLE IF NOT EXISTS audit (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                ts INTEGER NOT NULL,
                event TEXT NOT NULL,
                detail TEXT
            );",
        )
        .await?;
        db.execute_unprepared(
            "CREATE TABLE IF NOT EXISTS runners (
                id TEXT PRIMARY KEY,
                token_hash TEXT NOT NULL,
                created_at INTEGER NOT NULL,
                rotated_at INTEGER,
                revoked_at INTEGER
            );",
        )
        .await?;
        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        let db = manager.get_connection();
        db.execute_unprepared("DROP TABLE IF EXISTS runners;")
            .await?;
        db.execute_unprepared("DROP TABLE IF EXISTS audit;").await?;
        db.execute_unprepared("DROP TABLE IF EXISTS jobs;").await?;
        Ok(())
    }
}
