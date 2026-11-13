mod agent;
mod cli;
mod config;
mod doctor;
mod listener;
mod mentions;
mod repo_manager;
mod state;
mod webhook_manager;

use clap::{Parser, Subcommand};
use std::process::ExitCode;

#[derive(Parser)]
#[command(
    name = "factory",
    version,
    about = "AI Factory CLI",
    disable_help_subcommand = true
)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Initialize factory config
    Init {
        #[arg(long)]
        force: bool,
    },
    /// Manage repos
    Repo {
        #[command(subcommand)]
        action: RepoAction,
    },
    /// Start the daemon
    Start {
        #[arg(long)]
        foreground: bool,
    },
    /// Stop the daemon
    Stop,
    /// Restart the daemon
    Restart,
    /// Show status
    Status,
    /// Manage secret
    Secret {
        #[command(subcommand)]
        action: SecretAction,
    },
    /// Show daemon logs
    Logs {
        #[arg(short, long)]
        follow: bool,
        #[arg(short = 'n', long, default_value = "50")]
        lines: u32,
    },
    /// Run diagnostics
    Doctor,
    /// Run the webhook listener (foreground)
    Listen {
        #[arg(long)]
        port: Option<u16>,
    },
    /// Poll GitHub for configured mentions and dispatch authorized work
    Mentions {
        #[arg(long)]
        once: bool,
        #[arg(long)]
        interval: Option<u64>,
    },
    /// Register this runner with a gateway
    Connect {
        gateway: String,
        runner: String,
        #[arg(long)]
        bootstrap_secret: Option<String>,
    },
    /// Run as a connected runner (poll gateway for jobs)
    Runner {
        #[arg(long)]
        once: bool,
    },
    /// Manually launch an agent for an issue
    Test {
        repo: String,
        issue: u64,
        #[arg(long)]
        prompt: Option<String>,
    },
    /// Launch an agent with a custom prompt (no issue)
    Run {
        repo: String,
        #[arg(long)]
        prompt: String,
    },
}

#[derive(Subcommand)]
enum RepoAction {
    /// Add a repo
    Add { repo: String },
    /// List repos
    List,
    /// Remove a repo
    Remove { name: String },
    /// Sync labels for a repo
    Sync { name: String },
}

#[derive(Subcommand)]
enum SecretAction {
    /// Rotate webhook secret
    Rotate,
}

fn main() -> ExitCode {
    use tracing_subscriber::EnvFilter;
    let filter =
        EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info,sqlx=warn"));
    tracing_subscriber::fmt()
        .pretty()
        .with_env_filter(filter)
        .init();
    let cli = Cli::parse();
    let result = match cli.command {
        Commands::Init { force } => cli::init(force),
        Commands::Repo { action } => match action {
            RepoAction::Add { repo } => cli::repo_add(&repo),
            RepoAction::List => cli::repo_list(),
            RepoAction::Remove { name } => cli::repo_remove(&name),
            RepoAction::Sync { name } => cli::repo_sync(&name),
        },
        Commands::Start { foreground } => cli::start(foreground),
        Commands::Stop => cli::stop(),
        Commands::Restart => cli::restart(),
        Commands::Status => cli::status(),
        Commands::Secret { action } => match action {
            SecretAction::Rotate => cli::secret_rotate(),
        },
        Commands::Logs { follow, lines } => cli::logs(follow, lines),
        Commands::Doctor => cli::doctor(),
        Commands::Listen { port } => cli::listen(port),
        Commands::Mentions { once, interval } => cli::mentions(once, interval),
        Commands::Connect {
            gateway,
            runner,
            bootstrap_secret,
        } => cli::connect(&gateway, &runner, bootstrap_secret.as_deref()),
        Commands::Runner { once } => cli::runner(once),
        Commands::Test {
            repo,
            issue,
            prompt,
        } => cli::test(&repo, issue, prompt),
        Commands::Run { repo, prompt } => cli::run_prompt(&repo, &prompt),
    };
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("error: {e}");
            ExitCode::FAILURE
        }
    }
}
