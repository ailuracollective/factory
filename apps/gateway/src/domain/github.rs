#[derive(Debug, PartialEq, Eq)]
pub enum PermissionCheck {
    Allowed,
    Insufficient,
    Unavailable,
}

pub fn evaluate_permission(status: u16, body: &str) -> PermissionCheck {
    if status == 404 {
        return PermissionCheck::Insufficient;
    }
    if status != 200 {
        return PermissionCheck::Unavailable;
    }
    let value: serde_json::Value = match serde_json::from_str(body) {
        Ok(v) => v,
        Err(_) => return PermissionCheck::Unavailable,
    };
    match value.get("permission").and_then(|p| p.as_str()) {
        Some("admin" | "maintain" | "write") => PermissionCheck::Allowed,
        Some(_) => PermissionCheck::Insufficient,
        None => PermissionCheck::Insufficient,
    }
}

#[derive(Debug, PartialEq, Eq)]
pub enum RepoCheck {
    Authorized,
    Denied,
    Unavailable,
}

pub fn evaluate_repo(status: u16, body: &str, expected: &str) -> RepoCheck {
    if status == 403 || status == 404 {
        return RepoCheck::Denied;
    }
    if status != 200 {
        return RepoCheck::Unavailable;
    }
    let value: serde_json::Value = match serde_json::from_str(body) {
        Ok(v) => v,
        Err(_) => return RepoCheck::Unavailable,
    };
    match value.get("full_name").and_then(|v| v.as_str()) {
        Some(name) if name.eq_ignore_ascii_case(expected) => RepoCheck::Authorized,
        _ => RepoCheck::Denied,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn permission_evaluation_denies_unknown_and_failures() {
        assert_eq!(
            evaluate_permission(200, r#"{"permission":"admin"}"#),
            PermissionCheck::Allowed
        );
        assert_eq!(
            evaluate_permission(200, r#"{"permission":"write"}"#),
            PermissionCheck::Allowed
        );
        assert_eq!(
            evaluate_permission(200, r#"{"permission":"read"}"#),
            PermissionCheck::Insufficient
        );
        assert_eq!(
            evaluate_permission(200, r#"{"permission":"triage"}"#),
            PermissionCheck::Insufficient
        );
        assert_eq!(
            evaluate_permission(200, "{}"),
            PermissionCheck::Insufficient
        );
        assert_eq!(
            evaluate_permission(404, r#"{"message":"Not Found"}"#),
            PermissionCheck::Insufficient
        );
        assert_eq!(evaluate_permission(500, "{}"), PermissionCheck::Unavailable);
        assert_eq!(
            evaluate_permission(200, "not json"),
            PermissionCheck::Unavailable
        );
    }

    #[test]
    fn repo_scope_evaluation_denies_unknown_and_failures() {
        assert_eq!(
            evaluate_repo(200, r#"{"full_name":"owner/repo"}"#, "owner/repo"),
            RepoCheck::Authorized
        );
        assert_eq!(
            evaluate_repo(200, r#"{"full_name":"owner/repo"}"#, "Owner/Repo"),
            RepoCheck::Authorized
        );
        assert_eq!(
            evaluate_repo(200, r#"{"full_name":"attacker/repo"}"#, "owner/repo"),
            RepoCheck::Denied
        );
        assert_eq!(evaluate_repo(404, "{}", "owner/repo"), RepoCheck::Denied);
        assert_eq!(
            evaluate_repo(500, "{}", "owner/repo"),
            RepoCheck::Unavailable
        );
        assert_eq!(
            evaluate_repo(200, "not json", "owner/repo"),
            RepoCheck::Unavailable
        );
    }
}
