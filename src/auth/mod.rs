use std::time::Duration;

use inquire::{InquireError, Password, Text};

use crate::Result;
use crate::api::{ApiClient, cookie_from_headers, read_parts, truncate};
use crate::constants::{
    ACCOUNT_BASE, AUTH_TIMEOUT_SECS, BROWSER_USER_AGENT, CONNECT_PROTOCOL_VERSION, M_REFRESH_TOKEN,
    M_REGISTER_DEVICE, M_SIGN_IN_BY_PASSWORD, OASIS_APP_ID, OASIS_PLATFORM, OASIS_TOKEN_COOKIE,
    OASIS_TOKEN_HEADER, PASSPORT_PATH_PREFIX, PLATFORM_BASE, REFRESH_MARGIN_SECS,
    TOKEN_HALF_SEPARATOR,
};
use crate::data::now_secs;
use crate::error::StepFunError;
use crate::types::{DeviceResponse, RefreshTokenResponse, SignInRequest, session_claims};

pub mod storage;

/// Credentials kept on disk after a successful login.
///
/// No password is ever stored: the token's device half renews the session on
/// its own for 30 days (see [`AuthManager::refresh_token`]).
#[derive(Debug, Clone, Default)]
pub struct Credentials {
    pub username: String,
    /// The whole `Oasis-Token` cookie value — both JWT halves joined by
    /// [`TOKEN_HALF_SEPARATOR`].
    pub token: String,
    pub webid: String,
}

/// Orchestrates the password login flow and credential management.
///
/// The flow mirrors the web app: register an anonymous device, sign in with
/// the account password carrying that device's cookie, then keep the returned
/// token together with the device id it is bound to.
pub struct AuthManager {
    agent: ureq::Agent,
    base_url: String,
    api_base_url: String,
}

impl Default for AuthManager {
    fn default() -> Self {
        Self::new()
    }
}

impl AuthManager {
    pub fn new() -> Self {
        Self::with_base_urls(ACCOUNT_BASE, PLATFORM_BASE)
    }

    /// Aim the login flow and the token validation at another origin. Used by
    /// tests to point both at a local mock server.
    pub fn with_base_urls(account_base: &str, api_base: &str) -> Self {
        let config = ureq::Agent::config_builder()
            .timeout_global(Some(Duration::from_secs(AUTH_TIMEOUT_SECS)))
            .https_only(!account_base.starts_with("http://"))
            .http_status_as_error(false)
            .build();
        Self {
            agent: config.new_agent(),
            base_url: account_base.trim_end_matches('/').to_string(),
            api_base_url: api_base.trim_end_matches('/').to_string(),
        }
    }

    /// Run the login flow without touching disk: register a device, sign in,
    /// and check that the resulting token is accepted.
    pub fn authenticate(&self, username: &str, password: &str) -> Result<Credentials> {
        let (device_id, anonymous_token) = self.register_device()?;
        let token = self.sign_in(username, password, &device_id, &anonymous_token)?;

        // Verify the credentials work before handing them back.
        let api = ApiClient::with_base_url(&self.api_base_url, &token, &device_id);
        if !api.validate() {
            return Err(StepFunError::LoginFailed(
                "the server accepted the login but rejected the resulting token".to_string(),
            ));
        }

        Ok(Credentials {
            username: username.to_string(),
            token,
            webid: device_id,
        })
    }

    /// Ask for the account and password interactively, then log in.
    pub fn login_interactive(&self) -> Result<Credentials> {
        let username = Text::new("Account (phone or email):")
            .prompt()
            .map_err(map_inquire_error)?;
        let password = Password::new("Password:")
            .without_confirmation()
            .prompt()
            .map_err(map_inquire_error)?;

        let username = username.trim().to_string();
        if username.is_empty() || password.is_empty() {
            return Err(StepFunError::LoginFailed(
                "account and password must not be empty".to_string(),
            ));
        }

        login_step("logging in");
        let credentials = self.authenticate(&username, &password)?;
        storage::store(&credentials)?;
        Ok(credentials)
    }

    /// Register an anonymous device, returning `(device_id, anonymous_token)`.
    pub fn register_device(&self) -> Result<(String, String)> {
        let response = self
            .agent
            .post(&format!(
                "{}{}{}",
                self.base_url, PASSPORT_PATH_PREFIX, M_REGISTER_DEVICE
            ))
            .header("content-type", "application/json")
            .header("user-agent", BROWSER_USER_AGENT)
            .header("oasis-appid", OASIS_APP_ID)
            .header("oasis-platform", OASIS_PLATFORM)
            .header("connect-protocol-version", CONNECT_PROTOCOL_VERSION)
            .header("origin", ACCOUNT_BASE)
            .header("referer", format!("{}/login", ACCOUNT_BASE))
            .send(b"{}".as_slice())
            .map_err(StepFunError::from)?;

        let (status, headers, text) = read_parts(response)?;
        if !(200..300).contains(&status) {
            return Err(StepFunError::LoginFailed(format!(
                "device registration returned HTTP {} — {}",
                status,
                error_message(&text)
            )));
        }

        let parsed: DeviceResponse = serde_json::from_str(&text).map_err(|e| {
            StepFunError::Parse(format!(
                "failed to parse device registration response: {} — body: {}",
                e,
                truncate(&text, 200)
            ))
        })?;
        let device_id = parsed
            .device
            .and_then(|d| d.device_id)
            .filter(|id| !id.is_empty())
            .ok_or_else(|| {
                StepFunError::Parse(format!(
                    "device registration returned no deviceID: {}",
                    truncate(&text, 200)
                ))
            })?;
        let anonymous_token =
            cookie_from_headers(&headers, OASIS_TOKEN_COOKIE).ok_or_else(|| {
                StepFunError::Parse(
                    "device registration returned no Oasis-Token cookie".to_string(),
                )
            })?;

        Ok((device_id, anonymous_token))
    }

    /// Sign in with a password, returning the resulting token.
    pub fn sign_in(
        &self,
        username: &str,
        password: &str,
        device_id: &str,
        anonymous_token: &str,
    ) -> Result<String> {
        let body = serde_json::to_vec(&SignInRequest {
            username: username.to_string(),
            password: password.to_string(),
        })
        .map_err(|e| StepFunError::Parse(format!("failed to encode sign-in body: {}", e)))?;

        let response = self
            .agent
            .post(&format!(
                "{}{}{}",
                self.base_url, PASSPORT_PATH_PREFIX, M_SIGN_IN_BY_PASSWORD
            ))
            .header("content-type", "application/json")
            .header("user-agent", BROWSER_USER_AGENT)
            .header("oasis-appid", OASIS_APP_ID)
            .header("oasis-platform", OASIS_PLATFORM)
            .header("connect-protocol-version", CONNECT_PROTOCOL_VERSION)
            .header("oasis-webid", device_id)
            .header("origin", ACCOUNT_BASE)
            .header("referer", format!("{}/login", ACCOUNT_BASE))
            .header(
                "cookie",
                &format!("{}={}", OASIS_TOKEN_COOKIE, anonymous_token),
            )
            .send(body.as_slice())
            .map_err(StepFunError::from)?;

        let (status, headers, text) = read_parts(response)?;
        if !(200..300).contains(&status) {
            return Err(StepFunError::LoginFailed(format!(
                "HTTP {} — {}",
                status,
                error_message(&text)
            )));
        }

        // The login token lives in the `oasis-token` response header; fall back
        // to the refreshed cookie.
        let token = headers
            .get(OASIS_TOKEN_HEADER)
            .and_then(|v| v.to_str().ok())
            .map(str::trim)
            .filter(|t| !t.is_empty())
            .map(str::to_string)
            .or_else(|| cookie_from_headers(&headers, OASIS_TOKEN_COOKIE))
            .ok_or_else(|| {
                StepFunError::Parse(format!(
                    "sign-in response carried no token — body: {}",
                    truncate(&text, 300)
                ))
            })?;

        Ok(token)
    }

    /// Exchange the current token for a fresh one, keeping the same device.
    ///
    /// The session half of an Oasis token lives 30 minutes and the device half
    /// 30 days; `RefreshToken` renews the former from the latter, with no
    /// password involved. The web app calls it on a timer and again whenever a
    /// request comes back `TOKEN_EXPIRED` — see `docs/protocol.md`.
    pub fn refresh_token(&self, token: &str, webid: &str) -> Result<String> {
        let response = self
            .agent
            .post(&format!(
                "{}{}{}",
                self.base_url, PASSPORT_PATH_PREFIX, M_REFRESH_TOKEN
            ))
            .header("content-type", "application/json")
            .header("user-agent", BROWSER_USER_AGENT)
            .header("oasis-appid", OASIS_APP_ID)
            .header("oasis-platform", OASIS_PLATFORM)
            .header("connect-protocol-version", CONNECT_PROTOCOL_VERSION)
            .header("oasis-webid", webid)
            .header("origin", ACCOUNT_BASE)
            .header("referer", format!("{}/login", ACCOUNT_BASE))
            .header("cookie", &format!("{}={}", OASIS_TOKEN_COOKIE, token))
            .send(b"{}".as_slice())
            .map_err(StepFunError::from)?;

        let (status, _headers, text) = read_parts(response)?;
        if !(200..300).contains(&status) {
            return Err(StepFunError::LoginFailed(format!(
                "HTTP {} — {}",
                status,
                error_message(&text)
            )));
        }

        let parsed: RefreshTokenResponse = serde_json::from_str(&text).map_err(|e| {
            StepFunError::Parse(format!(
                "failed to parse token refresh response: {} — body: {}",
                e,
                truncate(&text, 200)
            ))
        })?;
        let session = non_empty_half(parsed.access_token.map(|t| t.raw), "session", &text)?;
        let device = non_empty_half(parsed.refresh_token.map(|t| t.raw), "device", &text)?;
        let renewed = format!(
            "{}{}{}",
            session.trim(),
            TOKEN_HALF_SEPARATOR,
            device.trim()
        );

        // A device half that has lapsed does not come back as an error: the
        // server quietly issues an anonymous session instead. Refuse to save
        // that as though it were still the user's.
        if !same_account(token, &renewed) {
            return Err(StepFunError::TokenExpired);
        }
        Ok(renewed)
    }

    /// Forget the stored credentials.
    pub fn logout() -> Result<()> {
        storage::clear()
    }
}

/// Log in and persist the credentials.
pub fn login(username: &str, password: &str) -> Result<Credentials> {
    let credentials = AuthManager::new().authenticate(username, password)?;
    storage::store(&credentials)?;
    Ok(credentials)
}

/// Renew the stored token and save the result.
pub fn refresh() -> Result<()> {
    let credentials = credentials()?;
    let token = AuthManager::new().refresh_token(&credentials.token, &credentials.webid)?;
    storage::store(&Credentials {
        token,
        ..credentials
    })
}

/// Whether the stored session is close enough to expiring to be worth renewing
/// before the command runs.
///
/// Tokens whose claims cannot be read are left to the reactive path, which is
/// the only one that can tell whether they still work.
pub fn needs_refresh(credentials: &Credentials) -> bool {
    session_claims(&credentials.token)
        .and_then(|claims| claims.exp)
        .is_some_and(|exp| exp - now_secs() <= REFRESH_MARGIN_SECS)
}

/// Run a command, renewing the session first when it is about to lapse and
/// again when a call comes back expired, retrying the command once.
///
/// The closures keep the retry policy testable without touching the real
/// platform or the credential file.
pub fn with_auto_refresh<N, C, R>(
    mut needs_refresh: N,
    mut command: C,
    mut refresh: R,
) -> Result<()>
where
    N: FnMut() -> Result<bool>,
    C: FnMut() -> Result<()>,
    R: FnMut() -> Result<()>,
{
    // Renewing up front keeps the common case free of failed requests. A
    // failure here is not fatal on its own — the token may still be good — so
    // it falls through to the reactive path, which reports it properly.
    if needs_refresh().unwrap_or(false) {
        let _ = refresh();
    }
    match command() {
        Err(StepFunError::TokenExpired) => {
            refresh()?;
            command()
        }
        other => other,
    }
}

/// Load the stored credentials, or explain how to obtain them.
pub fn credentials() -> Result<Credentials> {
    storage::load()
}

/// Pull the human-readable `message` out of a Connect error body, falling back
/// to the raw text. Sign-in failures are JSON, and the useful part is buried in
/// it — `{"code":"invalid_argument","message":"password is wrong",...}`.
fn error_message(text: &str) -> String {
    if let Ok(value) = serde_json::from_str::<serde_json::Value>(text)
        && let Some(message) = value.get("message").and_then(|m| m.as_str())
        && !message.is_empty()
    {
        return message.to_string();
    }
    truncate(text, 300).to_string()
}

/// Whether a renewed token still belongs to the account the old one did.
///
/// Tokens whose claims cannot be read are let through: there is nothing to
/// compare, and the server stays the authority on whether they work.
fn same_account(before: &str, after: &str) -> bool {
    match (session_claims(before), session_claims(after)) {
        (Some(before), Some(after)) => before.oasis_id == after.oasis_id,
        _ => true,
    }
}

/// Unwrap one half of a refreshed token, naming it in the error if absent.
fn non_empty_half(half: Option<String>, which: &str, body: &str) -> Result<String> {
    half.filter(|raw| !raw.trim().is_empty()).ok_or_else(|| {
        StepFunError::Parse(format!(
            "token refresh returned no {} half — body: {}",
            which,
            truncate(body, 200)
        ))
    })
}

/// Turn an `inquire` failure into something a user can act on.
pub fn map_inquire_error(e: InquireError) -> StepFunError {
    match e {
        InquireError::OperationCanceled | InquireError::OperationInterrupted => {
            StepFunError::Canceled
        }
        other => StepFunError::Prompt(other.to_string()),
    }
}

/// Send a status line for the login flow to stderr.
pub fn login_step(message: &str) {
    eprintln!("→ {}", message);
}

/// Report a successful login, including where the credentials were written.
pub fn login_success(credentials: &Credentials) {
    println!(
        "✓ Logged in as {} (device {})",
        crate::data::mask_username(&credentials.username),
        crate::data::mask_webid(&credentials.webid)
    );
    if let Some(path) = storage::path() {
        println!("  Credentials saved to {}", path);
    }
    println!("  No password stored — the token renews itself for 30 days.");
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::base64url_encode;

    fn expired() -> StepFunError {
        StepFunError::TokenExpired
    }

    /// A token in the platform's shape whose session half carries these claims.
    fn token_with(exp: Option<i64>, oasis_id: Option<i64>) -> String {
        let payload = base64url_encode(
            format!(
                r#"{{"exp":{},"oasis_id":{}}}"#,
                exp.map(|e| e.to_string()).unwrap_or_else(|| "null".into()),
                oasis_id
                    .map(|o| o.to_string())
                    .unwrap_or_else(|| "null".into()),
            )
            .as_bytes(),
        );
        format!("e30.{payload}.e30...e30.device.e30")
    }

    fn storing(token: &str) -> Credentials {
        Credentials {
            username: "13812348888".into(),
            token: token.to_string(),
            webid: "device-1".into(),
        }
    }

    #[test]
    fn needs_refresh_once_the_session_is_near_or_past_expiry() {
        let now = now_secs();
        assert!(needs_refresh(&storing(&token_with(
            Some(now + 60),
            Some(42)
        ))));
        assert!(needs_refresh(&storing(&token_with(
            Some(now - 60),
            Some(42)
        ))));

        let healthy = token_with(Some(now + REFRESH_MARGIN_SECS + 3600), Some(42));
        assert!(!needs_refresh(&storing(&healthy)));
    }

    /// Nothing to act on means no refresh: the reactive path is the only one
    /// that can tell whether such a token still works.
    #[test]
    fn a_token_without_readable_claims_is_left_to_the_reactive_path() {
        assert!(!needs_refresh(&storing("not-a-token")));
        assert!(!needs_refresh(&storing("e30.e30.e30")));
        assert!(!needs_refresh(&storing(&token_with(None, None))));
    }

    #[test]
    fn a_refresh_that_switches_account_is_refused() {
        let mine = token_with(Some(1789914879), Some(378765055100170240));
        let anonymous = token_with(Some(1789914879), Some(412970297706733568));
        assert!(same_account(&mine, &mine));
        assert!(!same_account(&mine, &anonymous));
        // No claims on either side: nothing to compare, so nothing to refuse.
        assert!(same_account("garbage", "also-garbage"));
    }

    #[test]
    fn a_session_near_expiry_is_renewed_before_the_command_runs() {
        let mut attempts = 0;
        let mut refreshes = 0;
        let result = with_auto_refresh(
            || Ok(true),
            || {
                attempts += 1;
                Ok(())
            },
            || {
                refreshes += 1;
                Ok(())
            },
        );
        assert!(result.is_ok());
        assert_eq!(refreshes, 1, "the session is renewed up front");
        assert_eq!(
            attempts, 1,
            "and the command runs once, with no failed call"
        );
    }

    /// The up-front renewal is a convenience, not a gate: if it fails the token
    /// may still be good, so the command runs and the reactive path decides.
    #[test]
    fn a_failed_up_front_refresh_falls_through_to_the_command() {
        let mut attempts = 0;
        let result = with_auto_refresh(
            || Ok(true),
            || {
                attempts += 1;
                Ok(())
            },
            || Err(StepFunError::LoginFailed("offline".into())),
        );
        assert!(result.is_ok());
        assert_eq!(attempts, 1);
    }

    #[test]
    fn a_session_that_is_not_near_expiry_is_left_alone() {
        let mut refreshes = 0;
        let result: Result<()> = with_auto_refresh(
            || Ok(false),
            || Ok(()),
            || {
                refreshes += 1;
                Ok(())
            },
        );
        assert!(result.is_ok());
        assert_eq!(refreshes, 0);
    }

    /// Not being logged in is not a reason to renew anything.
    #[test]
    fn credentials_that_cannot_be_loaded_skip_the_up_front_refresh() {
        let mut refreshes = 0;
        let result: Result<()> = with_auto_refresh(
            || Err(StepFunError::NotAuthenticated),
            || Ok(()),
            || {
                refreshes += 1;
                Ok(())
            },
        );
        assert!(result.is_ok());
        assert_eq!(refreshes, 0);
    }

    #[test]
    fn an_expired_token_is_refreshed_and_the_command_retried_once() {
        let mut attempts = 0;
        let mut refreshes = 0;
        let result = with_auto_refresh(
            || Ok(false),
            || {
                attempts += 1;
                if attempts == 1 {
                    Err(expired())
                } else {
                    Ok(())
                }
            },
            || {
                refreshes += 1;
                Ok(())
            },
        );
        assert!(result.is_ok());
        assert_eq!(attempts, 2, "the command must run again after a refresh");
        assert_eq!(refreshes, 1, "a refresh must happen exactly once");
    }

    #[test]
    fn a_failed_refresh_surfaces_without_retrying() {
        let mut attempts = 0;
        let result: Result<()> = with_auto_refresh(
            || Ok(false),
            || {
                attempts += 1;
                Err(expired())
            },
            || Err(StepFunError::LoginFailed("session is gone".into())),
        );
        assert!(matches!(result, Err(StepFunError::LoginFailed(_))));
        assert_eq!(attempts, 1, "a failed refresh must not trigger a retry");
    }

    #[test]
    fn a_second_expiry_is_not_retried_again() {
        let mut attempts = 0;
        let result: Result<()> = with_auto_refresh(
            || Ok(false),
            || {
                attempts += 1;
                Err(expired())
            },
            || Ok(()),
        );
        assert!(matches!(result, Err(StepFunError::TokenExpired)));
        assert_eq!(attempts, 2, "only one retry is allowed");
    }

    #[test]
    fn other_errors_do_not_trigger_a_refresh() {
        let mut refreshes = 0;
        let result: Result<()> = with_auto_refresh(
            || Ok(false),
            || Err(StepFunError::Parse("bad json".into())),
            || {
                refreshes += 1;
                Ok(())
            },
        );
        assert!(result.is_err());
        assert_eq!(refreshes, 0);
    }
}
