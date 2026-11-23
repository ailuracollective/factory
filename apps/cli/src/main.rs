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
        #[arg(short = 'g', long)]
        global: bool,
    },
    /// Manage repos
    Repo {
        #[command(subcommand)]
        action: RepoAction,
    },
    /// Manage secret
    Secret {
        #[command(subcommand)]
        action: SecretAction,
    },
    /// Run diagnostics
    Doctor,
    /// List harnesses installed in T3 Code
    Harnesses,
    /// List models of a harness
    Models { harness: String },
    /// Get/set default config
    Config {
        #[command(subcommand)]
        action: ConfigAction,
    },
    /// Start the daemon (deprecated: use `daemon start`)
    #[command(hide = true)]
    Start,
    /// Stop the daemon (deprecated: use `daemon stop`)
    #[command(hide = true)]
    Stop,
    /// Restart the daemon (deprecated: use `daemon restart`)
    #[command(hide = true)]
    Restart,
    /// Show status (deprecated: use `daemon status`)
    #[command(hide = true)]
    Status {
        #[arg(long)]
        json: bool,
    },
    /// Show daemon logs (deprecated: use `daemon logs`)
    #[command(hide = true)]
    Logs {
        #[arg(short, long)]
        follow: bool,
        #[arg(short = 'n', long, default_value = "50")]
        lines: u32,
    },
    /// Run the webhook listener (deprecated: use `gateway listen`)
    #[command(hide = true)]
    Listen {
        #[arg(long)]
        port: Option<u16>,
    },
    /// Register this runner (deprecated: use `gateway connect`)
    #[command(hide = true)]
    Connect {
        gateway: String,
        runner: String,
        #[arg(long)]
        bootstrap_secret: Option<String>,
    },
    /// Run as connected runner (deprecated: use `gateway runner`)
    #[command(hide = true)]
    Runner {
        #[arg(long)]
        once: bool,
    },
    /// Launch agent for issue (deprecated: use `agent run`)
    #[command(hide = true)]
    Test {
        repo: String,
        issue: u64,
        #[arg(long)]
        prompt: Option<String>,
    },
    /// Launch agent with prompt (deprecated: use `agent prompt`)
    #[command(hide = true)]
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
    List {
        #[arg(long)]
        json: bool,
    },
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
    Get {
        #[arg(long)]
        json: bool,
    },
    /// Set a default value
    Set {
        key: String,
        value: String,
        #[arg(short = 'g', long)]
        global: bool,
    },
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
        Commands::Init { force, global } => cli::init(force, !global),
        Commands::Repo { action } => match action {
            RepoAction::Add { repo } => cli::repo_add(repo.as_deref().unwrap_or(".")),
            RepoAction::List { json } => cli::repo_list(json),
            RepoAction::Remove { name } => cli::repo_remove(&name),
            RepoAction::Sync { name } => cli::repo_sync(&name),
            RepoAction::Check { name } => cli::repo_check(name.as_deref()),
        },
        Commands::Start => cli::start(),
        Commands::Stop => cli::stop(),
        Commands::Restart => cli::restart(),
        Commands::Status { json } => cli::status(json),
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
        Commands::Harnesses => cli::harnesses(),
        Commands::Models { harness } => cli::models(&harness),
        Commands::Config { action } => match action {
            ConfigAction::Get { json } => cli::config_get(json),
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
