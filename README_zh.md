# StepFun CLI

在终端里查看 [StepFun（阶跃星辰）开放平台](https://platform.stepfun.com)的账户余额、订阅 Credit 和各模型调用量。不需要浏览器，单个二进制文件，无运行时依赖。

<p align="center">
  <img src="https://img.shields.io/badge/rust-1.85%2B-orange?logo=rust" alt="Rust 1.85+">
  <img src="https://img.shields.io/badge/license-MIT-blue" alt="License: MIT">
  <img src="https://img.shields.io/badge/tests-29%20passing-brightgreen" alt="Tests: 29 passing">
</p>

[English](README.md)

---

## 简介

`stepfun-cli` 调用的是 [platform.stepfun.com](https://platform.stepfun.com) Web 端使用的同一套接口：用账号密码登录一次，之后所有命令都直接读账户面板的 API。

这些接口没有公开文档，是从 Web 端逆向出来并用真实账号逐一校验的。本项目对它的全部认知都记录在
[docs/protocol.md](docs/protocol.md)，包括那些花了时间才找到的坑。

### 功能

- **一个命令看全貌** — `stepfun status`：余额、订阅、Credit 额度、近 7 天模型用量
- **按主题拆分的命令** — `stepfun balance` / `stepfun credit` / `stepfun usage`
- **只有 usage 能选日期** — `stepfun usage --days 30`，或 `stepfun usage --start 2026-09-01 --end 2026-09-20`
- **机器可读** — `--json` 输出结构化数据，方便脚本处理
- **详细模式** — `-v` 追加 Credit 桶、逐条用量记录，以及本项目尚未建模的响应字段
- **单二进制** — 基于 `ureq` 的阻塞式 HTTP，无 async 运行时，零运行时依赖
- **只存 token** — 密码绝不落盘，只保存 Oasis token 及其绑定的设备 id

## 安装

编译需要 Rust ≥ 1.85；产出的二进制文件本身没有任何运行时依赖。

```bash
# 从源码安装
git clone https://github.com/Zephyruston/stepfun-cli.git
cd stepfun-cli
cargo install --path . --locked
```

### Shell 补全

```bash
# bash
echo 'source <(stepfun completions bash)' >> ~/.bashrc

# zsh
echo 'source <(stepfun completions zsh)' >> ~/.zshrc

# fish
stepfun completions fish > ~/.config/fish/completions/stepfun.fish
```

## 命令

| 命令                                                | 说明                                   |
| --------------------------------------------------- | -------------------------------------- |
| `stepfun status`                                    | 总览：余额 + 订阅 + Credit + 用量      |
| `stepfun status -v`                                 | 追加 Credit 桶、逐条记录、未建模字段   |
| `stepfun status --json`                             | 总览输出 JSON                          |
| `stepfun balance`                                   | 余额、现金、代金券、Credit、消费       |
| `stepfun credit`                                    | 套餐、Credit 剩余、重置时间、Credit 桶 |
| `stepfun usage`                                     | 近 7 天各模型用量                      |
| `stepfun usage --days 30`                           | 近 30 天各模型用量                     |
| `stepfun usage --start 2026-09-01 --end 2026-09-20` | 自定义日期区间                         |
| `stepfun usage -v`                                  | 列出每条用量记录，而非按模型汇总       |
| `stepfun usage --json`                              | 用量输出 JSON                          |
| `stepfun login`                                     | 登录（交互输入账号密码）               |
| `stepfun login -u <账号> -p <密码>`                 | 非交互登录                             |
| `stepfun logout`                                    | 清除已保存的凭据                       |
| `stepfun completions <SHELL>`                       | 生成 shell 补全脚本                    |

所有取数命令都支持 `--json`；除 `balance` 外都支持 `-v`。

## 示例

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
  ...追加一张 Records 表：每条记录的起止时间、模型、调用数、credit

$ stepfun status --json | jq '.credit.subscription_left_rate'
  0.425
```

## 登录

### 账号密码

```bash
stepfun login
# Account (phone or email): 13812348888
# Password: ********
```

流程与 Web 端一致：

1. `RegisterDevice` — 匿名注册设备，返回 deviceID 和匿名 `Oasis-Token` cookie
2. `SignInByPassword` — 携带该设备的 cookie 提交账号密码，响应头 `oasis-token` 即登录 token
3. 落盘前先用一次 `QueryAccountBalance` 验证 token 可用

凭据由 [`confy`](https://docs.rs/confy) 保存在 `~/.config/stepfun-cli/credentials.toml`，Unix 下权限为 `0600`。只保存 token 和它绑定的设备 id，**不保存密码**。

> **注意：** `SignInByPassword` 无验证码，但返回的 token**有效期只有 30 分钟**——登录后半小时起，所有命令都会报 `Session token rejected by the server`。重新 `stepfun login` 即可，没有别的问题，也不需要清理任何状态。
>
> 这是用真实会话实测出来的（19:23 登录，19:53 首次被拒），与常见的「设备那半段能撑 300 天、查询不受影响」说法**不一致**。交互使用时，请按每半小时重新登录一次来预期。

### 退出登录

```bash
stepfun logout
```

## 开发

```bash
cargo build                    # 编译
cargo build --release          # release 构建
cargo test                     # 单元测试 + 集成测试
cargo clippy --all-targets --all-features --tests -- -D warnings
cargo fmt
```

集成测试通过 mock HTTP server（`tests/common/mod.rs`）运行，不会访问真实的平台。

- [docs/protocol.md](docs/protocol.md) — 接口清单、那些坑、已实测确认的字段

## 许可证

MIT — 见 [LICENSE](LICENSE)。
