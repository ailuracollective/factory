pub mod adapters;
pub mod application;
pub mod infra;
pub mod interface;

// Backward-compatible flat paths.
pub use adapters::{listener, mentions, webhook_manager};
pub use application::{agent, execution, repo_manager};
pub use infra::{config, state};
pub use interface::{cli, doctor};
