use anyhow::Result;
use std::process::Command;

fn run(cmd: &[String]) -> (i32, String) {
    if cmd.is_empty() {
        return (127, "empty command".to_string());
    }
    match Command::new(&cmd[0]).args(&cmd[1..]).output() {
        Ok(o) => {
            let out = String::from_utf8_lossy(&o.stdout);
            let err = String::from_utf8_lossy(&o.stderr);
            let msg = if out.trim().is_empty() {
                err.trim().to_string()
            } else {
                out.trim().to_string()
            };
            (o.status.code().unwrap_or(1), msg)
        }
        Err(_) => (127, "not found".to_string()),
    }
}

pub fn post_issue_comment(repo: &str, number: u64, body: &str) -> bool {
    let path = format!("repos/{}/issues/{}/comments", repo, number);
    let body_arg = format!("body={body}");
    match std::process::Command::new("gh")
        .args(["api", "-X", "POST", &path, "-f", &body_arg])
        .output()
    {
        Ok(o) => o.status.success(),
        Err(_) => false,
    }
}

pub fn create_webhook(repo_slug: &str, url: &str, secret: &str) -> Result<Option<u64>> {
    let path = format!("repos/{}/hooks", repo_slug);
    let url_arg = format!("config[url]={}", url);
    let secret_arg = format!("config[secret]={}", secret);
    let args = vec![
        "gh".to_string(),
        "api".to_string(),
        path,
        "-X".to_string(),
        "POST".to_string(),
        "-f".to_string(),
        "name=web".to_string(),
        "-F".to_string(),
        "active=true".to_string(),
        "-f".to_string(),
        "events[]=issues".to_string(),
        "-f".to_string(),
        url_arg,
        "-f".to_string(),
        "config[content_type]=json".to_string(),
        "-f".to_string(),
        secret_arg,
        "-f".to_string(),
        "config[insecure_ssl]=0".to_string(),
    ];
    let (code, out) = run(&args);
    if code != 0 {
        return Ok(None);
    }
    let val: serde_json::Value = serde_json::from_str(&out)?;
    Ok(val.get("id").and_then(|v| v.as_u64()))
}

pub fn update_webhook(repo_slug: &str, hook_id: u64, url: &str, secret: &str) -> Result<bool> {
    let path = format!("repos/{}/hooks/{}", repo_slug, hook_id);
    let url_arg = format!("config[url]={}", url);
    let secret_arg = format!("config[secret]={}", secret);
    let args = vec![
        "gh".to_string(),
        "api".to_string(),
        path,
        "-X".to_string(),
        "PATCH".to_string(),
        "-f".to_string(),
        url_arg,
        "-f".to_string(),
        "config[content_type]=json".to_string(),
        "-f".to_string(),
        secret_arg,
        "-f".to_string(),
        "config[insecure_ssl]=0".to_string(),
    ];
    let (code, _) = run(&args);
    Ok(code == 0)
}

pub fn delete_webhook(repo_slug: &str, hook_id: u64) -> Result<bool> {
    let path = format!("repos/{}/hooks/{}", repo_slug, hook_id);
    let (code, _) = run(&[
        "gh".to_string(),
        "api".to_string(),
        path,
        "-X".to_string(),
        "DELETE".to_string(),
    ]);
    Ok(code == 0)
}

pub fn list_webhooks(repo_slug: &str) -> Result<Vec<(u64, String)>> {
    let path = format!("repos/{repo_slug}/hooks");
    let (code, output) = run(&["gh".to_string(), "api".to_string(), path]);
    anyhow::ensure!(
        code == 0,
        "failed to list webhooks for {repo_slug}: {output}"
    );
    let hooks: Vec<serde_json::Value> = serde_json::from_str(&output)?;
    Ok(hooks
        .into_iter()
        .filter_map(|hook| {
            Some((
                hook.get("id")?.as_u64()?,
                hook.get("config")?.get("url")?.as_str()?.to_string(),
            ))
        })
        .collect())
}
