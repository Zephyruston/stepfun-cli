use clap::{CommandFactory, Parser};
use clap_complete::generate;

use stepfun_cli::Result;
use stepfun_cli::api::ApiClient;
use stepfun_cli::auth::{self, AuthManager};
use stepfun_cli::cli::{Cli, Commands};
use stepfun_cli::constants::STATUS_USAGE_DAYS;
use stepfun_cli::data;
use stepfun_cli::display;
use stepfun_cli::error::StepFunError;

fn main() {
    let cli = Cli::parse();

    // The session half of the token expires 30 minutes after it is issued, so
    // it is renewed up front when it is about to lapse, and again whenever a
    // call comes back expired — then the command is retried once. Only the
    // commands that actually query the platform need a session to renew.
    let result = if matches!(
        cli.command,
        Commands::Login { .. } | Commands::Logout | Commands::Completions { .. }
    ) {
        run(cli.command)
    } else {
        auth::with_auto_refresh(
            || auth::credentials().map(|c| auth::needs_refresh(&c)),
            || run(cli.command.clone()),
            || auth::refresh().map(|_| ()),
        )
    };

    if let Err(e) = result {
        eprintln!("Error: {}", e);
        std::process::exit(1);
    }
}

fn run(command: Commands) -> Result<()> {
    match command {
        Commands::Status { verbose, json } => status(verbose, json),
        Commands::Balance { json } => balance(json),
        Commands::Credit { verbose, json } => credit(verbose, json),
        Commands::Usage {
            days,
            start,
            end,
            verbose,
            json,
        } => usage(days, start, end, verbose, json),
        Commands::Login { username, password } => login(username, password),
        Commands::Logout => logout(),
        Commands::Completions { shell } => completions(shell),
    }
}

fn status(verbose: bool, json: bool) -> Result<()> {
    let credentials = auth::credentials()?;
    let api = ApiClient::from_credentials(&credentials);

    let window = data::window_from_days(STATUS_USAGE_DAYS);
    let balance = api.query_account_balance()?;
    let plan = api.get_step_plan_status()?;
    let rate_limit = api.query_step_plan_rate_limit()?;
    let usages = api.query_step_plan_usages(window.start_ms, window.end_ms)?;

    let view = data::status_view(&credentials.username, &balance, &plan, &rate_limit, &usages);
    if json {
        display::print_json(&view);
    } else {
        display::show_status(&view, verbose);
    }
    Ok(())
}

fn balance(json: bool) -> Result<()> {
    let credentials = auth::credentials()?;
    let api = ApiClient::from_credentials(&credentials);

    let account = api.query_account_balance()?;
    let view = data::balance_view(&account);
    if json {
        display::print_json(&view);
    } else {
        display::show_balance(&view, &data::mask_username(&credentials.username));
    }
    Ok(())
}

fn credit(verbose: bool, json: bool) -> Result<()> {
    let api = ApiClient::from_credentials(&auth::credentials()?);

    let plan = api.get_step_plan_status()?;
    let rate_limit = api.query_step_plan_rate_limit()?;
    let view = data::credit_view(&rate_limit, data::plan_view(&plan).as_ref());
    if json {
        display::print_json(&view);
    } else {
        display::show_credit(&view, verbose);
    }
    Ok(())
}

fn usage(
    days: i64,
    start: Option<String>,
    end: Option<String>,
    verbose: bool,
    json: bool,
) -> Result<()> {
    let window = match (start, end) {
        (Some(start), Some(end)) => data::window_from_dates(&start, &end)?,
        (None, None) => data::window_from_days(days),
        _ => {
            return Err(StepFunError::InvalidInput(
                "--start and --end must be used together".to_string(),
            ));
        }
    };

    let credentials = auth::credentials()?;
    let api = ApiClient::from_credentials(&credentials);
    let usages = api.query_step_plan_usages(window.start_ms, window.end_ms)?;
    let view = data::usage_view(&usages, &window);
    if json {
        display::print_json(&view);
    } else {
        display::show_usage(&view, verbose);
    }
    Ok(())
}

fn login(username: Option<String>, password: Option<String>) -> Result<()> {
    let auth = AuthManager::new();
    let credentials = match (username, password) {
        (Some(username), Some(password)) => {
            let username = username.trim().to_string();
            if username.is_empty() || password.is_empty() {
                return Err(StepFunError::LoginFailed(
                    "account and password must not be empty".to_string(),
                ));
            }
            stepfun_cli::auth::login(&username, &password)?
        }
        (None, None) => auth.login_interactive()?,
        _ => {
            return Err(StepFunError::InvalidInput(
                "--username and --password must be used together".to_string(),
            ));
        }
    };

    auth::login_success(&credentials);
    Ok(())
}

fn logout() -> Result<()> {
    AuthManager::logout()?;
    println!("Logged out. Credentials cleared.");
    Ok(())
}

fn completions(shell: clap_complete::Shell) -> Result<()> {
    let mut cmd = Cli::command();
    let name = cmd.get_name().to_string();
    generate(shell, &mut cmd, name, &mut std::io::stdout());
    Ok(())
}
