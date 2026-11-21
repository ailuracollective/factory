#[cfg(target_arch = "wasm32")]
pub mod admin;
#[cfg(target_arch = "wasm32")]
pub mod runner;
#[cfg(target_arch = "wasm32")]
pub mod webhook;

#[cfg(target_arch = "wasm32")]
pub fn json_response(value: &serde_json::Value, status: u16) -> ohkami::Response {
    ohkami::Response::new(ohkami::Status::from(status)).with_json(value)
}

#[cfg(target_arch = "wasm32")]
pub fn header(req: &ohkami::Request, name: &str) -> String {
    req.headers.get(name).unwrap_or("").to_string()
}

#[cfg(target_arch = "wasm32")]
pub fn guard(
    req: &ohkami::Request,
    env: &worker::Env,
    header_name: &str,
    env_key: &str,
) -> Option<ohkami::Response> {
    let decision = crate::domain::auth::check_secret(
        &crate::infra::env_str(env, env_key),
        &header(req, header_name),
    );
    match decision {
        crate::domain::auth::AuthDecision::Allowed => None,
        crate::domain::auth::AuthDecision::MissingConfig => Some(json_response(
            &serde_json::json!({"error": "configuration error", "reason": format!("{} is not configured", env_key)}),
            503,
        )),
        crate::domain::auth::AuthDecision::Denied => Some(json_response(
            &serde_json::json!({"error": "unauthorized"}),
            401,
        )),
    }
}
