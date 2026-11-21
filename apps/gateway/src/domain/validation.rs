pub fn is_valid_segment(segment: &str) -> bool {
    !segment.is_empty()
        && segment.len() <= 100
        && !segment.starts_with('.')
        && segment
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.'))
}

pub fn is_valid_repo_name(repo: &str) -> bool {
    let mut parts = repo.split('/');
    let owner = parts.next().unwrap_or("");
    let name = parts.next().unwrap_or("");
    parts.next().is_none() && is_valid_segment(owner) && is_valid_segment(name)
}

pub fn is_valid_runner_id(runner_id: &str) -> bool {
    !runner_id.is_empty()
        && runner_id.len() <= 128
        && runner_id
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.' | '@'))
}

pub fn repo_is_allowed(configured: &str, repo: &str) -> bool {
    let allowed: Vec<&str> = configured
        .split(',')
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .collect();
    allowed.is_empty() || allowed.iter().any(|r| r.eq_ignore_ascii_case(repo))
}

pub fn installation_allowed(configured: &str, payload: &serde_json::Value) -> bool {
    let configured = configured.trim();
    if configured.is_empty() {
        return true;
    }
    let expected: i64 = match configured.parse() {
        Ok(v) => v,
        Err(_) => return false,
    };
    payload
        .get("installation")
        .and_then(|i| i.get("id"))
        .and_then(|v| v.as_i64())
        == Some(expected)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn runner_and_repo_identifiers_are_validated() {
        assert!(is_valid_runner_id("runner-1"));
        assert!(is_valid_runner_id("user@host"));
        assert!(!is_valid_runner_id(""));
        assert!(!is_valid_runner_id("bad runner"));
        assert!(!is_valid_runner_id("../runner"));

        assert!(is_valid_repo_name("owner/repo"));
        assert!(is_valid_repo_name("owner/repo.name"));
        assert!(!is_valid_repo_name("owner"));
        assert!(!is_valid_repo_name("owner/repo/extra"));
        assert!(!is_valid_repo_name("owner/../repo"));
        assert!(!is_valid_repo_name("owner/repo name"));
    }

    #[test]
    fn repo_allowlist_matches_case_insensitively() {
        assert!(repo_is_allowed("", "owner/repo"));
        assert!(repo_is_allowed("owner/repo", "Owner/Repo"));
        assert!(repo_is_allowed("a/b, owner/repo", "owner/repo"));
        assert!(!repo_is_allowed("owner/other", "owner/repo"));
    }

    #[test]
    fn installation_scope_is_enforced_when_configured() {
        let payload = serde_json::json!({"installation": {"id": 42}});
        assert!(installation_allowed("", &payload));
        assert!(installation_allowed("42", &payload));
        assert!(!installation_allowed("43", &payload));
        assert!(!installation_allowed("not-a-number", &payload));
        assert!(!installation_allowed("42", &serde_json::json!({})));
    }
}
