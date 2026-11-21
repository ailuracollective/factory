use sea_orm::entity::prelude::*;

#[derive(Clone, Debug, PartialEq, DeriveEntityModel)]
#[sea_orm(table_name = "policy_events")]
pub struct Model {
    #[sea_orm(primary_key)]
    pub id: i64,
    pub ts: i64,
    pub repo: String,
    pub actor: Option<String>,
    pub decision: String,
    pub reason: Option<String>,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {}

impl ActiveModelBehavior for ActiveModel {}
