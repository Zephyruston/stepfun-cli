// ── Endpoints ────────────────────────────────────────────────────────────

/// Account center (passport login).
pub const ACCOUNT_BASE: &str = "https://account.stepfun.com";
/// Open platform (business APIs).
pub const PLATFORM_BASE: &str = "https://platform.stepfun.com";
/// Connect-style RPC prefix for business APIs.
pub const API_PATH_PREFIX: &str = "/api/step.openapi.devcenter.Dashboard/";
/// Connect-style RPC prefix for passport APIs.
pub const PASSPORT_PATH_PREFIX: &str = "/passport/proto.api.passport.v1.PassportService/";
/// Web page the browser uses for the credit card, sent as `referer`.
pub const ACCOUNT_OVERVIEW_PATH: &str = "/account-overview";

// ── Business API methods ─────────────────────────────────────────────────

pub const M_QUERY_ACCOUNT_BALANCE: &str = "QueryAccountBalance";
pub const M_GET_STEP_PLAN_STATUS: &str = "GetStepPlanStatus";
pub const M_QUERY_STEP_PLAN_RATE_LIMIT: &str = "QueryStepPlanRateLimit";
pub const M_QUERY_STEP_PLAN_USAGES: &str = "QueryStepPlanUsages";

// ── Passport API methods ─────────────────────────────────────────────────

pub const M_REGISTER_DEVICE: &str = "RegisterDevice";
pub const M_SIGN_IN_BY_PASSWORD: &str = "SignInByPassword";

// ── Headers ──────────────────────────────────────────────────────────────

/// Browser-like User-Agent. The platform sits behind a WAF that rejects `ureq/*`.
pub const BROWSER_USER_AGENT: &str =
    "Mozilla/5.0 (X11; Linux x86_64; rv:156.0) Gecko/20100101 Firefox/156.0";
/// `oasis-appid` value used by the web app.
pub const OASIS_APP_ID: &str = "10300";
pub const OASIS_PLATFORM: &str = "web";
pub const CONNECT_PROTOCOL_VERSION: &str = "1";
/// Name of the cookie carrying the (dual) Oasis token.
pub const OASIS_TOKEN_COOKIE: &str = "Oasis-Token";
/// Response header carrying the login token on `SignInByPassword`.
pub const OASIS_TOKEN_HEADER: &str = "oasis-token";

// ── Tunables ─────────────────────────────────────────────────────────────

pub const API_TIMEOUT_SECS: u64 = 15;
pub const AUTH_TIMEOUT_SECS: u64 = 35;

/// Page size used when querying model usages. The endpoint is paginated.
pub const USAGE_PAGE_SIZE: u32 = 50;
/// Granularity (hours) of the usage records requested.
pub const USAGE_GRANULAR_HOUR: u32 = 1;
/// Default window of `stepfun usage`.
pub const DEFAULT_USAGE_DAYS: i64 = 7;
/// Window of the usage summary shown by `stepfun status`.
pub const STATUS_USAGE_DAYS: i64 = 7;

/// Number of digits kept in the masked username.
pub const USERNAME_KEEP_HEAD: usize = 3;
pub const USERNAME_KEEP_TAIL: usize = 4;
