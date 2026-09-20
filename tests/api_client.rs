//! Tests of the business API client against a mock platform.

mod common;

use common::{MockServer, Response};

use stepfun_cli::api::ApiClient;
use stepfun_cli::error::StepFunError;

#[test]
fn business_api_sends_single_field_cookie_and_oasis_headers() {
    let server = MockServer::start(vec![Response::ok(r#"{"balance":"12.34"}"#)]);
    let api = ApiClient::with_base_url(&server.url, "user.jwt", "device-9");

    let balance = api.query_account_balance().unwrap();

    assert_eq!(balance.balance.as_ref().map(|b| b.as_str()), Some("12.34"));
    let request = server.next_request();
    assert!(
        request.contains("POST /api/step.openapi.devcenter.Dashboard/QueryAccountBalance"),
        "{request}"
    );
    // The whole cookie value is one JWT pair; any extra field breaks the server.
    assert!(
        request.contains("cookie: Oasis-Token=user.jwt"),
        "{request}"
    );
    assert!(request.contains("oasis-appid: 10300"), "{request}");
    assert!(request.contains("oasis-platform: web"), "{request}");
    assert!(request.contains("oasis-webid: device-9"), "{request}");

    server.finish();
}

#[test]
fn rejected_token_maps_to_token_expired() {
    let server = MockServer::start(vec![Response {
        status: 401,
        headers: Vec::new(),
        body: "unauthorized".to_string(),
    }]);
    let api = ApiClient::with_base_url(&server.url, "bad", "device-9");

    let error = api.query_account_balance().unwrap_err();

    assert!(matches!(error, StepFunError::TokenExpired), "{error}");

    server.finish();
}

#[test]
fn server_error_keeps_the_status_and_body() {
    let server = MockServer::start(vec![Response {
        status: 500,
        headers: Vec::new(),
        body: "upstream exploded".to_string(),
    }]);
    let api = ApiClient::with_base_url(&server.url, "user.jwt", "device-9");

    let error = api.query_step_plan_rate_limit().unwrap_err();

    match error {
        StepFunError::Api { code, msg } => {
            assert_eq!(code, 500);
            assert!(msg.contains("upstream exploded"), "{msg}");
        }
        other => panic!("expected an API error, got {other}"),
    }

    server.finish();
}

#[test]
fn usage_query_sends_a_millisecond_window() {
    let server = MockServer::start(vec![Response::ok(r#"{"total":"1","records":[]}"#)]);
    let api = ApiClient::with_base_url(&server.url, "user.jwt", "device-9");

    api.query_step_plan_usages(1_000_000, 2_000_000).unwrap();

    let request = server.next_request();
    assert!(request.contains("QueryStepPlanUsages"), "{request}");
    assert!(request.contains(r#""startTime":"1000000""#), "{request}");
    assert!(request.contains(r#""toTime":"2000000""#), "{request}");
    assert!(request.contains(r#""pageSize":50"#), "{request}");
    assert!(request.contains(r#""granularHour":1"#), "{request}");

    server.finish();
}

#[test]
fn unparsable_body_is_reported_with_a_snippet() {
    let server = MockServer::start(vec![Response::ok("<html>blocked by waf</html>")]);
    let api = ApiClient::with_base_url(&server.url, "user.jwt", "device-9");

    let error = api.query_account_balance().unwrap_err();

    match error {
        StepFunError::Parse(msg) => assert!(msg.contains("blocked by waf"), "{msg}"),
        other => panic!("expected a parse error, got {other}"),
    }

    server.finish();
}
