use sea_orm_migration::prelude::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        let db = manager.get_connection();
        db.execute_unprepared("ALTER TABLE runners ADD COLUMN callback_url TEXT;")
            .await?;
        db.execute_unprepared("ALTER TABLE runners ADD COLUMN callback_secret TEXT;")
            .await?;
        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        let db = manager.get_connection();
        db.execute_unprepared(
            "CREATE TABLE runners_new (
                id TEXT PRIMARY KEY,
                token_hash TEXT NOT NULL,
                created_at INTEGER NOT NULL,
                rotated_at INTEGER,
                revoked_at INTEGER
            );",
        )
        .await?;
        db.execute_unprepared(
            "INSERT INTO runners_new (id, token_hash, created_at, rotated_at, revoked_at)
             SELECT id, token_hash, created_at, rotated_at, revoked_at FROM runners;",
        )
        .await?;
        db.execute_unprepared("DROP TABLE runners;").await?;
        db.execute_unprepared("ALTER TABLE runners_new RENAME TO runners;")
            .await?;
        Ok(())
    }
}
