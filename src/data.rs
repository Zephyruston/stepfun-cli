use std::collections::BTreeMap;

use chrono::{DateTime, Local, NaiveDate, TimeZone};
use serde::Serialize;
use serde_json::{Map, Value};

use crate::Result;
use crate::constants::{STATUS_USAGE_DAYS, USERNAME_KEEP_HEAD, USERNAME_KEEP_TAIL};
use crate::error::StepFunError;
use crate::types::{
    AccountBalance, CreditBucket, StepPlanRateLimit, StepPlanStatus, StepPlanUsages, Str,
    UsageRecord, to_epoch_secs,
};

const SECS_PER_DAY: i64 = 86_400;
const MS_PER_SEC: i64 = 1_000;

// ── Time ranges ──────────────────────────────────────────────────────────

/// The window a usage query covers, in the units the API expects.
#[derive(Debug, Clone, Copy)]
pub struct UsageWindow {
    pub start_ms: i64,
    pub end_ms: i64,
    pub days: i64,
}

/// Last `days` days, ending now.
pub fn window_from_days(days: i64) -> UsageWindow {
    let days = days.max(1);
    let now = now_secs();
    UsageWindow {
        start_ms: (now - days * SECS_PER_DAY) * MS_PER_SEC,
        end_ms: now * MS_PER_SEC,
        days,
    }
}

/// Inclusive `start`..=`end` date range, in the local timezone.
pub fn window_from_dates(start: &str, end: &str) -> Result<UsageWindow> {
    let start = parse_date(start)?;
    let end = parse_date(end)?;
    if end < start {
        return Err(StepFunError::InvalidInput(format!(
            "--start ({}) is after --end ({})",
            start, end
        )));
    }
    let days = (end - start).num_days() + 1;
    Ok(UsageWindow {
        start_ms: local_midnight(start) * MS_PER_SEC,
        // Exclusive upper bound: midnight of the day after the end date.
        end_ms: (local_midnight(end) + SECS_PER_DAY) * MS_PER_SEC,
        days,
    })
}

fn parse_date(value: &str) -> Result<NaiveDate> {
    NaiveDate::parse_from_str(value.trim(), "%Y-%m-%d").map_err(|e| {
        StepFunError::InvalidInput(format!("invalid date '{}' (want YYYY-MM-DD): {}", value, e))
    })
}

fn local_midnight(date: NaiveDate) -> i64 {
    let midnight = date.and_hms_opt(0, 0, 0).expect("midnight is always valid");
    Local
        .from_local_datetime(&midnight)
        .earliest()
        .map(|dt| dt.timestamp())
        .unwrap_or_else(|| midnight.and_utc().timestamp())
}

pub fn now_secs() -> i64 {
    Local::now().timestamp()
}

// ── Formatting helpers ───────────────────────────────────────────────────

/// `13812348888` → `138****8888`.
pub fn mask_username(username: &str) -> String {
    let chars: Vec<char> = username.trim().chars().collect();
    if chars.len() <= USERNAME_KEEP_HEAD + USERNAME_KEEP_TAIL {
        return username.trim().to_string();
    }
    let head: String = chars[..USERNAME_KEEP_HEAD].iter().collect();
    let tail: String = chars[chars.len() - USERNAME_KEEP_TAIL..].iter().collect();
    format!("{}****{}", head, tail)
}

/// Keep a device id short while staying recognisable.
pub fn mask_webid(webid: &str) -> String {
    let chars: Vec<char> = webid.trim().chars().collect();
    if chars.len() <= 12 {
        return webid.trim().to_string();
    }
    let head: String = chars[..8].iter().collect();
    format!("{}…", head)
}

/// Thousands-separated amount, `-` when absent.
pub fn format_amount(value: Option<f64>) -> String {
    match value {
        Some(v) => with_thousands(&format!("{:.2}", v)),
        None => "-".to_string(),
    }
}

/// Fraction (`0.425`) as a percentage string (`42.5%`).
pub fn format_rate(value: Option<f64>) -> String {
    match value {
        Some(v) => format!("{:.1}%", v * 100.0),
        None => "-".to_string(),
    }
}

pub fn format_count(value: Option<u64>) -> String {
    match value {
        Some(v) => with_thousands(&v.to_string()),
        None => "-".to_string(),
    }
}

/// Credit is an integer count, not money — `1234567` prints as `1,234,567`,
/// never `4,489,133.00`. Falls back to two decimals if a fractional value ever
/// shows up rather than silently rounding it.
pub fn format_credits(value: f64) -> String {
    if value.fract() == 0.0 {
        with_thousands(&format!("{:.0}", value))
    } else {
        with_thousands(&format!("{:.2}", value))
    }
}

pub fn format_ts(secs: Option<i64>) -> String {
    match secs.and_then(|s| DateTime::from_timestamp(s, 0)) {
        Some(dt) => dt
            .with_timezone(&Local)
            .format("%Y-%m-%d %H:%M:%S")
            .to_string(),
        None => "-".to_string(),
    }
}

fn with_thousands(int_and_frac: &str) -> String {
    let (int, frac) = match int_and_frac.split_once('.') {
        Some((i, f)) => (i, Some(f)),
        None => (int_and_frac, None),
    };
    let negative = int.starts_with('-');
    let digits = int.trim_start_matches('-');
    let mut out = String::new();
    for (i, ch) in digits.chars().enumerate() {
        if i > 0 && (digits.len() - i) % 3 == 0 {
            out.push(',');
        }
        out.push(ch);
    }
    let mut result = String::new();
    if negative {
        result.push('-');
    }
    result.push_str(&out);
    if let Some(f) = frac {
        result.push('.');
        result.push_str(f);
    }
    result
}

fn opt_f64(value: Option<&Str>) -> Option<f64> {
    value.and_then(Str::as_f64)
}

/// Money fields on `QueryAccountBalance` and in `plan_definition` are reported
/// in 分 — `"1500"` is 15.00 元, and the Plus plan's `"9900"` price is 99 元.
/// Verified against the live API: a balance of `"1500"` matched the 15 元 shown
/// on the platform.
fn fen_to_yuan(cents: Option<&Str>) -> f64 {
    opt_f64(cents).unwrap_or(0.0) / 100.0
}

fn opt_fen_to_yuan(cents: Option<&Str>) -> Option<f64> {
    opt_f64(cents).map(|v| v / 100.0)
}

/// `notify_threshold` is the exception in `QueryAccountBalance`: reported in 元
/// even though the money fields beside it are in 分. Verified against the
/// platform UI, where a raw `"2"` shows as a 2 元 alert threshold.
fn yuan(value: Option<&Str>) -> Option<f64> {
    opt_f64(value)
}

fn opt_u64(value: Option<&Str>) -> Option<u64> {
    value.and_then(Str::as_u64)
}

/// Unix seconds, treating `0` as "not set".
fn opt_ts(value: Option<&Str>) -> Option<i64> {
    to_epoch_secs_from(value).filter(|t| *t > 0)
}

fn to_epoch_secs_from(value: Option<&Str>) -> Option<i64> {
    value.and_then(to_epoch_secs)
}

/// Render leftover response fields for verbose output.
fn flatten_extra(map: &Map<String, Value>) -> Vec<(String, String)> {
    let mut out: Vec<(String, String)> = map
        .iter()
        .filter(|(k, v)| !k.is_empty() && !is_blank(v))
        .map(|(k, v)| (k.clone(), scalar_or_compact(v)))
        .collect();
    out.sort_by(|a, b| a.0.cmp(&b.0));
    out
}

/// Combine the unknown fields of two levels of the same response.
fn merge_extra(outer: &Map<String, Value>, inner: &Map<String, Value>) -> Vec<(String, String)> {
    let mut out = flatten_extra(outer);
    for (key, value) in flatten_extra(inner) {
        if !out.iter().any(|(k, _)| *k == key) {
            out.push((key, value));
        }
    }
    out
}

/// Skip fields the server leaves empty, so `-v` is not cluttered with blanks.
fn is_blank(value: &Value) -> bool {
    value.as_str().is_some_and(str::is_empty)
}

fn scalar_or_compact(value: &Value) -> String {
    match value {
        Value::String(s) => s.clone(),
        Value::Null => "-".to_string(),
        other => other.to_string(),
    }
}

// ── View models ──────────────────────────────────────────────────────────

#[derive(Debug, Serialize)]
pub struct BalanceView {
    pub balance: f64,
    pub cash: f64,
    pub voucher: f64,
    /// Part of the voucher usable for API calls, in 元.
    pub voucher_api: f64,
    pub credit: f64,
    pub yesterday: f64,
    pub month: f64,
    pub total: f64,
    pub threshold: Option<f64>,
}

#[derive(Debug, Clone, Serialize)]
pub struct PlanView {
    pub name: Option<String>,
    pub plan_type: Option<String>,
    /// Monthly fee in 元.
    pub price: Option<f64>,
    pub duration_days: Option<u32>,
    pub expired_at: Option<i64>,
    pub auto_renew: Option<bool>,
}

#[derive(Debug, Clone, Serialize)]
pub struct CreditBucketView {
    pub left: f64,
    pub total: f64,
    pub next_reset_at: Option<i64>,
    pub expire_at: Option<i64>,
}

#[derive(Debug, Clone, Serialize)]
pub struct CreditView {
    pub plan: Option<PlanView>,
    pub subscription_left_rate: Option<f64>,
    pub topup_left_rate: Option<f64>,
    pub reset_at: Option<i64>,
    /// Fraction of the 5-hour quota left, absent when the plan has none.
    pub five_hour_left_rate: Option<f64>,
    pub five_hour_reset_at: Option<i64>,
    /// Fraction of the weekly quota left, absent when the plan has none.
    pub weekly_left_rate: Option<f64>,
    pub weekly_reset_at: Option<i64>,
    pub buckets: Vec<CreditBucketView>,
    pub extra: Vec<(String, String)>,
}

#[derive(Debug, Serialize)]
pub struct UsageRow {
    pub model: String,
    pub calls: u64,
    pub credit: f64,
}

#[derive(Debug, Serialize)]
pub struct UsageRecordView {
    pub from: Option<i64>,
    pub to: Option<i64>,
    pub model: String,
    pub calls: u64,
    pub credit: f64,
}

#[derive(Debug, Serialize)]
pub struct UsageView {
    pub days: i64,
    pub total: Option<u64>,
    pub shown: usize,
    pub rows: Vec<UsageRow>,
    pub records: Vec<UsageRecordView>,
}

#[derive(Debug, Serialize)]
pub struct StatusView {
    pub username: String,
    pub balance: BalanceView,
    pub plan: Option<PlanView>,
    pub credit: CreditView,
    pub usage: UsageView,
    pub generated_at: String,
}

// ── Aggregation ──────────────────────────────────────────────────────────

pub fn balance_view(balance: &AccountBalance) -> BalanceView {
    BalanceView {
        balance: fen_to_yuan(balance.balance.as_ref()),
        cash: fen_to_yuan(balance.payment.as_ref()),
        voucher: fen_to_yuan(balance.voucher.as_ref()),
        voucher_api: fen_to_yuan(balance.voucher_api.as_ref()),
        credit: fen_to_yuan(balance.credit.as_ref()),
        yesterday: fen_to_yuan(balance.cost_yesterday.as_ref()),
        month: fen_to_yuan(balance.cost_month.as_ref()),
        total: fen_to_yuan(balance.cost_total.as_ref()),
        threshold: yuan(balance.notify_threshold.as_ref()),
    }
}

pub fn plan_view(status: &StepPlanStatus) -> Option<PlanView> {
    let subscription = status.subscription.as_ref()?;
    let definition = status.plan_definition.as_ref();
    Some(PlanView {
        name: subscription
            .name
            .as_ref()
            .map(Str::as_str)
            .map(str::to_string)
            .filter(|s| !s.is_empty()),
        plan_type: subscription
            .plan_type
            .as_ref()
            .map(Str::as_str)
            .map(str::to_string)
            .filter(|s| !s.is_empty()),
        price: definition.and_then(|d| opt_fen_to_yuan(d.price.as_ref())),
        duration_days: definition.and_then(|d| d.duration_days),
        expired_at: opt_ts(subscription.expired_at.as_ref()),
        auto_renew: subscription.auto_renew,
    })
}

pub fn credit_view(rate_limit: &StepPlanRateLimit, plan: Option<&PlanView>) -> CreditView {
    let Some(limit) = rate_limit.plan_credit_rate_limit.as_ref() else {
        return CreditView {
            plan: plan.cloned(),
            subscription_left_rate: None,
            topup_left_rate: None,
            reset_at: None,
            five_hour_left_rate: quota_rate(rate_limit.five_hour_usage_left_rate.as_ref()),
            five_hour_reset_at: opt_ts(rate_limit.five_hour_usage_reset_time.as_ref()),
            weekly_left_rate: quota_rate(rate_limit.weekly_usage_left_rate.as_ref()),
            weekly_reset_at: opt_ts(rate_limit.weekly_usage_reset_time.as_ref()),
            buckets: Vec::new(),
            extra: flatten_extra(&rate_limit.extra),
        };
    };
    CreditView {
        plan: plan.cloned(),
        subscription_left_rate: opt_f64(limit.subscription_credit_left_rate.as_ref()),
        topup_left_rate: opt_f64(limit.topup_credit_left_rate.as_ref()),
        reset_at: opt_ts(limit.subscription_credit_reset_time.as_ref()),
        five_hour_left_rate: quota_rate(rate_limit.five_hour_usage_left_rate.as_ref()),
        five_hour_reset_at: opt_ts(rate_limit.five_hour_usage_reset_time.as_ref()),
        weekly_left_rate: quota_rate(rate_limit.weekly_usage_left_rate.as_ref()),
        weekly_reset_at: opt_ts(rate_limit.weekly_usage_reset_time.as_ref()),
        buckets: limit
            .credit_buckets
            .as_deref()
            .unwrap_or_default()
            .iter()
            .map(bucket_view)
            .collect(),
        // Both the top level and the rate-limit object carry fields this CLI
        // does not know about yet.
        extra: merge_extra(&rate_limit.extra, &limit.extra),
    }
}

/// A `0` rate means the plan carries no such quota, so it is reported as
/// absent rather than as an exhausted one.
fn quota_rate(value: Option<&Str>) -> Option<f64> {
    opt_f64(value).filter(|rate| *rate > 0.0)
}

fn bucket_view(bucket: &CreditBucket) -> CreditBucketView {
    let total = opt_f64(bucket.credit_total.as_ref()).unwrap_or(0.0);
    CreditBucketView {
        left: opt_f64(bucket.credit_residual.as_ref()).unwrap_or(0.0),
        total,
        next_reset_at: opt_ts(bucket.next_reset_at.as_ref()),
        expire_at: opt_ts(bucket.expire_at.as_ref()),
    }
}

pub fn usage_view(usages: &StepPlanUsages, window: &UsageWindow) -> UsageView {
    let records: Vec<UsageRecordView> = usages
        .records
        .as_deref()
        .unwrap_or_default()
        .iter()
        .map(record_view)
        .collect();

    let mut totals: BTreeMap<String, (u64, f64)> = BTreeMap::new();
    for record in &records {
        let entry = totals.entry(record.model.clone()).or_insert((0, 0.0));
        entry.0 += record.calls;
        entry.1 += record.credit;
    }
    let mut rows: Vec<UsageRow> = totals
        .into_iter()
        .map(|(model, (calls, credit))| UsageRow {
            model,
            calls,
            credit,
        })
        .collect();
    rows.sort_by(|a, b| {
        b.credit
            .partial_cmp(&a.credit)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then(b.calls.cmp(&a.calls))
            .then(a.model.cmp(&b.model))
    });

    UsageView {
        days: window.days,
        total: opt_u64(usages.total.as_ref()),
        shown: records.len(),
        rows,
        records,
    }
}

fn record_view(record: &UsageRecord) -> UsageRecordView {
    UsageRecordView {
        from: to_epoch_secs_from(record.from_time.as_ref()),
        to: to_epoch_secs_from(record.to_time.as_ref()),
        model: record
            .model_id
            .as_ref()
            .map(Str::as_str)
            .unwrap_or("-")
            .to_string(),
        calls: opt_u64(record.calls.as_ref()).unwrap_or(0),
        credit: opt_f64(record.credit_consumed.as_ref()).unwrap_or(0.0),
    }
}

/// Everything `stepfun status` prints: balance, plan, credit, and a recent
/// usage summary.
pub fn status_view(
    username: &str,
    balance: &AccountBalance,
    plan: &StepPlanStatus,
    rate_limit: &StepPlanRateLimit,
    usages: &StepPlanUsages,
) -> StatusView {
    let plan = plan_view(plan);
    let credit = credit_view(rate_limit, plan.as_ref());
    StatusView {
        username: mask_username(username),
        balance: balance_view(balance),
        plan,
        credit,
        usage: usage_view(usages, &window_from_days(STATUS_USAGE_DAYS)),
        generated_at: Local::now().format("%Y-%m-%d %H:%M:%S").to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn str(s: &str) -> Str {
        Str(s.to_string())
    }

    #[test]
    fn window_from_days_covers_whole_span() {
        let w = window_from_days(7);
        assert_eq!(w.days, 7);
        assert!(w.end_ms > w.start_ms);
        assert_eq!((w.end_ms - w.start_ms) / MS_PER_SEC, 7 * SECS_PER_DAY);
    }

    #[test]
    fn window_from_days_clamps_to_at_least_one_day() {
        let w = window_from_days(0);
        assert_eq!(w.days, 1);
    }

    #[test]
    fn window_from_dates_is_inclusive() {
        let w = window_from_dates("2026-09-14", "2026-09-20").unwrap();
        assert_eq!(w.days, 7);
        assert_eq!((w.end_ms - w.start_ms) / MS_PER_SEC, 7 * SECS_PER_DAY);
    }

    #[test]
    fn window_from_dates_rejects_bad_input() {
        // chrono accepts non-padded dates, so use one that is not a date at all.
        assert!(window_from_dates("14/09/2026", "2026-09-20").is_err());
        assert!(window_from_dates("yesterday", "2026-09-20").is_err());
        assert!(window_from_dates("2026-09-20", "2026-09-14").is_err());
    }

    #[test]
    fn mask_username_keeps_head_and_tail() {
        assert_eq!(mask_username("13812348888"), "138****8888");
        assert_eq!(mask_username("ab"), "ab");
        assert_eq!(mask_username("  user@example.com  "), "use****.com");
    }

    #[test]
    fn format_helpers() {
        assert_eq!(format_amount(Some(1234567.5)), "1,234,567.50");
        assert_eq!(format_amount(None), "-");
        assert_eq!(format_rate(Some(0.425)), "42.5%");
        assert_eq!(format_rate(None), "-");
        assert_eq!(format_count(Some(1000)), "1,000");
        assert_eq!(format_ts(None), "-");
        // Rendered in the machine's local timezone, so the expectation is
        // built the same way instead of hardcoding UTC+8 — CI runs in UTC.
        assert_eq!(
            format_ts(Some(0)),
            DateTime::from_timestamp(0, 0)
                .unwrap()
                .with_timezone(&Local)
                .format("%Y-%m-%d %H:%M:%S")
                .to_string()
        );
    }

    #[test]
    fn balance_view_converts_fen_to_yuan() {
        // The API reports money in 分: `"1500"` is 15.00 元.
        let balance = AccountBalance {
            balance: Some(str("1500")),
            voucher: Some(str("1500")),
            voucher_api: Some(str("1500")),
            cost_yesterday: Some(str("0")),
            notify_threshold: Some(str("2")),
            ..Default::default()
        };
        let view = balance_view(&balance);
        assert_eq!(view.balance, 15.0);
        assert_eq!(view.voucher, 15.0);
        assert_eq!(view.voucher_api, 15.0);
        assert_eq!(view.yesterday, 0.0);
        assert_eq!(view.cash, 0.0);
        assert_eq!(view.total, 0.0);
        // Reported in 元, unlike the money fields around it.
        assert_eq!(view.threshold, Some(2.0));
    }

    #[test]
    fn balance_view_normalises_missing_numbers() {
        let balance = AccountBalance {
            balance: Some(str("100.5")),
            cost_yesterday: Some(str("0")),
            ..Default::default()
        };
        let view = balance_view(&balance);
        assert_eq!(view.balance, 1.005);
        assert_eq!(view.cash, 0.0);
        assert_eq!(view.total, 0.0);
        assert!(view.threshold.is_none());
    }

    #[test]
    fn format_credits_never_adds_false_decimals() {
        assert_eq!(format_credits(1234567.0), "1,234,567");
        assert_eq!(format_credits(2000000000.0), "2,000,000,000");
        assert_eq!(format_credits(0.0), "0");
        // A fractional credit is kept rather than rounded away.
        assert_eq!(format_credits(1.5), "1.50");
    }

    #[test]
    fn usage_view_sums_by_model_and_sorts_by_credit() {
        let usages: StepPlanUsages = serde_json::from_str(
            r#"{"total":"3","records":[
                 {"model_id":"step-2","calls":"5","credit_consumed":"1.5"},
                 {"model_id":"step-3","calls":"10","credit_consumed":"0.5"},
                 {"model_id":"step-2","calls":"2","credit_consumed":"0.25"}
               ]}"#,
        )
        .unwrap();
        let view = usage_view(&usages, &window_from_days(7));
        assert_eq!(view.days, 7);
        assert_eq!(view.total, Some(3));
        assert_eq!(view.shown, 3);
        assert_eq!(view.rows.len(), 2);
        assert_eq!(view.rows[0].model, "step-2");
        assert_eq!(view.rows[0].calls, 7);
        assert_eq!(view.rows[0].credit, 1.75);
        assert_eq!(view.rows[1].model, "step-3");
        assert_eq!(view.records[0].model, "step-2");
    }

    #[test]
    fn usage_view_survives_empty_payload() {
        let usages = StepPlanUsages::default();
        let view = usage_view(&usages, &window_from_days(7));
        assert!(view.rows.is_empty());
        assert!(view.records.is_empty());
        assert_eq!(view.shown, 0);
        assert!(view.total.is_none());
    }

    #[test]
    fn credit_view_reads_rates_buckets_and_extras() {
        let rl: StepPlanRateLimit = serde_json::from_str(
            r#"{"plan_credit_rate_limit":{
                 "subscription_credit_left_rate":"0.425",
                 "subscription_credit_reset_time":"1790160000",
                 "credit_buckets":[{"credit_total":"1000","credit_residual":"425","next_reset_at":"1790160000"}],
                 "weekly_credit_rate":"0.9"},
               "five_hour_usage_left_rate":"0.35",
               "five_hour_usage_reset_time":"1790160000",
               "weekly_usage_left_rate":"0",
               "weekly_usage_reset_time":"0"}"#,
        )
        .unwrap();
        let view = credit_view(&rl, None);
        assert_eq!(view.subscription_left_rate, Some(0.425));
        assert_eq!(view.reset_at, Some(1790160000));
        assert_eq!(view.buckets.len(), 1);
        assert_eq!(view.buckets[0].left, 425.0);
        assert_eq!(view.buckets[0].total, 1000.0);
        assert_eq!(view.five_hour_left_rate, Some(0.35));
        assert_eq!(view.five_hour_reset_at, Some(1790160000));
        // A zero rate means "no such quota", not "quota exhausted".
        assert_eq!(view.weekly_left_rate, None);
        assert_eq!(view.weekly_reset_at, None);
        // Unknown fields from both levels are kept for `-v`.
        assert_eq!(
            view.extra,
            vec![("weekly_credit_rate".to_string(), "0.9".to_string())]
        );
    }

    #[test]
    fn plan_view_carries_the_monthly_fee_in_yuan() {
        let plan: StepPlanStatus = serde_json::from_str(
            r#"{"subscription":{"name":"Plus","plan_type":"1","expired_at":"1792486753","auto_renew":false},
                "plan_definition":{"price":"9900","original_price":"12800","duration_days":30}}"#,
        )
        .unwrap();
        let view = plan_view(&plan).unwrap();
        assert_eq!(view.name.as_deref(), Some("Plus"));
        assert_eq!(view.price, Some(99.0));
        assert_eq!(view.duration_days, Some(30));
    }

    #[test]
    fn status_view_combines_all_sections() {
        let balance: AccountBalance = serde_json::from_str(r#"{"balance":"1500"}"#).unwrap();
        let plan: StepPlanStatus = serde_json::from_str(
            r#"{"subscription":{"name":"Step Plan","plan_type":"2","expired_at":"1790160000","auto_renew":false}}"#,
        )
        .unwrap();
        let rl: StepPlanRateLimit = serde_json::from_str(
            r#"{"plan_credit_rate_limit":{"subscription_credit_left_rate":"0.5"}}"#,
        )
        .unwrap();
        let usages: StepPlanUsages = serde_json::from_str(
            r#"{"total":"1","records":[{"model_id":"step-3","calls":"1","credit_consumed":"1"}]}"#,
        )
        .unwrap();

        let view = status_view("13812348888", &balance, &plan, &rl, &usages);
        assert_eq!(view.username, "138****8888");
        assert_eq!(view.balance.balance, 15.0);
        assert_eq!(
            view.plan.as_ref().unwrap().name.as_deref(),
            Some("Step Plan")
        );
        assert_eq!(view.plan.as_ref().unwrap().auto_renew, Some(false));
        assert_eq!(view.credit.subscription_left_rate, Some(0.5));
        assert_eq!(view.usage.days, STATUS_USAGE_DAYS);
        assert_eq!(view.usage.rows.len(), 1);
    }
}
