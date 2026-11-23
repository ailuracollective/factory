use serde::Serialize;
use serde_json::Value;

pub const SCHEMA_VERSION: u32 = 1;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OutputFormat {
    Human,
    Json,
}

impl OutputFormat {
    pub fn from_flag(json: bool) -> Self {
        if json {
            OutputFormat::Json
        } else {
            OutputFormat::Human
        }
    }
}

#[derive(Debug, Serialize)]
pub struct OutputEnvelope<T: Serialize> {
    pub schema_version: u32,
    pub data: T,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub meta: Option<OutputMeta>,
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct OutputMeta {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub count: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub limit: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub next_cursor: Option<String>,
}

impl OutputMeta {
    pub fn new() -> Self {
        Self::default()
    }
}

impl<T: Serialize> OutputEnvelope<T> {
    pub fn new(data: T) -> Self {
        Self {
            schema_version: SCHEMA_VERSION,
            data,
            meta: None,
        }
    }
    pub fn with_meta(mut self, meta: OutputMeta) -> Self {
        self.meta = Some(meta);
        self
    }
}

#[derive(Debug, Serialize)]
pub struct OutputError {
    pub schema_version: u32,
    pub error: OutputErrorDetail,
}

#[derive(Debug, Serialize)]
pub struct OutputErrorDetail {
    pub code: &'static str,
    pub message: String,
}

impl OutputError {
    pub fn new(code: &'static str, message: impl Into<String>) -> Self {
        Self {
            schema_version: SCHEMA_VERSION,
            error: OutputErrorDetail {
                code,
                message: message.into(),
            },
        }
    }
}

pub fn to_json<T: Serialize>(value: &T) -> Result<String, serde_json::Error> {
    serde_json::to_string(value)
}

pub fn to_json_pretty<T: Serialize>(value: &T) -> Result<String, serde_json::Error> {
    serde_json::to_string_pretty(value)
}

/// Redact sensitive fields. Operates on a JSON value by removing keys whose name
/// matches a known sensitive set, recursively.
pub fn redact(value: &mut Value) {
    const SENSITIVE: &[&str] = &[
        "secret",
        "token",
        "password",
        "credential",
        "prompt",
        "api_key",
        "access_token",
        "refresh_token",
        "webhook_secret",
    ];
    match value {
        Value::Object(map) => {
            let keys: Vec<String> = map.keys().cloned().collect();
            for k in keys {
                let lower = k.to_lowercase();
                if SENSITIVE.iter().any(|s| lower.contains(s)) {
                    map.remove(&k);
                } else if let Some(v) = map.get_mut(&k) {
                    redact(v);
                }
            }
        }
        Value::Array(arr) => {
            for v in arr.iter_mut() {
                redact(v);
            }
        }
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn envelope_serializes_empty_array() {
        let env = OutputEnvelope::new(Vec::<i32>::new());
        let s = to_json(&env).unwrap();
        assert!(s.contains("\"schema_version\":1"));
        assert!(s.contains("\"data\":[]"));
    }

    #[test]
    fn envelope_with_meta() {
        let env = OutputEnvelope::new(vec![1, 2]).with_meta(OutputMeta {
            count: Some(2),
            limit: Some(20),
            next_cursor: None,
        });
        let s = to_json(&env).unwrap();
        assert!(s.contains("\"count\":2"));
        assert!(s.contains("\"limit\":20"));
        assert!(!s.contains("next_cursor"));
    }

    #[test]
    fn nullable_field_skipped_when_none() {
        #[derive(Serialize)]
        struct R {
            id: String,
            note: Option<String>,
        }
        let r = R {
            id: "a".into(),
            note: None,
        };
        let s = to_json(&r).unwrap();
        assert!(s.contains("\"id\":\"a\""));
    }

    #[test]
    fn error_shape() {
        let e = OutputError::new("NOT_FOUND", "missing");
        let s = to_json(&e).unwrap();
        assert!(s.contains("\"code\":\"NOT_FOUND\""));
        assert!(s.contains("\"message\":\"missing\""));
    }

    #[test]
    fn redaction_removes_sensitive() {
        let mut v = serde_json::json!({"token": "abc", "name": "x", "nested": {"password": "p"}});
        redact(&mut v);
        assert!(!v.to_string().contains("abc"));
        assert!(!v.to_string().contains("p"));
        assert!(v.to_string().contains("x"));
    }

    #[test]
    fn single_resource() {
        #[derive(Serialize)]
        struct R {
            id: i64,
        }
        let env = OutputEnvelope::new(R { id: 5 });
        let s = to_json(&env).unwrap();
        assert!(s.contains("\"data\":{\"id\":5}"));
    }
}
