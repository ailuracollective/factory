use crate::config::{RepoEntry, get_repo, load_config, save_config};
use std::fs;
use std::path::PathBuf;
use std::process::Command;

fn run(cmd: &[&str], cwd: Option<&std::path::Path>) -> (i32, String) {
    let mut c = Command::new(cmd[0]);
    c.args(&cmd[1..]);
    if let Some(d) = cwd {
        c.current_dir(d);
    }
    match c.output() {
        Ok(o) => {
            let msg = command_message(&o.stdout, &o.stderr);
            (o.status.code().unwrap_or(1), msg)
        }
        Err(_) => (127, "not found".to_string()),
    }
}

fn command_message(stdout: &[u8], stderr: &[u8]) -> String {
    let out = String::from_utf8_lossy(stdout);
    let err = String::from_utf8_lossy(stderr);
    if out.trim().is_empty() {
        err.trim().to_string()
    } else {
        out.trim().to_string()
    }
}

pub fn resolve_project_path(
    name_or_path: &str,
    projects_base: &std::path::Path,
) -> Option<PathBuf> {
    let candidate = PathBuf::from(name_or_path);
    if candidate.exists() {
        return candidate.canonicalize().ok();
    }
    let name = name_or_path
        .trim_end_matches('/')
        .rsplit('/')
        .next()
        .unwrap_or("")
        .replace(".git", "");
    let dest = projects_base.join(&name);
    if dest.exists() {
        return dest.canonicalize().ok();
    }
    None
}

pub fn register_t3(project_path: &std::path::Path) -> bool {
    let t3_bin = std::env::var("ISSUE_FACTORY_T3").unwrap_or_else(|_| "t3".to_string());
    let (code, _) = run(
        &[&t3_bin, "project", "add", project_path.to_str().unwrap()],
        None,
    );
    code == 0
}

pub fn github_slug_from_remote(remote: &str) -> anyhow::Result<String> {
    let remote = remote.trim();
    let path = if let Some(path) = remote
        .strip_prefix("https://github.com/")
        .or_else(|| remote.strip_prefix("http://github.com/"))
        .or_else(|| remote.strip_prefix("ssh://git@github.com/"))
    {
        path
    } else if let Some(path) = remote.strip_prefix("git@github.com:") {
        path
    } else {
        anyhow::bail!("origin is not a supported GitHub URL: {remote}");
    }
    .trim_end_matches('/')
    .trim_end_matches(".git");
    anyhow::ensure!(
        path.split('/').count() == 2 && path.split('/').all(|part| !part.is_empty()),
        "origin is not a GitHub owner/repo URL: {remote}"
    );
    Ok(path.to_string())
}

pub fn sync_labels(repo_slug: &str) -> anyhow::Result<u32> {
    let label_re = regex::Regex::new(
        r#"- name: "([^"]+)"\s*\n\s*color: "([^"]+)"\s*\n\s*description: "([^"]*)""#,
    )?;
    let mut count = 0;
    for url in [
        "https://raw.githubusercontent.com/ailuracollective/standards/main/.github/labels.yml",
        "https://raw.githubusercontent.com/ailuracollective/factory/main/.github/labels.factory-execution.yml",
    ] {
        let (code, text) = match Command::new("curl").args(["-fsSL", url]).output() {
            Ok(output) if output.status.success() => (
                0,
                String::from_utf8(output.stdout)
                    .map_err(|e| anyhow::anyhow!("label manifest {url} is not UTF-8: {e}"))?,
            ),
            Ok(output) => (
                output.status.code().unwrap_or(1),
                command_message(&output.stdout, &output.stderr),
            ),
            Err(e) => return Err(anyhow::anyhow!("failed to run curl for {url}: {e}")),
        };
        anyhow::ensure!(code == 0, "failed to download label manifest {url}: {text}");
        anyhow::ensure!(
            text.contains("- name:"),
            "no labels found in manifest {url}"
        );
        let mut manifest_count = 0;
        for cap in label_re.captures_iter(&text) {
            let name = cap.get(1).unwrap().as_str();
            let color = cap.get(2).unwrap().as_str();
            let desc = cap.get(3).unwrap().as_str();
            manifest_count += 1;
            let (code, output) = run(
                &[
                    "gh",
                    "label",
                    "create",
                    name,
                    "--color",
                    color,
                    "--description",
                    desc,
                    "--repo",
                    repo_slug,
                ],
                None,
            );
            if code != 0 {
                let already_exists = output.to_ascii_lowercase().contains("already exists")
                    || output.to_ascii_lowercase().contains("already been taken");
                anyhow::ensure!(
                    already_exists,
                    "failed to create label {name} in {repo_slug}: {output}"
                );
                let (edit_code, edit_output) = run(
                    &[
                        "gh",
                        "label",
                        "edit",
                        name,
                        "--color",
                        color,
                        "--description",
                        desc,
                        "--repo",
                        repo_slug,
                    ],
                    None,
                );
                anyhow::ensure!(
                    edit_code == 0,
                    "failed to sync label {name} in {repo_slug}: {}",
                    if edit_output.is_empty() {
                        output
                    } else {
                        edit_output
                    }
                );
            }
            count += 1;
        }
        anyhow::ensure!(
            manifest_count > 0,
            "no supported labels found in manifest {url}"
        );
    }
    anyhow::ensure!(count > 0, "no labels synced for {repo_slug}");
    Ok(count)
}

pub fn add_repo(name_or_path: &str) -> anyhow::Result<RepoEntry> {
    let config = load_config()?;
    let projects_base = PathBuf::from(&config.projects_base);
    fs::create_dir_all(&projects_base)?;

    let project_path = resolve_project_path(name_or_path, &projects_base)
        .ok_or_else(|| anyhow::anyhow!("repo path not found: {name_or_path}"))?;
    let project_path_string = project_path.to_string_lossy().to_string();
    let (remote_code, remote) = run(
        &[
            "git",
            "-C",
            &project_path_string,
            "remote",
            "get-url",
            "origin",
        ],
        None,
    );
    anyhow::ensure!(
        remote_code == 0,
        "failed to read origin remote for {}: {remote}",
        project_path.display()
    );
    let slug = github_slug_from_remote(&remote)?;
    let name = slug.rsplit('/').next().unwrap_or(&slug).to_string();

    let existing = config
        .repos
        .iter()
        .find(|r| r.github == slug || r.path.as_str() == project_path_string.as_str());
    let mut entry = existing.cloned().unwrap_or(RepoEntry {
        name: name.clone(),
        path: project_path_string.clone(),
        github: slug.clone(),
        hook_id: None,
        enabled: true,
    });
    if let Some(existing) = existing {
        entry.hook_id = existing.hook_id;
        entry.enabled = existing.enabled;
    }
    entry.name = name;
    entry.path = project_path_string;
    let collision = config
        .repos
        .iter()
        .any(|r| r.github != slug && r.name == entry.name);
    anyhow::ensure!(
        !collision,
        "repo name {} is already registered for another repository",
        entry.name
    );
    entry.github = slug;
    let mut cfg = config;
    if let Some(existing) = cfg.repos.iter_mut().find(|r| r.github == entry.github) {
        *existing = entry.clone();
    } else if let Some(existing) = cfg.repos.iter_mut().find(|r| r.path == entry.path) {
        *existing = entry.clone();
    } else {
        cfg.repos.push(entry.clone());
    }
    save_config(&cfg)?;
    Ok(entry)
}

pub fn update_repo_hook_id(repo_slug: &str, hook_id: u64) -> anyhow::Result<()> {
    let mut config = load_config()?;
    let repo = config
        .repos
        .iter_mut()
        .find(|repo| repo.github == repo_slug)
        .ok_or_else(|| anyhow::anyhow!("repo not found: {repo_slug}"))?;
    repo.hook_id = Some(hook_id);
    save_config(&config)
}

pub fn sync_repo_labels(repo_slug: &str) -> anyhow::Result<u32> {
    let count = sync_labels(repo_slug)?;
    let config = load_config()?;
    let webhook_expected =
        !config.webhook.funnel_url.is_empty() && !config.webhook.secret.is_empty();
    if webhook_expected
        && let Some(repo) = config.repos.iter().find(|repo| repo.github == repo_slug)
        && repo.hook_id.is_none()
        && let Some(hook_id) = crate::webhook_manager::list_webhooks(repo_slug)?
            .into_iter()
            .find(|(_, url)| url == &config.webhook.funnel_url)
            .map(|(hook_id, _)| hook_id)
    {
        update_repo_hook_id(repo_slug, hook_id)?;
    }
    Ok(count)
}
#[cfg(test)]
mod tests {
    use super::{command_message, github_slug_from_remote};

    #[test]
    fn github_slug_is_parsed_from_common_remote_urls() {
        for remote in [
            "git@github.com:ailuracollective/standards.git",
            "https://github.com/ailuracollective/standards.git",
            "ssh://git@github.com/ailuracollective/standards.git",
            "https://github.com/ailuracollective/standards",
        ] {
            assert_eq!(
                github_slug_from_remote(remote).unwrap(),
                "ailuracollective/standards",
                "{remote}"
            );
        }
        assert!(github_slug_from_remote("git@gitlab.com:org/repo.git").is_err());
        assert!(github_slug_from_remote("https://github.com/org").is_err());
    }

    #[test]
    fn command_message_falls_back_to_stderr_when_stdout_is_empty() {
        assert_eq!(
            command_message(b"", b"permission denied\n"),
            "permission denied"
        );
        assert_eq!(command_message(b"done\n", b"warning"), "done");
    }
}

pub fn remove_repo(name: &str, delete_webhook: bool) -> anyhow::Result<bool> {
    let mut config = load_config()?;
    let Some(repo) = get_repo(&config, name).cloned() else {
        return Ok(false);
    };
    if delete_webhook
        && let Some(hook_id) = repo.hook_id
        && !repo.github.is_empty()
        && !crate::webhook_manager::delete_webhook(&repo.github, hook_id)?
    {
        anyhow::bail!(
            "failed to delete webhook {hook_id} for {}; repo registration is unchanged",
            repo.github
        );
    }
    config
        .repos
        .retain(|r| r.name != repo.name && r.github != repo.github);
    save_config(&config)?;
    Ok(true)
}

pub fn list_repos() -> anyhow::Result<Vec<RepoEntry>> {
    Ok(load_config()?.repos)
}
