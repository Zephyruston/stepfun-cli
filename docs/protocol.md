# StepFun 开放平台协议

本文档记录 `stepfun-cli` 实现的接口，逆向自 `platform.stepfun.com` Web 端
（2026 年 9 月，mitmproxy）。标注「实测」的字段已用真实账号返回数据校验。

由于这些接口没有公开文档，本文档是维护本项目的依据——服务端一旦变更字段，
请以这里的记录为基准对照排查。

2026-10-16 对照线上响应复查过一次：用量记录新增 token 数字段、套餐与余额接口
新增若干字段，均已更新到下表，细节见「用量记录的 token 数」一节。

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
   - 响应体同样带 `accessToken` / `refreshToken`（camelCase），CLI 取的是响应头。
3. 拿到的 session 可以直接用于 `platform.stepfun.com`，不需要再做 OAuth code 交换。

## 续期：`RefreshToken`

Web 端「永不退出」不是因为 token 长效，而是因为它一直在续：

```
POST .../PassportService/RefreshToken
body  {}   （proto 里还有 journal_b64 / encoded_device_info / deviceReqTimeMs，都可省）
头    cookie: Oasis-Token=<当前 token>、oasis-webid: <deviceID>、
      oasis-appid: 10300、oasis-platform: web、connect-protocol-version: 1
→ 200 {"accessToken":{"raw":...,"duration":1800,"mode":1},
       "refreshToken":{"raw":...}}
```

**不需要密码。** Web 端用了两条策略（读自 `account-static` 的 JS bundle）：

- 主动：登录成功后 `setTimeout(1000 * accessToken.duration)`，每 30 分钟续一次，拿到新
  duration 再挂下一个 timer；
- 被动：fetch 拦截器抓到 `TOKEN_EXPIRED` / `TOKEN_ILLEGAL` 就刷新（promise 去重，并发
  只刷一次）再重放原请求；刷失败才 `clearing session` 踢回登录页。

CLI 照抄这两条：过期前 5 分钟主动续，401 后再续一次并重试（`auth::with_auto_refresh`）。
`oasis-webid` 对身份没有影响（换个值甚至垃圾值照样 200），身份完全由 cookie 里的 token 决定。

**刷新对「新鲜」的 token 是幂等的。** 实测：登录后连续 20 次调用，token 从 59s 龄到 872s 龄
都原样返回同一个 token；到 917s 龄那一次才签发新的（另一次独立观测是 1015s 龄时换发）。
所以门槛在 **(872s, 917s]**，也就是服务端大约在会话段满 15 分钟后才肯重签。会话段 payload
里的 `age` 字段是续期计数：登录签发为 1，每次换发 +1。

这条对 CLI 的 5 分钟 margin 是有利的：触发时 token 已 25 分钟龄，稳过门槛，拿到的必然是
新会话段。反过来，登录后立刻刷新是空操作——无害，但也不要指望它能延长什么。

### token 的两段与寿命

`Oasis-Token` cookie 是两段 JWT 拼起来的，中间隔**三个点**（两个空段），共 8 个 dot 分段：

```
access_token.raw + "..." + refresh_token.raw
[0] JWT header          {"alg":"HS256"}
[1] JWT payload         activated / exp = create_at + 1800s / mode / oasis_id
[2] signature
[3] (空)
[4] (空)
[5] JWT header          {"alg":"HS256"}
[6] JWT payload         app_id / device_id / platform / oasis_id / exp
[7] signature
```

`access_token` 和 `refresh_token` 各自都只是**单段** JWT（3 个 dot 分段），cookie 由这两段
拼成。用错点数的拼法会被服务端判为 `token is illegal`。

| 段            | 来源                                        | 寿命                       |
| ------------- | ------------------------------------------- | -------------------------- |
| 会话段        | 响应的 `accessToken.raw`                    | 30 分钟，每次续期重发      |
| 设备段        | 响应的 `refreshToken.raw`                   | 30 天，**续期不滑动**      |

实测记录（下面用 T 表示登录时刻，具体时间已脱敏）：

- 会话段在 T + 1800s 过期，到期后业务接口返回
  `{"code":"unauthenticated","message":"auth failed: token is expired"}`；
- 会话段过期后 `RefreshToken` 仍返回 200 和新的账号态 token —— 被动刷新靠的就是这个；
- 设备段 exp 固定为登录时刻 + 2592000 秒，`RefreshToken` 刷完不变。所以 30 天后必须重新
  登录，Web 端也一样（它的处理是 clearing session）。

**本节推翻了两条早期错误记录**：①「两段 exp 都是 30 分钟」；②「设备段能撑约 300 天」。
另外 `oasis_r_at` 不是刷新时间戳——匿名设备时它等于 `create_at`，登录态时是账号创建时间
（可能比登录早几个月）。真正的长期凭据是 `refreshToken`。

**坑：设备段失效不报错。** 超过 30 天后再调 `RefreshToken`，服务端不返回错误，而是静默
签发一个匿名 token（`activated:false`、`oasis_id` 也变了）。CLI 因此比对刷新前后的
`oasis_id`，不一致就当作会话已死（`auth::same_account`）。

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
cookie 的拼法见上文「token 的两段与寿命」。

**会话段过期就会被拒。** 实测：会话段过期 21 秒、设备段还有 29 天时，业务接口返回
`token is expired`。所以「JWT1 过期不影响查询、服务端只认证 JWT2」的说法不成立——
两段都得是活的，这也是为什么必须续期而不是只靠设备段硬撑。

## 接口列表

| 方法                     | body                                                        | 响应字段                                                                                                                                                                                                                                                                                         |
| ------------------------ | ----------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ |
| `QueryAccountBalance`    | `{}`                                                        | `balance`、`payment`、`voucher`、`voucher_api`、`voucher_plan`、`credit`、`cost_yesterday`、`cost_month`、`cost_total`、`notify_threshold`（金额为分，阈值为例外）；另有 `voucher_expire_time`（**秒为单位的时长**，不是时间戳）                                                                                 |
| `GetStepPlanStatus`      | `{}`                                                        | `subscription{plan_type, name, activated_at, expired_at, auto_renew}`（另有 `status`、`pay_channel`、`plan_id`、`plan_family`、`source_channel_code`）、`plan_definition`（另有 `support_models`、`available`、`billing_cycle`、`upgrade_info`、`zh_display`/`en_display`）                                    |
| `QueryStepPlanRateLimit` | `{}`                                                        | `plan_credit_rate_limit{subscription_credit_left_rate, subscription_credit_reset_time, topup_credit_left_rate, credit_buckets[...]}`，以及顶层的 `five_hour_usage_left_rate`、`five_hour_usage_reset_time`、`weekly_usage_left_rate`、`weekly_usage_reset_time`、`plan_family`、`status`、`desc` |
| `QueryStepPlanUsages`    | `{startTime, toTime, page, pageSize, granularHour}`（毫秒） | `records[{from_time, to_time, model_id, model_type, calls, credit_consumed, in_amount, out_amount, cache_amount, image_count}]`、`total`                                                                                                                                                         |
| `ListAccessKeys`         | `{}`                                                        | API keys（尚未实现）                                                                                                                                                                                                                                                                             |
| `UserInfo`               | `{}`                                                        | 用户信息（尚未实现）                                                                                                                                                                                                                                                                             |
| `ListOrganizations`      | `{}`                                                        | 组织列表（尚未实现）                                                                                                                                                                                                                                                                             |

account-overview 页面的「Credit 账户」卡片，取的是 `QueryStepPlanRateLimit` 的
`subscription_credit_left_rate`（剩余百分比）加 `subscription_credit_reset_time`。

## 用量记录的 token 数

2026-10-16 起，`QueryStepPlanUsages` 的每条 record 在原有字段之外多了五项
（数值照旧是字符串/数字混用，都走 `types::Str`）：

| 字段           | 含义                              | 取值示例（脱敏） |
| -------------- | --------------------------------- | ---------------- |
| `in_amount`    | 输入 Token 数                     | `"12345678"`     |
| `out_amount`   | 输出 Token 数                     | `"98765"`        |
| `cache_amount` | 缓存 Token 数                     | `"11223344"`     |
| `image_count`  | 生成图片数，文本模型恒为 `"0"`    | `"0"`            |
| `model_type`   | 模型分类：`1` 文本、`11` 深度搜索 | `1` / `11`       |

网页端用量表的「输入/输出/缓存 Token数」三列就取前三项；CLI 的 `usage` 汇总表和
`-v` 记录表按模型分组求和后展示同样的三列，`--json` 里另外带上 `model_type` 和
`image_count`。

**`in_amount` 是否包含 `cache_amount` 未经确认。** 实测多条记录的这两个数字量级相近
（缓存往往占输入的九成上下），但响应本身无法说明缓存 token 有没有被算进输入 token
（计费很可能只按非缓存部分算）。CLI 因此把三列原样并列，不做「输入 = 新增 + 缓存」
这类推导。

**深度搜索记录没有 token 数。** `model_id` 为 `dr-search-api` 时 `model_type` 为
`11`，三个 token 字段直接缺失（不是 `"0"`）。CLI 按 0 计入汇总，不报错。

**记录时间是毫秒，且网页端按结束时刻归档。** `from_time`/`to_time` 是毫秒时间戳，
相差 3600 秒（`granularHour: 1`）。实测同一小时内、`to_time` 比 `from_time` 晚 1 秒
不到的记录（形如 `22:11:39` → `23:11:39`），网页端显示为「23:00 ~ 00:00」——取的是
`to_time` 所在的小时段，不是 `from_time` 的。CLI 两个时间都原样打印，不套用这个取整。

**没有模型列表接口。** 试过 `Dashboard` 前缀下的 `ListModels`/`QueryModels`/
`GetModelList` 等，以及 `Model`/`ModelService`/`Models`/`Openapi`/`Public` 等其它
service 前缀，全部 404。`dr-search-api` → 「深度搜索」这类映射只存在于前端 bundle，
CLI 因此继续显示原始 `model_id`。

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

**`voucher_plan` 的单位是推断的。** 2026-10-16 新增的这个字段（套餐赠送代金券）
按相邻 `voucher`/`voucher_api` 的惯例推断为**分**，但实测值只有 `"0"`，无从验证。
CLI 把它和另外两个 voucher 字段一样除以 100。

## credit 是整数计数，不是金额

`credit_consumed`、`credit_total`、`credit_residual` 都是整数计数，没有小数点：

```
credit_consumed = "1234567"    → 1,234,567
credit_total    = "2000000000" → 2,000,000,000
```

所以它们用 `data::format_credits` 格式化成整数（千分位、不带 `.00`），
而不是当成金额保留两位小数。

## credit bucket 的 `type`

`plan_credit_rate_limit.credit_buckets[]` 2026-10-16 起多一个 `type` 字段，标识桶
是哪一种 credit。实测账号只有一个桶、`type` 为 `1`，其
`credit_residual/credit_total` 与同一响应里的 `subscription_credit_left_rate` 完全相等
（具体数值已脱敏）——所以 **`1` = 订阅 credit 是已验证的**；`2` = 充值 credit 只是按
`topup_credit_left_rate` 的对应关系**推断**，未验证。

CLI 只把这两个码翻成 `Subscription`/`Top-up`，遇到其它码原样显示数字
（`data::format_bucket_kind`）。

## 字段编码

proto3 JSON：64 位整数以**字符串**下发，double 以**数字**下发，而且各接口并不统一。
所有数值字段都经过 `types::Str` 这个 newtype 读取、按需转换。
时间戳是 unix 秒或毫秒都有（`QueryStepPlanUsages` 用毫秒，`to_epoch_secs` 会统一处理）。

## 凭据存储

`~/.config/stepfun-cli/credentials.toml`，权限 `0600`：

| 字段 | 何时存在 |
| --- | --- |
| `username`、`token`、`webid` | 登录后始终存在 |

**不存密码。** 会话段 30 分钟过期，但设备段能靠 `RefreshToken` 续期 30 天，所以密码没有
留存的价值——最坏情况从「每 30 分钟重新登录一次」变成「每 30 天一次」，代价只是重新输入
一次账号密码。落盘的东西因此只剩「能查接口 + 能续期」，不含能接管账号的秘密。

## 安全提醒

账号密码在 mitmproxy 抓包里是明文。CLI 只保存 token；如果密码曾在抓包中出现过，请尽快修改。
