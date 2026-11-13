use anyhow::Result;
use sea_orm::{ActiveModelTrait, ConnectionTrait, Database, DatabaseConnection, DbBackend, EntityTrait, Set, Statement};
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
        pub updated_at: i64,
    }

    #[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
    pub enum Relation {}

    impl ActiveModelBehavior for ActiveModel {}
}

use issue_store::{ActiveModel as IssueActiveModel, Entity as IssueEntity};

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
    pub updated_at: u64,
}

impl Default for IssueState {
    fn default() -> Self {
        Self {
            status: IssueStatus::Pending,
            attempts: 0,
            last_error: None,
            thread_id: None,
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
    if let Ok(p) = std::env::var("ISSUE_FACTORY_DB_FILE") {
        if !p.trim().is_empty() {
            return PathBuf::from(p);
        }
    }
    if let Ok(p) = std::env::var("ISSUE_FACTORY_STATE_FILE") {
        if !p.trim().is_empty() {
            return PathBuf::from(p);
        }
    }
    dirs::config_dir()
        .unwrap_or_else(|| dirs::home_dir().unwrap().join(".config"))
        .join("ailuracollective")
        .join("factory")
        .join("factory.db")
}

fn db_url(path: &PathBuf) -> String {
    format!("sqlite://{}?mode=rwc", path.display())
}

fn ensure_parent(path: &PathBuf) {
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

async fn connect(path: &PathBuf) -> Result<DatabaseConnection> {
    ensure_parent(path);
    let db = Database::connect(db_url(path)).await?;
    // Remove legacy config table if present.
    let _ = db
        .execute_raw(Statement::from_string(
            DbBackend::Sqlite,
            "DROP TABLE IF EXISTS config",
        ))
        .await;
    let sql = "CREATE TABLE IF NOT EXISTS projects (
        id TEXT PRIMARY KEY NOT NULL,
        name TEXT NOT NULL UNIQUE,
        path TEXT NOT NULL,
        github TEXT NOT NULL,
        hook_id INTEGER,
        enabled INTEGER NOT NULL DEFAULT 1
    )";
    db.execute_raw(Statement::from_string(DbBackend::Sqlite, sql))
        .await?;
    let sql = "CREATE TABLE IF NOT EXISTS issues (
        id TEXT PRIMARY KEY NOT NULL,
        project_id TEXT,
        issue_key TEXT NOT NULL UNIQUE,
        status TEXT NOT NULL,
        attempts INTEGER NOT NULL DEFAULT 0,
        last_error TEXT,
        thread_id TEXT,
        updated_at INTEGER NOT NULL
    )";
    db.execute_raw(Statement::from_string(DbBackend::Sqlite, sql))
        .await?;
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
    let rows = db
        .query_all_raw(Statement::from_string(
            DbBackend::Sqlite,
            "SELECT name, path, github, hook_id, enabled FROM projects",
        ))
        .await
        .unwrap_or_default();
    let mut repos = Vec::new();
    for row in rows {
        let name: String = row.try_get("", "name").unwrap_or_default();
        let path: String = row.try_get("", "path").unwrap_or_default();
        let github: String = row.try_get("", "github").unwrap_or_default();
        let hook_id: Option<i64> = row.try_get("", "hook_id").ok();
        let enabled: i64 = row.try_get("", "enabled").unwrap_or(1);
        repos.push(crate::config::RepoEntry {
            name,
            path,
            github,
            hook_id: hook_id.map(|v| v as u64),
            enabled: enabled != 0,
        });
    }
    Ok(repos)
}

async fn sync_projects_db(path: PathBuf, repos: Vec<crate::config::RepoEntry>) -> Result<()> {
    let db = connect(&path).await?;
    // clear
    db.execute_raw(Statement::from_string(
        DbBackend::Sqlite,
        "DELETE FROM projects",
    ))
    .await?;
    for r in repos {
        let id = Uuid::now_v7();
        let stmt = Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "INSERT INTO projects (id, name, path, github, hook_id, enabled) VALUES (?, ?, ?, ?, ?, ?)",
            [
                id.to_string().into(),
                r.name.into(),
                r.path.into(),
                r.github.into(),
                r.hook_id.map(|v| v as i64).into(),
                (if r.enabled { 1i64 } else { 0i64 }).into(),
            ],
        );
        db.execute_raw(stmt).await?;
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

async fn find_project_id(path: &PathBuf, name: &str) -> Option<Uuid> {
    // projects table lives in same db
    let db = Database::connect(db_url(path)).await.ok()?;
    // raw query
    let stmt = Statement::from_sql_and_values(
        DbBackend::Sqlite,
        "SELECT id FROM projects WHERE name = ?",
        [name.into()],
    );
    let row = db.query_one_raw(stmt).await.ok()??;
    let id_str: String = row.try_get("", "id").ok()?;
    Uuid::parse_str(&id_str).ok()
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
            find_project_id(&path, &name).await
        } else {
            None
        };
        let am = IssueActiveModel {
            id: Set(Uuid::now_v7()),
            project_id: Set(project_id),
            issue_key: Set(key),
            status: Set(rec.status.as_str().to_string()),
            attempts: Set(rec.attempts as i32),
            last_error: Set(rec.last_error),
            thread_id: Set(rec.thread_id),
            updated_at: Set(rec.updated_at as i64),
        };
        am.insert(&db).await?;
    }

    Ok(())
}

pub fn load_state() -> HashMap<String, IssueState> {
    let path = state_file();

    if path.exists() {
        if let Ok(issues) = run_async(load_state_db(path.clone())) {
            return issues;
        }
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
        std::env::set_var("ISSUE_FACTORY_DB_FILE", tmp.to_str().unwrap());
        let _ = fs::remove_file(&tmp);
        save_state(&m).unwrap();
        let loaded = load_state();
        assert_eq!(loaded.get("repo#1").unwrap().status, IssueStatus::Completed);
        assert_eq!(
            loaded.get("repo#1").unwrap().thread_id.as_deref(),
            Some("t-1")
        );
        std::fs::remove_file(&tmp).ok();
        std::env::remove_var("ISSUE_FACTORY_DB_FILE");
    }

    #[test]
    fn reconcile_running_to_failed() {
        let _lock = TEST_ENV_LOCK.lock().unwrap();
        let tmp = std::env::temp_dir().join("factory_state_running.db");
        std::env::set_var("ISSUE_FACTORY_DB_FILE", tmp.to_str().unwrap());
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
        std::env::remove_var("ISSUE_FACTORY_DB_FILE");
    }
}
