#[cfg(target_arch = "wasm32")]
use js_sys::Date;
#[cfg(target_arch = "wasm32")]
use worker::*;

#[cfg(target_arch = "wasm32")]
pub fn now_ts() -> i64 {
    (Date::now() / 1000.0) as i64
}

#[cfg(target_arch = "wasm32")]
pub fn random_runner_token() -> Option<String> {
    use wasm_bindgen::JsCast;
    let global = js_sys::global();
    let crypto_val = js_sys::Reflect::get(&global, &js_sys::JsString::from("crypto")).ok()?;
    let crypto: web_sys::Crypto = crypto_val.dyn_into().ok()?;
    let mut bytes = [0u8; 32];
    crypto.get_random_values_with_u8_array(&mut bytes).ok()?;
    Some(crate::domain::auth::format_runner_token(&bytes))
}
