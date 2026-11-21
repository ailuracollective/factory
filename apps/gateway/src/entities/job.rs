use sea_orm::entity::prelude::*;

#[derive(Clone, Debug, PartialEq, DeriveEntityModel)]
#[sea_orm(table_name = "jobs")]
pub struct Model {
    #[sea_orm(primary_key, auto_increment = false)]
    pub id: String,
    pub repo: String,
    pub issue_number: i64,
    pub title: Option<String>,
    pub url: Option<String>,
    pub status: String,
    pub runner_id: Option<String>,
    pub lease_until: Option<i64>,
    pub attempts: i64,
    pub last_error: Option<String>,
    pub thread_id: Option<String>,
    pub created_at: i64,
    pub updated_at: i64,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {}

impl ActiveModelBehavior for ActiveModel {}
