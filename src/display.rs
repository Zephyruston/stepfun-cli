use serde::Serialize;
use tabled::Table;
use tabled::settings::Style;

use crate::data::{
    BalanceView, CreditView, PlanView, StatusView, UsageView, format_amount, format_bucket_kind,
    format_count, format_credits, format_rate, format_ts,
};

/// Print a value as pretty JSON (for scripting / piping).
pub fn print_json<T: Serialize + ?Sized>(value: &T) {
    match serde_json::to_string_pretty(value) {
        Ok(json) => println!("{}", json),
        Err(e) => eprintln!("Error: failed to serialize output: {}", e),
    }
}

// ── status ───────────────────────────────────────────────────────────────

pub fn show_status(view: &StatusView, verbose: bool) {
    println!();
    println!("  StepFun Account · {}", view.username);
    println!();

    let rows = balance_rows(&view.balance, "Balance");
    println!("{}", Table::new(rows).with(Style::rounded()));

    if let Some(threshold) = view.balance.threshold
        && threshold > 0.0
    {
        println!();
        println!("  Low balance alert at {}", format_amount(Some(threshold)));
    }

    // `show_credit` prints the subscription plan alongside the credit table.
    show_credit(&view.credit, verbose);
    show_usage(&view.usage, verbose);

    println!();
    println!("  Updated: {}", view.generated_at);
    println!();
}

fn show_plan(plan: &PlanView) {
    let mut rows = vec![ItemRow::new("Plan", plan_label(plan))];
    if let Some(plan_type) = &plan.plan_type {
        rows.push(ItemRow::stat("Plan type", plan_type.clone()));
    }
    if let Some(since) = plan.since {
        rows.push(ItemRow::stat("Since", format_ts(Some(since))));
    }
    if let Some(expired_at) = plan.expired_at {
        rows.push(ItemRow::stat("Expires", format_ts(Some(expired_at))));
    }
    if let Some(auto_renew) = plan.auto_renew {
        rows.push(ItemRow::stat(
            "Auto renew",
            if auto_renew { "on" } else { "off" }.to_string(),
        ));
    }
    println!("{}", Table::new(rows).with(Style::rounded()));
}

/// `Plus · 99.00 / 30 days`, or just `Plus` when the price is unknown.
fn plan_label(plan: &PlanView) -> String {
    let name = plan.name.clone().unwrap_or_else(|| "-".to_string());
    match (plan.price, plan.duration_days) {
        (Some(price), Some(days)) => {
            format!("{} · {} / {} days", name, format_amount(Some(price)), days)
        }
        (Some(price), None) => format!("{} · {}", name, format_amount(Some(price))),
        _ => name,
    }
}

fn show_plan_opt(plan: &Option<PlanView>) {
    let Some(plan) = plan else { return };
    println!();
    println!("  Subscription");
    println!();
    show_plan(plan);
}

/// The models a plan may call, listed one per line: a table would either wrap
/// them into an unreadable cell or waste a column on a single value.
fn show_support_models(models: &[String]) {
    if models.is_empty() {
        return;
    }
    println!();
    println!("  Supported models");
    println!();
    for model in models {
        println!("    {}", model);
    }
}

// ── balance ──────────────────────────────────────────────────────────────

/// The money rows shared by `status` and `balance`; only the label of the first
/// one differs between the two.
fn balance_rows(view: &BalanceView, total_label: &'static str) -> Vec<ItemRow> {
    vec![
        ItemRow::stat(total_label, format_amount(Some(view.balance))),
        ItemRow::stat("Cash", format_amount(Some(view.cash))),
        ItemRow::stat("Voucher", format_amount(Some(view.voucher))),
        ItemRow::stat("Voucher (API)", format_amount(Some(view.voucher_api))),
        ItemRow::stat("Voucher (Plan)", format_amount(Some(view.voucher_plan))),
        ItemRow::stat("Credit", format_amount(Some(view.credit))),
        ItemRow::stat("Yesterday", format_amount(Some(view.yesterday))),
        ItemRow::stat("This month", format_amount(Some(view.month))),
        ItemRow::stat("Total spend", format_amount(Some(view.total))),
    ]
}

pub fn show_balance(view: &BalanceView, username: &str) {
    println!();
    println!("  StepFun Balance · {}", username);
    println!();
    let rows = balance_rows(view, "Total");
    println!("{}", Table::new(rows).with(Style::rounded()));
    if let Some(threshold) = view.threshold
        && threshold > 0.0
    {
        println!();
        println!("  Low balance alert at {}", format_amount(Some(threshold)));
    }
    println!();
}

// ── credit ───────────────────────────────────────────────────────────────

pub fn show_credit(view: &CreditView, verbose: bool) {
    let has_limit = view.subscription_left_rate.is_some()
        || view.topup_left_rate.is_some()
        || view.reset_at.is_some();
    if !has_limit && view.buckets.is_empty() {
        return;
    }

    show_plan_opt(&view.plan);
    if verbose && let Some(plan) = &view.plan {
        show_support_models(&plan.support_models);
    }

    println!();
    println!("  Credit");
    println!();
    let mut rows = vec![
        ItemRow::stat(
            "Subscription credit",
            format_rate(view.subscription_left_rate),
        ),
        ItemRow::stat("Top-up credit", format_rate(view.topup_left_rate)),
    ];
    if let Some(reset_at) = view.reset_at {
        rows.push(ItemRow::stat("Resets at", format_ts(Some(reset_at))));
    }
    println!("{}", Table::new(rows).with(Style::rounded()));

    if !view.buckets.is_empty() {
        println!();
        println!("  Credit buckets");
        println!();
        let bucket_rows: Vec<BucketRow> = view
            .buckets
            .iter()
            .map(|b| BucketRow {
                kind: format_bucket_kind(b.kind.as_deref()),
                left: format_credits(b.left),
                total: format_credits(b.total),
                used: if b.total > 0.0 {
                    format!("{:.1}%", (1.0 - b.left / b.total) * 100.0)
                } else {
                    "-".to_string()
                },
                next_reset: format_ts(b.next_reset_at),
                expires: format_ts(b.expire_at),
            })
            .collect();
        println!("{}", Table::new(bucket_rows).with(Style::rounded()));
    }

    if verbose && !view.extra.is_empty() {
        println!();
        println!("  Other fields");
        println!();
        let extra_rows: Vec<ItemRow> = view
            .extra
            .iter()
            .map(|(k, v)| ItemRow::new(k.clone(), v.clone()))
            .collect();
        println!("{}", Table::new(extra_rows).with(Style::rounded()));
    }
}

// ── usage ────────────────────────────────────────────────────────────────

pub fn show_usage(view: &UsageView, verbose: bool) {
    println!();
    println!("  Model usage · last {} days", view.days);
    println!();

    if view.rows.is_empty() {
        println!("  No calls in this window.");
        return;
    }

    let rows: Vec<UsageRow> = view
        .rows
        .iter()
        .map(|r| UsageRow {
            model: r.model.clone(),
            calls: format_count(Some(r.calls)),
            input: format_count(Some(r.input_tokens)),
            output: format_count(Some(r.output_tokens)),
            cached: format_count(Some(r.cached_tokens)),
            credit: format_credits(r.credit),
        })
        .collect();
    println!("{}", Table::new(rows).with(Style::rounded()));

    let total_calls: u64 = view.rows.iter().map(|r| r.calls).sum();
    let total_input: u64 = view.rows.iter().map(|r| r.input_tokens).sum();
    let total_output: u64 = view.rows.iter().map(|r| r.output_tokens).sum();
    let total_cached: u64 = view.rows.iter().map(|r| r.cached_tokens).sum();
    let total_credit: f64 = view.rows.iter().map(|r| r.credit).sum();
    println!();
    println!(
        "  {} calls · {} in · {} out · {} cached · {} credits",
        format_count(Some(total_calls)),
        format_count(Some(total_input)),
        format_count(Some(total_output)),
        format_count(Some(total_cached)),
        format_credits(total_credit)
    );

    if let Some(total) = view.total
        && total > view.shown as u64
    {
        println!("  showing {} of {} records (one page)", view.shown, total);
    }

    if verbose && !view.records.is_empty() {
        println!();
        println!("  Records");
        println!();
        let record_rows: Vec<RecordRow> = view
            .records
            .iter()
            .map(|r| RecordRow {
                from: format_ts(r.from),
                to: format_ts(r.to),
                model: r.model.clone(),
                calls: format_count(Some(r.calls)),
                input: format_count(Some(r.input_tokens)),
                output: format_count(Some(r.output_tokens)),
                cached: format_count(Some(r.cached_tokens)),
                credit: format_credits(r.credit),
            })
            .collect();
        println!("{}", Table::new(record_rows).with(Style::rounded()));
    }
    println!();
}

// ── Table row types ──────────────────────────────────────────────────────

#[derive(tabled::Tabled)]
#[tabled(rename_all = "PascalCase")]
struct ItemRow {
    item: String,
    amount: String,
}

impl ItemRow {
    fn new(item: impl Into<String>, amount: String) -> Self {
        Self {
            item: item.into(),
            amount,
        }
    }

    fn stat(item: &'static str, amount: String) -> Self {
        Self {
            item: item.to_string(),
            amount,
        }
    }
}

#[derive(tabled::Tabled)]
#[tabled(rename_all = "PascalCase")]
struct BucketRow {
    kind: String,
    left: String,
    total: String,
    used: String,
    next_reset: String,
    expires: String,
}

#[derive(tabled::Tabled)]
#[tabled(rename_all = "PascalCase")]
struct UsageRow {
    model: String,
    calls: String,
    input: String,
    output: String,
    cached: String,
    credit: String,
}

#[derive(tabled::Tabled)]
#[tabled(rename_all = "PascalCase")]
struct RecordRow {
    from: String,
    to: String,
    model: String,
    calls: String,
    input: String,
    output: String,
    cached: String,
    credit: String,
}
