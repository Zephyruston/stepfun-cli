# CLAUDE.md

This file provides guidance to Claude Code (claude.ai) when working with code in this repository.

## Project Overview

`stepfun-cli` is a Rust CLI tool that reports a StepFun (阶跃星辰) open-platform account from the terminal: balance, subscription credit, and per-model API usage. It authenticates with the account password through the same passport endpoints the web app uses, then reads the `platform.stepfun.com` dashboard APIs.

## Build & Test Commands

```bash
cargo build                    # build
cargo build --release          # release build
cargo test                     # unit tests + integration tests (mock HTTP server)
cargo clippy --all-targets --all-features --tests -- -D warnings
cargo fmt
cargo run -- status            # smoke test (requires `cargo run -- login` first)
cargo run -- usage --days 30
```

No async runtime; builds on stable Rust ≥1.85.

## Architecture

### Module layout

- **`main.rs`** — thin entrypoint: clap dispatch and one handler per subcommand. Each handler loads credentials via `auth::credentials()`, builds an `ApiClient` with `ApiClient::from_credentials`, calls the API, builds view models, then hands them to `display`.
- **`cli.rs`** — `clap` `#[derive(Parser)]` enum: `status`, `balance`, `credit`, `usage`, `login`, `logout`, `completions`. Only `usage` takes a date range (`--days`, or `--start`/`--end`).
- **`api.rs`** — `ApiClient` (zero-state) with `query_account_balance`, `get_step_plan_status`, `query_step_plan_rate_limit`, `query_step_plan_usages`. Also `read_parts` and `cookie_from_headers`, shared with `auth`.
- **`auth/`** — `AuthManager` runs the password login flow (`register_device` → `sign_in` → validate → `storage::store`) and the passwordless `refresh_token` renewal; `authenticate` is the login flow without touching disk, so tests can use it. `storage.rs` wraps `confy` at `~/.config/stepfun-cli/credentials.toml` with `0600` permissions.
- **`data.rs`** — pure functions: `UsageWindow` computation (`window_from_days`, `window_from_dates`), formatters, and the view models (`BalanceView`, `PlanView`, `CreditView`, `UsageView`, `StatusView`) built from the API types.
- **`types.rs`** — `serde` structs for the Connect-style responses, plus the `Str` newtype that absorbs string-or-number fields.
- **`display.rs`** — `tabled` rendering; one `show_*` per command plus `print_json`.
- **`constants.rs`** — endpoints, method names, header values, timeouts.
- **`error.rs`** — `StepFunError` enum + `From<ureq::Error>`; `Result<T>` alias exported from `lib.rs`.

### Protocol notes

- **Single-field cookie** — business API calls send `cookie: Oasis-Token=<token>` and nothing else. Adding any second cookie field makes the server fail with 401 (it parses the whole value as one JWT). See `docs/protocol.md`.
- **`oasis-*` headers** — `oasis-appid: 10300`, `oasis-platform: web`, `oasis-webid: <deviceID>` are required on every business call; the webid must match the device id the token is bound to.
- **`http_status_as_error(false)`** — both agents disable it so error bodies can be reported instead of swallowed by `ureq`.
- **Token lifetime is two-tier** — measured live: the session half expires at `create_at + 1800` (30 min) and the device half at `create_at + 2592000` (30 days). The session half is renewed through `PassportService/RefreshToken`, which needs no password and works even once the session half has lapsed; the device half is *not* extended by a refresh, so a re-login is due once a month. A 401 is routine, not a bug — `api.rs` maps it to `StepFunError::TokenExpired`. Note that `RefreshToken` is idempotent until the session half is ~15 minutes old (it echoes the current token, so a refresh right after login changes nothing) — do not read that as a failed refresh. Two earlier claims are wrong and must not be resurrected: "both halves expire in 30 minutes" and "the device half lasts ~300 days". Details in `docs/protocol.md`.
- **Money is in 分, credits are not money** — `QueryAccountBalance` money fields are cents (`"1500"` = 15.00 元) except `notify_threshold`, which is 元; `plan_definition.price` is cents (`"9900"` = 99 元). Credit counts (`credit_consumed`, `credit_total`, `credit_residual`) are plain integers and format via `data::format_credits`.
- **Immutable agent** — `ureq` v3 agents cannot have headers added after construction, so the token/webid are injected per request.
- **Response body reading** — `http::Response<ureq::Body>`: clone the headers first, then `into_body().read_to_string()` (an inherent method taking `&mut self`, not `std::io::Read::read_to_string`).
- **Timestamps** — arrive as unix seconds or milliseconds, in either string or number form; `types::to_epoch_secs` normalizes.

### Key patterns

- **Blocking HTTP via `ureq`** — no async, no `tokio`. Every API call is synchronous.
- **Credentials in config, not env** — `confy` at `~/.config/stepfun-cli/credentials.toml`. Read via `auth::credentials()`; never hardcode a token or read one from an env var.
- **No password on disk, ever** — `storage` persists only `username`, `token`, `webid`. The token is two JWTs joined by `TOKEN_HALF_SEPARATOR`: a 30-minute session half and a 30-day device half. `auth::refresh` trades the stored token for a fresh session half via `PassportService/RefreshToken` — no password involved. Renewal has exactly two trigger points, both inside a command run in `auth::with_auto_refresh`: **before** the command when `auth::needs_refresh` sees `REFRESH_MARGIN_SECS` (5 min) or less of session left, and **after** it when a call comes back `StepFunError::TokenExpired`, in which case the command is retried once. There is no daemon and no timer: a session that expired hours ago is renewed by the next command you run. `auth::same_account` refuses a refresh that comes back anonymous, which is how a lapsed device half presents itself. Once the device half dies (30 days after login), the user logs in again — that is the only time the password is needed.
- **Password prompts** — `inquire::Password` (no echo) and `inquire::Text`; `auth::map_inquire_error` maps cancellation to `StepFunError::Canceled`. There is deliberately no "remember the password" prompt.
- **Readable sign-in errors** — Connect failures return JSON (`{"code":...,"message":"password is wrong",...}`), so `auth::error_message` extracts `message` instead of dumping the body.
- **Undocumented fields** — every response struct has `#[serde(flatten)] extra: Map<String, Value>`, so unknown fields are surfaced under `-v` instead of being dropped. New fields should be added to the structs once the platform documents them.
- **Date range only on `usage`** — `status`, `balance`, and `credit` read snapshot endpoints with no time parameter; only `QueryStepPlanUsages` takes a window.

## Testing

- Unit tests live in `#[cfg(test)]` blocks in `data.rs` and `types.rs` (pure functions and deserialization).
- Integration tests live in `tests/` and share `tests/common/mod.rs`, a one-shot `TcpListener` mock server that records requests verbatim and replies with canned responses. It reads until `content-length` bytes arrive, because `ureq` may split headers and body across packets.
- `AuthManager::with_base_urls` and `ApiClient::with_base_url` point the client at the mock server. Tests call `authenticate`, not `login`, so they never write to the real config directory.

## Pre-commit hooks

On commit: `cargo fmt`, `cargo clippy -D warnings`. On push: `cargo test`, `cargo build --locked`, `cargo check`.

## Dist

`cargo dist` build defined in `dist-workspace.toml`. Built with `--profile dist` (`lto = "thin"`).
