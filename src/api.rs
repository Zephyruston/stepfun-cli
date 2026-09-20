use std::time::Duration;

use serde::de::DeserializeOwned;
use serde_json::json;
use ureq::http;
use ureq::http::HeaderMap;

use crate::Result;
use crate::auth::Credentials;
use crate::constants::{
    ACCOUNT_OVERVIEW_PATH, API_PATH_PREFIX, API_TIMEOUT_SECS, BROWSER_USER_AGENT,
    M_GET_STEP_PLAN_STATUS, M_QUERY_ACCOUNT_BALANCE, M_QUERY_STEP_PLAN_RATE_LIMIT,
    M_QUERY_STEP_PLAN_USAGES, OASIS_APP_ID, OASIS_PLATFORM, OASIS_TOKEN_COOKIE, PLATFORM_BASE,
    USAGE_GRANULAR_HOUR, USAGE_PAGE_SIZE,
};
use crate::error::StepFunError;
use crate::types::*;

/// Zero-state client for the StepFun open-platform APIs.
///
/// The `Oasis-Token` cookie plus the `oasis-*` headers form the whole
/// authentication set; the token is injected per request because a `ureq`
/// agent is immutable once built.
pub struct ApiClient {
    agent: ureq::Agent,
    base_url: String,
    token: String,
    webid: String,
}

impl ApiClient {
    pub fn new(token: &str, webid: &str) -> Self {
        Self::with_base_url(PLATFORM_BASE, token, webid)
    }

    /// Build a client for credentials that are already loaded.
    pub fn from_credentials(credentials: &Credentials) -> Self {
        Self::new(&credentials.token, &credentials.webid)
    }

    /// Same as [`ApiClient::new`] but aimed at another origin. Used by tests
    /// to point the client at a local mock server.
    pub fn with_base_url(base_url: &str, token: &str, webid: &str) -> Self {
        let config = ureq::Agent::config_builder()
            .timeout_global(Some(Duration::from_secs(API_TIMEOUT_SECS)))
            .https_only(!base_url.starts_with("http://"))
            // We read error bodies ourselves to report the server's message.
            .http_status_as_error(false)
            .build();
        Self {
            agent: config.new_agent(),
            base_url: base_url.trim_end_matches('/').to_string(),
            token: token.to_string(),
            webid: webid.to_string(),
        }
    }

    pub fn query_account_balance(&self) -> Result<AccountBalance> {
        self.call(M_QUERY_ACCOUNT_BALANCE, None)
    }

    pub fn get_step_plan_status(&self) -> Result<StepPlanStatus> {
        self.call(M_GET_STEP_PLAN_STATUS, None)
    }

    pub fn query_step_plan_rate_limit(&self) -> Result<StepPlanRateLimit> {
        self.call(M_QUERY_STEP_PLAN_RATE_LIMIT, None)
    }

    /// Model usage records between two unix-millisecond timestamps (inclusive).
    pub fn query_step_plan_usages(&self, start_ms: i64, end_ms: i64) -> Result<StepPlanUsages> {
        let query = UsageQuery {
            start_time: start_ms.to_string(),
            to_time: end_ms.to_string(),
            page: 1,
            page_size: USAGE_PAGE_SIZE,
            granular_hour: USAGE_GRANULAR_HOUR,
        };
        let payload = serde_json::to_value(&query)
            .map_err(|e| StepFunError::Parse(format!("failed to encode usage query: {}", e)))?;
        self.call(M_QUERY_STEP_PLAN_USAGES, Some(&payload))
    }

    /// Whether the stored credentials still work, checked with one cheap call.
    pub fn validate(&self) -> bool {
        self.query_account_balance().is_ok()
    }

    fn call<T: DeserializeOwned>(
        &self,
        method: &str,
        payload: Option<&serde_json::Value>,
    ) -> Result<T> {
        let url = format!("{}{}{}", self.base_url, API_PATH_PREFIX, method);
        let body = payload.cloned().unwrap_or_else(|| json!({}));
        let bytes = serde_json::to_vec(&body)
            .map_err(|e| StepFunError::Parse(format!("failed to encode request body: {}", e)))?;

        let response = self
            .agent
            .post(&url)
            .header("content-type", "application/json")
            .header("accept", "application/json")
            .header("user-agent", BROWSER_USER_AGENT)
            .header("origin", PLATFORM_BASE)
            .header(
                "referer",
                format!("{}{}", PLATFORM_BASE, ACCOUNT_OVERVIEW_PATH),
            )
            .header("oasis-appid", OASIS_APP_ID)
            .header("oasis-platform", OASIS_PLATFORM)
            .header("oasis-webid", &self.webid)
            // Exactly one cookie field: the server parses the whole value as a
            // single JWT and fails on anything else (see docs/protocol.md).
            .header("cookie", &format!("{}={}", OASIS_TOKEN_COOKIE, self.token))
            .send(bytes.as_slice())
            .map_err(StepFunError::from)?;

        let (status, _headers, text) = read_parts(response)?;

        if matches!(status, 401 | 403) {
            return Err(StepFunError::TokenExpired);
        }
        if !(200..300).contains(&status) {
            return Err(StepFunError::Api {
                code: status,
                msg: truncate(&text, 200).to_string(),
            });
        }

        serde_json::from_str::<T>(&text).map_err(|e| {
            StepFunError::Parse(format!(
                "failed to deserialize {} response: {} — body: {}",
                method,
                e,
                truncate(&text, 200)
            ))
        })
    }
}

/// Extract the value of a cookie from the `Set-Cookie` headers of a response.
pub fn cookie_from_headers(headers: &HeaderMap, name: &str) -> Option<String> {
    headers
        .get_all("set-cookie")
        .iter()
        .filter_map(|raw| raw.to_str().ok())
        .find_map(|entry| {
            let (key, value) = entry.split_once('=')?;
            if !key.trim().eq_ignore_ascii_case(name) {
                return None;
            }
            let value = value.split(';').next().unwrap_or("").trim();
            if value.is_empty() {
                None
            } else {
                Some(value.to_string())
            }
        })
}

/// Read a response in full: status code, headers, and body text.
///
/// The headers have to be cloned before the body is consumed, so both parts
/// are returned together.
pub fn read_parts(response: http::Response<ureq::Body>) -> Result<(u16, HeaderMap, String)> {
    let status = response.status().as_u16();
    let headers = response.headers().clone();
    let text = response
        .into_body()
        .read_to_string()
        .map_err(|e| StepFunError::Parse(format!("failed to read response body: {}", e)))?;
    Ok((status, headers, text))
}

/// Shorten a string to `max` bytes, cutting on a character boundary so a
/// multi-byte error message cannot panic the slice.
pub fn truncate(s: &str, max: usize) -> &str {
    if s.len() <= max {
        return s;
    }
    let mut end = max;
    while !s.is_char_boundary(end) {
        end -= 1;
    }
    &s[..end]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn truncate_cuts_on_a_character_boundary() {
        // 6 CJK characters = 18 bytes; cutting at 4 would split one in half.
        let chinese = "密码错误，请重试";
        let cut = truncate(chinese, 4);
        assert!(chinese.is_char_boundary(cut.len()), "{cut:?}");
        assert_eq!(cut, "密");

        assert_eq!(truncate("short", 200), "short");
        assert_eq!(truncate("abcdef", 6), "abcdef");
        assert_eq!(truncate("abcdef", 3), "abc");
    }
}
