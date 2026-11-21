#[cfg(target_arch = "wasm32")]
use worker::*;
#[cfg(target_arch = "wasm32")]
pub async fn http_get(url: &str, token: &str) -> Result<(u16, String)> {
    use wasm_bindgen::JsCast;
    let global = js_sys::global();
    let fetch_val = js_sys::Reflect::get(&global, &js_sys::JsString::from("fetch"))
        .map_err(|_| worker::Error::RustError("no fetch".into()))?;
    let fetch_fn: js_sys::Function = fetch_val
        .dyn_into()
        .map_err(|_| worker::Error::RustError("fetch not fn".into()))?;
    let init = js_sys::Object::new();
    let headers = js_sys::Object::new();
    let _ = js_sys::Reflect::set(
        &headers,
        &js_sys::JsString::from("Authorization"),
        &js_sys::JsString::from(format!("Bearer {}", token)),
    );
    let _ = js_sys::Reflect::set(
        &headers,
        &js_sys::JsString::from("Accept"),
        &js_sys::JsString::from("application/vnd.github+json"),
    );
    let _ = js_sys::Reflect::set(
        &headers,
        &js_sys::JsString::from("User-Agent"),
        &js_sys::JsString::from("ailura-factory-gateway"),
    );
    let _ = js_sys::Reflect::set(&init, &js_sys::JsString::from("headers"), &headers);
    let promise = fetch_fn
        .call2(&global, &js_sys::JsString::from(url), &init)
        .map_err(|_| worker::Error::RustError("fetch call failed".into()))?;
    let promise: js_sys::Promise = promise
        .dyn_into()
        .map_err(|_| worker::Error::RustError("not promise".into()))?;
    let resp_val = wasm_bindgen_futures::JsFuture::from(promise)
        .await
        .map_err(|_| worker::Error::RustError("fetch await failed".into()))?;
    let resp: web_sys::Response = resp_val
        .dyn_into()
        .map_err(|_| worker::Error::RustError("not response".into()))?;
    let status = resp.status();
    let text_promise = resp
        .text()
        .map_err(|_| worker::Error::RustError("no text".into()))?;
    let text_val = wasm_bindgen_futures::JsFuture::from(text_promise)
        .await
        .map_err(|_| worker::Error::RustError("text await failed".into()))?;
    let text = text_val.as_string().unwrap_or_default();
    Ok((status, text))
}
#[cfg(target_arch = "wasm32")]
pub async fn github_repo_scope(
    api_base: &str,
    repo: &str,
    token: &str,
) -> crate::domain::github::RepoCheck {
    let base = if api_base.is_empty() {
        "https://api.github.com"
    } else {
        api_base.trim_end_matches('/')
    };
    let url = format!("{base}/repos/{repo}");
    match http_get(&url, token).await {
        Ok((status, body)) => crate::domain::github::evaluate_repo(status, &body, repo),
        Err(_) => crate::domain::github::RepoCheck::Unavailable,
    }
}

#[cfg(target_arch = "wasm32")]
pub async fn github_permission(
    api_base: &str,
    repo: &str,
    user: &str,
    token: &str,
) -> crate::domain::github::PermissionCheck {
    let base = if api_base.is_empty() {
        "https://api.github.com"
    } else {
        api_base.trim_end_matches('/')
    };
    let url = format!("{base}/repos/{repo}/collaborators/{user}/permission");
    match http_get(&url, token).await {
        Ok((status, body)) => crate::domain::github::evaluate_permission(status, &body),
        Err(_) => crate::domain::github::PermissionCheck::Unavailable,
    }
}
