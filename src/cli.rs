use clap::{Parser, Subcommand};

use crate::constants::DEFAULT_USAGE_DAYS;

#[derive(Parser)]
#[command(
    name = "stepfun",
    about = "Monitor StepFun API usage and credits from the terminal",
    version,
    long_about = "Fetch StepFun platform data (balance, subscription credit, model usage) and display it as terminal tables. Authenticate once with your account and password."
)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Commands,
}

#[derive(Subcommand)]
pub enum Commands {
    /// Show everything: balance, subscription, credit, and recent usage
    Status {
        /// Show extra detail (credit buckets, per-record usage, unknown fields)
        #[arg(short, long)]
        verbose: bool,
        /// Output as JSON instead of tables
        #[arg(long)]
        json: bool,
    },

    /// Show account balance and spend
    Balance {
        /// Output as JSON instead of a table
        #[arg(long)]
        json: bool,
    },

    /// Show subscription plan and credit allowance
    Credit {
        /// Show extra detail (credit buckets, unknown fields)
        #[arg(short, long)]
        verbose: bool,
        /// Output as JSON instead of tables
        #[arg(long)]
        json: bool,
    },

    /// Show model usage for a period (defaults to the last 7 days)
    Usage {
        /// Number of days to look back
        #[arg(short, long, default_value_t = DEFAULT_USAGE_DAYS)]
        days: i64,
        /// Start date for a custom range (YYYY-MM-DD). Requires --end.
        #[arg(long)]
        start: Option<String>,
        /// End date for a custom range (YYYY-MM-DD). Requires --start.
        #[arg(long)]
        end: Option<String>,
        /// List every usage record, not just the per-model totals
        #[arg(short, long)]
        verbose: bool,
        /// Output as JSON instead of tables
        #[arg(long)]
        json: bool,
    },

    /// Log in with your StepFun account and password
    Login {
        /// Account (phone or email). If omitted, prompted for.
        #[arg(short, long)]
        username: Option<String>,
        /// Password. If omitted, prompted for without echo.
        #[arg(short, long)]
        password: Option<String>,
    },

    /// Log out and clear stored credentials
    Logout,

    /// Generate a shell completion script
    #[command(name = "completions")]
    Completions {
        /// Shell to generate completions for
        #[arg(value_enum)]
        shell: clap_complete::Shell,
    },
}
