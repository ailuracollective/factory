use anyhow::anyhow;
use sea_orm::{
    ActiveModelTrait, ColumnTrait, ConnectionTrait, Database, DatabaseConnection, DbBackend,
    EntityTrait, QueryFilter, QueryOrder, QuerySelect, Schema, Set, sea_query::OnConflict,
};
use serde::{Deserialize, Serialize};

mod execution_store {
    use sea_orm::entity::prelude::*;

    #[derive(Clone, Debug, PartialEq, Eq, DeriveEntityModel)]
    #[sea_orm(table_name = "executions")]
    pub struct Model {
        #[sea_orm(primary_key, auto_increment = false)]
        pub execution_id: String,
        pub issue_key: String,
        pub repo: String,
        pub issue_number: i64,
        pub attempt: i32,
        pub status: String,
        pub requester: Option<String>,
        pub runner: Option<String>,
        pub thread_id: Option<String>,
        pub pr_url: Option<String>,
        pub reason: Option<String>,
        pub cancel_requested: i32,
        pub cancel_requested_by: Option<String>,
        pub cancel_requested_at: Option<i64>,
        pub cancel_reason: Option<String>,
        pub cancel_confirmed_at: Option<i64>,
        pub cancel_outcome: Option<String>,
        pub started_at: i64,
        pub ended_at: Option<i64>,
        pub updated_at: i64,
        pub audit: Option<String>,
    }

    #[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
    pub enum Relation {}

    impl ActiveModelBehavior for ActiveModel {}
}

use execution_store::{
    ActiveModel as ExecutionActiveModel, Column as ExecutionColumn, Entity as ExecutionEntity,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ExecutionStatus {
    Queued,
    Running,
    Cancelling,
    Cancelled,
    Completed,
    Failed,
}

impl ExecutionStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Queued => "queued",
            Self::Running => "running",
            Self::Cancelling => "cancelling",
            Self::Cancelled => "cancelled",
            Self::Completed => "completed",
            Self::Failed => "failed",
        }
    }

    pub fn parse_status(s: &str) -> Self {
        match s {
            "running" => Self::Running,
            "cancelling" => Self::Cancelling,
            "cancelled" => Self::Cancelled,
            "completed" => Self::Completed,
            "failed" => Self::Failed,
            _ => Self::Queued,
        }
    }

    pub fn is_terminal(self) -> bool {
        matches!(self, Self::Completed | Self::Failed | Self::Cancelled)
    }

    pub fn is_cancellable(self) -> bool {
        matches!(self, Self::Queued | Self::Running | Self::Cancelling)
    }

    /// Guard centralizado de transiciones de la FSM de ejecución.
    /// Fuente única de verdad; los consumidores deben consultarlo antes de mutar.
    pub fn can_transition_to(self, to: Self) -> bool {
        use ExecutionStatus::*;
        matches!(
            (self, to),
            (Queued, Running)
                | (Queued, Cancelled)
                | (Queued, Failed)
                | (Running, Cancelling)
                | (Running, Completed)
                | (Running, Failed)
                | (Running, Cancelled)
                | (Cancelling, Cancelled)
                | (Cancelling, Failed)
        )
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExecutionRecord {
    pub execution_id: String,
    pub issue_key: String,
    pub repo: String,
    pub issue_number: i64,
    pub attempt: u32,
    pub status: ExecutionStatus,
    pub requester: Option<String>,
    pub runner: Option<String>,
    pub thread_id: Option<String>,
    pub pr_url: Option<String>,
    pub reason: Option<String>,
    pub cancel_requested: bool,
    pub cancel_requested_by: Option<String>,
    pub cancel_requested_at: Option<u64>,
    pub cancel_reason: Option<String>,
    pub cancel_confirmed_at: Option<u64>,
    pub cancel_outcome: Option<String>,
    pub started_at: u64,
    pub ended_at: Option<u64>,
    pub updated_at: u64,
}

impl ExecutionRecord {
    fn from_model(m: execution_store::Model) -> Self {
        Self {
            execution_id: m.execution_id,
            issue_key: m.issue_key,
            repo: m.repo,
            issue_number: m.issue_number,
            attempt: m.attempt.max(0) as u32,
            status: ExecutionStatus::parse_status(&m.status),
            requester: m.requester,
            runner: m.runner,
            thread_id: m.thread_id,
            pr_url: m.pr_url,
            reason: m.reason,
            cancel_requested: m.cancel_requested != 0,
            cancel_requested_by: m.cancel_requested_by,
            cancel_requested_at: m.cancel_requested_at.map(|v| v.max(0) as u64),
            cancel_reason: m.cancel_reason,
            cancel_confirmed_at: m.cancel_confirmed_at.map(|v| v.max(0) as u64),
            cancel_outcome: m.cancel_outcome,
            started_at: m.started_at.max(0) as u64,
            ended_at: m.ended_at.map(|v| v.max(0) as u64),
            updated_at: m.updated_at.max(0) as u64,
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct CancelResult {
    pub execution_id: String,
    pub status: ExecutionStatus,
    pub requested: bool,
    pub confirmed: bool,
    pub message: String,
}

mod store;
pub use store::*;
