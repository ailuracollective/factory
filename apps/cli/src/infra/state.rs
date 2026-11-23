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

pub fn sync_projects(repos: &[crate::config::RepoEntry]) {
    let path = state_file();
    let repos = repos.to_vec();
    let _ = run_async(sync_projects_db(path, repos));
}

pub fn load_projects() -> Vec<crate::config::RepoEntry> {
    let path = state_file();
    run_async(load_projects_db(path)).unwrap_or_default()
}

async fn load_projects_db(path: PathBuf) -> Result<Vec<crate::config::RepoEntry>> {
    let db = connect(&path).await?;
    let rows = ProjectEntity::find().all(&db).await.unwrap_or_default();
    Ok(rows
        .into_iter()
        .map(|r| crate::config::RepoEntry {
            name: r.name,
            path: r.path,
            github: r.github,
            hook_id: r.hook_id.map(|v| v as u64),
            enabled: r.enabled != 0,
        })
        .collect())
}

async fn sync_projects_db(path: PathBuf, repos: Vec<crate::config::RepoEntry>) -> Result<()> {
    let db = connect(&path).await?;
    ProjectEntity::delete_many().exec(&db).await?;
    for r in repos {
        ProjectActiveModel {
            id: Set(Uuid::now_v7().to_string()),
            name: Set(r.name),
            path: Set(r.path),
            github: Set(r.github),
            hook_id: Set(r.hook_id.map(|v| v as i64)),
            enabled: Set(if r.enabled { 1 } else { 0 }),
        }
        .insert(&db)
        .await?;
    }
    Ok(())
}
fn project_name_from_key(key: &str) -> Option<String> {
    let before = key.split('#').next()?;
    let name = before.rsplit('/').next()?;
    if name.is_empty() {
        None
    } else {
        Some(name.to_string())
    }
}

async fn find_project_id(db: &DatabaseConnection, name: &str) -> Option<Uuid> {
    let row = ProjectEntity::find()
        .filter(ProjectColumn::Name.eq(name))
        .one(db)
        .await
        .ok()??;
    Uuid::parse_str(&row.id).ok()
}

async fn load_state_db(path: PathBuf) -> Result<HashMap<String, IssueState>> {
    let db = connect(&path).await?;
    let models = IssueEntity::find().all(&db).await?;

    let mut issues = HashMap::new();
    for m in models {
        issues.insert(
            m.issue_key,
            IssueState {
                status: IssueStatus::from_str(&m.status),
                attempts: m.attempts as u32,
                last_error: m.last_error,
                thread_id: m.thread_id,
                pr_url: m.pr_url,
                updated_at: m.updated_at as u64,
            },
        );
    }

    let mut changed = false;
    for rec in issues.values_mut() {
        if rec.status == IssueStatus::Running {
            rec.status = IssueStatus::Failed;
            if rec.last_error.is_none() {
                rec.last_error = Some("interrupted by restart".to_string());
            }
            rec.updated_at = now();
            changed = true;
        }
    }

    if changed {
        // persist via save_state_db
        let _ = save_state_db(path.clone(), issues.clone()).await;
    }

    Ok(issues)
}

async fn save_state_db(path: PathBuf, issues: HashMap<String, IssueState>) -> Result<()> {
    let db = connect(&path).await?;
    IssueEntity::delete_many().exec(&db).await?;

    for (key, rec) in issues {
        let project_id = if let Some(name) = project_name_from_key(&key) {
            find_project_id(&db, &name).await
        } else {
            None
        };
        IssueActiveModel {
            id: Set(Uuid::now_v7()),
            project_id: Set(project_id),
            issue_key: Set(key),
            status: Set(rec.status.as_str().to_string()),
            attempts: Set(rec.attempts as i32),
            last_error: Set(rec.last_error),
            thread_id: Set(rec.thread_id),
            pr_url: Set(rec.pr_url),
            updated_at: Set(rec.updated_at as i64),
        }
        .insert(&db)
        .await?;
    }

    Ok(())
}

pub fn load_state() -> HashMap<String, IssueState> {
    let path = state_file();

    if path.exists()
        && let Ok(issues) = run_async(load_state_db(path.clone()))
    {
        return issues;
    }

    let issues = HashMap::new();
    let _ = save_state(&issues);
    issues
}

pub fn save_state(issues: &HashMap<String, IssueState>) -> Result<()> {
    let path = state_file();
    let issues = issues.clone();
    run_async(save_state_db(path, issues))
}

pub fn issue_state(key: &str) -> Option<IssueState> {
    load_state().remove(key)
}

pub fn update_issue_state<F: FnOnce(&mut IssueState)>(key: &str, f: F) -> Result<()> {
    let mut issues = load_state();
    let rec = issues.entry(key.to_string()).or_default();
    f(rec);
    rec.updated_at = now();
    save_state(&issues)
}

async fn mention_cursor_db(path: PathBuf, repo: String) -> Result<Option<String>> {
    let db = connect(&path).await?;
    let row = MentionCursorEntity::find_by_id(repo).one(&db).await?;
    Ok(row.map(|r| r.cursor))
}

pub fn mention_cursor(repo: &str) -> Option<String> {
    let path = state_file();
    run_async(mention_cursor_db(path, repo.to_string()))
        .ok()
        .flatten()
}

async fn set_mention_cursor_db(path: PathBuf, repo: String, cursor: String) -> Result<()> {
    let db = connect(&path).await?;
    let ts = now() as i64;
    let am = MentionCursorActiveModel {
        repo: Set(repo),
        cursor: Set(cursor),
        updated_at: Set(ts),
    };
    MentionCursorEntity::insert(am)
        .on_conflict(
            OnConflict::column(MentionCursorColumn::Repo)
                .update_columns([MentionCursorColumn::Cursor, MentionCursorColumn::UpdatedAt])
                .to_owned(),
        )
        .exec(&db)
        .await?;
    Ok(())
}

pub fn set_mention_cursor(repo: &str, cursor: &str) -> Result<()> {
    let path = state_file();
    run_async(set_mention_cursor_db(
        path,
        repo.to_string(),
        cursor.to_string(),
    ))
}

async fn mention_event_seen_db(path: PathBuf, key: String) -> Result<bool> {
    let db = connect(&path).await?;
    Ok(MentionEventEntity::find_by_id(key)
        .one(&db)
        .await?
        .is_some())
}

pub fn mention_event_seen(key: &str) -> bool {
    let path = state_file();
    run_async(mention_event_seen_db(path, key.to_string())).unwrap_or(false)
}

async fn record_mention_event_db(
    path: PathBuf,
    key: String,
    status: String,
    detail: String,
) -> Result<()> {
    let db = connect(&path).await?;
    let ts = now() as i64;
    let am = MentionEventActiveModel {
        event_key: Set(key),
        status: Set(status),
        detail: Set(Some(detail)),
        created_at: Set(ts),
        updated_at: Set(ts),
    };
    MentionEventEntity::insert(am)
        .on_conflict(
            OnConflict::column(MentionEventColumn::EventKey)
                .update_columns([
                    MentionEventColumn::Status,
                    MentionEventColumn::Detail,
                    MentionEventColumn::UpdatedAt,
                ])
                .to_owned(),
        )
        .exec(&db)
        .await?;
    Ok(())
}

pub fn record_mention_event(key: &str, status: &str, detail: &str) -> Result<()> {
    let path = state_file();
    run_async(record_mention_event_db(
        path,
        key.to_string(),
        status.to_string(),
        detail.to_string(),
    ))
}

async fn count_mention_launches_db(path: PathBuf, since: i64) -> Result<u64> {
    let db = connect(&path).await?;
    MentionEventEntity::find()
        .filter(MentionEventColumn::Status.is_in(["accepted", "completed"]))
        .filter(MentionEventColumn::CreatedAt.gte(since))
        .count(&db)
        .await
        .map_err(Into::into)
}

pub fn count_mention_launches_since(since: u64) -> usize {
    let path = state_file();
    run_async(count_mention_launches_db(path, since as i64)).unwrap_or(0) as usize
}

#[cfg(test)]
pub mod tests {
    use super::*;
    use std::sync::Mutex;
    pub static TEST_ENV_LOCK: Mutex<()> = Mutex::new(());

    #[test]
    fn roundtrip_state() {
        let _lock = TEST_ENV_LOCK.lock().unwrap();
        let mut m = HashMap::new();
        m.insert(
            "repo#1".to_string(),
            IssueState {
                status: IssueStatus::Completed,
                attempts: 2,
                thread_id: Some("t-1".to_string()),
                ..Default::default()
            },
        );
        let tmp = std::env::temp_dir().join("factory_state_test.db");
        // FIXME: Audit that the environment access only happens in single-threaded code.
        unsafe { std::env::set_var("ISSUE_FACTORY_DB_FILE", tmp.to_str().unwrap()) };
        let _ = fs::remove_file(&tmp);
        save_state(&m).unwrap();
        let loaded = load_state();
        assert_eq!(loaded.get("repo#1").unwrap().status, IssueStatus::Completed);
        assert_eq!(
            loaded.get("repo#1").unwrap().thread_id.as_deref(),
            Some("t-1")
        );
        std::fs::remove_file(&tmp).ok();
        // FIXME: Audit that the environment access only happens in single-threaded code.
        unsafe { std::env::remove_var("ISSUE_FACTORY_DB_FILE") };
    }

    #[test]
    fn reconcile_running_to_failed() {
        let _lock = TEST_ENV_LOCK.lock().unwrap();
        let tmp = std::env::temp_dir().join("factory_state_running.db");
        // FIXME: Audit that the environment access only happens in single-threaded code.
        unsafe { std::env::set_var("ISSUE_FACTORY_DB_FILE", tmp.to_str().unwrap()) };
        let _ = fs::remove_file(&tmp);
        let mut m = HashMap::new();
        m.insert(
            "r#1".to_string(),
            IssueState {
                status: IssueStatus::Running,
                attempts: 1,
                ..Default::default()
            },
        );
        save_state(&m).unwrap();
        let loaded = load_state();
        assert_eq!(loaded.get("r#1").unwrap().status, IssueStatus::Failed);
        std::fs::remove_file(&tmp).ok();
        // FIXME: Audit that the environment access only happens in single-threaded code.
        unsafe { std::env::remove_var("ISSUE_FACTORY_DB_FILE") };
    }
}
