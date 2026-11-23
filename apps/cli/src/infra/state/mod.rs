use anyhow::Result;
use sea_orm::{
    ActiveModelTrait, ColumnTrait, ConnectionTrait, Database, DatabaseConnection, DbBackend,
    EntityTrait, PaginatorTrait, QueryFilter, Schema, Set, sea_query::OnConflict,
};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};
use uuid::Uuid;

mod issue_store {
    use sea_orm::entity::prelude::*;
    use uuid::Uuid;

    #[derive(Clone, Debug, PartialEq, Eq, DeriveEntityModel)]
    #[sea_orm(table_name = "issues")]
    pub struct Model {
        #[sea_orm(primary_key, auto_increment = false)]
        pub id: Uuid,
        pub project_id: Option<Uuid>,
        #[sea_orm(unique)]
        pub issue_key: String,
        pub status: String,
        pub attempts: i32,
        pub last_error: Option<String>,
        pub thread_id: Option<String>,
        pub pr_url: Option<String>,
        pub updated_at: i64,
    }

    #[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
    pub enum Relation {}

    impl ActiveModelBehavior for ActiveModel {}
}

mod project_store {
    use sea_orm::entity::prelude::*;

    #[derive(Clone, Debug, PartialEq, Eq, DeriveEntityModel)]
    #[sea_orm(table_name = "projects")]
    pub struct Model {
        #[sea_orm(primary_key, auto_increment = false)]
        pub id: String,
        #[sea_orm(unique)]
        pub name: String,
        pub path: String,
        pub github: String,
        pub hook_id: Option<i64>,
        pub enabled: i64,
    }

    #[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
    pub enum Relation {}

    impl ActiveModelBehavior for ActiveModel {}
}

mod mention_event_store {
    use sea_orm::entity::prelude::*;

    #[derive(Clone, Debug, PartialEq, Eq, DeriveEntityModel)]
    #[sea_orm(table_name = "mention_events")]
    pub struct Model {
        #[sea_orm(primary_key, auto_increment = false)]
        pub event_key: String,
        pub status: String,
        pub detail: Option<String>,
        pub created_at: i64,
        pub updated_at: i64,
    }

    #[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
    pub enum Relation {}

    impl ActiveModelBehavior for ActiveModel {}
}

mod mention_cursor_store {
    use sea_orm::entity::prelude::*;

    #[derive(Clone, Debug, PartialEq, Eq, DeriveEntityModel)]
    #[sea_orm(table_name = "mention_cursors")]
    pub struct Model {
        #[sea_orm(primary_key, auto_increment = false)]
        pub repo: String,
        pub cursor: String,
        pub updated_at: i64,
    }

    #[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
    pub enum Relation {}

    impl ActiveModelBehavior for ActiveModel {}
}

use issue_store::{ActiveModel as IssueActiveModel, Entity as IssueEntity};
use mention_cursor_store::{
    ActiveModel as MentionCursorActiveModel, Column as MentionCursorColumn,
    Entity as MentionCursorEntity,
};
use mention_event_store::{
    ActiveModel as MentionEventActiveModel, Column as MentionEventColumn,
    Entity as MentionEventEntity,
};
use project_store::{
    ActiveModel as ProjectActiveModel, Column as ProjectColumn, Entity as ProjectEntity,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum IssueStatus {
    #[default]
    Pending,
    Running,
    Failed,
    Completed,
}

impl IssueStatus {
    fn as_str(self) -> &'static str {
        match self {
            IssueStatus::Pending => "pending",
            IssueStatus::Running => "running",
            IssueStatus::Failed => "failed",
            IssueStatus::Completed => "completed",
        }
    }

    fn from_str(value: &str) -> Self {
        match value {
            "running" => IssueStatus::Running,
            "failed" => IssueStatus::Failed,
            "completed" => IssueStatus::Completed,
            _ => IssueStatus::Pending,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IssueState {
    pub status: IssueStatus,
    #[serde(default)]
    pub attempts: u32,
    #[serde(default)]
    pub last_error: Option<String>,
    #[serde(default)]
    pub thread_id: Option<String>,
    #[serde(default)]
    pub pr_url: Option<String>,
    #[serde(default)]
    pub updated_at: u64,
}

impl Default for IssueState {
    fn default() -> Self {
        Self {
            status: IssueStatus::Pending,
            attempts: 0,
            last_error: None,
            thread_id: None,
            pr_url: None,
            updated_at: now(),
        }
    }
}

pub fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

pub fn state_file() -> PathBuf {
    if let Ok(p) = std::env::var("ISSUE_FACTORY_DB_FILE")
        && !p.trim().is_empty()
    {
        return PathBuf::from(p);
    }
    if let Ok(p) = std::env::var("ISSUE_FACTORY_STATE_FILE")
        && !p.trim().is_empty()
    {
        return PathBuf::from(p);
    }
    dirs::config_dir()
        .unwrap_or_else(|| dirs::home_dir().unwrap().join(".config"))
        .join("ailuracollective")
        .join("factory")
        .join("factory.db")
}

fn db_url(path: &std::path::Path) -> String {
    format!("sqlite://{}?mode=rwc", path.display())
}

fn ensure_parent(path: &std::path::Path) {
    if let Some(parent) = path.parent() {
        let _ = fs::create_dir_all(parent);
    }
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
    ensure_parent(path);
    let db = Database::connect(db_url(path)).await?;
    // Remove legacy config table if present.
    let _ = db.execute_unprepared("DROP TABLE IF EXISTS config").await;
    let schema = Schema::new(DbBackend::Sqlite);
    for stmt in [
        schema.create_table_from_entity(ProjectEntity),
        schema.create_table_from_entity(IssueEntity),
        schema.create_table_from_entity(MentionEventEntity),
        schema.create_table_from_entity(MentionCursorEntity),
    ] {
        let mut stmt = stmt;
        stmt.if_not_exists();
        db.execute(&stmt).await?;
    }
    // Lightweight migration for existing DBs.
    let _ = db
        .execute_unprepared("ALTER TABLE issues ADD COLUMN pr_url TEXT")
        .await;
    Ok(db)
}

mod issues;
mod mentions_store;
mod projects;

pub use issues::{issue_state, load_state, save_state, update_issue_state};
pub use mentions_store::{
    count_mention_launches_since, mention_cursor, mention_event_seen, record_mention_event,
    set_mention_cursor,
};
pub use projects::{load_projects, sync_projects};

#[cfg(test)]
pub mod tests;
