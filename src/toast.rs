//! 系统 Toast 通知：通知中心可回查，带「复制验证码」按钮。
//! unpackaged 应用靠 AUMID + 进程内 Activated 事件响应按钮点击。
use crate::app;
use crate::config::Config;
use crate::event::SmsEvent;
use crate::rules::Decision;
use crate::{event, store, win};
use std::sync::Mutex;
use windows::core::{ComInterface, IInspectable, HSTRING};
use windows::Data::Xml::Dom::XmlDocument;
use windows::Foundation::TypedEventHandler;
use windows::UI::Notifications::{
    ToastActivatedEventArgs, ToastNotification, ToastNotificationManager,
};

/// Toast 对象必须保持存活，否则 Activated 事件不会送达。
static LIVE: Mutex<Vec<LiveToast>> = Mutex::new(Vec::new());

struct LiveToast {
    tag: String,
    group: String,
    _toast: ToastNotification,
}

fn xml_escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&apos;")
}

fn admin_url(cfg: &Config) -> String {
    format!("http://127.0.0.1:{}/?token={}", cfg.port, cfg.admin_token)
}

pub fn show(d: &Decision, ev: &SmsEvent, row_id: i64) {
    if let Err(e) = try_show(d, ev, row_id) {
        eprintln!("[toast] 显示失败: {e}（可在管理台「最近记录」里手动复制）");
    }
}

fn try_show(d: &Decision, ev: &SmsEvent, row_id: i64) -> Result<(), String> {
    let cfg = app::get().cfg.read().map_err(|e| e.to_string())?.clone();

    let tag = format!("sms-{}-{}", row_id, win::now_ms());
    let group = d.rule_name.clone();
    let title = format!("{} · 验证码", crate::config::APP_DISPLAY_NAME);
    let body = format!("{}  ·  来自 {}", d.code, ev.from);
    let sub = format!(
        "{} · 规则「{}」· 有效期 {} 秒",
        ev.local_time, d.rule_name, d.ttl_sec
    );
    let copy_text = d.copy_text.clone();
    let url = admin_url(&cfg);

    let xml = format!(
        r#"<toast launch="open" activationType="foreground" duration="long">
  <visual>
    <binding template="ToastGeneric">
      <text>{}</text>
      <text>{}</text>
      <text placement="attribution">{}</text>
    </binding>
  </visual>
  <actions>
    <action content="复制验证码" arguments="copy" activationType="foreground"/>
    <action content="打开管理台" arguments="open" activationType="foreground"/>
  </actions>
  <audio silent="true"/>
</toast>"#,
        xml_escape(&title),
        xml_escape(&body),
        xml_escape(&sub)
    );

    let doc = XmlDocument::new().map_err(|e| e.to_string())?;
    doc.LoadXml(&HSTRING::from(xml.as_str()))
        .map_err(|e| e.to_string())?;
    let toast = ToastNotification::CreateToastNotification(&doc).map_err(|e| e.to_string())?;
    toast
        .SetTag(&HSTRING::from(tag.as_str()))
        .map_err(|e| e.to_string())?;
    toast
        .SetGroup(&HSTRING::from(group.as_str()))
        .map_err(|e| e.to_string())?;

    let handler = TypedEventHandler::new(
        move |_sender: &Option<ToastNotification>, args: &Option<IInspectable>| {
            let Some(raw) = args else {
                return Ok(());
            };
            let Ok(activated) = raw.cast::<ToastActivatedEventArgs>() else {
                return Ok(());
            };
            let Ok(arg) = activated.Arguments() else {
                return Ok(());
            };
            match arg.to_string_lossy().as_str() {
                "copy" => {
                    if event::request_copy(&copy_text).is_ok() {
                        store::mark_copied(row_id);
                        println!("[toast] 按钮复制成功 (row={row_id})");
                    }
                }
                "open" => win::open_url(&url),
                _ => {}
            }
            Ok(())
        },
    );
    toast.Activated(&handler).map_err(|e| e.to_string())?;

    let notifier =
        ToastNotificationManager::CreateToastNotifierWithId(&HSTRING::from(crate::config::AUMID))
            .map_err(|e| e.to_string())?;
    notifier.Show(&toast).map_err(|e| e.to_string())?;

    let mut live = LIVE.lock().unwrap_or_else(|e| e.into_inner());
    live.push(LiveToast {
        tag: tag.clone(),
        group: group.clone(),
        _toast: toast,
    });

    // 高频场景：通知中心最多保留 N 条，超出移除最旧的一条
    while live.len() > cfg.toast_limit.max(1) {
        let old = live.remove(0);
        if let Ok(history) = ToastNotificationManager::History() {
            let _ = history.RemoveGroupedTagWithId(
                &HSTRING::from(old.tag.as_str()),
                &HSTRING::from(old.group.as_str()),
                &HSTRING::from(crate::config::AUMID),
            );
        }
    }
    Ok(())
}

/// 退出时清掉本应用留下的通知，避免遗留无法点击的按钮
pub fn clear_all() {
    if let Ok(history) = ToastNotificationManager::History() {
        let _ = history.Clear();
    }
}
