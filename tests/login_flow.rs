//! End-to-end tests of the login flow against a mock platform.

mod common;

use common::{MockServer, Response, balance_response, login_responses};

use stepfun_cli::auth::AuthManager;
use stepfun_cli::error::StepFunError;

/// Full login: register a device, sign in, validate the token, and keep both.
#[test]
fn authenticate_registers_device_signs_in_and_validates() {
    let mut responses = login_responses("user.jwt.device", "device-42");
    // Third call is the balance check that validates the new token.
    responses.push(balance_response("10.00"));
    let server = MockServer::start(responses);
    let auth = AuthManager::with_base_urls(&server.url, &server.url);

    let credentials = auth.authenticate("13812348888", "hunter2").unwrap();

    assert_eq!(credentials.webid, "device-42");
    assert_eq!(credentials.token, "user.jwt.device");

    let device_request = server.next_request();
    assert!(
        device_request
            .starts_with("POST /passport/proto.api.passport.v1.PassportService/RegisterDevice")
    );
    assert!(device_request.contains("oasis-appid: 10300"));
    assert!(device_request.contains("oasis-platform: web"));
    assert!(device_request.contains("connect-protocol-version: 1"));

    let sign_in_request = server.next_request();
    assert!(sign_in_request.contains("SignInByPassword"));
    assert!(sign_in_request.contains("cookie: Oasis-Token=anon.jwt.device"));
    assert!(sign_in_request.contains("oasis-webid: device-42"));
    assert!(sign_in_request.contains(r#""password":"hunter2""#));

    server.finish();
}

/// The anonymous device token must reach the sign-in call as a single cookie.
#[test]
fn sign_in_uses_anonymous_cookie_and_returns_login_token() {
    let server = MockServer::start(login_responses("user.jwt.device", "device-7"));
    let auth = AuthManager::with_base_urls(&server.url, &server.url);

    let (device_id, anonymous_token) = auth.register_device().unwrap();
    assert_eq!(device_id, "device-7");
    assert_eq!(anonymous_token, "anon.jwt.device");

    let token = auth
        .sign_in("user@example.com", "pw", &device_id, &anonymous_token)
        .unwrap();
    assert_eq!(token, "user.jwt.device");

    server.finish();
}

/// A device registration without the cookie cannot be completed.
#[test]
fn register_device_without_cookie_is_an_error() {
    let server = MockServer::start(vec![Response::ok(r#"{"device":{"deviceID":"device-1"}}"#)]);
    let auth = AuthManager::with_base_urls(&server.url, &server.url);

    let error = auth.register_device().unwrap_err();
    assert!(matches!(error, StepFunError::Parse(_)), "{error}");
    assert!(error.to_string().contains("Oasis-Token"));

    server.finish();
}

/// Wrong credentials surface the server's message.
#[test]
fn login_failure_reports_server_message() {
    let server = MockServer::start(vec![
        Response::ok(r#"{"device":{"deviceID":"device-1"}}"#)
            .with_header("set-cookie", "Oasis-Token=anon.jwt.device"),
        Response {
            status: 400,
            headers: vec![("content-type".to_string(), "application/json".to_string())],
            body: r#"{"code":"invalid_argument","message":"wrong password"}"#.to_string(),
        },
    ]);
    let auth = AuthManager::with_base_urls(&server.url, &server.url);

    let error = auth.authenticate("13812348888", "wrong").unwrap_err();
    assert!(matches!(error, StepFunError::LoginFailed(_)), "{error}");
    assert!(error.to_string().contains("wrong password"));

    server.finish();
}

// ── RefreshToken ──────────────────────────────────────────────────────────

const ACCOUNT: &str = r#"{"exp":1789914879,"oasis_id":378765055100170240}"#;
const ANONYMOUS: &str = r#"{"exp":1789914879,"oasis_id":412970297706733568}"#;

/// Renewing a session needs no password: the stored token is exchanged for a
/// fresh pair of halves and reassembled into the cookie shape.
#[test]
fn refresh_token_renews_both_halves_into_the_cookie_shape() {
    let session = common::jwt(ACCOUNT);
    let server = MockServer::start(vec![Response::ok(&format!(
        r#"{{"accessToken":{{"raw":"{session}","duration":1800}},
             "refreshToken":{{"raw":"fresh.device.jwt"}}}}"#
    ))]);
    let auth = AuthManager::with_base_urls(&server.url, &server.url);

    let renewed = auth
        .refresh_token(&common::oasis_token(ACCOUNT), "device-1")
        .unwrap();

    assert_eq!(renewed, format!("{session}...fresh.device.jwt"));

    let request = server.next_request();
    assert!(request.contains("PassportService/RefreshToken"));
    // The current token travels as the single cookie field, and the device id
    // the session is bound to rides along as a header.
    assert!(request.contains("cookie: Oasis-Token="));
    assert!(request.contains("oasis-webid: device-1"));

    server.finish();
}

/// A lapsed device half does not come back as an error: the server quietly
/// issues an anonymous session. Saving that would silently drop the account,
/// so the refresh is refused instead.
#[test]
fn a_refresh_that_comes_back_anonymous_is_refused() {
    let server = MockServer::start(vec![Response::ok(&format!(
        r#"{{"accessToken":{{"raw":"{}","duration":1800}},
             "refreshToken":{{"raw":"anon.device.jwt"}}}}"#,
        common::jwt(ANONYMOUS)
    ))]);
    let auth = AuthManager::with_base_urls(&server.url, &server.url);

    let error = auth
        .refresh_token(&common::oasis_token(ACCOUNT), "device-1")
        .unwrap_err();
    assert!(matches!(error, StepFunError::TokenExpired), "{error}");

    server.finish();
}

/// Half a token cannot be reassembled into a cookie.
#[test]
fn a_refresh_missing_a_half_is_reported() {
    let server = MockServer::start(vec![Response::ok(
        r#"{"accessToken":{"raw":"e30.e30.e30","duration":1800}}"#,
    )]);
    let auth = AuthManager::with_base_urls(&server.url, &server.url);

    let error = auth
        .refresh_token(&common::oasis_token(ACCOUNT), "device-1")
        .unwrap_err();
    assert!(matches!(error, StepFunError::Parse(_)), "{error}");
    assert!(error.to_string().contains("device half"));

    server.finish();
}

#[test]
fn a_failed_refresh_reports_the_server_message() {
    let server = MockServer::start(vec![Response {
        status: 400,
        headers: vec![("content-type".to_string(), "application/json".to_string())],
        body: r#"{"code":"invalid_argument","message":"device is gone"}"#.to_string(),
    }]);
    let auth = AuthManager::with_base_urls(&server.url, &server.url);

    let error = auth
        .refresh_token(&common::oasis_token(ACCOUNT), "device-1")
        .unwrap_err();
    assert!(matches!(error, StepFunError::LoginFailed(_)), "{error}");
    assert!(error.to_string().contains("device is gone"));

    server.finish();
}
