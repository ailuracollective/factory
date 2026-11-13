use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct State {
    pub processed: Vec<String>,
    pub threads: HashMap<String, String>,
}

pub fn state_file() -> PathBuf {
    if let Ok(p) = std::env::var("ISSUE_FACTORY_STATE_FILE") {
        if !p.trim().is_empty() {
            return PathBuf::from(p);
        }
    }
    dirs::config_dir()
        .unwrap_or_else(|| dirs::home_dir().unwrap().join(".config"))
        .join("ailuracollective")
        .join("factory")
        .join("state.json")
}

pub fn load_state() -> (Vec<String>, HashMap<String, String>) {
    let path = state_file();
    if !path.exists() {
        return (Vec::new(), HashMap::new());
    }
    let data = match fs::read_to_string(&path) {
        Ok(d) => d,
        Err(_) => return (Vec::new(), HashMap::new()),
    };
    let state: State = match serde_json::from_str(&data) {
        Ok(s) => s,
        Err(_) => return (Vec::new(), HashMap::new()),
    };
    (state.processed, state.threads)
}

pub fn save_state(processed: &[String], threads: &HashMap<String, String>) -> Result<()> {
    let state = State {
        processed: processed.to_vec(),
        threads: threads.clone(),
    };
    let path = state_file();
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let data = serde_json::to_string_pretty(&state)?;
    fs::write(path, data)?;
    Ok(())
}
