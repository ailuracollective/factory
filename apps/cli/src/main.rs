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
    Doctor {
        #[arg(long)]
        json: bool,
    },
    /// Get/set default config
    Config {
        #[command(subcommand)]
        action: ConfigAction,
    },
    /// Manage the daemon
    Daemon {
        #[command(subcommand)]
        action: DaemonAction,
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
    /// Manage agents
    Agent {
        #[command(subcommand)]
        action: AgentAction,
    },
    /// Cancel a running or queued execution
    #[command(hide = true)]
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
        #[arg(long)]
        json: bool,
    },
    /// Manage executions
    Execution {
        #[command(subcommand)]
        action: ExecutionAction,
    },
}

#[derive(Subcommand)]
enum AgentAction {
    /// Run an agent for an issue or prompt
    Run {
        #[arg(long)]
        repo: String,
        #[arg(long, group = "source")]
        issue: Option<u64>,
        #[arg(long, group = "source")]
        prompt: Option<String>,
        #[arg(long)]
        profile: Option<String>,
        #[arg(long)]
        json: bool,
    },
    /// Preflight diagnostics for agent execution
    Check {
        #[arg(long)]
        repo: Option<String>,
        #[arg(long)]
        profile: Option<String>,
        #[arg(long)]
        json: bool,
    },
    /// List harnesses installed in T3 Code
    Harnesses {
        #[arg(long)]
        json: bool,
    },
    /// List models of a harness
    Models {
        harness: String,
        #[arg(long)]
        json: bool,
    },
}

#[derive(Subcommand)]
enum ExecutionAction {
    /// List executions
    List {
        #[arg(long)]
        status: Option<String>,
        #[arg(long)]
        repo: Option<String>,
        #[arg(long)]
        issue: Option<u64>,
        #[arg(long)]
        source: Option<String>,
        #[arg(long)]
        since: Option<u64>,
        #[arg(long)]
        until: Option<u64>,
        #[arg(long)]
        cursor: Option<String>,
        #[arg(long, default_value = "20")]
        limit: u64,
        #[arg(long)]
        json: bool,
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
        #[arg(long)]
        json: bool,
    },
    /// Show details of one execution
    Show {
        /// Execution ID
        execution_id: String,
        #[arg(long)]
        json: bool,
    },
}

#[derive(Subcommand)]
enum RepoAction {
    /// Add a repo (defaults to the current directory)
    Add {
        repo: Option<String>,
        #[arg(long)]
        json: bool,
    },
    /// List repos
    List {
        #[arg(long)]
        json: bool,
    },
    /// Remove a repo
    Remove {
        name: String,
        #[arg(long)]
        json: bool,
    },
    /// Sync labels for a repo
    Sync {
        name: String,
        #[arg(long)]
        json: bool,
    },
    /// Check whether repos are correctly configured
    Check {
        name: Option<String>,
        #[arg(long)]
        json: bool,
    },
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

#[derive(Subcommand)]
enum DaemonAction {
    /// Start the daemon
    Start,
    /// Stop the daemon
    Stop,
    /// Restart the daemon
    Restart,
    /// Resume accepting jobs after drain
    Resume {
        #[arg(long)]
        json: bool,
    },
    /// Show daemon status
    Status {
        #[arg(long)]
        json: bool,
        #[arg(long)]
        verbose: bool,
    },
    /// Drain the daemon (stop accepting new work)
    Drain {
        #[arg(long, default_value = "300")]
        timeout: u64,
        #[arg(long)]
        json: bool,
    },
    /// Show daemon logs
    Logs {
        #[arg(short, long)]
        follow: bool,
        #[arg(short = 'n', long, default_value = "50")]
        lines: u32,
    },
    /// Run the daemon in the foreground
    Run {
        #[arg(long, default_value = "auto")]
        mode: String,
        #[arg(long)]
        port: Option<u16>,
    },
    /// Reload daemon configuration
    Reload {
        #[arg(long)]
        config: Option<String>,
        #[arg(long)]
        json: bool,
    },
}

fn main() -> ExitCode {
    use tracing_subscriber::EnvFilter;
    let filter =
        EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("warn,cli=info"));
    tracing_subscriber::fmt().with_env_filter(filter).init();
    let cli = Cli::parse();
    let result = match cli.command {
        Commands::Init { force, global } => cli::init(force, !global),
        Commands::Repo { action } => match action {
            RepoAction::Add { repo, json } => cli::repo_add(repo.as_deref().unwrap_or("."), json),
            RepoAction::List { json } => cli::repo_list(json),
            RepoAction::Remove { name, json } => cli::repo_remove(&name, json),
            RepoAction::Sync { name, json } => cli::repo_sync(&name, json),
            RepoAction::Check { name, json } => cli::repo_check(name.as_deref(), json),
        },
        Commands::Daemon { action } => match action {
            DaemonAction::Start => cli::start(),
            DaemonAction::Stop => cli::stop(),
            DaemonAction::Restart => cli::restart(),
            DaemonAction::Resume { json } => cli::resume(json),
            DaemonAction::Status { json, verbose } => cli::status(json, verbose),
            DaemonAction::Drain { timeout, json } => cli::drain(timeout, json),
            DaemonAction::Logs { follow, lines } => cli::logs(follow, lines),
            DaemonAction::Run { mode, port } => cli::daemon_run(&mode, port),
            DaemonAction::Reload { config, json } => cli::daemon_reload(config.as_deref(), json),
        },
        Commands::Secret { action } => match action {
            SecretAction::Rotate => cli::secret_rotate(),
        },
        Commands::Doctor { json } => cli::doctor(json),
        Commands::Connect {
            gateway,
            runner,
            bootstrap_secret,
        } => cli::connect(&gateway, &runner, bootstrap_secret.as_deref()),
        Commands::Runner { once } => {
            let rt = tokio::runtime::Runtime::new().unwrap();
            rt.block_on(cli::runner(once))
        }
        Commands::Config { action } => match action {
            ConfigAction::Get { json } => cli::config_get(json),
            ConfigAction::Set { key, value, global } => cli::config_set(&key, &value, global),
        },
        Commands::Agent { action } => match action {
            AgentAction::Run {
                repo,
                issue,
                prompt,
                profile,
                json,
            } => cli::agent_run(&repo, issue, prompt.as_deref(), profile.as_deref(), json),
            AgentAction::Check {
                repo,
                profile,
                json,
            } => cli::agent_check(repo.as_deref(), profile.as_deref(), json),
            AgentAction::Harnesses { json } => cli::harnesses(json),
            AgentAction::Models { harness, json } => cli::models(&harness, json),
        },
        Commands::Cancel {
            execution_id,
            repo,
            issue,
            pr,
            force,
            json,
        } => cli::cancel(
            execution_id.as_deref(),
            repo.as_deref(),
            issue,
            pr,
            force,
            json,
        ),
        Commands::Execution { action } => match action {
            ExecutionAction::List {
                status,
                repo,
                issue,
                source,
                since,
                until,
                cursor,
                limit,
                json,
            } => cli::execution_list(
                status.as_deref(),
                repo.as_deref(),
                issue,
                source.as_deref(),
                since,
                until,
                cursor.as_deref(),
                limit,
                json,
            ),
            ExecutionAction::Cancel {
                execution_id,
                repo,
                issue,
                pr,
                force,
                json,
            } => cli::cancel(
                execution_id.as_deref(),
                repo.as_deref(),
                issue,
                pr,
                force,
                json,
            ),
            ExecutionAction::Show { execution_id, json } => {
                cli::execution_show(&execution_id, json)
            }
        },
    };
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("error: {e}");
            ExitCode::FAILURE
        }
    }
}
