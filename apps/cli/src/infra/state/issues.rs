use super::projects::{find_project_id, project_name_from_key};
use super::*;

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
