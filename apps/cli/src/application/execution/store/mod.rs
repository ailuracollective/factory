use super::*;
use anyhow::Result;
use std::path::PathBuf;

use crate::state;

fn db_path() -> PathBuf {
    state::state_file()
}

fn run_async<F, T>(fut: F) -> T
where
    F: std::future::Future<Output = T> + Send,
    T: Send,
{
    std::thread::scope(|s| {
        let handle = s.spawn(|| {
            let rt = tokio::runtime::Runtime::new().expect("tokio runtime");
            rt.block_on(fut)
        });
        handle.join().expect("worker thread panicked")
    })
}

async fn connect(path: &std::path::Path) -> Result<DatabaseConnection> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let db = Database::connect(format!("sqlite://{}?mode=rwc", path.display())).await?;
    let schema = Schema::new(DbBackend::Sqlite);
    let mut stmt = schema.create_table_from_entity(ExecutionEntity);
    stmt.if_not_exists();
    db.execute(&stmt).await?;
    // Lightweight migrations for existing DBs.
    let _ = db
        .execute_unprepared("ALTER TABLE executions ADD COLUMN cancel_confirmed_at INTEGER")
        .await;
    let _ = db
        .execute_unprepared("ALTER TABLE executions ADD COLUMN cancel_outcome TEXT")
        .await;
    let _ = db
        .execute_unprepared("ALTER TABLE executions ADD COLUMN pr_url TEXT")
        .await;
    Ok(db)
}

mod audit;
mod ops;

pub use audit::*;
pub use ops::*;
