use super::ops::encode_path_segment;
use super::*;

pub fn runner(once: bool) -> Result<()> {
    use std::sync::atomic::{AtomicBool, Ordering};
    loop {
        let config = load_config()?;
        let gw = config.gateway.url.clone();
        let runner_id = config.gateway.runner_id.clone();
        let token = config.gateway.token.clone();
        if gw.is_empty() || runner_id.is_empty() || token.is_empty() {
            println!("Not connected. Run 'factory connect' first.");
            return Ok(());
        }
        let client = reqwest::blocking::Client::new();
        let claim_url = format!("{}/jobs/claim", gw);
        let resp = client
            .post(&claim_url)
            .json(&serde_json::json!({"runner_id": runner_id, "token": token}))
            .send();
        match resp {
            Ok(r) if r.status().as_u16() == 204 => {
                if once {
                    println!("No job available.");
                    return Ok(());
                }
                std::thread::sleep(std::time::Duration::from_secs(5));
                continue;
            }
            Ok(r) if r.status().is_success() => {
                let job: serde_json::Value = match r.json() {
                    Ok(v) => v,
                    Err(_) => {
                        println!("Invalid job response.");
                        if once {
                            return Ok(());
                        }
                        std::thread::sleep(std::time::Duration::from_secs(5));
                        continue;
                    }
                };
                let job_id = job
                    .get("id")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string();
                let repo = job
                    .get("repo")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string();
                let number = job
                    .get("issue_number")
                    .and_then(|v| v.as_i64())
                    .unwrap_or(0);
                let title = job
                    .get("title")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string();
                let url = job
                    .get("url")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string();

                let repo_entry = config
                    .repos
                    .iter()
                    .find(|r| r.github == repo || r.name == repo)
                    .cloned();
                let project = repo_entry.map(|r| std::path::PathBuf::from(r.path));
                let project = match project {
                    Some(p) if p.exists() => p,
                    _ => {
                        println!("No project path for {}", repo);
                        let _ = client
                            .post(format!("{}/jobs/{}/fail", gw, encode_path_segment(&job_id)))
                            .json(&serde_json::json!({"runner_id": runner_id, "token": token, "error": "no project path"}))
                            .send();
                        if once {
                            return Ok(());
                        }
                        continue;
                    }
                };

                let issue =
                    serde_json::json!({"number": number, "title": title, "url": url, "labels": []});
                let key = format!("{}#{}", repo, number);
                let prior = crate::state::issue_state(&key);
                let existing = prior.as_ref().and_then(|s| s.thread_id.clone());
                let attempt = prior
                    .as_ref()
                    .map(|s| s.attempts.saturating_add(1))
                    .unwrap_or(1);
                let exec = Some(crate::execution::create_execution(
                    &key,
                    &repo,
                    number,
                    attempt,
                    Some(&format!("runner:{runner_id}")),
                    Some(&runner_id),
                    Some(&format!("gateway-job:{job_id}")),
                )?);
                let exec_id = exec.as_ref().map(|e| e.execution_id.clone());

                let stop = std::sync::Arc::new(AtomicBool::new(false));
                let stop_hb = stop.clone();
                let gw_hb = gw.clone();
                let runner_hb = runner_id.clone();
                let token_hb = token.clone();
                let job_hb = job_id.clone();
                let exec_hb = exec_id.clone();
                let hb = std::thread::spawn(move || {
                    let client = reqwest::blocking::Client::new();
                    while !stop_hb.load(Ordering::Relaxed) {
                        let resp = client
                            .post(format!("{}/jobs/{}/heartbeat", gw_hb, encode_path_segment(&job_hb)))
                            .json(&serde_json::json!({"runner_id": runner_hb, "token": token_hb, "status": "running"}))
                            .send();
                        if let Ok(r) = resp
                            && let Ok(body) = r.json::<serde_json::Value>()
                        {
                            let job_status = body
                                .get("job_status")
                                .and_then(|v| v.as_str())
                                .unwrap_or("");
                            if (job_status == "cancelled" || job_status == "cancelling")
                                && let Some(ref id) = exec_hb
                            {
                                let _ = crate::execution::request_cancel(
                                    id,
                                    "gateway",
                                    false,
                                    Some("gateway job cancelled"),
                                );
                            }
                        }
                        for _ in 0..15 {
                            if stop_hb.load(Ordering::Relaxed) {
                                break;
                            }
                            std::thread::sleep(std::time::Duration::from_secs(1));
                        }
                    }
                });

                let launch =
                    crate::agent::launch_agent_with_execution(crate::agent::AgentLaunchRequest {
                        issue: &issue,
                        project: &project,
                        config: &config,
                        prompt: None,
                        existing_thread: existing.as_deref(),
                        attempt,
                        execution_id: exec_id.as_deref(),
                    });

                stop.store(true, Ordering::Relaxed);
                let _ = hb.join();

                match launch {
                    Ok(tid) => {
                        let cancelled = exec_id
                            .as_deref()
                            .map(crate::execution::is_cancel_requested)
                            .unwrap_or(false);
                        if cancelled {
                            if let Some(ref id) = exec_id {
                                let _ =
                                    crate::execution::mark_cancel_confirmed(id, "runner_cancelled");
                            }
                            let _ = client
                                .post(format!("{}/jobs/{}/fail", gw, encode_path_segment(&job_id)))
                                .json(&serde_json::json!({"runner_id": runner_id, "token": token, "error": "cancelled"}))
                                .send();
                            println!("Cancelled job {}", job_id);
                        } else {
                            let pr_url = crate::mentions::discover_pr_url(&repo, number as u64);
                            if let Some(ref id) = exec_id {
                                let _ = crate::execution::complete_execution(
                                    id,
                                    Some(&tid),
                                    pr_url.as_deref(),
                                );
                            }
                            let _ = crate::state::update_issue_state(&key, |rec| {
                                rec.status = crate::state::IssueStatus::Completed;
                                rec.thread_id = Some(tid.clone());
                                rec.last_error = None;
                                if rec.pr_url.is_none() {
                                    rec.pr_url = pr_url;
                                }
                            });
                            let _ = client
                                .post(format!("{}/jobs/{}/complete", gw, encode_path_segment(&job_id)))
                                .json(&serde_json::json!({"runner_id": runner_id, "token": token, "thread_id": tid}))
                                .send();
                            println!("Completed job {} -> {}", job_id, tid);
                        }
                    }
                    Err(e) => {
                        let cancelled = exec_id
                            .as_deref()
                            .map(crate::execution::is_cancel_requested)
                            .unwrap_or_else(|| e.to_lowercase().contains("cancel"));
                        if cancelled {
                            if let Some(ref id) = exec_id {
                                let _ =
                                    crate::execution::mark_cancel_confirmed(id, "runner_cancelled");
                            }
                            let _ = client
                                .post(format!("{}/jobs/{}/fail", gw, encode_path_segment(&job_id)))
                                .json(&serde_json::json!({"runner_id": runner_id, "token": token, "error": "cancelled"}))
                                .send();
                            println!("Cancelled job {}", job_id);
                        } else {
                            if let Some(ref id) = exec_id {
                                let _ = crate::execution::fail_execution(id, &e);
                            }
                            let _ = client
                                .post(format!("{}/jobs/{}/fail", gw, encode_path_segment(&job_id)))
                                .json(&serde_json::json!({"runner_id": runner_id, "token": token, "error": e}))
                                .send();
                            println!("Failed job {}: {}", job_id, e);
                        }
                    }
                }

                if once {
                    return Ok(());
                }
            }
            Ok(r) => {
                println!("Claim failed: {}", r.status());
                if once {
                    return Ok(());
                }
                std::thread::sleep(std::time::Duration::from_secs(5));
            }
            Err(e) => {
                println!("Network error: {}", e);
                if once {
                    return Ok(());
                }
                std::thread::sleep(std::time::Duration::from_secs(5));
            }
        }
    }
}
