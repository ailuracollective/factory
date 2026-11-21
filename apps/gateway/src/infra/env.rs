#[cfg(target_arch = "wasm32")]
pub fn env_str(env: &worker::Env, key: &str) -> String {
    if let Ok(s) = env.secret(key) {
        return s.to_string();
    }
    if let Ok(v) = env.var(key) {
        return v.to_string();
    }
    String::new()
}
