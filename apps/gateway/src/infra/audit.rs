use sea_orm::{ActiveModelTrait, DatabaseConnection, Set};

pub async fn audit(conn: &DatabaseConnection, event: &str, detail: &str) {
    let model = crate::entities::audit::ActiveModel {
        ts: Set(super::time::now_ts()),
        event: Set(event.to_string()),
        detail: Set(Some(detail.to_string())),
        ..Default::default()
    };
    let _ = model.insert(conn).await;
}
