# StepFun 开放平台协议

本文档记录 `stepfun-cli` 实现的接口，逆向自 `platform.stepfun.com` Web 端
（2026 年 9 月，mitmproxy）。标注「实测」的字段已用真实账号返回数据校验。

由于这些接口没有公开文档，本文档是维护本项目的依据——服务端一旦变更字段，
请以这里的记录为基准对照排查。

## 域名

| 域名                   | 用途                                                                   |
| ---------------------- | ---------------------------------------------------------------------- |
| `account.stepfun.com`  | 账号中心 —— passport 登录，接口挂在 `/passport/...` 下                 |
| `platform.stepfun.com` | 开放平台 —— 业务接口，挂在 `/api/step.openapi.devcenter.Dashboard/` 下 |

两边都是 Connect 风格 RPC：`POST <prefix>/<Service>/<Method>`，body 为 JSON。

## 登录

1. `POST https://account.stepfun.com/passport/proto.api.passport.v1.PassportService/RegisterDevice`
   - body `{}`，匿名即可调用，不需要 cookie。
   - 响应：`Set-Cookie: Oasis-Token=<匿名 JWT>`，以及 body 里的 `.device.deviceID`。
2. `POST .../SignInByPassword`，body `{"username": ..., "password": ...}`
   - 头：`cookie: Oasis-Token=<匿名 token>`、`oasis-webid: <deviceID>`、
     `oasis-appid: 10300`、`oasis-platform: web`、`connect-protocol-version: 1`、
     `origin`/`referer: account.stepfun.com`
   - 响应头 `oasis-token` 携带登录 token（取不到时回退到刷新后的 `Oasis-Token` cookie）。
3. 拿到的 session 可以直接用于 `platform.stepfun.com`，不需要再做 OAuth code 交换。

### token 只有 30 分钟有效期

`oasis-token` 实际是两段 JWT 拼接（用 `.` 分隔，共 8 个 dot 分段），实测两段的
`exp` 都等于登录时刻 `create_at + 1800`：

```
[0] JWT header          {"alg":"HS256"}
[1] JWT payload         exp = create_at + 1800s   ← 会话，30 分钟
[2] signature
[3] (空)
[4] (空)
[5] JWT header          {"alg":"HS256"}
[6] JWT payload         app_id / device_id / platform / oasis_r_at / exp
[7] signature
```

实测记录：19:23:15 登录，19:53:15 起业务接口返回 401。

**这与「设备那段能撑约 300 天、账号那段过期不影响查询」的说法不符。** 30 分钟一到，
所有业务接口一律 401，没有例外。CLI 对此的处理是把 401 映射成
`Session token rejected by the server`，用户重新登录即可。

## 业务接口认证（实测最小集）

`POST https://platform.stepfun.com/api/step.openapi.devcenter.Dashboard/<Method>`

- cookie **只能含一个字段**：`Oasis-Token=<JWT1>...<JWT2>`
- 必需头：`oasis-appid: 10300`、`oasis-platform: web`、`oasis-webid: <deviceID>`
  （需与第二个 JWT 内的 device id 一致）
- `content-type`、`referer`、`User-Agent`、`accept` 都不是必需的

**坑：cookie 多带字段必然 401。** 只要额外带上 `INGRESSCOOKIE`、`_wafdytokenv1`
或 `Oasis-Webid` 中任意一个，服务端就会把 `"JWT1...JWT2"` 当成单个 JWT 解析，报错：

```
token is malformed: could not base64 decode signature: illegal base64 data at input byte 43
```

原因：第一个 JWT 的签名长度恰好是 43 字符，不是 4 的倍数。
另外，replay 抓到的原始 flow 会返回 200，别被 replay 结果误导——以实际发出的请求为准。
账号那半段（JWT1）过期不影响查询，服务端认证用的是设备那半段（JWT2）。

## 接口列表

| 方法                     | body                                                        | 响应字段                                                                                                                                                                                                                                                                                         |
| ------------------------ | ----------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ |
| `QueryAccountBalance`    | `{}`                                                        | `balance`、`payment`、`voucher`、`voucher_api`、`credit`、`cost_yesterday`、`cost_month`、`cost_total`、`notify_threshold`（金额为分，阈值为例外）；另有 `voucher_expire_time`（**秒为单位的时长**，不是时间戳）                                                                                 |
| `GetStepPlanStatus`      | `{}`                                                        | `subscription{plan_type, name, expired_at, auto_renew}`、`plan_definition`                                                                                                                                                                                                                       |
| `QueryStepPlanRateLimit` | `{}`                                                        | `plan_credit_rate_limit{subscription_credit_left_rate, subscription_credit_reset_time, topup_credit_left_rate, credit_buckets[...]}`，以及顶层的 `five_hour_usage_left_rate`、`five_hour_usage_reset_time`、`weekly_usage_left_rate`、`weekly_usage_reset_time`、`plan_family`、`status`、`desc` |
| `QueryStepPlanUsages`    | `{startTime, toTime, page, pageSize, granularHour}`（毫秒） | `records[{from_time, to_time, model_id, calls, credit_consumed}]`、`total`                                                                                                                                                                                                                       |
| `ListAccessKeys`         | `{}`                                                        | API keys（尚未实现）                                                                                                                                                                                                                                                                             |
| `UserInfo`               | `{}`                                                        | 用户信息（尚未实现）                                                                                                                                                                                                                                                                             |
| `ListOrganizations`      | `{}`                                                        | 组织列表（尚未实现）                                                                                                                                                                                                                                                                             |

account-overview 页面的「Credit 账户」卡片，取的是 `QueryStepPlanRateLimit` 的
`subscription_credit_left_rate`（剩余百分比）加 `subscription_credit_reset_time`。

## 滚动配额：`0` 表示「没有配额」，不是「已用完」

实测（Plus 套餐；以下数值已脱敏，仅用于说明「字段存在但为 0」这一行为）：

```
plan_credit_rate_limit.subscription_credit_left_rate = 0.87
five_hour_usage_left_rate                          = "0"
five_hour_usage_reset_time                         = "0"
weekly_usage_left_rate                             = "0"
weekly_usage_reset_time                            = "0"
plan_family                                        = "2"
status                                             = "1"
desc                                               = ""
```

也就是说，套餐本身不带 5 小时/每周滚动配额时，这些字段返回 `0` 而不是省略。
如果直接渲染成 `Weekly credit: 0.0%` 会让人误以为配额已耗尽，
因此 CLI 对为 0 的配额整行隐藏（`data::quota_rate`），`--json` 里仍保留原始值。
同理，`desc` 为空串时不进入 `-v` 的「Other fields」表。

## 金额单位：分，不是元

`QueryAccountBalance` 的金额字段全部以**分**下发，`"1500"` 是 15.00 元：

| 字段                                             | 原始值           | 实际     |
| ------------------------------------------------ | ---------------- | -------- |
| `balance`                                        | `"1500"`         | 15.00 元 |
| `payment` / `voucher` / `voucher_api` / `credit` | `"1500"` / `"0"` | 同上     |
| `cost_yesterday` / `cost_month` / `cost_total`   | `"0"`            | 同上     |

旁证：`GetStepPlanStatus.plan_definition.price` 为 `"9900"`，对应 Plus 套餐 99 元/月。

**例外：`notify_threshold` 按元下发**，同一个响应里就它不一样——原始值 `"5"` 在
平台「低余额提醒」设置里显示为 5 元（已与平台页面对照确认）。

CLI 里由 `data::fen_to_yuan` 处理分→元，`data::yuan` 处理阈值这个例外。

## credit 是整数计数，不是金额

`credit_consumed`、`credit_total`、`credit_residual` 都是整数计数，没有小数点：

```
credit_consumed = "1234567"    → 1,234,567
credit_total    = "2000000000" → 2,000,000,000
```

所以它们用 `data::format_credits` 格式化成整数（千分位、不带 `.00`），
而不是当成金额保留两位小数。

## 字段编码

proto3 JSON：64 位整数以**字符串**下发，double 以**数字**下发，而且各接口并不统一。
所有数值字段都经过 `types::Str` 这个 newtype 读取、按需转换。
时间戳是 unix 秒或毫秒都有（`QueryStepPlanUsages` 用毫秒，`to_epoch_secs` 会统一处理）。

## 凭据存储

`~/.config/stepfun-cli/credentials.toml`，权限 `0600`，只存 `username`、`token`、`webid`——
**不存密码**。

## 安全提醒

账号密码在 mitmproxy 抓包里是明文。CLI 只保存 token；如果密码曾在抓包中出现过，请尽快修改。
