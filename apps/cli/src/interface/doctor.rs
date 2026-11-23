use anyhow::Result;
use serde::Serialize;
use std::process::Command;

fn run(cmd: &[&str]) -> (i32, String) {
    match Command::new(cmd[0]).args(&cmd[1..]).output() {
        Ok(o) => {
            let out = String::from_utf8_lossy(&o.stdout);
            let err = String::from_utf8_lossy(&o.stderr);
            let msg = if out.trim().is_empty() {
                err.trim().to_string()
            } else {
                out.trim().to_string()
            };
            let msg = msg.lines().next().unwrap_or("").to_string();
            (o.status.code().unwrap_or(1), msg)
        }
        Err(_) => (127, "not found".to_string()),
    }
}

#[derive(Serialize)]
struct Check {
    id: String,
    status: String,
    subject: String,
    message: String,
}

pub fn run_doctor(json: bool) -> Result<()> {
    let checks: Vec<(&str, (i32, String))> = vec![
        ("gh auth", run(&["gh", "auth", "status"])),
        ("t3 service", run(&["t3", "service", "status"])),
        ("tailscale", run(&["tailscale", "status"])),
        ("tailscale funnel", run(&["tailscale", "funnel", "status"])),
    ];

    let cfg = crate::config::load_config().unwrap_or_default();

    // funnel reachable
    let mut funnel_check: Option<(i32, String)> = None;
    if !cfg.webhook.funnel_url.is_empty() {
        let url = format!("{}/health", cfg.webhook.funnel_url.trim_end_matches('/'));
        let r = run(&["curl", "-s", "-o", "/dev/null", "-w", "%{http_code}", &url]);
        funnel_check = Some(r);
    }

    let mut records: Vec<Check> = Vec::new();
    let mut failed = false;

    for (name, (code, msg)) in &checks {
        let status = if *code == 0 { "OK" } else { "FAIL" };
        if *code != 0 {
            failed = true;
        }
        if !json {
            println!("[{}] {}: {}", status, name, msg);
        }
        records.push(Check {
            id: name.to_string(),
            status: status.to_string(),
            subject: name.to_string(),
            message: msg.clone(),
        });
    }

    // config
    if !json {
        println!("[OK] config: {} repos configured", cfg.repos.len());
    }
    records.push(Check {
        id: "config".to_string(),
        status: "OK".to_string(),
        subject: "config".to_string(),
        message: format!("{} repos configured", cfg.repos.len()),
    });

    if let Some((code, msg)) = &funnel_check {
        let ok = msg == "200";
        let status = if ok { "OK" } else { "FAIL" };
        if !ok {
            failed = true;
        }
        if !json {
            println!("[{}] funnel reachable: {}", status, msg);
        }
        records.push(Check {
            id: "funnel reachable".to_string(),
            status: status.to_string(),
            subject: "funnel".to_string(),
            message: msg.clone(),
        });
        let _ = code;
    }

    if json {
        use crate::interface::output::{OutputEnvelope, to_json};
        let env = OutputEnvelope::new(records);
        println!("{}", to_json(&env).unwrap());
    }

    let failed_checks = checks.iter().any(|(_, (c, _))| *c != 0);
    if failed || failed_checks {
        anyhow::bail!("some checks failed");
    }
    Ok(())
}
