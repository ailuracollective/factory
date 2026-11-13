use anyhow::Result;
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

pub fn run_doctor() -> Result<()> {
    let checks: Vec<(&str, (i32, String))> = vec![
        ("gh auth", run(&["gh", "auth", "status"])),
        ("t3 service", run(&["t3", "service", "status"])),
        ("tailscale", run(&["tailscale", "status"])),
        ("tailscale funnel", run(&["tailscale", "funnel", "status"])),
    ];
    for (name, (code, msg)) in &checks {
        let status = if *code == 0 { "OK" } else { "FAIL" };
        println!("[{}] {}: {}", status, name, msg);
    }
    // config
    let cfg = crate::config::load_config().unwrap_or_default();
    println!("[OK] config: {} repos configured", cfg.repos.len());
    // funnel reachable
    if !cfg.webhook.funnel_url.is_empty() {
        let url = format!("{}/health", cfg.webhook.funnel_url.trim_end_matches('/'));
        let (_code, msg) = run(&["curl", "-s", "-o", "/dev/null", "-w", "%{http_code}", &url]);
        let ok = msg == "200";
        println!(
            "[{}] funnel reachable: {}",
            if ok { "OK" } else { "FAIL" },
            msg
        );
    }
    let failed = checks.iter().any(|(_, (c, _))| *c != 0);
    if failed {
        anyhow::bail!("some checks failed");
    }
    Ok(())
}
