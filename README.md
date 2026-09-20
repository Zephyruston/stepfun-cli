# StepFun CLI

Monitor your [StepFun (阶跃星辰)](https://platform.stepfun.com) open-platform account from the terminal — balance, subscription credit, and per-model API usage. No browser, no runtime dependencies.

<p align="center">
  <img src="https://img.shields.io/badge/rust-1.85%2B-orange?logo=rust" alt="Rust 1.85+">
  <img src="https://img.shields.io/badge/license-MIT-blue" alt="License: MIT">
  <img src="https://img.shields.io/badge/tests-29%20passing-brightgreen" alt="Tests: 29 passing">
</p>

[中文文档](README_zh.md)

---

## Overview

`stepfun-cli` talks to the same endpoints the [platform.stepfun.com](https://platform.stepfun.com) web app uses: log in with your account and password once, and every subsequent command reads the account dashboard APIs.

The interface is not publicly documented, so it was reverse-engineered from the web app and verified against a live account. Everything this CLI knows about it is written down in [docs/protocol.md](docs/protocol.md) — including the quirks that cost time to find.

### Features

- **One command** — `stepfun status` shows balance, subscription, credit allowance, and the last 7 days of model usage
- **Per-topic commands** — `stepfun balance`, `stepfun credit`, `stepfun usage`
- **Date selection on usage only** — `stepfun usage --days 30` or `stepfun usage --start 2026-09-01 --end 2026-09-20`
- **Machine-readable** — `--json` outputs structured data for scripting / piping
- **Verbose mode** — `-v` adds credit buckets, per-record usage rows, and any response fields this CLI does not model yet
- **Single binary** — blocking HTTP via `ureq`, no async runtime, zero runtime dependencies
- **Token only** — your password is never written to disk; only the Oasis token and the device id it is bound to

## Install

Requires Rust ≥ 1.85 to build. The binary itself has no runtime dependencies.

```bash
# From source
git clone https://github.com/Zephyruston/stepfun-cli.git
cd stepfun-cli
cargo install --path . --locked
```

### Shell completions

```bash
# bash
echo 'source <(stepfun completions bash)' >> ~/.bashrc

# zsh
echo 'source <(stepfun completions zsh)' >> ~/.zshrc

# fish
stepfun completions fish > ~/.config/fish/completions/stepfun.fish
```

## Commands

| Command                                             | Description                                           |
| --------------------------------------------------- | ----------------------------------------------------- |
| `stepfun status`                                    | Dashboard: balance + subscription + credit + usage    |
| `stepfun status -v`                                 | Add credit buckets, per-record usage, unmapped fields |
| `stepfun status --json`                             | Dashboard as JSON                                     |
| `stepfun balance`                                   | Balance, cash, voucher, credit, spend                 |
| `stepfun credit`                                    | Plan, credit remaining, reset time, credit buckets    |
| `stepfun usage`                                     | Model usage for the last 7 days                       |
| `stepfun usage --days 30`                           | Model usage for the last 30 days                      |
| `stepfun usage --start 2026-09-01 --end 2026-09-20` | Model usage for a custom date range                   |
| `stepfun usage -v`                                  | List every usage record, not just per-model totals    |
| `stepfun usage --json`                              | Usage as JSON                                         |
| `stepfun login`                                     | Log in (prompts for account and password)             |
| `stepfun login -u <account> -p <password>`          | Log in non-interactively                              |
| `stepfun logout`                                    | Clear stored credentials                              |
| `stepfun completions <SHELL>`                       | Generate a shell completion script                    |

Every data command accepts `--json`; every one except `balance` accepts `-v`.

## Demo

```
$ stepfun status

  StepFun Account · 138****8888

╭─────────────┬────────╮
│ Item        │ Amount │
├─────────────┼────────┤
│ Balance     │ 128.45 │
│ Cash        │ 100.00 │
│ Voucher     │ 28.45  │
│ Credit      │ 0.00   │
│ Yesterday   │ 1.23   │
│ This month  │ 45.68  │
│ Total spend │ 678.90 │
╰─────────────┴────────╯

  Subscription

╭────────────┬─────────────────────╮
│ Item       │ Amount              │
├────────────┼─────────────────────┤
│ Plan       │ Plus                │
│ Plan type  │ 1                   │
│ Expires    │ 2027-06-15 08:00:00 │
│ Auto renew │ off                 │
╰────────────┴─────────────────────╯

  Credit

╭─────────────────────┬─────────────────────╮
│ Item                │ Amount              │
├─────────────────────┼─────────────────────┤
│ Subscription credit │ 42.5%               │
│ Top-up credit       │ 100.0%              │
│ Resets at           │ 2027-03-22 17:00:00 │
╰─────────────────────┴─────────────────────╯

  Credit buckets

╭──────────────┬──────────────┬───────┬─────────────────────┬─────────────────────╮
│ Left         │ Total        │ Used  │ NextReset           │ Expires             │
├──────────────┼──────────────┼───────┼─────────────────────┼─────────────────────┤
│ 425.00       │ 1,000.00     │ 57.5% │ 2027-03-22 17:00:00 │ 2027-06-15 08:00:00 │
╰──────────────┴──────────────┴───────┴─────────────────────┴─────────────────────╯

  Model usage · last 7 days

╭────────────────┬───────┬──────────────╮
│ Model          │ Calls │ Credit       │
├────────────────┼───────┼──────────────┤
│ step-5-preview │ 120   │ 8,640,000.00 │
╰────────────────┴───────┴──────────────╯

  120 calls · 8,640,000.00 credits
```

```
$ stepfun usage -v
  ...adds a Records table with the time window, model, calls, and credit of each row

$ stepfun status --json | jq '.credit.subscription_left_rate'
  0.425
```

## Authentication

### Account + password

```bash
stepfun login
# Account (phone or email): 13812348888
# Password: ********
```

The flow mirrors the web app:

1. `RegisterDevice` — an anonymous device is registered, returning a device id and an anonymous `Oasis-Token` cookie
2. `SignInByPassword` — the account password is posted carrying that device's cookie; the response header `oasis-token` holds the login token
3. The token is verified with one `QueryAccountBalance` call before anything is written to disk

Credentials are stored by [`confy`](https://docs.rs/confy) at `~/.config/stepfun-cli/credentials.toml`, restricted to `0600` on Unix. Only the token and the device id it is bound to are saved — never the password.

> **Note:** `SignInByPassword` has no captcha, but the token it returns is short-lived: the session half expires **30 minutes** after login, after which every command reports `Session token rejected by the server`. Just run `stepfun login` again — there is nothing to fix and no state to clear.
>
> This was measured on a live session (login at 19:23, first rejection at 19:53), and it contradicts the common claim that the device half of the token lasts ~300 days. Budget a re-login roughly every half hour when using the CLI interactively.

### Logout

```bash
stepfun logout
```

## Development

```bash
cargo build                    # build
cargo build --release          # release build
cargo test                     # unit tests + integration tests
cargo clippy --all-targets --all-features --tests -- -D warnings
cargo fmt
```

The integration tests run against a mock HTTP server (`tests/common/mod.rs`), so they never touch the real platform.

- [docs/protocol.md](docs/protocol.md) — the API surface, the gotchas, and the verified field list

## License

MIT — see [LICENSE](LICENSE).
