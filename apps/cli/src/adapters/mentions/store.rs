use super::*;

/// SQLite-backed store used by the CLI.
#[derive(Clone, Copy)]
pub struct SqliteEventStore;

impl EventStore for SqliteEventStore {
    fn cursor(&self, repo: &str) -> Option<String> {
        crate::state::mention_cursor(repo)
    }

    fn set_cursor(&self, repo: &str, cursor: &str) -> Result<()> {
        crate::state::set_mention_cursor(repo, cursor)
    }

    fn seen(&self, key: &str) -> bool {
        crate::state::mention_event_seen(key)
    }

    fn record(&self, key: &str, status: &str, detail: &str) -> Result<()> {
        crate::state::record_mention_event(key, status, detail)
    }

    fn count_launched_since(&self, since: u64) -> usize {
        crate::state::count_mention_launches_since(since)
    }
}
