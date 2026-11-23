//! Standalone (Gateway-free) mention ingestion.
//!
//! GitHub does not send notifications for self-mentions, so the CLI polls the
//! allowlisted repositories itself and dispatches authorized requests through
//! the existing agent launcher. A future Gateway-backed `EventSource` can reuse
//! the same parser and engine.

use anyhow::{Result, bail};
use serde_json::Value;
use std::path::PathBuf;
use std::process::Command;

use crate::config::Config;

/// Marks comments written by Factory so they never re-trigger a request.
pub const REPORT_MARKER: &str = "<!-- factory:mention-report -->";

const CURSOR_OVERLAP_SECS: u64 = 5;
const DAY_SECS: u64 = 86_400;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EventKind {
    Issue,
    Comment,
}

impl EventKind {
    fn label(self) -> &'static str {
        match self {
            EventKind::Issue => "issue",
            EventKind::Comment => "comment",
        }
    }
}

/// Transport-independent representation of a GitHub issue or issue/PR comment.
#[derive(Debug, Clone)]
pub struct TriggerEvent {
    pub repo: String,
    pub kind: EventKind,
    /// Stable GitHub object id (issue id or comment id).
    pub id: String,
    pub number: u64,
    pub title: String,
    pub body: String,
    pub url: String,
    pub author: String,
    pub author_is_bot: bool,
    pub is_pr: bool,
    /// Last update time, used for the polling cursor.
    pub updated_at: String,
}

impl TriggerEvent {
    pub fn audit_key(&self) -> String {
        // Hash the body instead of using `updated_at`: an issue's timestamp also
        // changes when anyone comments, which would cause duplicate launches.
        format!(
            "{}:{}:{}:{}",
            self.repo,
            self.kind.label(),
            self.id,
            body_hash(&self.body)
        )
    }
}

fn body_hash(body: &str) -> String {
    use sha2::{Digest, Sha256};
    let digest = Sha256::digest(body.as_bytes());
    hex::encode(&digest[..16])
}

/// A GitHub source of trigger events. Implemented by the standalone poller now
/// and by a Gateway client later.
pub trait EventSource {
    fn fetch(&mut self, repo: &str, since: Option<&str>) -> Result<Vec<TriggerEvent>>;
}

/// Cursor, dedupe and audit persistence.
pub trait EventStore {
    fn cursor(&self, repo: &str) -> Option<String>;
    fn set_cursor(&self, repo: &str, cursor: &str) -> Result<()>;
    fn seen(&self, key: &str) -> bool;
    fn record(&self, key: &str, status: &str, detail: &str) -> Result<()>;
    fn count_launched_since(&self, since: u64) -> usize;
}

/// Launch abstraction so tests do not invoke `t3` or GitHub.
pub trait Dispatcher {
    fn launch(&self, event: &TriggerEvent, instruction: &str) -> Result<String, String>;
    fn report(&self, event: &TriggerEvent, status: &str);
}

/// Write-permission lookup for authors that are not explicitly allowlisted.
pub trait Authorizer {
    fn can_execute(&self, repo: &str, user: &str) -> bool;
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Outcome {
    Ignored(&'static str),
    Rejected(String),
    Launched(String),
    Failed(String),
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Summary {
    pub ignored: usize,
    pub rejected: usize,
    pub launched: usize,
    pub failed: usize,
}

impl Summary {
    fn add(&mut self, outcome: &Outcome) {
        match outcome {
            Outcome::Ignored(_) => self.ignored += 1,
            Outcome::Rejected(_) => self.rejected += 1,
            Outcome::Launched(_) => self.launched += 1,
            Outcome::Failed(_) => self.failed += 1,
        }
    }
}

mod dispatch;
mod engine;
mod github;
mod parse;
mod store;

pub use dispatch::{
    AgentDispatcher, build_mention_prompt, discover_pr_url, format_timestamp, parse_timestamp,
};
pub use engine::Engine;
pub use github::{GhAuthorizer, GhEventSource, event_from_webhook};
#[cfg(test)]
pub(super) use github::{comment_event, issue_event};
pub use parse::{CancelInvocation, Invocation, parse_cancel_invocation, parse_invocation};
pub use store::SqliteEventStore;

#[cfg(test)]
mod tests;
