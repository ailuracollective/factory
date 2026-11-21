use std::{collections::BTreeMap, sync::Arc};

use sea_orm::{
    Database, DatabaseConnection, DbBackend, DbErr, ProxyDatabaseTrait, ProxyExecResult, ProxyRow,
    RuntimeErr, Statement, Value, Values,
};
use worker::wasm_bindgen::JsValue;
use worker::D1Database;

struct D1Bridge(Arc<D1Database>);

impl std::fmt::Debug for D1Bridge {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "D1Bridge")
    }
}

pub async fn connect(d1: D1Database) -> Result<DatabaseConnection, DbErr> {
    Database::connect_proxy(DbBackend::Sqlite, Arc::new(Box::new(D1Bridge(d1.into())))).await
}

#[async_trait::async_trait]
impl ProxyDatabaseTrait for D1Bridge {
    async fn query(&self, statement: Statement) -> Result<Vec<ProxyRow>, DbErr> {
        let d1 = Arc::clone(&self.0);
        let values = map_values(&statement);
        let sql = statement.sql;

        worker::send::SendFuture::new(async move {
            let res = d1.prepare(sql).bind(&values)?.all().await?;
            if let Some(message) = res.error() {
                anyhow_like_err(message.to_string())?;
            }
            let rows = res.results::<serde_json::Value>()?;
            Ok(rows.into_iter().map(json_to_proxy_row).collect())
        })
        .await
        .map_err(|e: worker::Error| DbErr::Exec(RuntimeErr::Internal(e.to_string())))
    }

    async fn execute(&self, statement: Statement) -> Result<ProxyExecResult, DbErr> {
        let d1 = Arc::clone(&self.0);
        let values = map_values(&statement);
        let sql = statement.sql;

        worker::send::SendFuture::new(async move {
            let result = d1.prepare(sql).bind(&values)?.run().await?;
            if let Some(message) = result.error() {
                return Err(worker::Error::RustError(message.to_string()));
            }
            let meta = result.meta()?;
            let last_insert_id = meta.as_ref().and_then(|m| m.last_row_id).unwrap_or(0) as u64;
            let rows_affected = meta.as_ref().and_then(|m| m.changes).unwrap_or(0) as u64;
            Ok(ProxyExecResult {
                last_insert_id,
                rows_affected,
            })
        })
        .await
        .map_err(|e| DbErr::Exec(RuntimeErr::Internal(e.to_string())))
    }
}

fn anyhow_like_err(msg: String) -> Result<(), worker::Error> {
    Err(worker::Error::RustError(msg))
}

fn map_values(statement: &Statement) -> Vec<JsValue> {
    match &statement.values {
        Some(Values(values)) => values.iter().map(sea_value_to_js).collect(),
        None => Vec::new(),
    }
}

fn sea_value_to_js(val: &Value) -> JsValue {
    match val {
        Value::Bool(Some(v)) => JsValue::from(*v),
        Value::TinyInt(Some(v)) => JsValue::from(*v),
        Value::SmallInt(Some(v)) => JsValue::from(*v),
        Value::Int(Some(v)) => JsValue::from(*v),
        Value::BigInt(Some(v)) => JsValue::from(*v as f64),
        Value::TinyUnsigned(Some(v)) => JsValue::from(*v),
        Value::SmallUnsigned(Some(v)) => JsValue::from(*v),
        Value::Unsigned(Some(v)) => JsValue::from(*v),
        Value::BigUnsigned(Some(v)) => JsValue::from(*v as f64),
        Value::Float(Some(v)) => JsValue::from_f64(*v as f64),
        Value::Double(Some(v)) => JsValue::from_f64(*v),
        Value::String(Some(v)) => JsValue::from(v.as_str()),
        Value::Char(Some(v)) => JsValue::from(v.to_string()),
        Value::Json(Some(v)) => JsValue::from(v.to_string()),
        Value::ChronoDateTimeUtc(Some(v)) => JsValue::from(v.to_string()),
        Value::ChronoDateTimeLocal(Some(v)) => JsValue::from(v.to_string()),
        Value::ChronoDateTimeWithTimeZone(Some(v)) => JsValue::from(v.to_string()),
        Value::ChronoDateTime(Some(v)) => JsValue::from(v.to_string()),
        Value::ChronoDate(Some(v)) => JsValue::from(v.to_string()),
        _ => JsValue::NULL,
    }
}

fn json_to_proxy_row(row: serde_json::Value) -> ProxyRow {
    let mut values = BTreeMap::new();
    let Some(obj) = row.as_object() else {
        return ProxyRow { values };
    };
    for (k, v) in obj {
        let sea_val = match v {
            serde_json::Value::Null => Value::BigInt(None),
            serde_json::Value::Bool(b) => Value::Bool(Some(*b)),
            serde_json::Value::Number(n) => {
                if let Some(i) = n.as_i64() {
                    Value::BigInt(Some(i))
                } else if let Some(u) = n.as_u64() {
                    Value::BigUnsigned(Some(u))
                } else {
                    Value::Double(Some(n.as_f64().unwrap_or(0.0)))
                }
            }
            serde_json::Value::String(s) => Value::String(Some(s.clone())),
            serde_json::Value::Array(_) | serde_json::Value::Object(_) => {
                Value::Json(Some(Box::new(v.clone())))
            }
        };
        values.insert(k.clone(), sea_val);
    }
    ProxyRow { values }
}
