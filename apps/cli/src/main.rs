use clap::{Parser, Subcommand};
use cli::cli;
use std::process::ExitCode;

#[derive(Parser)]
#[command(
    name = env!("CARGO_BIN_NAME"),
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
    Start,
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
    /// List providers
    Providers,
    /// List models (optionally by provider)
    Models { provider: Option<String> },
    /// Get/set default config
    Config {
        #[command(subcommand)]
        action: ConfigAction,
    },
    /// Run the webhook listener (foreground)
    Listen {
        #[arg(long)]
        port: Option<u16>,
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
    /// Cancel a running or queued execution
    Cancel {
        /// Execution ID (preferred)
        execution_id: Option<String>,
        #[arg(long)]
        repo: Option<String>,
        #[arg(long)]
        issue: Option<u64>,
        #[arg(long)]
        pr: Option<u64>,
        #[arg(long)]
        force: bool,
    },
}

#[derive(Subcommand)]
enum RepoAction {
    /// Add a repo (defaults to the current directory)
    Add { repo: Option<String> },
    /// List repos
    List,
    /// Remove a repo
    Remove { name: String },
    /// Sync labels for a repo
    Sync { name: String },
    /// Check whether repos are correctly configured
    Check { name: Option<String> },
}

#[derive(Subcommand)]
enum SecretAction {
    /// Rotate webhook secret
    Rotate,
}

#[derive(Subcommand)]
enum ConfigAction {
    /// Get current defaults
    Get,
    /// Set a default value
    Set { key: String, value: String, #[arg(short = 'g', long)] global: bool },
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
            RepoAction::Add { repo } => cli::repo_add(repo.as_deref().unwrap_or(".")),
            RepoAction::List => cli::repo_list(),
            RepoAction::Remove { name } => cli::repo_remove(&name),
            RepoAction::Sync { name } => cli::repo_sync(&name),
            RepoAction::Check { name } => cli::repo_check(name.as_deref()),
        },
        Commands::Start => cli::start(),
        Commands::Stop => cli::stop(),
        Commands::Restart => cli::restart(),
        Commands::Status => cli::status(),
        Commands::Secret { action } => match action {
            SecretAction::Rotate => cli::secret_rotate(),
        },
        Commands::Logs { follow, lines } => cli::logs(follow, lines),
        Commands::Doctor => cli::doctor(),
        Commands::Listen { port } => cli::listen(port),
        Commands::Connect {
            gateway,
            runner,
            bootstrap_secret,
        } => cli::connect(&gateway, &runner, bootstrap_secret.as_deref()),
        Commands::Runner { once } => {
            let rt = tokio::runtime::Runtime::new().unwrap();
            rt.block_on(cli::runner(once))
        }
        Commands::Test {
            repo,
            issue,
            prompt,
        } => cli::test(&repo, issue, prompt),
        Commands::Run { repo, prompt } => cli::run_prompt(&repo, &prompt),
        Commands::Providers => cli::providers(),
        Commands::Models { provider } => cli::models(provider.as_deref()),
        Commands::Config { action } => match action {
            ConfigAction::Get => cli::config_get(),
            ConfigAction::Set { key, value, global } => cli::config_set(&key, &value, global),
        },
        Commands::Cancel {
            execution_id,
            repo,
            issue,
            pr,
            force,
        } => cli::cancel(execution_id.as_deref(), repo.as_deref(), issue, pr, force),
    };
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("error: {e}");
            ExitCode::FAILURE
        }
    }
}
