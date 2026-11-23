use super::*;

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
pub(super) fn project_name_from_key(key: &str) -> Option<String> {
    let before = key.split('#').next()?;
    let name = before.rsplit('/').next()?;
    if name.is_empty() {
        None
    } else {
        Some(name.to_string())
    }
}

pub(super) async fn find_project_id(db: &DatabaseConnection, name: &str) -> Option<Uuid> {
    let row = ProjectEntity::find()
        .filter(ProjectColumn::Name.eq(name))
        .one(db)
        .await
        .ok()??;
    Uuid::parse_str(&row.id).ok()
}
