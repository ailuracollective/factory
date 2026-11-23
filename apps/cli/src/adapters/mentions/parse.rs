use super::*;

pub struct Invocation {
    pub username: String,
    /// Untrusted text after the command, passed to the agent as guidance.
    pub instruction: String,
}

/// Match `@username <command> ...`. `command` is required when configured, so
/// ordinary discussion that merely mentions the account does not launch work.
pub fn parse_invocation(body: &str, cfg: &crate::config::MentionsConfig) -> Option<Invocation> {
    if body.contains(REPORT_MARKER) {
        return None;
    }
    for username in &cfg.usernames {
        let username = username.trim().trim_start_matches('@');
        if username.is_empty() {
            continue;
        }
        let mut search = 0;
        while let Some(found) = find_mention(&body[search..], username) {
            let at = search + found;
            let after = at + 1 + username.len();
            let rest = body[after..].trim_start();
            if cfg.command.trim().is_empty() {
                return Some(Invocation {
                    username: username.to_string(),
                    instruction: rest.trim().to_string(),
                });
            }
            let token_len = rest.find(char::is_whitespace).unwrap_or(rest.len());
            let token = &rest[..token_len];
            let token = token.trim_end_matches(|c: char| {
                matches!(c, ',' | ':' | ';' | '.' | '!' | '?' | ')' | '"' | '\'')
            });
            let expected = cfg.command.trim().trim_start_matches('/');
            if token.trim_start_matches('/').eq_ignore_ascii_case(expected) {
                return Some(Invocation {
                    username: username.to_string(),
                    instruction: rest[token_len..].trim().to_string(),
                });
            }
            search = at + 1;
        }
    }
    None
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CancelInvocation {
    pub username: String,
    pub execution_id: Option<String>,
}

pub fn parse_cancel_invocation(
    body: &str,
    cfg: &crate::config::MentionsConfig,
) -> Option<CancelInvocation> {
    if body.contains(REPORT_MARKER) {
        return None;
    }
    for username in &cfg.usernames {
        let username = username.trim().trim_start_matches('@');
        if username.is_empty() {
            continue;
        }
        let mut search = 0;
        while let Some(found) = find_mention(&body[search..], username) {
            let at = search + found;
            let after = at + 1 + username.len();
            let rest = body[after..].trim_start();
            let mut parts = rest.split_whitespace();
            let first = parts.next().unwrap_or("");
            let first = first.trim_end_matches(|c: char| {
                matches!(c, ',' | ':' | ';' | '.' | '!' | '?' | ')' | '"' | '\'')
            });
            let expected_cmd = cfg.command.trim().trim_start_matches('/');
            let (is_cancel, exec_id) =
                if first.trim_start_matches('/').eq_ignore_ascii_case("cancel") {
                    (true, parts.next().map(clean_token))
                } else if first
                    .trim_start_matches('/')
                    .eq_ignore_ascii_case(expected_cmd)
                {
                    let second = parts.next().unwrap_or("");
                    let second = second.trim_end_matches(|c: char| {
                        matches!(c, ',' | ':' | ';' | '.' | '!' | '?' | ')' | '"' | '\'')
                    });
                    if second
                        .trim_start_matches('/')
                        .eq_ignore_ascii_case("cancel")
                    {
                        (true, parts.next().map(clean_token))
                    } else {
                        (false, None)
                    }
                } else {
                    (false, None)
                };
            if is_cancel {
                return Some(CancelInvocation {
                    username: username.to_string(),
                    execution_id: exec_id.filter(|s| !s.is_empty()),
                });
            }
            search = at + 1;
        }
    }
    None
}

fn clean_token(s: &str) -> String {
    s.trim()
        .trim_end_matches(|c: char| {
            matches!(c, ',' | ':' | ';' | '.' | '!' | '?' | ')' | '"' | '\'')
        })
        .to_string()
}

/// Byte offset of `@username` with a GitHub-style login boundary.
fn find_mention(body: &str, username: &str) -> Option<usize> {
    let needle = username.as_bytes();
    let bytes = body.as_bytes();
    if needle.is_empty() {
        return None;
    }
    let mut i = 0;
    while i + 1 + needle.len() <= bytes.len() {
        if bytes[i] == b'@' && bytes[i + 1..i + 1 + needle.len()].eq_ignore_ascii_case(needle) {
            let before_ok = i == 0 || !is_login_byte(bytes[i - 1]);
            let after = i + 1 + needle.len();
            let after_ok = after == bytes.len() || !is_login_byte(bytes[after]);
            if before_ok && after_ok {
                return Some(i);
            }
        }
        i += 1;
    }
    None
}

fn is_login_byte(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || byte == b'-' || byte == b'_'
}
