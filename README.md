# SMS 接码器 · PC 版 / SmsSink

Windows 11 上被动接收 [SmsForwarder](https://github.com/pppscn/SmsForwarder) 推送的短信，按自定义规则提取内容（主要是数字验证码），并及时通知。手机与电脑在同一局域网时直连，无需任何中转服务或云函数。

常驻形态是**托盘 + 阻塞式消息泵**：空闲时 CPU 为 0，无常驻异步运行时、无 WebView2、无浏览器进程。管理台是 `127.0.0.1` 上的本地网页，用系统默认浏览器打开。

## 快速开始

```powershell
cargo build --release
.\target\release\SmsSink.exe
```

首次启动会生成 `config.json`（优先 exe 同目录，回退 `%APPDATA%\SmsSink\`）和 `%APPDATA%\SmsSink\data.db`，并在控制台打印管理台地址与访问令牌：

```
管理台 http://127.0.0.1:8787/?token=<32位十六进制>
```

托盘图标右键菜单：**打开管理台 / 最近验证码 / 发送测试通知 / 重载配置 / 开机自动启动 / 退出**。
开机自启写 `HKCU\Software\Microsoft\Windows\CurrentVersion\Run`，不需要管理员权限。进程用互斥体保证单实例。

调试或自动化时用 `SmsSink.exe --no-ui`：只接收与落盘，不创建托盘、不弹通知、不写剪贴板。`--help` 查看参数。

## 手机端 SmsForwarder 配置

1. 添加发送通道 → **Webhook**
2. 请求方法：`POST`
3. URL：`http://<电脑局域网IP>:8787/hook`
4. `webParams` 留空，使用默认 form 报文（`from` / `content` / `timestamp` / `sign`）
5. 「成功标识」填 `success`（服务端返回 `{"code":0,"msg":"success"}`）
6. 可选：填写 Secret 启用加签，需与 `config.json` 的 `secret` 一致

也兼容自定义 JSON 模板：`webParams` 以 `{` 开头时，服务端从 `from`/`sender`、`content`/`msg`/`message`/`text`、`timestamp`/`time`、`sign`/`signature` 中取字段。

加签算法为 `urlEncode(Base64(HmacSHA256(timestamp + "\n" + secret, secret)))`，服务端校验 ±300 秒时间窗并做重放去重。`secret` 留空时自动跳过校验，便于先联调。

## 网络与防火墙

Windows 默认拦截入站连接，需以管理员身份添加一条**仅专用网络**、仅本程序、仅该端口的规则：

```powershell
New-NetFirewallRule `
  -DisplayName "SMS 接码器 · 局域网入站" `
  -Direction Inbound -Protocol TCP -LocalPort 8787 `
  -Action Allow -Profile Private `
  -Program "E:\001-Program\SmsSink\target\release\SmsSink.exe"
```

建议在路由器上给电脑绑定固定 IP，否则 DHCP 续租后手机端会发丢。**电脑休眠时收不到推送**，需要在电源设置里允许不休眠或网络唤醒。

## 规则

管理台「规则管理」里增删改，保存后立即生效，无需重启。也支持直接编辑 `config.json` 后点托盘「重载配置」。

| 字段 | 说明 |
|---|---|
| `name` / `enabled` / `priority` | 规则名、开关、优先级（大者优先，首个命中即用） |
| `match_from` | 发送方号码 / 包名，包含即命中；留空不限来源 |
| `keywords_any` / `keywords_all` / `exclude` | 任一包含 / 全部包含 / 排除关键词 |
| `extract` / `capture` | 提取正则与捕获组序号 |
| `notify` | `toast`（通知中心 + 复制按钮）/ `card`（右下角大号卡片）/ `silent`（仅入历史） |
| `auto_copy` | 收到即写入剪贴板，**默认 false** |
| `copy_template` | 复制文本模板，支持 `{code}` `{from}` `{content}` |
| `ttl_sec` / `keep_history` | 验证码有效期提示 / 是否落盘留痕 |

样例短信：

```
【中国移动统一认证】您的验证码为633382，您正在使用短信验证码登录功能，该验证码仅用于身份验证，3分钟内有效，请勿泄露给他人使用。
```

默认正则 `(?:验证码|校验码|动态密码)[^\d]{0,8}(\d{4,8})` 提取出 `633382`；「您的订单号 123456 已发货」不会误命中。**不要用裸 `\d{6}`**，订单号、金额、卡号后六位都会误报。编辑页有「样例短信试跑」，可先看结果再保存。

> 高频共用业务系统里，短信内容无法区分归属，请把该规则的 `auto_copy` 保持 `false`，否则会冲掉你正在使用的剪贴板。需要哪条码，到「最近记录」里单独点复制。

## 通知与复制

- **Toast**：进入系统通知中心，带「复制验证码」「打开管理台」两个按钮；用唯一 `Tag` 堆叠、不互相覆盖，按 `toast_limit` 自动移除最旧的一条。
- **卡片**：右下角自绘 Win32 窗口，大号显示验证码 + 真按钮 + 倒计时自动消失，不进通知中心，不抢焦点。
- **静默**：不弹任何东西，只在管理台记录里出现。

自动复制按规则开关；所有通知/面板的复制都走同一个剪贴板入口，并回写 `copied` 标记用于统计。

## 数据与统计

SQLite 落盘于 `%APPDATA%\SmsSink\data.db`（可用 `db_path` 改），记录时间、来源、原文、命中规则、提取值、通知方式、自动复制与已复制状态，建了时间/来源/规则三个索引。

管理台「统计分析」支持近 7/30/90/365 天的总量、命中率、每日柱状图、按规则与按来源聚合，以及 CSV 导出。`retention_days` 默认 180 天，启动时自动清理超期记录；`mask_content = true` 时原文中的数字（除提取出的验证码外）会打码落盘。

管理台仅监听回环地址（`0.0.0.0:8787` 上只放行 `/hook`），所有 `/api/*` 需要 `admin_token`。非本机 IP 访问管理接口一律 403。

## 已知限制

- 未打包 Win32 应用的 Toast 按钮激活依赖 AUMID + 进程内 `Activated` 事件。启动时会自动设置 AUMID 并写入 `HKCU\Software\Classes\AppUserModelId\SmsSink.Desktop`；个别环境若 Toast 按钮失效，兜底是管理台「最近记录」里逐条复制，核心链路不受影响。
- 进程退出后，通知中心里遗留的 Toast 按钮不再可用（需二期实现 COM `INotificationActivationCallback`）。
- 端口、监听地址、hook 路径变更后需重启程序；其余配置热重载即可。

## 目录

```
Cargo.toml            依赖与 Release 优化（lto + strip）
src/main.rs           启动编排、单实例、COM/AUMID、DB 初始化
src/config.rs         配置结构、落盘、令牌、DB 路径
src/app.rs            可热重载的全局状态
src/win.rs            Win32 手写 FFI（托盘/剪贴板/注册表/自启）
src/server.rs         TcpListener 极简 HTTP、form/JSON 解析、加签校验、路由
src/rules.rs          优先级规则匹配、上下文正则提取、试跑
src/event.rs          HTTP 线程 → UI 线程的事件投递与动作分发
src/toast.rs          WinRT Toast + 复制按钮事件
src/card.rs           自绘右下角卡片
src/tray.rs           托盘与消息泵
src/store.rs          SQLite 落盘、统计、CSV、保留期清理
src/admin.rs          管理台 API
assets/index.html     管理台单页（规则 / 记录 / 统计 / 设置）
config.example.json   完整配置示例
```
