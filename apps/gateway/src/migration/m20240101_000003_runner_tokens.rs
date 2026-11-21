use sea_orm_migration::prelude::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        let db = manager.get_connection();
        // Intentional: invalidate plaintext-era runner tokens.
        db.execute_unprepared("DROP TABLE IF EXISTS runners;")
            .await?;
        db.execute_unprepared(
            "CREATE TABLE runners (
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
        db.execute_unprepared(
            "CREATE TABLE runners (
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
}
