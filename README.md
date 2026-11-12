# SMS 接码器 · PC 版 / SmsSink

Windows 11 上被动接收 [SmsForwarder](https://github.com/pppscn/SmsForwarder) 推送的短信，按自定义规则提取内容（主要是数字验证码），并及时通知。手机与电脑在同一局域网时直连，无需任何中转服务或云函数。

常驻形态是**托盘 + 阻塞式消息泵**：空闲时 CPU 为 0，无常驻异步运行时、无 WebView2、无浏览器进程。管理台是 `127.0.0.1` 上的本地网页，用系统默认浏览器打开。

## 快速开始

> 📖 **新手用户**：请看图文教程 [docs/tutorial.html](docs/tutorial.html)（单文件、图片内嵌，可直接发给朋友或在浏览器打开），10 分钟完成手机↔电脑全链路配置。

```powershell
cargo build --release
.\target\release\SmsSink.exe
```

首次启动会生成 `config.json`（优先 exe 同目录，回退 `%APPDATA%\SmsSink\`）和 `%APPDATA%\SmsSink\data.db`，并打印管理台地址与访问令牌：

```
管理台 http://127.0.0.1:8787/?token=<32位十六进制>
```

**双击启动只有托盘图标，不会弹控制台窗口**：Release 以 GUI 子系统（`windows_subsystem = "windows"`）链接。启动方式决定日志去向：

| 启动方式 | 控制台 | 日志 |
|---|---|---|
| 双击 exe / 开机自启 | 无 | 丢弃（stdout 指向 NUL） |
| 从终端（PowerShell / cmd / Windows Terminal）启动 | 复用该终端 | 直接打印在终端里 |

所以要看启动日志就用终端跑一次；日常使用双击即可。调试构建（`cargo run`）仍带控制台。

托盘图标右键菜单：**打开管理台 / 使用教程 / 产品介绍 / 最近验证码 / 发送测试通知 / 重载配置 / 开机自动启动 / 退出**。  
「使用教程」与「产品介绍」两页随 exe 一起编进资源（`/tutorial`、`/launch`），从本地服务打开，不依赖安装目录里的 `docs/`——exe 单独拷走也能看。管理台右上角也有「使用教程」入口。
开机自启写 `HKCU\Software\Microsoft\Windows\CurrentVersion\Run`，不需要管理员权限。进程用互斥体保证单实例。

> Windows 11 默认把新应用的托盘图标放进「隐藏的图标」折叠区。想让它常驻任务栏，点任务栏的 `^` 把图标拖到外面即可（本机已为该路径设置 `IsPromoted=1`）。

调试或自动化时用 `SmsSink.exe --no-ui`：只接收与落盘，不创建托盘、不弹通知、不写剪贴板。`--help` 查看参数。

## 手机端 SmsForwarder 配置

1. 添加发送通道 → **Webhook**
2. 请求方法：`POST`
3. URL：管理台「设置 → 手机端 SmsForwarder」会自动填好本机局域网地址，点「复制」粘贴过去（形如 `http://192.168.x.x:8787/hook`）。多网卡时页面会出现「本机地址」下拉框，选与手机同网段的那个。**手机连电脑热点时**，地址固定用热点网卡的 `http://192.168.137.1:8787/hook`——Windows 强主机模型会丢弃「从热点进、目标是 WiFi 网卡 IP」的包，填 WiFi 网卡地址（如 `192.168.31.x`）手机会一直「请求失败」
4. `webParams` 留空，使用默认 form 报文（`from` / `content` / `timestamp` / `sign`）
5. 「成功标识」填 `success`（服务端返回 `{"code":0,"msg":"success"}`）
6. 可选：填写 Secret 启用加签，需与 `config.json` 的 `secret` 一致

也兼容自定义 JSON 模板：`webParams` 以 `{` 开头时，服务端从 `from`/`sender`、`content`/`msg`/`message`/`text`、`timestamp`/`time`、`sign`/`signature` 中取字段。

加签算法为 `urlEncode(Base64(HmacSHA256(timestamp + "\n" + secret, secret)))`，服务端校验 ±300 秒时间窗并做重放去重。`secret` 留空时自动跳过校验，便于先联调。

## 网络与防火墙

Windows 默认拦截入站连接，需以管理员身份添加一条入站规则。规则必须覆盖**所有网络配置文件**（`-Profile Any`），并用 `LocalSubnet` 把来源限制在本地子网：

```powershell
New-NetFirewallRule `
  -DisplayName "SMS 接码器 · 局域网入站" `
  -Direction Inbound -Protocol TCP -LocalPort 8787 `
  -Action Allow -Profile Any -RemoteAddress LocalSubnet `
  -Program "E:\001-Program\SmsSink\target\release\SmsSink.exe"
```

> **不要用 `-Profile Private`。** 电脑开启「移动热点」让手机直连时，热点虚拟网卡（`Microsoft Wi-Fi Direct Virtual Adapter`，IP 通常是 `192.168.137.1`）属于**公用网络**，专用网络规则不会对它生效，手机端只会得到「请求失败：网络连接超时」。已建的规则可就地修正：
>
> ```powershell
> Set-NetFirewallRule -DisplayName "SMS 接码器 · 局域网入站" -Profile Any -RemoteAddress LocalSubnet
> ```
>
> 也可直接双击 `scripts\fix-firewall.cmd`（自动申请管理员权限，并清理指向旧目录的失效规则）。

### 连不上时先做连通性自检

手机浏览器直接打开 `http://<电脑IP>:8787/ping`，返回 `{"code":0,...,"peer":"192.168.137.51"}` 即说明网络与防火墙都通了，此时再排查 SmsForwarder 通道配置；若浏览器同样超时，问题在网络层，不在本程序。

- 服务端自查：程序需在运行且监听 `0.0.0.0:8787`（`Get-NetTCPConnection -LocalPort 8787 -State Listen`）。管理台与 `/api/*` 仅允许本机回环访问，手机访问会返回 403，这是预期行为。
- 手机若同时开着移动数据，部分 Android 会在 Wi-Fi 被判定「无互联网」时把请求切到蜂窝网络，`192.168.137.1` 自然不可达。排查时先关闭移动数据，并确认 Wi-Fi 详情里手机 IP 在 `192.168.137.x` 网段。
- 建议在路由器上给电脑绑定固定 IP，否则 DHCP 续租后手机端会发丢。**电脑休眠时收不到推送**，需要在电源设置里允许不休眠或网络唤醒。

## 规则

管理台「规则管理」里增删改，保存后立即生效，无需重启。也支持直接编辑 `config.json` 后点托盘「重载配置」。

| 字段                                          | 说明                                                    |
| ------------------------------------------- | ----------------------------------------------------- |
| `name` / `enabled` / `priority`             | 规则名、开关、优先级（大者优先，首个命中即用）                               |
| `match_from`                                | 发送方号码 / 包名，包含即命中；留空不限来源                               |
| `keywords_any` / `keywords_all` / `exclude` | 任一包含 / 全部包含 / 排除关键词                                   |
| `extract` / `capture`                       | 提取正则与捕获组序号                                            |
| `notify`                                    | `toast`（通知中心 + 复制按钮）/ `card`（右下角大号卡片）/ `silent`（仅入历史） |
| `auto_copy`                                 | 收到即写入剪贴板，**默认 false**                                 |
| `copy_template`                             | 复制文本模板，支持 `{code}` `{from}` `{content}`               |
| `ttl_sec` / `keep_history`                  | 验证码有效期提示 / 是否落盘留痕                                     |

样例短信：

```
【中国移动统一认证】您的验证码为633382，您正在使用短信验证码登录功能，该验证码仅用于身份验证，3分钟内有效，请勿泄露给他人使用。
```

默认正则 `(?:验证码|校验码|动态密码)[^\d]{0,8}(\d{4,8})` 提取出 `633382`；「您的订单号 123456 已发货」不会误命中。**不要用裸 `\d{6}`**，订单号、金额、卡号后六位都会误报。编辑页有「样例短信试跑」，可先看结果再保存。

> 高频共用业务系统里，短信内容无法区分归属，请把该规则的 `auto_copy` 保持 `false`，否则会冲掉你正在使用的剪贴板。需要哪条码，到「最近记录」里单独点复制。

## 通知与复制

- **卡片**（默认）：右下角自绘 Win32 窗口，大号显示验证码 + 真按钮 + 倒计时自动消失，不进通知中心，不抢焦点。底部一条细进度条显示剩余时间，鼠标停在卡片上会暂停倒计时。
- **Toast**：进入系统通知中心，带「复制验证码」「打开管理台」两个按钮；用唯一 `Tag` 堆叠、不互相覆盖，按 `toast_limit` 自动移除最旧的一条。**注意**：未打包应用只有注册了带 `AppUserModelID` 的开始菜单快捷方式，系统才会放行横幅与按钮；本机实测只有通知中心条目、屏幕不弹横幅，所以默认档位是 `card`（见 `docs/ux-audit.md` 的 UF-5）。
- **静默**：不弹任何东西，只在管理台记录里出现。

关键规则命中关键词、但正则没取到数字时，**不再静默丢弃**：会弹一张「未识别到验证码」的卡片（正文是短信原文，按钮为「关闭 / 复制原文」），并照常入库（通知方式记为 `unparsed`）。对验证码工具来说，"什么都没收到"是最不能接受的反馈。

自动复制按规则开关；所有通知/面板的复制都走同一个剪贴板入口，并回写 `copied` 标记用于统计。

## 托盘与回执

托盘图标左键单击 = 打开管理台，右键或双击 = 菜单。菜单结构固定为：状态行（`监听 8787 · 今日 N 条 · 最后 HH:MM`，灰显）→ 最近验证码（最近 3 条，选中即复制）→ 打开管理台 / 使用教程 / 产品介绍 / 发送测试通知 → 设置（开机自动启动、重载配置）→ 退出。鼠标悬停在托盘图标上会显示 `SMS 接码器 · PC 版 · 监听 8787 · 今日 N 条 · 最后 HH:MM`——应用名在前，状态在后。

Win32 的托盘气泡（`NIF_INFO`）在没在开始菜单注册身份的应用里会被系统直接吞掉（实测消息送达、屏幕上什么都没有），所以程序自带一个单行提示条 `src/hint.rs`（40 逻辑像素高、2.6 秒自动消失、不抢焦点）作为所有菜单动作的可见回执，例如 `配置已重载 · 3 条规则生效`、`已复制 633382`。程序已在运行时再次双击 exe，新进程会把请求转给常驻实例，由它弹一条 `SmsSink 已在运行 · 右键托盘图标操作`，而不是静默退出。

## 数据与统计

SQLite 落盘于 `%APPDATA%\SmsSink\data.db`（可用 `db_path` 改），记录时间、来源、原文、命中规则、提取值、通知方式、自动复制与已复制状态，建了时间/来源/规则三个索引。

管理台「统计分析」支持近 7/30/90/365 天的总量、命中率、每日柱状图、按规则与按来源聚合，以及 CSV 导出。`retention_days` 默认 180 天，启动时自动清理超期记录；`mask_content = true` 时原文中的数字（除提取出的验证码外）会打码落盘。

管理台仅监听回环地址（`0.0.0.0:8787` 上只放行 `/hook`），所有 `/api/*` 需要 `admin_token`。非本机 IP 访问管理接口一律 403。

## 已知限制

- 未打包 Win32 应用的 Toast 按钮激活依赖 AUMID + 进程内 `Activated` 事件。启动时会自动设置 AUMID 并写入 `HKCU\Software\Classes\AppUserModelId\SmsSink.Desktop`；个别环境若 Toast 按钮失效，兜底是管理台「最近记录」里逐条复制，核心链路不受影响。
- 进程退出后，通知中心里遗留的 Toast 按钮不再可用（需二期实现 COM `INotificationActivationCallback`）。
- 端口、监听地址、hook 路径变更后需重启程序；其余配置热重载即可。

## 界面与排版

自绘界面只有两处容易显廉价，都在早期踩过：

1. **字体必须自带中文字形**。早期卡片只请求 `Segoe UI`，它没有汉字，GDI 的字体链接会把每个汉字换成宋体——宋体是衬线点阵字体，小字号下笔画细、粗细不匀，和旁边的无衬线数字完全两种气质，整体就散了。现在统一用 `Microsoft YaHei UI`（系统 zh-CN 界面字体，中英混排一致），并在 `src/font.rs` 里用 `EnumFontFamiliesExW` 确认字族真的存在，避免 GDI 静默替换。
2. **DPI 感知必须早于第一扇窗**。没有 manifest 时进程默认「不感知 DPI」，Windows 会先按 96 DPI 渲染整张卡片再位图放大 1.25 倍，字体边缘必然发虚。现在 `app.manifest` 声明 `permonitorv2`，启动第一件事就是 `win::init_dpi_awareness()`，尺寸一律按 `dpi / 96` 换算。程序启动日志会打印实际字体与 DPI，例如 `界面字体 Microsoft YaHei UI · DPI 120 (125%)`。

排印细节：每行字都放在「刚好一行高」的矩形里并 `DT_VCENTER` 垂直居中，避免各行基线忽高忽低；背景是不透明实色，因此 `SetBkMode(OPAQUE)` + `SetBkColor(卡片底色)`，ClearType 才能走最优子像素路径。

卡片的纵向节奏（380×192 逻辑像素）：标题行 `16–34`（时间右对齐在同一行）、验证码 `40–94`（36px）、原文 `100–136`（两行，超出补 `…`）、按钮贴着底边。三处细节值得记一笔：

- **正文两行是显式裁的**，不靠 `DrawText` 的矩形裁切：`DT_WORDBREAK` 只要矩形够高就会漏出第三行半截字，所以先按可用宽度算总预算、逐字累加到放不下为止，再补省略号。
- **按钮宽度按文案实测**（`GetTextExtentPoint32W` + 两侧各 14px，下限 52px）。写死宽度会让「复制」两个字两侧各空 20 多像素；状态文案变长（`已复制` / `复制失败`）时按钮向左长开、右边缘不动，视觉上不跳。
- **按钮是子类化的**：`WM_SETCURSOR` 换成手型光标，`WM_MOUSEMOVE` / `WM_MOUSELEAVE` 自己上报 hover。鼠标一落到子按钮上就不再有消息冒泡到卡片，只靠轮询会慢半拍。

## 目录

```
Cargo.toml            依赖与 Release 优化（lto + strip）
build.rs              编译期把图标与版本信息写入 exe 资源
app.rc                资源脚本：主图标 ID=1、manifest、VERSIONINFO
app.manifest          DPI 感知（permonitorv2）与以当前用户身份运行
src/main.rs           启动编排、DPI 感知、单实例、COM/AUMID、DB 初始化
src/config.rs         配置结构、落盘、令牌、DB 路径
src/app.rs            可热重载的全局状态
src/win.rs            Win32 手写 FFI（托盘/剪贴板/注册表/自启/控制台/图标）
src/font.rs           界面字体选型（中英同一套字形）与按 DPI 建字体
src/server.rs         TcpListener 极简 HTTP、form/JSON 解析、加签校验、路由
src/rules.rs          优先级规则匹配、上下文正则提取、试跑
src/event.rs          HTTP 线程 → UI 线程的事件投递与动作分发
src/toast.rs          WinRT Toast + 复制按钮事件
src/card.rs           自绘右下角卡片
src/hint.rs           自绘单行提示条（菜单动作的可见回执）
src/tray.rs           托盘、菜单与消息泵
src/store.rs          SQLite 落盘、统计、CSV、保留期清理
src/admin.rs          管理台 API
assets/index.html     管理台单页（规则 / 记录 / 统计 / 设置）
assets/app.ico        应用图标（9 个尺寸：16～256）
tools/make_icon.py    图标生成脚本，改设计后重跑即可
config.example.json   完整配置示例
docs/ux-audit.md      托盘/卡片/反馈的体验走查（问题分级 + 处理状态 + 复现证据）
```
