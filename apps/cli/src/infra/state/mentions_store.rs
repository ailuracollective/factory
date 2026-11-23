use super::*;

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
