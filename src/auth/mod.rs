use std::time::Duration;

use inquire::{InquireError, Password, Text};

use crate::Result;
use crate::api::{ApiClient, cookie_from_headers, read_parts, truncate};
use crate::constants::{
    ACCOUNT_BASE, AUTH_TIMEOUT_SECS, BROWSER_USER_AGENT, CONNECT_PROTOCOL_VERSION,
    M_REGISTER_DEVICE, M_SIGN_IN_BY_PASSWORD, OASIS_APP_ID, OASIS_PLATFORM, OASIS_TOKEN_COOKIE,
    OASIS_TOKEN_HEADER, PASSPORT_PATH_PREFIX, PLATFORM_BASE,
};
use crate::error::StepFunError;
use crate::types::{DeviceResponse, SignInRequest};

pub mod storage;

/// Credentials kept on disk after a successful login.
#[derive(Debug, Clone, Default)]
pub struct Credentials {
    pub username: String,
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

    /// Log in with an account and password, storing the resulting credentials.
    pub fn login(&self, username: &str, password: &str) -> Result<Credentials> {
        let credentials = self.authenticate(username, password)?;
        storage::store(&credentials)?;
        Ok(credentials)
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
        self.login(&username, &password)
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
                truncate(&text, 200)
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
                truncate(&text, 300)
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

    /// Forget the stored credentials.
    pub fn logout() -> Result<()> {
        storage::clear()
    }
}

/// Load the stored credentials, or explain how to obtain them.
pub fn credentials() -> Result<Credentials> {
    storage::load()
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
}
