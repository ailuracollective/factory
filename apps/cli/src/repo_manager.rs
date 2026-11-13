use crate::config::{get_repo, load_config, save_config, RepoEntry};
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

pub fn sync_labels(repo_slug: &str) -> u32 {
    let url =
        "https://raw.githubusercontent.com/ailuracollective/standards/main/.github/labels.yml";
    let labels_file = std::env::temp_dir().join("labels.yml");
    // download
    let status = Command::new("curl")
        .args(["-s", "-o", labels_file.to_str().unwrap(), url])
        .status();
    if status.map(|s| !s.success()).unwrap_or(true) {
        return 0;
    }
    let text = match fs::read_to_string(&labels_file) {
        Ok(t) => t,
        Err(_) => return 0,
    };
    // simple regex parse
    let re = regex::Regex::new(
        r#"- name: "([^"]+)"\s*\n\s*color: "([^"]+)"\s*\n\s*description: "([^"]*)""#,
    );
    let re = match re {
        Ok(r) => r,
        Err(_) => return 0,
    };
    let mut count = 0;
    for cap in re.captures_iter(&text) {
        let name = cap.get(1).unwrap().as_str();
        let color = cap.get(2).unwrap().as_str();
        let desc = cap.get(3).unwrap().as_str();
        run(
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
        run(
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
        count += 1;
    }
    count
}

pub fn add_repo(name_or_path: &str) -> Option<RepoEntry> {
    let config = load_config().ok()?;
    let projects_base = PathBuf::from(&config.projects_base);
    let _ = std::fs::create_dir_all(&projects_base);

    let project_path = resolve_project_path(name_or_path, &projects_base)?;
    // detect slug from git remote
    let (_, remote) = run(
        &[
            "git",
            "-C",
            project_path.to_str().unwrap(),
            "remote",
            "get-url",
            "origin",
        ],
        None,
    );
    let slug = if remote.contains("github.com") {
        remote
            .split("github.com")
            .last()
            .unwrap_or("")
            .trim_matches(|c: char| c == ':' || c == '/')
            .replace(".git", "")
    } else {
        project_path
            .file_name()
            .unwrap_or_default()
            .to_string_lossy()
            .to_string()
    };
    let name = slug.rsplit('/').next().unwrap_or(&slug).to_string();

    register_t3(&project_path);
    sync_labels(&slug);

    let entry = RepoEntry {
        name: name.clone(),
        path: project_path.to_string_lossy().to_string(),
        github: slug,
        hook_id: None,
        enabled: true,
    };
    let mut cfg = config;
    // add if not exists
    if !cfg.repos.iter().any(|r| r.name == entry.name) {
        cfg.repos.push(entry.clone());
    }
    save_config(&cfg).ok()?;
    Some(entry)
}

pub fn remove_repo(name: &str) -> bool {
    let mut config = match load_config() {
        Ok(c) => c,
        Err(_) => return false,
    };
    let repo = match get_repo(&config, name) {
        Some(r) => r.clone(),
        None => return false,
    };
    if let Some(hook_id) = repo.hook_id {
        if !repo.github.is_empty() {
            run(
                &[
                    "gh",
                    "api",
                    &format!("repos/{}/hooks/{}", repo.github, hook_id),
                    "-X",
                    "DELETE",
                ],
                None,
            );
        }
    }
    config.repos.retain(|r| r.name != name && r.github != name);
    save_config(&config).ok();
    true
}

pub fn list_repos() -> Vec<RepoEntry> {
    load_config().map(|c| c.repos).unwrap_or_default()
}
