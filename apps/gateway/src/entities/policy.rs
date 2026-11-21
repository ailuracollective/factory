use sea_orm::entity::prelude::*;

#[derive(Clone, Debug, PartialEq, DeriveEntityModel)]
#[sea_orm(table_name = "policies")]
pub struct Model {
    #[sea_orm(primary_key, auto_increment = false)]
    pub repo: String,
    pub enabled: i64,
    pub allowed_users: String,
    pub max_concurrent: i64,
    pub daily_quota: i64,
    pub updated_at: i64,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {}

impl ActiveModelBehavior for ActiveModel {}
