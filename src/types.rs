use std::fmt;

use serde::de::{self, Deserializer, Visitor};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

/// A JSON scalar that may arrive as a string, a number, or a boolean.
///
/// proto3 encodes 64-bit integers as JSON strings and doubles as JSON numbers,
/// and the StepFun endpoints are inconsistent about which one they use, so
/// every numeric field is read through this type and converted on demand.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(transparent)]
pub struct Str(pub String);

impl Str {
    pub fn as_f64(&self) -> Option<f64> {
        self.0.trim().parse().ok()
    }

    pub fn as_i64(&self) -> Option<i64> {
        self.0.trim().parse().ok()
    }

    pub fn as_u64(&self) -> Option<u64> {
        self.0.trim().parse().ok()
    }

    pub fn as_str(&self) -> &str {
        self.0.as_str()
    }

    pub fn is_empty(&self) -> bool {
        self.0.trim().is_empty()
    }
}

impl fmt::Display for Str {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl<'de> Deserialize<'de> for Str {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        struct StrVisitor;

        impl Visitor<'_> for StrVisitor {
            type Value = Str;

            fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str("a string, number, or boolean")
            }

            fn visit_str<E: de::Error>(self, v: &str) -> Result<Self::Value, E> {
                Ok(Str(v.to_owned()))
            }

            fn visit_string<E: de::Error>(self, v: String) -> Result<Self::Value, E> {
                Ok(Str(v))
            }

            fn visit_i64<E: de::Error>(self, v: i64) -> Result<Self::Value, E> {
                Ok(Str(v.to_string()))
            }

            fn visit_u64<E: de::Error>(self, v: u64) -> Result<Self::Value, E> {
                Ok(Str(v.to_string()))
            }

            fn visit_f64<E: de::Error>(self, v: f64) -> Result<Self::Value, E> {
                Ok(Str(format!("{v}")))
            }

            fn visit_bool<E: de::Error>(self, v: bool) -> Result<Self::Value, E> {
                Ok(Str(v.to_string()))
            }

            fn visit_unit<E: de::Error>(self) -> Result<Self::Value, E> {
                Ok(Str(String::new()))
            }
        }

        deserializer.deserialize_any(StrVisitor)
    }
}

/// Timestamps arrive either as unix seconds or unix milliseconds.
pub fn to_epoch_secs(value: &Str) -> Option<i64> {
    let raw = value.as_i64()?;
    Some(if raw > 100_000_000_000 {
        raw / 1000
    } else {
        raw
    })
}

// ── QueryAccountBalance ──────────────────────────────────────────────────

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct AccountBalance {
    /// 元 with two implied decimals: `"1500"` is 15.00.
    pub balance: Option<Str>,
    pub payment: Option<Str>,
    pub voucher: Option<Str>,
    /// Part of the voucher usable for API calls.
    pub voucher_api: Option<Str>,
    pub credit: Option<Str>,
    pub cost_yesterday: Option<Str>,
    pub cost_month: Option<Str>,
    pub cost_total: Option<Str>,
    pub notify_threshold: Option<Str>,
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}

// ── GetStepPlanStatus ────────────────────────────────────────────────────

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct StepPlanStatus {
    pub subscription: Option<Subscription>,
    pub plan_definition: Option<PlanDefinition>,
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}

/// Commercial details of the plan. `price` and `original_price` are in 分.
#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct PlanDefinition {
    pub price: Option<Str>,
    pub original_price: Option<Str>,
    pub duration_days: Option<u32>,
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct Subscription {
    pub plan_type: Option<Str>,
    pub name: Option<Str>,
    /// Unix seconds.
    pub expired_at: Option<Str>,
    pub auto_renew: Option<bool>,
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}

// ── QueryStepPlanRateLimit ───────────────────────────────────────────────

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct StepPlanRateLimit {
    pub plan_credit_rate_limit: Option<PlanCreditRateLimit>,
    /// Fraction of the 5-hour quota left; `0` when the plan has none.
    pub five_hour_usage_left_rate: Option<Str>,
    /// Unix seconds of the next 5-hour reset; `0` when the plan has none.
    pub five_hour_usage_reset_time: Option<Str>,
    /// Fraction of the weekly quota left; `0` when the plan has none.
    pub weekly_usage_left_rate: Option<Str>,
    /// Unix seconds of the next weekly reset; `0` when the plan has none.
    pub weekly_usage_reset_time: Option<Str>,
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct PlanCreditRateLimit {
    /// Fraction of the subscription credit left, e.g. `"0.85"`.
    pub subscription_credit_left_rate: Option<Str>,
    /// Fraction of the topped-up credit left.
    pub topup_credit_left_rate: Option<Str>,
    /// Unix seconds of the next subscription credit reset.
    pub subscription_credit_reset_time: Option<Str>,
    pub credit_buckets: Option<Vec<CreditBucket>>,
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct CreditBucket {
    pub credit_total: Option<Str>,
    pub credit_residual: Option<Str>,
    /// Unix seconds.
    pub next_reset_at: Option<Str>,
    pub expire_at: Option<Str>,
}

// ── QueryStepPlanUsages ──────────────────────────────────────────────────

/// Request body of `QueryStepPlanUsages`. Times are unix milliseconds.
#[derive(Debug, Clone, Serialize)]
pub struct UsageQuery {
    #[serde(rename = "startTime")]
    pub start_time: String,
    #[serde(rename = "toTime")]
    pub to_time: String,
    pub page: u32,
    #[serde(rename = "pageSize")]
    pub page_size: u32,
    #[serde(rename = "granularHour")]
    pub granular_hour: u32,
}

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct StepPlanUsages {
    pub records: Option<Vec<UsageRecord>>,
    pub total: Option<Str>,
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct UsageRecord {
    /// Unix seconds.
    pub from_time: Option<Str>,
    pub to_time: Option<Str>,
    pub model_id: Option<Str>,
    pub calls: Option<Str>,
    pub credit_consumed: Option<Str>,
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}

// ── RegisterDevice / SignInByPassword ────────────────────────────────────

#[derive(Debug, Clone, Deserialize)]
pub struct DeviceResponse {
    pub device: Option<Device>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Device {
    #[serde(rename = "deviceID")]
    pub device_id: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct SignInRequest {
    pub username: String,
    pub password: String,
}

// ── RefreshToken ──────────────────────────────────────────────────────────

/// Response of `RefreshToken`: both halves of the renewed token.
#[derive(Debug, Clone, Deserialize)]
pub struct RefreshTokenResponse {
    /// The 30-minute session half.
    #[serde(rename = "accessToken", alias = "access_token")]
    pub access_token: Option<Token>,
    /// The 30-day device half, which is what makes the refresh possible.
    #[serde(rename = "refreshToken", alias = "refresh_token")]
    pub refresh_token: Option<Token>,
}

/// One half of the Oasis token, as the platform reports it.
#[derive(Debug, Clone, Deserialize)]
pub struct Token {
    pub raw: String,
    /// Lifetime of the session half in seconds (`1800`).
    pub duration: Option<i64>,
}

// ── Claims read out of a token ────────────────────────────────────────────

/// The two claims the CLI needs from a token's session half.
///
/// The signature is never verified — only the server can do that — so these
/// serve local decisions only: whether the session is about to lapse, and
/// whether a refresh still belongs to the same account.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct SessionClaims {
    /// Unix seconds at which the session half stops being accepted.
    pub exp: Option<i64>,
    /// Account the session belongs to.
    #[serde(rename = "oasis_id")]
    pub oasis_id: Option<i64>,
}

/// Read the claims of the session (first) half of an Oasis token.
///
/// Returns `None` when the value is not the two-JWT shape the platform issues,
/// which leaves every caller on its safest path.
pub fn session_claims(token: &str) -> Option<SessionClaims> {
    let payload = token.split('.').nth(1)?;
    serde_json::from_slice(&base64url_decode(payload)?).ok()
}

/// Decode one unpadded base64url segment, as JWTs use.
fn base64url_decode(segment: &str) -> Option<Vec<u8>> {
    let mut out = Vec::with_capacity(segment.len() * 3 / 4);
    let mut buffer = 0u32;
    let mut bits = 0u32;
    for byte in segment.bytes() {
        let value = match byte {
            b'A'..=b'Z' => byte - b'A',
            b'a'..=b'z' => byte - b'a' + 26,
            b'0'..=b'9' => byte - b'0' + 52,
            b'-' => 62,
            b'_' => 63,
            _ => return None,
        };
        buffer = (buffer << 6) | u32::from(value);
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push((buffer >> bits) as u8);
        }
    }
    Some(out)
}

/// Encode to unpadded base64url — the inverse of [`base64url_decode`], so
/// tests can build token-shaped fixtures without shipping an encoder.
#[cfg(test)]
pub(crate) fn base64url_encode(bytes: &[u8]) -> String {
    const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";
    let mut out = String::new();
    for chunk in bytes.chunks(3) {
        let mut buffer = 0u32;
        for (i, byte) in chunk.iter().enumerate() {
            buffer |= u32::from(*byte) << (16 - 8 * i);
        }
        for i in 0..chunk.len() + 1 {
            out.push(ALPHABET[(buffer >> (18 - 6 * i)) as usize & 63] as char);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn json(s: &str) -> Value {
        serde_json::from_str(s).unwrap()
    }

    #[test]
    fn balance_accepts_string_and_number_fields() {
        let body = json(
            r#"{"balance":"123.45","payment":100,"voucher":"23.45","credit":0,
                "cost_yesterday":"1.2","cost_month":"45.6","cost_total":678.9,
                "notify_threshold":"10"}"#,
        );
        let parsed: AccountBalance = serde_json::from_value(body).unwrap();
        assert_eq!(parsed.balance.as_ref().and_then(Str::as_f64), Some(123.45));
        assert_eq!(parsed.payment.as_ref().and_then(Str::as_f64), Some(100.0));
        assert_eq!(parsed.credit.as_ref().and_then(Str::as_f64), Some(0.0));
        assert_eq!(
            parsed.cost_total.as_ref().and_then(Str::as_f64),
            Some(678.9)
        );
        assert_eq!(
            parsed.notify_threshold.as_ref().and_then(Str::as_f64),
            Some(10.0)
        );
    }

    #[test]
    fn balance_survives_missing_fields() {
        let parsed: AccountBalance = serde_json::from_value(json(r#"{"balance":"1"}"#)).unwrap();
        assert!(parsed.payment.is_none());
        assert!(parsed.voucher.is_none());
        assert!(parsed.notify_threshold.is_none());
    }

    #[test]
    fn rate_limit_parses_rates_and_buckets() {
        let body = json(
            r#"{"plan_credit_rate_limit":{
                 "subscription_credit_left_rate":"0.425",
                 "topup_credit_left_rate":1,
                 "subscription_credit_reset_time":"1790160000",
                 "credit_buckets":[
                   {"credit_total":"1000","credit_residual":"425","next_reset_at":"1790160000","expire_at":"1800000000"}
                 ]},
               "five_hour_usage":"whatever"}"#,
        );
        let parsed: StepPlanRateLimit = serde_json::from_value(body).unwrap();
        let rl = parsed.plan_credit_rate_limit.unwrap();
        assert_eq!(
            rl.subscription_credit_left_rate
                .as_ref()
                .and_then(Str::as_f64),
            Some(0.425)
        );
        assert_eq!(
            rl.topup_credit_left_rate.as_ref().and_then(Str::as_f64),
            Some(1.0)
        );
        assert_eq!(
            rl.subscription_credit_reset_time
                .as_ref()
                .and_then(to_epoch_secs),
            Some(1790160000)
        );
        let buckets = rl.credit_buckets.unwrap();
        assert_eq!(buckets.len(), 1);
        assert_eq!(
            buckets[0].credit_residual.as_ref().and_then(Str::as_f64),
            Some(425.0)
        );
        // Unknown top-level fields are kept rather than dropped.
        assert!(parsed.extra.contains_key("five_hour_usage"));
    }

    #[test]
    fn subscription_parses_plan_and_expiry() {
        let parsed: StepPlanStatus = serde_json::from_value(json(
            r#"{"subscription":{"plan_type":2,"name":"Step Plan","expired_at":"1790160000","auto_renew":true}}"#,
        ))
        .unwrap();
        let sub = parsed.subscription.unwrap();
        assert_eq!(sub.name.as_ref().map(Str::as_str), Some("Step Plan"));
        assert_eq!(sub.plan_type.as_ref().map(Str::as_str), Some("2"));
        assert_eq!(
            sub.expired_at.as_ref().and_then(to_epoch_secs),
            Some(1790160000)
        );
        assert_eq!(sub.auto_renew, Some(true));
    }

    #[test]
    fn usages_parse_records_with_mixed_types() {
        let parsed: StepPlanUsages = serde_json::from_value(json(
            r#"{"total":"128","records":[
                 {"from_time":"1758000000","to_time":"1758003600","model_id":"step-3","calls":"12","credit_consumed":0.5},
                 {"from_time":"1758003600","to_time":"1758007200","model_id":"step-3","calls":3,"credit_consumed":"1.25"}
               ]}"#,
        ))
        .unwrap();
        let records = parsed.records.unwrap();
        assert_eq!(records.len(), 2);
        assert_eq!(parsed.total.as_ref().and_then(Str::as_u64), Some(128));
        assert_eq!(records[0].calls.as_ref().and_then(Str::as_u64), Some(12));
        assert_eq!(
            records[1].credit_consumed.as_ref().and_then(Str::as_f64),
            Some(1.25)
        );
        assert_eq!(
            records[0].model_id.as_ref().map(Str::as_str),
            Some("step-3")
        );
    }

    #[test]
    fn to_epoch_secs_handles_millis() {
        assert_eq!(to_epoch_secs(&Str("1758000000".into())), Some(1758000000));
        assert_eq!(
            to_epoch_secs(&Str("1758000000000".into())),
            Some(1758000000)
        );
        assert_eq!(to_epoch_secs(&Str("0".into())), Some(0));
        assert_eq!(to_epoch_secs(&Str("".into())), None);
        assert_eq!(to_epoch_secs(&Str("nope".into())), None);
    }

    #[test]
    fn device_response_reads_device_id() {
        let parsed: DeviceResponse = serde_json::from_value(json(
            r#"{"device":{"deviceID":"abc-123","deviceType":"web"}}"#,
        ))
        .unwrap();
        assert_eq!(parsed.device.unwrap().device_id.as_deref(), Some("abc-123"));
    }

    #[test]
    fn usage_query_serializes_with_connect_field_names() {
        let q = UsageQuery {
            start_time: "1".into(),
            to_time: "2".into(),
            page: 1,
            page_size: 50,
            granular_hour: 1,
        };
        let value: Value = serde_json::to_value(&q).unwrap();
        assert_eq!(value["startTime"], "1");
        assert_eq!(value["toTime"], "2");
        assert_eq!(value["pageSize"], 50);
        assert_eq!(value["granularHour"], 1);
    }

    #[test]
    fn refresh_token_response_reads_both_halves() {
        let parsed: RefreshTokenResponse = serde_json::from_value(json(
            r#"{"accessToken":{"raw":"session.jwt","duration":1800,"mode":2},
                "refreshToken":{"raw":"device.jwt"}}"#,
        ))
        .unwrap();
        let access = parsed.access_token.unwrap();
        assert_eq!(access.raw, "session.jwt");
        assert_eq!(access.duration, Some(1800));
        assert_eq!(parsed.refresh_token.unwrap().raw, "device.jwt");
    }

    /// A cookie as the platform issues it: session half, two empty segments,
    /// device half. The claims are read from the session half only.
    #[test]
    fn session_claims_reads_the_session_half_of_a_real_shaped_token() {
        // Payload of a live session half, shortened: exp and oasis_id are the
        // fields the CLI acts on.
        let payload = base64url_encode(
            br#"{"activated":true,"exp":1789914879,"mode":2,"oasis_id":378765055100170240}"#,
        );
        let token = format!("e30.{payload}.e30...e30.again.e30");

        let claims = session_claims(&token).unwrap();
        assert_eq!(claims.exp, Some(1789914879));
        assert_eq!(claims.oasis_id, Some(378765055100170240));
    }

    #[test]
    fn session_claims_rejects_anything_that_is_not_a_token() {
        assert!(session_claims("").is_none());
        assert!(session_claims("nodots").is_none());
        assert!(session_claims("header.!!!not-base64!!!.sig").is_none());
        assert!(session_claims("header.not-json.sig").is_none());
    }

    /// A payload that parses but carries neither claim leaves the caller with
    /// nothing to act on, which every caller treats as "do not refresh".
    #[test]
    fn session_claims_of_an_empty_payload_carry_nothing() {
        let claims = session_claims("e30.e30.e30").unwrap();
        assert_eq!(claims.exp, None);
        assert_eq!(claims.oasis_id, None);
    }

    #[test]
    fn base64url_decode_round_trips_through_the_encoder() {
        for raw in [
            &b"{}"[..],
            b"{\"exp\":1789914879}",
            "余额".as_bytes(),
            &[0u8, 1, 2, 253, 254, 255],
        ] {
            assert_eq!(
                base64url_decode(&base64url_encode(raw)).as_deref(),
                Some(raw)
            );
        }
    }
}
