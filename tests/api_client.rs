//! Tests of the business API client against a mock platform.

mod common;

use common::{MockServer, Response};

use stepfun_cli::api::ApiClient;
use stepfun_cli::error::StepFunError;
use stepfun_cli::types::Str;

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

/// The shape the platform added alongside the token-count columns on the usage
/// page: every record now carries its prompt, completion, and cached tokens.
#[test]
fn usage_records_carry_token_counts() {
    let server = MockServer::start(vec![Response::ok(
        r#"{"total":"2","records":[
             {"from_time":"1760000000000","to_time":"1760003600000","model_id":"step-5-preview",
              "model_type":1,"calls":12,"credit_consumed":"1234567","in_amount":"12345678",
              "out_amount":"24680","cache_amount":"11223344","image_count":"0"},
             {"from_time":"1760007200000","to_time":"1760010800000","model_id":"dr-search-api",
              "model_type":11,"calls":5,"credit_consumed":"12345"}
           ]}"#,
    )]);
    let api = ApiClient::with_base_url(&server.url, "user.jwt", "device-9");

    let usages = api.query_step_plan_usages(1_000_000, 2_000_000).unwrap();

    let records = usages.records.unwrap();
    assert_eq!(records.len(), 2);
    assert_eq!(
        records[0].in_amount.as_ref().and_then(Str::as_u64),
        Some(12345678)
    );
    assert_eq!(
        records[0].out_amount.as_ref().and_then(Str::as_u64),
        Some(24680)
    );
    assert_eq!(
        records[0].cache_amount.as_ref().and_then(Str::as_u64),
        Some(11223344)
    );
    assert_eq!(records[0].model_type.as_ref().map(Str::as_str), Some("1"));
    // Deep-search records report no token counts at all.
    assert!(records[1].in_amount.is_none());
    assert!(records[1].cache_amount.is_none());
    assert_eq!(records[1].model_type.as_ref().map(Str::as_str), Some("11"));

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
