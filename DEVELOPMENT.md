# 开发与维护指引

面向接手这个仓库的人 —— 也包括下一个 AI 会话。用户视角的说明在 [README.md](README.md)，这里只讲三件事：**代码怎么改、改完怎么验、发出去会不会坏**。

> 项目全部由 AI 编写。这意味着两点：代码里「为什么这样写」的注释密度偏高，值得读；同时没有任何历史包袱之外的隐性约定——凡是约定都写在这个文件里，没写的可以改。

---

## 0. 先读这一段

如果只有五分钟，记住这三条，它们导致的线上问题最多：

1. **改 `docs/*.html` 后必须重新编译 exe。** 教程页和介绍页通过 `include_str!` 编进二进制（`src/admin.rs`），不重编译，已安装用户看到的还是旧页面。
2. **HTTP 线程不许碰 UI、剪贴板、Toast。** 一律 `PostMessage` 投给 UI 线程，见 `src/event.rs`。Win32 窗口有线程亲和性，跨线程直接调用会静默失败或死锁。
3. **给配置加字段必须带默认值。** `Config` / `Rule` 都靠 `#[serde(default)]` 兜底，新字段要在 `Default` 里给出值，否则老用户升级后旧 `config.json` 直接解析失败、回落到全默认配置（表现为"我的规则全没了"）。

---

## 1. 环境与最快上手

| 工具 | 版本 | 用途 |
|---|---|---|
| Rust | stable（CI 用 `dtolnay/rust-toolchain@stable`） | 编译 |
| MSVC 工具链 | VS 2022 Build Tools | 链接 exe、`rusqlite` 的 bundled SQLite 需要 C 编译器 |
| Inno Setup 6 | 6.x | 只在本机打安装包时需要；CI 用 `choco install innosetup` |
| Python 3 + Pillow | 任意 | `tools/make_icon.py` 重新生成图标 |

```bash
# 日常开发：debug 版保留控制台，cargo run 能直接看日志
cargo run

# 发版用的构建
cargo build --release --locked

# 只出安装包（会先校验 exe 资源版本与 Cargo.toml 是否一致）
powershell -ExecutionPolicy Bypass -File installer\build.ps1
```

### 命令行参数

| 参数 | 作用 |
|---|---|
| `--no-ui` | 无界面模式：只接收、落盘，不建托盘、不弹通知、不写剪贴板。联调和自动化测试用 |
| `--probe-ui` | **跳过单实例检查**，允许与正在运行的正式实例并存。预览界面改动时用它，不必先退出自己正在用的那份 |
| `--help` | 打印用法 |

### debug 与 release 的差别

| | debug | release |
|---|---|---|
| 子系统 | 控制台（`cargo run` 直接看 `println!`） | `windows`（双击不弹黑框） |
| 控制台输出 | 天然有 | 需从终端启动，靠 `win::init_stdio()` 挂回父进程控制台 |
| 优化 | 默认 | `opt-level="s"` + `lto` + `strip`，体积优先 |

### 数据与配置落在哪

| 内容 | 路径 | 说明 |
|---|---|---|
| 配置 | `%APPDATA%\SmsSink\config.json` | exe 旁边**已存在** `config.json` 时优先用它（便携版语义） |
| 数据 | `%APPDATA%\SmsSink\data.db` | 可由 `db_path` 覆盖 |
| 单实例互斥体 | `SmsSink.SingleInstance.v1` | 全局命名，所以测试实例必须加 `--probe-ui` |
| 开机自启 | `HKCU\Software\Microsoft\Windows\CurrentVersion\Run\SmsSink` | 只写 HKCU，不需要管理员 |

**调试时别污染自己的真实数据**：把 `APPDATA` 指向临时目录，再配一个不冲突的端口，就是一套完整的隔离实例（第 7.2 节有可直接复制的命令）。

---

## 2. 运行形态

一个进程，两个线程域，没有异步运行时、没有定时器轮询、空闲 CPU 严格为 0。

```
手机 SmsForwarder ──► POST /hook ──► HTTP 线程（每连接一个：只解析、验签，不碰 UI）
                                          │
                                          │  PostMessage(WM_APP_SMS)
                                          │  事件所有权随裸指针转移给 UI 线程
                                          ▼
                     UI 线程：GetMessageW → wnd_proc → handle_event
                                          │
                                          └─► 依序做四件事
                                              1. store::record   落库（按规则 keep_history / keep_unmatched）
                                              2. clipboard_set   自动复制（仅 auto_copy = true）
                                              3. card::show 或 toast::show   通知
                                              4. hint::show      菜单动作的可见回执（另一条路径，不经此路）
```

COM（STA）在 UI 线程初始化，Toast、剪贴板、注册表、托盘都只在这条线程上使用。

### 启动顺序（`src/main.rs`，顺序有讲究）

1. `win::init_dpi_awareness()` —— 必须早于任何窗口和任何 DC，否则整个进程按 96 DPI 渲染再位图放大
2. `win::init_stdio()` + 控制台切 UTF-8 —— GUI 子系统进程默认没有可写 stdout
3. 解析参数
4. `config::load()` —— 注意它有副作用：缺 `admin_token` 时会生成一个并**写回文件**
5. 单实例互斥体（`--probe-ui` 跳过）；已有实例则 `PostMessage(WM_APP_POKE)` 让对方弹提示，然后自己退出
6. `CoInitialize`（STA）+ 设置 AUMID —— Toast 依赖 AUMID 才能在通知中心被识别
7. `store::init()` + `store::purge()` 清理超期记录
8. 端口预检查：先 `TcpListener::bind` 一次，失败就弹错误框（双击启动没有控制台，静默退出会被当成"程序坏了"）
9. `app::init(cfg)` 建立全局状态
10. `--no-ui` 直接在当前线程跑服务；否则开后台线程跑服务、主线程进 `tray::run()` 消息泵

> 第 8 步与 `server::run()` 里的第二次 `bind` 之间存在一个极窄的竞态窗口。实际从未触发过，若将来要收严，把第 8 步的 `TcpListener` 直接传给 `server::run()` 即可（现在多了一次 bind）。

---

## 3. 模块地图

| 文件 | 行数 | 职责 | 改这些需求时动它 |
|---|---|---|---|
| `main.rs` | 161 | 启动编排、参数、单实例、致命错误提示 | 加启动参数、改启动顺序 |
| `app.rs` | 40 | 全局状态：`Config`（可热重载）+ 重放去重集合 | 加共享状态 |
| `config.rs` | 246 | 配置结构、路径选择（便携/APPDATA）、令牌生成 | 加配置字段、改默认值 |
| `server.rs` | 665 | 极简 HTTP/1.1、form/JSON 解析、HMAC 校验、路由、`/ping` | 改协议兼容、加路由、改鉴权 |
| `rules.rs` | 141 | 优先级匹配、上下文正则提取、试跑预览 | 改匹配逻辑、加规则字段 |
| `event.rs` | 163 | HTTP 线程 → UI 线程投递、落盘/复制/通知分发 | 加一种通知方式、改落盘策略 |
| `store.rs` | 322 | SQLite 单连接 + Mutex、统计、CSV、保留期清理 | 加表字段、加统计维度 |
| `admin.rs` | 273 | 管理台 API + 三个内嵌页面路由 | 加 API、改页面路由 |
| `tray.rs` | 457 | 托盘图标、右键菜单、消息泵、菜单动作回执 | 改菜单结构、加菜单项 |
| `card.rs` | 1350 | 自绘右下角卡片：验证码 36px、自绘按钮、倒计时、悬停暂停 | 改卡片版式/交互 |
| `toast.rs` | 145 | WinRT Toast + 复制/打开按钮的回调 | 改通知中心行为 |
| `hint.rs` | 277 | 自绘单行提示条（40px 高、2.6 秒、不抢焦点） | 改回执样式 |
| `font.rs` | 97 | 字体选型（中英同一套字形）、按 DPI 建字体 | 改字体 |
| `win.rs` | 1001 | 手写 Win32 FFI + 少量封装（剪贴板/自启/URL/错误框/菜单） | 加 Win32 调用 |
| `net.rs` | 257 | 局域网 IPv4 枚举，给管理台提供候选地址 | 改地址排序/过滤规则 |
| `assets/index.html` | 726 | 管理台单页（记录/规则/统计/设置），无框架 | 改管理台界面 |
| `build.rs` | — | 把图标、manifest、版本信息写进 exe 资源 | 改资源版本规则 |

---

## 4. 线程模型与三条铁律

### 铁律一：UI 相关操作只在 UI 线程

窗口、剪贴板、Toast、注册表、托盘全部有线程亲和性或 COM STA 要求。`event.rs` 的模式是**所有权转移**：把 `Box<SmsEvent>` 转成裸指针塞进 `PostMessageW` 的 lparam，UI 线程收到后用 `Box::from_raw` 收回。投递失败时要就地回收，否则泄漏（`event::submit` 里已有这个分支，加新消息类型时照抄）。

加一种新的跨线程动作 = 新增一个 `WM_APP_*` 常量 + 在 `tray::wnd_proc` 里加一个分支 + 写一对 `Box::into_raw` / `Box::from_raw`。**不要**用 `Mutex<Option<T>>` 传值，会丢掉消息的时序性。

### 铁律二：不要阻塞消息泵

`TrackPopupMenu` 本身就是阻塞的（菜单打开期间消息泵停在它里面）。如果在 UI 线程上再做几百毫秒的同步 IO，用户的感觉是"点菜单没反应"。所以 HTTP 线程只做解析和校验，落盘和通知都扔给 UI 线程；`store` 的查询走短 SQL + 索引，`history_limit` 默认 200、有上限保护。

反过来，DB 不是 UI 线程独占的：管理台查询在 HTTP 线程执行，与本线程的写入争同一把 `Mutex<Connection>`。这是有意的（单连接、免连接池），代价是长查询会推迟落盘。别写全表扫描的统计 SQL。

### 铁律三：定时器只给"看得见的东西"

`tray.rs` 没有任何 `SetTimer`，悬停提示是"打开菜单那一刻的快照"。只有卡片（倒计时进度条）和提示条（自动消失）用 `WM_TIMER`，它们本来就在屏幕上。加功能时若想"每 30 秒刷新一次状态"，先问：用户此刻看得见吗？

---

## 5. 关键设计决策

每一条都曾是 bug 或体验事故的产物，改动前先理解代价。

### 5.1 只做接收端，永不主动连接

整个程序没有任何出站请求（`net.rs` 的 UDP `connect` 不发包，只用来问本机路由）。好处是零云端依赖、隐私可核对；代价是必须和手机端在同一局域网，且依赖用户配通防火墙——所以有了 `/ping`、`scripts/fix-firewall.ps1` 和一堆"连不上"的排障文案。

### 5.2 加签要兼容三种编码形态

`sign = urlEncode(Base64(HMAC-SHA256(timestamp + "\n" + secret, secret)))`。同一个签名到达服务端时有三种形态：

| 来源 | 到服务端时的样子 |
|---|---|
| 标准表单客户端 | 原始 Base64 |
| 官方 App（表单通道） | 残留 `%2B` / `%3D`（URLEncoder 一次 + OkHttp FormBody 再编一次） |
| 官方 App（JSON 通道） | 残留 `%2B`（JSON 解析不做 urlDecode） |

`server::verify` 的处理是：含 `%` 就再解一层，然后把空格换回 `+`（表单解析会把裸 `+` 吃成空格，而 Base64 不含空格）。**这三条路径都有回归测试**（`server::smsforwarder_compat`），改签名逻辑必须让它们全绿。

时间戳按位数归一（10 位当秒、13 位当毫秒）。不归一的话，秒级时间戳永远"超出时间窗"，表现为"开了加签后全部被拒"。

重放去重用的键是**归一后的签名**，所以同一签名换个编码形态再来也会被拦。集合超过 2000 条直接清空——它是防重放，不是审计日志，清空可接受。

### 5.3 「未命中」和「未解析」是两件事

早期两者都返回 `None`，于是"用户正等着的验证码"被静默丢弃。现在 `rules::Outcome` 分三档：

| 结果 | 含义 | 处理 |
|---|---|---|
| `Code` | 命中且提取到码 | 正常通知 + 落库（按规则 `keep_history`） |
| `NoCode` | 命中关键词但正则没提到码 | 降级卡片展示原文，**一律落库**（这是规则该改的信号） |
| `NoMatch` | 没命中任何规则 | 默认不落库（`keep_unmatched` 可开） |

改规则匹配时不要把这个区分压平。

### 5.4 默认走自绘卡片，而不是系统 Toast

未在开始菜单注册身份的应用发 Toast 只会静默进通知中心、不弹横幅，用户会以为没收到。卡片是我们完全掌控的通道：一定可见、带复制按钮、不抢焦点（`WS_EX_NOACTIVATE`）。Toast 仍保留为可选（`notify: "toast"`），适合想看通知中心留存的人。

同一原因，托盘菜单的动作回执不能用 `Shell_NotifyIcon` 气泡（实测消息送达、屏幕无反应），于是有了 `hint.rs`。

### 5.5 手写 FFI，不引 Win32 crate

`win.rs` 里全是 `extern "system"` 声明。收益是二进制小、没有版本地狱；代价是类型名按 Windows 头文件大写（`HWND`、`MSG`、`POINT`），因此 clippy 会报 `upper_case_acronyms`——**这是刻意保留的**，别为了消警告把它们改成驼峰。仅 Toast 用了 `windows` crate（WinRT 的 COM 样板不值得手写）。

### 5.6 页面内嵌进 exe

`admin.rs` 用 `include_str!` 把 `assets/index.html`、`docs/tutorial.html`、`docs/launch.html` 编进二进制。用户把 exe 单独拷走也能看教程。代价见第 0 节第 1 条。

三个页面都是自包含的（图片 base64 内嵌），所以 `docs/launch.html` 有 800KB+、`tutorial.html` 有 1.5MB。改页面时别引入外链图片/CDN——既破坏离线可用，也让 exe 体积之外的加载不可控。

### 5.7 单实例 = 互斥体 + 跨进程打招呼

第二份 exe 被启动时不静默退出，而是 `FindWindowW(tray::WINDOW_CLASS)` 找到常驻实例，`PostMessage(WM_APP_POKE)` 让它弹一条「SmsSink 已在运行」。所以 `tray::WINDOW_CLASS` 是**跨进程契约**，改名要两边同步（目前共用同一个常量，安全）。

托盘窗口刻意不用 `HWND_MESSAGE`：消息专用窗口永远无法成为前台窗口，`SetForegroundWindow` 失败会导致菜单拿不到键盘焦点、点外面也不消失——菜单卡住的同时 UI 线程停在 `TrackPopupMenu`，之后所有推送都弹不出通知。

---

## 6. 常见改动食谱

### 6.1 加一个配置项（5 步）

1. `config.rs`：`Config` 加字段 + `Default` 给值，**必须** `#[serde(default)]` 生效（结构体上已有，别删）
2. `config.rs`：若要在管理台可改，加到 `admin::put_settings` 的字段解析里
3. `admin.rs`：`state()` 里输出给前端
4. `assets/index.html`：设置页加控件 + 保存时带上该字段
5. `config.example.json`：补示例值（用户文档会引用它）

验证：拿一份**旧版** `config.json`（缺新字段）启动，确认不报错且新字段取默认值。

### 6.2 加一个管理台 API

1. `admin.rs` 的 `match (method, path)` 加分支
2. 需要鉴权的自动生效（`route` 入口统一拦），静态页与 `/api/health` 是例外
3. 返回统一用 `Resp::json`，错误用 `err(status, msg)`
4. 前端 `assets/index.html` 用 `fetch(path, {headers:{'X-SmsSink-Token':TOKEN}})` 调

注意：非回环地址的请求在 `server.rs` 就被 403 拦掉了，不要在 handler 里重复判。

### 6.3 改规则匹配逻辑

改 `rules::conditions_met` 或 `extract_from`，同时改 `rules::preview`（管理台「试跑」必须和真实链路给出一致结论，否则用户会以为规则坏了）。`evaluate_outcome` 的语义是"按 priority 降序，首个命中且提取成功的胜出；全失败则回报第一条命中的规则"，改它等于改所有用户的行为。

### 6.4 改卡片版式

`card.rs` 顶部有排版约定的注释。要点：

- 尺寸一律写 96 DPI 逻辑像素，用 `px(dpi, v)` 换算
- 每行文字放在"刚好一行高"的矩形里 + `DT_VCENTER`，不要靠矩形裁切（`DT_WORDBREAK` 会漏出半截字）
- 正文两行是**显式按宽度逐字累加**裁的，见 `fit_lines`
- 按钮宽度按文案实测，右边缘固定、向左长开
- 背景不透明，必须 `SetBkMode(OPAQUE)` + `SetBkColor(底色)`，ClearType 才走最优子像素路径

自绘界面没有布局引擎，改一个数字就可能挤出边界。改完在 125% / 150% 缩放下各看一眼。

### 6.5 改管理台前端

单文件、无构建、无框架，直接编辑 `assets/index.html`。它是 `include_str!` 进来的，改完需要重新 `cargo build` 才能在托盘里看到。调试时可以用 `--probe-ui` 起一个并存实例，端口改成别的，避免打断自己正在用的那份。

### 6.6 改官网页面（`docs/`）

这里有两套投递通道，改一次要顾两边：

| 通道 | 触发 | 生效对象 |
|---|---|---|
| GitHub Pages（`pages.yml`） | push 命中 `docs/**` 白名单 | 外部访客 |
| 内嵌进 exe（`include_str!`） | 重新 `cargo build --release` 并发版 | 已安装用户点「使用教程」/「产品介绍」 |

配套的纪律：

- `docs/sitemap.xml` 里的 `__LASTMOD__` 由 CI `sed` 替换成当天日期，**本地别手写日期**
- `launch.html` 的 JSON-LD `FAQPage` 必须与页面可见 FAQ 文本**逐字一致**，改文案时两处同步（搜索引擎会比对，不一致可能被判为作弊）
- 页面 title / description 的写法约定：搜索词优先（SmsForwarder、短信转发到电脑、验证码自动提取），description 控制在 80 全角宽以内
- 主色分三级：`--blue` 只用于图形和标题，`--blue-ink` 用于小字和白字底色（对比度过 AA），背景用 `--blue-bg`。直接把 `#0377FC` 用到 12px 小字上会掉到 AA 线下
- `docs/img/` 里**只有 `og-cover.jpg` 会被部署**（`pages.yml` 逐文件复制）。新增分享图需要在 workflow 的组装步骤里加一行，否则上线后是 404

### 6.7 改图标

```bash
python tools/make_icon.py            # 重新生成 assets/app.ico（9 个尺寸）+ app.png
cargo build --release                # build.rs 会把 ico 编进 exe 资源
```

图标改完必须重编译，资源是在构建期嵌进去的。

### 6.8 给数据库加字段（当前没有迁移框架）

`store::init` 只有 `CREATE TABLE IF NOT EXISTS`，没有 schema 版本。加字段的安全做法：

1. `CREATE TABLE` 里加列（新库直接带上）
2. 紧接着 `let _ = db.execute("ALTER TABLE messages ADD COLUMN xxx TEXT NOT NULL DEFAULT ''", []);` —— 旧库补列，已存在时的报错直接忽略
3. 查询/写入的 SQL、`Row`、`map_row` 同步改

如果迁移开始变复杂（改类型、拆表），再引入 `PRAGMA user_version` 做版本化，别继续堆 `ALTER`。

---

## 7. 调试与自测

### 7.1 日志

全部走 `println!` / `eprintln!`，没有日志库。启动时会打印版本、配置路径、管理台地址、字体与 DPI；每条推送打印来源、字数、推送延迟、命中规则与提取值。排障时先看这几行。

### 7.2 起一个隔离实例（不打断自己正在用的那份）

单实例互斥体是全局的，所以必须 `--probe-ui`；数据要隔离，就把 `APPDATA` 指到临时目录：

```bash
# Git Bash
mkdir -p /c/Users/<你>/AppData/Local/Temp/smsdev/SmsSink
# 先写一份 config.json（端口挑没占用的，比如 18787，secret 随便设）
cd E:/001-Program/SmsSink
APPDATA='C:/Users/<你>/AppData/Local/Temp/smsdev' ./target/debug/SmsSink.exe --no-ui --probe-ui
```

`--no-ui` 不弹卡片、不动剪贴板，适合验证链路；想看界面就去掉它（但会影响你的真实托盘，注意分辨两个实例）。

### 7.3 造一条合法推送（实测可用）

签名规则是 `Base64(HMAC-SHA256(timestamp + "\n" + secret, secret))`，再 URL 编码一次。

```bash
SECRET=dev-secret
TS=$(date +%s%3N)
SIGN=$(printf '%s\n%s' "$TS" "$SECRET" | openssl dgst -sha256 -hmac "$SECRET" -binary | base64)

curl.exe -s --noproxy '*' -X POST http://127.0.0.1:8787/hook \
  --data-urlencode "from=10086" \
  --data-urlencode "content=您的验证码为633382，3分钟内有效。" \
  --data-urlencode "timestamp=$TS" \
  --data-urlencode "sign=$SIGN"
```

PowerShell 版：

```powershell
$secret = 'dev-secret'
$ts = [string]([DateTimeOffset]::UtcNow.ToUnixTimeMilliseconds())
$h = [System.Security.Cryptography.HMACSHA256]::new([Text.Encoding]::UTF8.GetBytes($secret))
$b64 = [Convert]::ToBase64String($h.ComputeHash([Text.Encoding]::UTF8.GetBytes("$ts`n$secret")))
$sign = [Uri]::EscapeDataString($b64)
curl.exe -s -X POST http://127.0.0.1:8787/hook `
  --data-urlencode "from=10086" --data-urlencode "content=code 633382" `
  --data-urlencode "timestamp=$ts" --data-urlencode "sign=$sign"
```

> 两个坑：**`--noproxy '*'` 不能省**，否则系统代理会把 `127.0.0.1` 也转出去（报 `upstream connect failed`）；PowerShell 里 `content` 带中文时，受控制台代码页影响可能变成乱码，先用英文内容确认链路通，再测中文。

预期返回与日志：

```
{"code":0,"msg":"success"}

[sink] 接到推送 from=10086 字数=26
[sink]   推送延迟 269 ms
[sink]   2026-11-13 00:41:32 规则「短信验证码」→ 提取 633382 | 通知=card | 自动复制=false
```

用同一个 `timestamp` + `sign` 再发一次，应当返回 `401 重复请求（疑似重放）`；改坏 `sign` 返回 `401 sign 校验失败`。

### 7.4 连通性自检

```bash
curl.exe -s --noproxy '*' http://127.0.0.1:8787/ping
# {"code":0,"app":"SmsSink","status":"reachable","peer":"192.168.1.23"}
```

手机浏览器打开 `http://<电脑IP>:8787/ping` 能返回就说明网络和防火墙通了，剩下的问题一定在签名或字段名上。这是排障时最有价值的一步，别跳过。

### 7.5 直接调管理台 API

```bash
curl.exe -s --noproxy '*' -H "X-SmsSink-Token: dev-token" \
  "http://127.0.0.1:8787/api/messages?limit=5"
```

令牌三种给法都行：query `?token=`、header `X-SmsSink-Token`、`Authorization: Bearer`。不带令牌返回 401。

### 7.6 本地预览官网页面

`docs/launch.html` 是自包含单文件，直接双击打开即可，不需要起服务器（唯一的外链是同目录的 `tutorial.html`，两者放在一起就正常）。

### 7.7 故障对照

| 现象 | 先查什么 |
|---|---|
| 手机端"请求失败" | 电脑日志里有没有 `[sink] 连接来自 <IP>`。没有 = 请求没到电脑（防火墙/网段/监听地址），有 = 到了但被拒 |
| 开了加签后全被拒 | 时间戳是秒级还是毫秒级；`secret` 两端是否一致；签名有没有被多重编码 |
| 界面字体变宋体 | `font.rs` 的 `installed()` 没匹配到 `Microsoft YaHei UI`，看启动日志打印的实际字族 |
| 字体发虚 | DPI 感知是否在第一个窗口之前声明；`app.manifest` 的 `permonitorv2` 是否被改掉 |
| 双击 exe 没反应 | 多半是已有实例在跑；看托盘提示条有没有弹「已在运行」 |
| Toast 不弹只进通知中心 | 预期行为，见 5.4；改用 `notify: "card"` |
| 菜单动作没回执 | `hint.rs` 的窗口是否创建失败（看 stderr） |

---

## 8. 测试

当前 7 个单元测试，全部集中在"签名兼容"和"局域网地址判定"这两处最容易被改坏的地方：

| 测试 | 守的是什么 |
|---|---|
| `form_sign_with_plus_survives_parsing` | Base64 里的 `+` 经表单解析变空格后必须换回，否则约 1/3 的推送随机失败 |
| `raw_plus_sign_is_tolerated` | 客户端没编码 `+` 时的容错 |
| `seconds_timestamp_is_normalized` | 10 位秒级时间戳要乘 1000 |
| `double_encoded_sign_is_accepted` | 官方 App 的双重编码链路 |
| `json_channel_encoded_sign_is_accepted` | JSON 通道的残留编码 |
| `query_sign_is_merged_into_body` | GET 通道把 timestamp/sign 放 query 的情况 |
| `local_v4_is_usable` | 枚举出的地址得能真的用 |

```bash
cargo test                    # 日常
cargo test --release --locked # CI 发版前
```

加测试的规矩：**只测纯函数和确定性逻辑**。窗口、剪贴板、Toast、注册表这些有线程和桌面会话依赖的东西不做单测，靠 7.x 的手工链路验证。测签名时不要写死时间戳，用 `win::now_ms()` 现算，否则半年后测试会因为超出时间窗而红。

---

## 9. 发布

三条流水线，职责不重叠：

| 工作流 | 触发 | 做什么 | 耗时 |
|---|---|---|---|
| `ci.yml` | push 到 `main`（忽略 `docs/**`、`*.md`）、PR、手动 | `cargo fmt --check` → `cargo clippy`（提示不阻断）→ `cargo test` | ~1 分 45 秒 |
| `pages.yml` | push 命中 `docs/**` 白名单 | 组装 `_site`（launch 作首页）、替换 `__LASTMOD__`、部署 Pages | ~1 分钟 |
| `release.yml` | push tag `v*`、手动 | 校验 tag 与 `Cargo.toml` 版本一致 → 测试 → 安装包 + 便携版 → GitHub Release | ~2–3 分钟（含编译） |

### 发一个版本

```bash
# 1. 改版本号（exe 资源版本、安装包名、Release 标题都读这里）
#    Cargo.toml: version = "0.3.0"
cargo build --release          # 同时刷新 Cargo.lock 里根包的版本

# 2. 提交（不会触发构建）
git add Cargo.toml Cargo.lock && git commit -m "chore: 版本号 0.3.0" && git push

# 3. 打标签并只推这个标签
git tag -a v0.3.0 -m "SmsSink 0.3.0"
git push origin v0.3.0

# 4. 等 Actions 跑完，Release 自动带两个产物和更新说明
```

**版本号只有一处真相**：`Cargo.toml` 的 `version`。`build.rs` 拿它生成 exe 资源版本（`0.3.0` → `0.3.0.0`），`installer/build.ps1` 校验 exe 资源版本与目标一致（防止改了 Cargo.toml 忘记重编译），`release.yml` 首步校验 tag 名与之一致（防止 tag 打错）。

### 只要更新官网

改 `docs/` 下的文件并 push 到 `main` 即可，一分钟上线，不用发版。注意：**这次改动不会进 exe**，已经装了旧版的用户点「使用教程」看到的还是旧页面，要等下一个版本。

### 回滚

- 官网：`git revert <commit>` 再 push，`pages.yml` 重新部署
- 版本：GitHub Release 页可以直接把某个版本标为 latest 或删除；代码侧 `git revert` 后重新打一个小版本 tag（不要移动已发布的 tag，会触发二次构建并覆盖同名产物、下载计数归零）

---

## 10. 红线

改代码时这些不要碰，碰之前先读完对应小节：

- ❌ 在 HTTP 线程直接操作窗口 / 剪贴板 / Toast（→ 铁律一）
- ❌ 用 `Shell_NotifyIcon` 气泡做菜单回执（→ 5.4）
- ❌ 把 `--probe-ui` 的保护去掉，或改单实例互斥体的名字（→ 5.7）
- ❌ 给"同事也会触发"的高频规则默认 `auto_copy: true`——会覆盖用户剪贴板里正在用的内容
- ❌ 在 UI 线程做同步 IO 或全表扫描（→ 铁律二）
- ❌ 页面里引入外链图片、CDN、远程字体（→ 5.6）
- ❌ 把 `admin_token` 拼进教程/介绍页的 URL（静态页不需要令牌，泄露无收益有风险）
- ❌ 为消 clippy 警告把 `HWND`/`MSG` 改成驼峰（→ 5.5）
- ❌ 移动已发布的 tag（→ 9 回滚）

---

## 11. 已知技术债

按影响排序，欢迎接手时偿还：

1. **没有代码签名**：SmartScreen 会提示「未知发布者」。`release.yml` 里已预留位置，需要一张 OV/EV 证书。
2. **clippy 未开 `-D warnings`**：现有约 37 条警告，多数是 FFI 类型命名（刻意保留），其余是零散风格问题（`Box<String>`、`assert_eq!(x, true)`、多余的 `as_bytes`）。清零后应把 CI 切成阻断模式（`ci.yml` 里有注释标记）。
3. **启动时端口预检与真正 bind 之间有一次重复绑定**（→ 第 2 节末尾），窄竞态。
4. **进程退出后通知中心里的 Toast 按钮失效**：需要实现 COM `INotificationActivationCallback`，属于二期。
5. **数据库无 schema 版本**：只有 `CREATE TABLE IF NOT EXISTS`，字段多了以后靠 `ALTER` 堆叠会难维护（→ 6.8）。
6. **教程页在 500px 宽度下横向溢出**（`<pre>` 代码块，既有问题，非某次改动引入）。桌面场景不受影响。
7. **没有自动更新**：用户要自己来 Release 页下载。

---

## 12. 交给下一个 AI 会话

如果继续由 AI 接手，这些约定能让它少走弯路：

- **注释写"为什么"，不写"是什么"**。现有代码里的长注释几乎都是踩坑记录（某个 API 为什么不能用、某个数字为什么是这个值）。保留它们，比保留测试更有价值。
- **提交信息**：`feat:` / `fix:` / `docs:` / `chore:` / `refactor:` + 中文描述，一句话说清改了什么、为什么。
- **改完必须自己验**：`cargo fmt` + `cargo build` + `cargo test`，涉及链路的按第 7 节实测一遍。别只声称"已修复"。
- **别相信文档里的旧描述**：这个文件描述的是当前状态，代码变了就同步改这里，否则下一个人（或下一个会话）会被误导。
- **改动范围最小化**：这是个人项目，不需要为未来可能的扩展做抽象。三个重复的 `match` 比一个泛型 trait 更好维护。
- **用户偏好**：中文界面文案、不用 emoji、不用花哨动效；面向普通用户，错误提示要给出"下一步怎么办"而不是错误码。

---

最后更新：2026-11-13（对应 v0.2.0）
