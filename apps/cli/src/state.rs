use anyhow::Result;
use sea_orm::{
    ActiveModelTrait, ConnectionTrait, Database, DatabaseConnection, DbBackend, EntityTrait, Set,
    Statement,
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
    let sql = "CREATE TABLE IF NOT EXISTS mention_events (
        event_key TEXT PRIMARY KEY NOT NULL,
        status TEXT NOT NULL,
        detail TEXT,
        created_at INTEGER NOT NULL,
        updated_at INTEGER NOT NULL
    )";
    db.execute_raw(Statement::from_string(DbBackend::Sqlite, sql))
        .await?;
    let sql = "CREATE TABLE IF NOT EXISTS mention_cursors (
        repo TEXT PRIMARY KEY NOT NULL,
        cursor TEXT NOT NULL,
        updated_at INTEGER NOT NULL
    )";
    db.execute_raw(Statement::from_string(DbBackend::Sqlite, sql))
        .await?;
    let sql = "CREATE TABLE IF NOT EXISTS executions (
        id TEXT PRIMARY KEY NOT NULL,
        event_key TEXT NOT NULL UNIQUE,
        repo TEXT NOT NULL,
        target TEXT NOT NULL,
        context TEXT NOT NULL,
        command TEXT NOT NULL,
        actor TEXT NOT NULL,
        role TEXT NOT NULL,
        agent TEXT NOT NULL,
        state TEXT NOT NULL,
        detail TEXT,
        thread_id TEXT,
        created_at INTEGER NOT NULL,
        updated_at INTEGER NOT NULL
    )";
    db.execute_raw(Statement::from_string(DbBackend::Sqlite, sql))
        .await?;
    let sql = "CREATE TABLE IF NOT EXISTS execution_transitions (
        row_id INTEGER PRIMARY KEY AUTOINCREMENT,
        execution_id TEXT NOT NULL,
        state TEXT NOT NULL,
        detail TEXT,
        at INTEGER NOT NULL
    )";
    db.execute_raw(Statement::from_string(DbBackend::Sqlite, sql))
        .await?;
    let sql = "CREATE INDEX IF NOT EXISTS idx_executions_target ON executions (repo, target)";
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

async fn mention_cursor_db(path: PathBuf, repo: String) -> Result<Option<String>> {
    let db = connect(&path).await?;
    let stmt = Statement::from_sql_and_values(
        DbBackend::Sqlite,
        "SELECT cursor FROM mention_cursors WHERE repo = ?",
        [repo.into()],
    );
    let row = db.query_one_raw(stmt).await?;
    Ok(row.and_then(|r| r.try_get::<String>("", "cursor").ok()))
}

pub fn mention_cursor(repo: &str) -> Option<String> {
    let path = state_file();
    run_async(mention_cursor_db(path, repo.to_string()))
        .ok()
        .flatten()
}

async fn set_mention_cursor_db(path: PathBuf, repo: String, cursor: String) -> Result<()> {
    let db = connect(&path).await?;
    let stmt = Statement::from_sql_and_values(
        DbBackend::Sqlite,
        "INSERT INTO mention_cursors (repo, cursor, updated_at) VALUES (?, ?, ?)
         ON CONFLICT(repo) DO UPDATE SET cursor = excluded.cursor, updated_at = excluded.updated_at",
        [repo.into(), cursor.into(), (now() as i64).into()],
    );
    db.execute_raw(stmt).await?;
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
    let stmt = Statement::from_sql_and_values(
        DbBackend::Sqlite,
        "SELECT 1 FROM mention_events WHERE event_key = ?",
        [key.into()],
    );
    Ok(db.query_one_raw(stmt).await?.is_some())
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
    let stmt = Statement::from_sql_and_values(
        DbBackend::Sqlite,
        "INSERT INTO mention_events (event_key, status, detail, created_at, updated_at)
         VALUES (?, ?, ?, ?, ?)
         ON CONFLICT(event_key) DO UPDATE SET status = excluded.status, detail = excluded.detail, updated_at = excluded.updated_at",
        [
            key.into(),
            status.into(),
            detail.into(),
            (now() as i64).into(),
            (now() as i64).into(),
        ],
    );
    db.execute_raw(stmt).await?;
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

fn row_to_execution(row: &sea_orm::QueryResult) -> Option<crate::commands::Execution> {
    use crate::commands::{CommandKind, Context, Execution, ExecutionState, Role};
    let state: String = row.try_get("", "state").ok()?;
    let command: String = row.try_get("", "command").ok()?;
    let context: String = row.try_get("", "context").ok()?;
    let role: String = row.try_get("", "role").ok()?;
    Some(Execution {
        id: row.try_get("", "id").ok()?,
        event_key: row.try_get("", "event_key").ok()?,
        repo: row.try_get("", "repo").ok()?,
        target: row.try_get("", "target").ok()?,
        context: Context::from_str(&context)?,
        command: CommandKind::from_str(&command)?,
        actor: row.try_get("", "actor").ok()?,
        role: Role::from_str(&role)?,
        agent: row.try_get("", "agent").ok()?,
        state: ExecutionState::from_str(&state)?,
        detail: row.try_get("", "detail").ok(),
        thread_id: row.try_get("", "thread_id").ok(),
        created_at: row.try_get::<i64>("", "created_at").unwrap_or(0) as u64,
        updated_at: row.try_get::<i64>("", "updated_at").unwrap_or(0) as u64,
    })
}

async fn create_execution_db(path: PathBuf, execution: crate::commands::Execution) -> Result<bool> {
    let db = connect(&path).await?;
    let transition = execution.clone();
    let stmt = Statement::from_sql_and_values(
        DbBackend::Sqlite,
        "INSERT OR IGNORE INTO executions
         (id, event_key, repo, target, context, command, actor, role, agent, state, detail, thread_id, created_at, updated_at)
         VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
        [
            execution.id.clone().into(),
            execution.event_key.into(),
            execution.repo.into(),
            execution.target.into(),
            execution.context.as_str().into(),
            execution.command.name().into(),
            execution.actor.into(),
            execution.role.as_str().into(),
            execution.agent.into(),
            execution.state.as_str().into(),
            execution.detail.clone().into(),
            execution.thread_id.clone().into(),
            (execution.created_at as i64).into(),
            (execution.updated_at as i64).into(),
        ],
    );
    let result = db.execute_raw(stmt).await?;
    if result.rows_affected() == 0 {
        return Ok(false);
    }
    insert_execution_transition(&db, &transition).await?;
    Ok(true)
}

pub fn create_execution(execution: &crate::commands::Execution) -> Result<bool> {
    let path = state_file();
    run_async(create_execution_db(path, execution.clone()))
}

async fn insert_execution_transition(
    db: &DatabaseConnection,
    execution: &crate::commands::Execution,
) -> Result<()> {
    let stmt = Statement::from_sql_and_values(
        DbBackend::Sqlite,
        "INSERT INTO execution_transitions (execution_id, state, detail, at) VALUES (?, ?, ?, ?)",
        [
            execution.id.clone().into(),
            execution.state.as_str().into(),
            execution.detail.clone().into(),
            (execution.updated_at as i64).into(),
        ],
    );
    db.execute_raw(stmt).await?;
    Ok(())
}

async fn execution_get_db(path: PathBuf, id: String) -> Option<crate::commands::Execution> {
    let db = connect(&path).await.ok()?;
    let stmt = Statement::from_sql_and_values(
        DbBackend::Sqlite,
        "SELECT * FROM executions WHERE id = ?",
        [id.into()],
    );
    db.query_one_raw(stmt)
        .await
        .ok()
        .flatten()
        .as_ref()
        .and_then(row_to_execution)
}

pub fn execution_get(id: &str) -> Option<crate::commands::Execution> {
    let path = state_file();
    run_async(execution_get_db(path, id.to_string()))
}

async fn save_execution_db(path: PathBuf, execution: crate::commands::Execution) -> Result<()> {
    let db = connect(&path).await?;
    let stmt = Statement::from_sql_and_values(
        DbBackend::Sqlite,
        "UPDATE executions SET state = ?, detail = ?, thread_id = ?, updated_at = ? WHERE id = ?",
        [
            execution.state.as_str().into(),
            execution.detail.clone().into(),
            execution.thread_id.clone().into(),
            (execution.updated_at as i64).into(),
            execution.id.clone().into(),
        ],
    );
    db.execute_raw(stmt).await?;
    insert_execution_transition(&db, &execution).await?;
    Ok(())
}

pub fn save_execution(execution: &crate::commands::Execution) -> Result<()> {
    let path = state_file();
    run_async(save_execution_db(path, execution.clone()))
}

async fn executions_for_target_db(
    path: PathBuf,
    repo: String,
    target: String,
) -> Vec<crate::commands::Execution> {
    let db = match connect(&path).await {
        Ok(db) => db,
        Err(_) => return Vec::new(),
    };
    let stmt = Statement::from_sql_and_values(
        DbBackend::Sqlite,
        "SELECT * FROM executions WHERE repo = ? AND target = ? ORDER BY created_at DESC LIMIT 50",
        [repo.into(), target.into()],
    );
    db.query_all_raw(stmt)
        .await
        .unwrap_or_default()
        .iter()
        .filter_map(row_to_execution)
        .collect()
}

pub fn executions_for_target(repo: &str, target: &str) -> Vec<crate::commands::Execution> {
    let path = state_file();
    run_async(executions_for_target_db(
        path,
        repo.to_string(),
        target.to_string(),
    ))
}

async fn active_executions_db(path: PathBuf) -> Vec<crate::commands::Execution> {
    let db = match connect(&path).await {
        Ok(db) => db,
        Err(_) => return Vec::new(),
    };
    let stmt = Statement::from_string(
        DbBackend::Sqlite,
        "SELECT * FROM executions WHERE state IN ('queued', 'running', 'cancelling') ORDER BY updated_at ASC",
    );
    db.query_all_raw(stmt)
        .await
        .unwrap_or_default()
        .iter()
        .filter_map(row_to_execution)
        .collect()
}

pub fn active_executions() -> Vec<crate::commands::Execution> {
    let path = state_file();
    run_async(active_executions_db(path))
}

async fn count_task_executions_db(path: PathBuf, since: i64) -> Result<usize> {
    let db = connect(&path).await?;
    let stmt = Statement::from_sql_and_values(
        DbBackend::Sqlite,
        "SELECT COUNT(*) AS c FROM executions
         WHERE command IN ('plan', 'implement', 'review', 'test')
           AND state NOT IN ('received', 'validating', 'rejected')
           AND created_at >= ?",
        [since.into()],
    );
    let row = db.query_one_raw(stmt).await?;
    let count: i64 = row.and_then(|r| r.try_get("", "c").ok()).unwrap_or(0);
    Ok(count.max(0) as usize)
}

pub fn count_task_executions_since(since: u64) -> usize {
    let path = state_file();
    run_async(count_task_executions_db(path, since as i64)).unwrap_or(0)
}

#[cfg(test)]
async fn execution_transitions_db(path: PathBuf, id: String) -> Vec<(String, u64)> {
    let db = match connect(&path).await {
        Ok(db) => db,
        Err(_) => return Vec::new(),
    };
    let stmt = Statement::from_sql_and_values(
        DbBackend::Sqlite,
        "SELECT state, at FROM execution_transitions WHERE execution_id = ? ORDER BY row_id ASC",
        [id.into()],
    );
    db.query_all_raw(stmt)
        .await
        .unwrap_or_default()
        .iter()
        .filter_map(|row| {
            let state: String = row.try_get("", "state").ok()?;
            let at: i64 = row.try_get("", "at").ok()?;
            Some((state, at as u64))
        })
        .collect()
}

#[cfg(test)]
pub fn execution_transitions(id: &str) -> Vec<(String, u64)> {
    let path = state_file();
    run_async(execution_transitions_db(path, id.to_string()))
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
