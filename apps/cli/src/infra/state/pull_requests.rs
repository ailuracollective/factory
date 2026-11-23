use super::*;
use sea_orm::QueryOrder;
use sea_orm::sea_query::OnConflict;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LinkedPullRequest {
    pub number: u64,
    pub url: String,
    pub issue_key: String,
    pub thread_id: Option<String>,
}

pub fn pull_request_for_event(repo: &str, number: u64) -> Option<LinkedPullRequest> {
    let url = format!("https://github.com/{repo}/pull/{number}");
    let path = state_file();
    run_async(async move {
        let db = connect(&path).await?;
        let row = PullRequestEntity::find()
            .filter(pull_request_store::Column::Url.eq(url))
            .one(&db)
            .await?;
        Ok::<_, anyhow::Error>(row.map(|row| LinkedPullRequest {
            number: row.number.max(0) as u64,
            url: row.url,
            issue_key: row.issue_key,
            thread_id: row.thread_id,
        }))
    })
    .ok()
    .flatten()
}

pub fn linked_pull_requests(issue_key: &str) -> Result<Vec<LinkedPullRequest>> {
    let path = state_file();
    let issue_key = issue_key.to_string();
    run_async(async move {
        let db = connect(&path).await?;
        let rows = PullRequestEntity::find()
            .filter(pull_request_store::Column::IssueKey.eq(issue_key))
            .order_by_asc(pull_request_store::Column::UpdatedAt)
            .all(&db)
            .await?;
        Ok(rows
            .into_iter()
            .map(|row| LinkedPullRequest {
                number: row.number.max(0) as u64,
                url: row.url,
                issue_key: row.issue_key,
                thread_id: row.thread_id,
            })
            .collect())
    })
}

pub fn link_pull_request(
    issue_key: &str,
    number: u64,
    url: &str,
    thread_id: Option<&str>,
) -> Result<()> {
    let path = state_file();
    let issue_key = issue_key.to_string();
    let url = url.to_string();
    let thread_id = thread_id.map(str::to_string);
    run_async(async move {
        let db = connect(&path).await?;
        let active = PullRequestActiveModel {
            id: Set(Uuid::now_v7()),
            issue_key: Set(issue_key),
            number: Set(number as i64),
            url: Set(url.clone()),
            thread_id: Set(thread_id.clone()),
            updated_at: Set(now() as i64),
        };
        PullRequestEntity::insert(active)
            .on_conflict(
                OnConflict::column(pull_request_store::Column::Url)
                    .update_columns([
                        pull_request_store::Column::IssueKey,
                        pull_request_store::Column::Number,
                        pull_request_store::Column::UpdatedAt,
                    ])
                    .to_owned(),
            )
            .exec(&db)
            .await?;
        if let Some(thread_id) = thread_id {
            PullRequestEntity::update_many()
                .col_expr(
                    pull_request_store::Column::ThreadId,
                    sea_orm::sea_query::Expr::value(thread_id),
                )
                .filter(pull_request_store::Column::Url.eq(url))
                .exec(&db)
                .await?;
        }
        Ok::<(), anyhow::Error>(())
    })
}
