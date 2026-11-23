use super::*;

fn gh_json(path: &str) -> Result<Value> {
    let output = Command::new("gh").args(["api", path]).output()?;
    if !output.status.success() {
        bail!(
            "gh api {} failed: {}",
            path,
            String::from_utf8_lossy(&output.stderr).trim()
        );
    }
    Ok(serde_json::from_slice(&output.stdout)?)
}

pub fn issue_event(repo: &str, value: &Value) -> Option<TriggerEvent> {
    let number = value.get("number").and_then(Value::as_u64)?;
    let updated_at = value.get("updated_at").and_then(Value::as_str)?.to_string();
    let id = value
        .get("id")
        .and_then(id_string)
        .unwrap_or_else(|| number.to_string());
    let (author, author_is_bot) = author_of(value.get("user"));
    Some(TriggerEvent {
        repo: repo.to_string(),
        kind: EventKind::Issue,
        id,
        number,
        title: text(value, "title"),
        body: text(value, "body"),
        url: url_of(value),
        author,
        author_is_bot,
        is_pr: value.get("pull_request").is_some(),
        updated_at,
    })
}

fn id_string(value: &Value) -> Option<String> {
    value
        .as_u64()
        .map(|n| n.to_string())
        .or_else(|| value.as_str().map(|s| s.to_string()))
}

fn author_of(user: Option<&Value>) -> (String, bool) {
    let user = user.unwrap_or(&Value::Null);
    let login = user
        .get("login")
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_string();
    let is_bot =
        user.get("type").and_then(Value::as_str) == Some("Bot") || login.ends_with("[bot]");
    (login, is_bot)
}

fn text(value: &Value, key: &str) -> String {
    value
        .get(key)
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_string()
}

fn url_of(value: &Value) -> String {
    value
        .get("html_url")
        .and_then(Value::as_str)
        .or_else(|| value.get("url").and_then(Value::as_str))
        .unwrap_or("")
        .to_string()
}

/// Build a TriggerEvent from a GitHub webhook payload.
/// Supports `issue_comment`, `issues`, and `pull_request` events.
pub fn event_from_webhook(event: &str, payload: &Value) -> Option<TriggerEvent> {
    let repo = payload
        .get("repository")
        .and_then(|r| r.get("full_name"))
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_string();
    if repo.is_empty() {
        return None;
    }
    match event {
        "issue_comment" => {
            let comment = payload.get("comment")?;
            let issue = payload.get("issue")?;
            let number = issue.get("number").and_then(Value::as_u64)?;
            let updated_at = comment
                .get("updated_at")
                .and_then(Value::as_str)?
                .to_string();
            let id = comment.get("id").and_then(id_string)?;
            let (author, author_is_bot) = author_of(comment.get("user"));
            Some(TriggerEvent {
                repo,
                kind: EventKind::Comment,
                id,
                number,
                title: text(issue, "title"),
                body: text(comment, "body"),
                url: url_of(comment),
                author,
                author_is_bot,
                is_pr: issue.get("pull_request").is_some(),
                updated_at,
            })
        }
        "issues" => issue_event(&repo, payload.get("issue")?).map(|mut e| {
            e.repo = repo;
            e
        }),
        "pull_request" => {
            let pr = payload.get("pull_request")?;
            let number = pr.get("number").and_then(Value::as_u64)?;
            let updated_at = pr.get("updated_at").and_then(Value::as_str)?.to_string();
            let id = pr
                .get("id")
                .and_then(id_string)
                .unwrap_or_else(|| number.to_string());
            let (author, author_is_bot) = author_of(pr.get("user"));
            Some(TriggerEvent {
                repo,
                kind: EventKind::Issue,
                id,
                number,
                title: text(pr, "title"),
                body: text(pr, "body"),
                url: url_of(pr),
                author,
                author_is_bot,
                is_pr: true,
                updated_at,
            })
        }
        "pull_request_review_comment" => {
            let comment = payload.get("comment")?;
            let pr = payload.get("pull_request")?;
            let number = pr.get("number").and_then(Value::as_u64)?;
            let updated_at = comment
                .get("updated_at")
                .and_then(Value::as_str)?
                .to_string();
            let id = comment.get("id").and_then(id_string)?;
            let (author, author_is_bot) = author_of(comment.get("user"));
            Some(TriggerEvent {
                repo,
                kind: EventKind::Comment,
                id,
                number,
                title: text(pr, "title"),
                body: text(comment, "body"),
                url: url_of(comment),
                author,
                author_is_bot,
                is_pr: true,
                updated_at,
            })
        }
        _ => None,
    }
}

/// Write-permission check through `gh`. Fails closed.
#[derive(Clone, Copy)]
pub struct GhAuthorizer;

impl Authorizer for GhAuthorizer {
    fn can_execute(&self, repo: &str, user: &str) -> bool {
        if user.trim().is_empty() {
            return false;
        }
        let path = format!(
            "repos/{}/collaborators/{}/permission",
            repo,
            crate::cli::encode_path_segment(user)
        );
        match gh_json(&path) {
            Ok(value) => matches!(
                value.get("permission").and_then(Value::as_str),
                Some("admin") | Some("maintain") | Some("write")
            ),
            Err(e) => {
                tracing::warn!(user = %user, "permission lookup failed: {e}");
                false
            }
        }
    }
}
