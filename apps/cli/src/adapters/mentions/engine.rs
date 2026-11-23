use super::*;

pub struct Engine<'a> {
    config: &'a Config,
    store: &'a dyn EventStore,
    dispatcher: &'a dyn Dispatcher,
    authorizer: &'a dyn Authorizer,
}

impl<'a> Engine<'a> {
    pub fn new(
        config: &'a Config,
        store: &'a dyn EventStore,
        dispatcher: &'a dyn Dispatcher,
        authorizer: &'a dyn Authorizer,
    ) -> Self {
        Self {
            config,
            store,
            dispatcher,
            authorizer,
        }
    }

    /// Poll every enabled allowlisted repo and process new revisions.
    pub fn poll(&self, source: &mut dyn EventSource, now: u64) -> Result<Summary> {
        let mut summary = Summary::default();
        let lookback = self.config.mentions.initial_lookback_secs;
        for repo in self.config.repos.iter().filter(|r| r.enabled) {
            if repo.github.trim().is_empty() {
                continue;
            }
            let cursor = self.store.cursor(&repo.github);
            let since = cursor
                .clone()
                .unwrap_or_else(|| format_timestamp(now.saturating_sub(lookback)));
            let events = match source.fetch(&repo.github, Some(&since)) {
                Ok(events) => events,
                Err(e) => {
                    tracing::error!(repo = %repo.github, "mention poll failed: {e}");
                    continue;
                }
            };
            let mut max_ts = parse_timestamp(&since).unwrap_or(0);
            let mut cancels = Vec::new();
            let mut others = Vec::new();
            for event in &events {
                if let Some(ts) = parse_timestamp(&event.updated_at) {
                    max_ts = max_ts.max(ts);
                }
                if parse_cancel_invocation(&event.body, &self.config.mentions).is_some() {
                    cancels.push(event.clone());
                } else {
                    others.push(event.clone());
                }
            }
            for event in &cancels {
                summary.add(&self.handle(event, now));
            }
            for event in &others {
                summary.add(&self.handle(event, now));
            }
            let cursor_ts = cursor.as_deref().and_then(parse_timestamp).unwrap_or(0);
            if max_ts > 0 && max_ts > cursor_ts {
                let next = format_timestamp(max_ts.saturating_sub(CURSOR_OVERLAP_SECS));
                if let Err(e) = self.store.set_cursor(&repo.github, &next) {
                    tracing::error!(repo = %repo.github, "failed to save mention cursor: {e}");
                }
            }
        }
        Ok(summary)
    }

    /// Authorize and execute one event. Dedupe and audit are persisted.
    pub fn handle(&self, event: &TriggerEvent, now: u64) -> Outcome {
        let key = event.audit_key();
        if self.store.seen(&key) {
            return Outcome::Ignored("duplicate");
        }
        if event.author_is_bot {
            return Outcome::Ignored("bot");
        }

        let known_repo = self.config.repos.iter().any(|r| {
            r.enabled
                && (r.github.eq_ignore_ascii_case(&event.repo)
                    || r.name.eq_ignore_ascii_case(&event.repo))
        });
        if !known_repo {
            if parse_cancel_invocation(&event.body, &self.config.mentions).is_some()
                || parse_invocation(&event.body, &self.config.mentions).is_some()
            {
                return self.reject(&key, format!("repo not allowed: {}", event.repo));
            }
            return Outcome::Ignored("no-invocation");
        }

        if let Some(cancel) = parse_cancel_invocation(&event.body, &self.config.mentions) {
            return self.handle_cancel(event, &key, &cancel);
        }

        let Some(invocation) = parse_invocation(&event.body, &self.config.mentions) else {
            return Outcome::Ignored("no-invocation");
        };

        if !self.author_allowed(&event.repo, &event.author) {
            return self.reject(
                &key,
                format!("unauthorized author @{} in {}", event.author, event.repo),
            );
        }
        let quota = self.config.mentions.daily_quota as usize;
        if quota > 0
            && self
                .store
                .count_launched_since(now.saturating_sub(DAY_SECS))
                >= quota
        {
            return self.reject(&key, "daily quota exceeded".to_string());
        }

        let _ = self.store.record(
            &key,
            "accepted",
            &format!(
                "op=fix kind={} author={} username={}",
                event.kind.label(),
                event.author,
                invocation.username
            ),
        );
        tracing::info!(
            repo = %event.repo,
            number = event.number,
            author = %event.author,
            "dispatching mention"
        );

        match self.dispatcher.launch(event, &invocation.instruction) {
            Ok(thread) => {
                let _ = self
                    .store
                    .record(&key, "completed", &format!("op=fix thread={thread}"));
                self.dispatcher
                    .report(event, &format!("completed; agent thread `{thread}`"));
                Outcome::Launched(thread)
            }
            Err(e) => {
                let detail = e.chars().take(500).collect::<String>();
                let _ =
                    self.store
                        .record(&key, "failed", &format!("op=fix launch failed: {detail}"));
                self.dispatcher
                    .report(event, &format!("failed to launch agent: {detail}"));
                Outcome::Failed(e)
            }
        }
    }

    fn handle_cancel(&self, event: &TriggerEvent, key: &str, cancel: &CancelInvocation) -> Outcome {
        let target = if let Some(ref exec_id) = cancel.execution_id {
            match crate::execution::get_execution(exec_id) {
                Ok(Some(rec)) if rec.repo.eq_ignore_ascii_case(&event.repo) => rec,
                Ok(Some(_)) => {
                    return self.reject(key, "execution does not belong to repo".to_string());
                }
                Ok(None) => return self.reject(key, format!("execution not found: {exec_id}")),
                Err(e) => return self.reject(key, format!("execution lookup failed: {e}")),
            }
        } else {
            let matches = match crate::execution::list_cancellable_for_issue(
                &event.repo,
                event.number as i64,
            ) {
                Ok(m) => m,
                Err(e) => return self.reject(key, format!("execution lookup failed: {e}")),
            };
            if matches.len() == 1 {
                matches.into_iter().next().unwrap()
            } else if matches.is_empty() {
                return self.reject(key, "no cancellable execution for this context".to_string());
            } else {
                let ids = matches
                    .iter()
                    .map(|m| m.execution_id.as_str())
                    .collect::<Vec<_>>()
                    .join(", ");
                return self.reject(
                    key,
                    format!("ambiguous cancel context; specify execution id: {ids}"),
                );
            }
        };

        if !self.cancel_author_allowed(&event.repo, &event.author, Some(&target)) {
            return self.reject(
                key,
                format!(
                    "unauthorized cancel author @{} in {}",
                    event.author, event.repo
                ),
            );
        }

        let _ = self.store.record(
            key,
            "accepted",
            &format!(
                "op=cancel target={} author={}",
                target.execution_id, event.author
            ),
        );
        let res = crate::execution::request_cancel(
            &target.execution_id,
            &format!("github:{}", event.author),
            false,
            Some("github mention cancel"),
        );
        match res {
            Ok(r) => {
                let _ = crate::agent::cancel_running_execution(&target.execution_id, false);
                let _ = crate::execution::append_audit(crate::execution::ExecutionAuditEvent {
                    ts: crate::state::now(),
                    actor: format!("github:{}", event.author),
                    action: "cancel".to_string(),
                    target: target.execution_id.clone(),
                    outcome: r.status.as_str().to_string(),
                    detail: Some(r.message.clone()),
                });
                let _ = self.store.record(
                    key,
                    "completed",
                    &format!(
                        "op=cancel target={} status={}",
                        target.execution_id,
                        r.status.as_str()
                    ),
                );
                self.dispatcher.report(
                    event,
                    &format!(
                        "cancel requested for `{}`: {} (confirmed={})",
                        target.execution_id, r.message, r.confirmed
                    ),
                );
                Outcome::Launched(target.execution_id)
            }
            Err(e) => {
                let _ = self
                    .store
                    .record(key, "failed", &format!("op=cancel failed: {e}"));
                self.dispatcher.report(
                    event,
                    &format!("cancel failed for `{}`: {e}", target.execution_id),
                );
                Outcome::Failed(e.to_string())
            }
        }
    }

    fn cancel_author_allowed(
        &self,
        _repo: &str,
        author: &str,
        target: Option<&crate::execution::ExecutionRecord>,
    ) -> bool {
        let cfg = &self.config.mentions;
        if cfg.usernames.iter().any(|u| login_eq(u, author)) {
            return true;
        }
        if cfg.allowed_authors.iter().any(|u| login_eq(u, author)) {
            return true;
        }
        if let Some(rec) = target
            && let Some(req) = rec.requester.as_deref()
            && (req.eq_ignore_ascii_case(&format!("github:{author}"))
                || req.eq_ignore_ascii_case(author))
        {
            return true;
        }
        false
    }

    fn reject(&self, key: &str, reason: String) -> Outcome {
        tracing::warn!(reason = %reason, "mention rejected");
        let _ = self.store.record(key, "rejected", &reason);
        Outcome::Rejected(reason)
    }

    fn author_allowed(&self, repo: &str, author: &str) -> bool {
        let cfg = &self.config.mentions;
        if cfg.usernames.iter().any(|u| login_eq(u, author)) {
            return true;
        }
        if cfg.allowed_authors.iter().any(|u| login_eq(u, author)) {
            return true;
        }
        self.authorizer.can_execute(repo, author)
    }
}

fn login_eq(a: &str, b: &str) -> bool {
    a.trim()
        .trim_start_matches('@')
        .eq_ignore_ascii_case(b.trim())
}
