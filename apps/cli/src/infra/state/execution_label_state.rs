use super::execution_label_state_store::{
    ActiveModel as ExecutionLabelStateActiveModel, Entity as ExecutionLabelStateEntity,
};
use super::*;

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ExecutionLabelState {
    pub installed_prefix: Option<String>,
    pub active_prefix: Option<String>,
}

async fn get_state_db(path: PathBuf, repo_key: String) -> Result<ExecutionLabelState> {
    let db = connect(&path).await?;
    let model = ExecutionLabelStateEntity::find_by_id(repo_key)
        .one(&db)
        .await?;
    Ok(model
        .map(|m| ExecutionLabelState {
            installed_prefix: m.installed_prefix,
            active_prefix: m.active_prefix,
        })
        .unwrap_or_default())
}

async fn set_state_db(path: PathBuf, repo_key: String, state: ExecutionLabelState) -> Result<()> {
    let db = connect(&path).await?;
    let active = ExecutionLabelStateActiveModel {
        repo_key: Set(repo_key),
        installed_prefix: Set(state.installed_prefix),
        active_prefix: Set(state.active_prefix),
        updated_at: Set(now() as i64),
    };
    ExecutionLabelStateEntity::insert(active)
        .on_conflict(
            OnConflict::column(execution_label_state_store::Column::RepoKey)
                .update_columns([
                    execution_label_state_store::Column::InstalledPrefix,
                    execution_label_state_store::Column::ActivePrefix,
                    execution_label_state_store::Column::UpdatedAt,
                ])
                .to_owned(),
        )
        .exec(&db)
        .await?;
    Ok(())
}

pub fn get_execution_label_state(repo_key: &str) -> ExecutionLabelState {
    let path = state_file();
    run_async(get_state_db(path, repo_key.to_string())).unwrap_or_default()
}

pub fn set_execution_label_state(repo_key: &str, state: ExecutionLabelState) -> Result<()> {
    let path = state_file();
    run_async(set_state_db(path, repo_key.to_string(), state))
}
