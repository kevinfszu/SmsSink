//! 本地 SQLite 落盘：最近记录、长期统计、CSV 导出、保留期清理。
//! 数据文件只放当前用户目录，进程内单连接 + Mutex，无连接池、无常驻额外线程。
use crate::event::SmsEvent;
use crate::win;
use rusqlite::{params, Connection};
use serde::Serialize;
use serde_json::json;
use std::path::Path;
use std::sync::{Mutex, OnceLock};

static DB: OnceLock<Mutex<Connection>> = OnceLock::new();

#[derive(Debug, Serialize)]
pub struct Row {
    pub id: i64,
    pub ts_ms: i64,
    pub local_time: String,
    pub sender: String,
    pub content: String,
    pub rule_name: String,
    pub extracted: String,
    pub notify: String,
    pub auto_copy: bool,
    pub copied: bool,
}

fn conn() -> Option<&'static Mutex<Connection>> {
    DB.get()
}

pub fn init(path: &Path) -> Result<(), String> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(|e| format!("创建数据目录失败: {e}"))?;
    }
    let db = Connection::open(path).map_err(|e| format!("打开 SQLite 失败: {e}"))?;
    db.execute_batch(
        r#"
        PRAGMA journal_mode = WAL;
        PRAGMA synchronous = NORMAL;
        CREATE TABLE IF NOT EXISTS messages (
            id          INTEGER PRIMARY KEY AUTOINCREMENT,
            ts_ms       INTEGER NOT NULL,
            local_time  TEXT    NOT NULL,
            sender      TEXT    NOT NULL,
            content     TEXT    NOT NULL,
            rule_name   TEXT    NOT NULL DEFAULT '',
            extracted   TEXT    NOT NULL DEFAULT '',
            notify      TEXT    NOT NULL DEFAULT '',
            auto_copy   INTEGER NOT NULL DEFAULT 0,
            copied      INTEGER NOT NULL DEFAULT 0
        );
        CREATE INDEX IF NOT EXISTS idx_messages_ts     ON messages(ts_ms);
        CREATE INDEX IF NOT EXISTS idx_messages_sender ON messages(sender);
        CREATE INDEX IF NOT EXISTS idx_messages_rule   ON messages(rule_name);
        "#,
    )
    .map_err(|e| format!("初始化表结构失败: {e}"))?;
    let _ = DB.set(Mutex::new(db));
    Ok(())
}

/// 原文打码：保留提取出的验证码本身，其余数字替换为圆点
pub fn mask_digits(content: &str, keep_code: Option<&str>) -> String {
    const SENTINEL: char = '\u{E000}';
    let guarded = match keep_code {
        Some(code) if !code.is_empty() => content.replace(code, &SENTINEL.to_string()),
        _ => content.to_string(),
    };
    let masked: String = guarded
        .chars()
        .map(|c| if c.is_ascii_digit() { '•' } else { c })
        .collect();
    match keep_code {
        Some(code) if !code.is_empty() => masked.replace(SENTINEL, code),
        _ => masked,
    }
}

pub fn record(ev: &SmsEvent, stored_content: &str, copied: bool) -> i64 {
    let Some(db) = conn() else {
        return 0;
    };
    let Ok(guard) = db.lock() else {
        return 0;
    };
    let (rule, code, notify, auto_copy) = match &ev.decision {
        Some(d) => (
            d.rule_name.clone(),
            d.code.clone(),
            d.notify.as_str().to_string(),
            d.auto_copy,
        ),
        // 命中规则但没提取到码：规则名要记下来，通知方式标记成 unparsed，
        // 管理台据此把它和"完全没命中"区分开（前者是规则该改的信号）
        None => match &ev.no_code {
            Some(hit) => (hit.rule_name.clone(), String::new(), "unparsed".into(), false),
            None => (String::new(), String::new(), "none".into(), false),
        },
    };
    let result = guard.execute(
        "INSERT INTO messages
         (ts_ms, local_time, sender, content, rule_name, extracted, notify, auto_copy, copied)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
        params![
            // 统计与保留期都以本机接收时间为准，避免手机端时间偏差影响聚合
            win::now_ms(),
            ev.local_time,
            ev.from,
            stored_content,
            rule,
            code,
            notify,
            auto_copy as i32,
            copied as i32,
        ],
    );
    match result {
        Ok(_) => guard.last_insert_rowid(),
        Err(e) => {
            eprintln!("[store] 写入失败: {e}");
            0
        }
    }
}

pub fn mark_copied(id: i64) {
    if id <= 0 {
        return;
    }
    if let Some(db) = conn() {
        if let Ok(guard) = db.lock() {
            let _ = guard.execute("UPDATE messages SET copied = 1 WHERE id = ?1", params![id]);
        }
    }
}

/// 托盘菜单用的轻量汇总：今日条数 + 最后一条时间（没有记录时时间串为空）。
///
/// 菜单每次弹出都会调用，必须足够便宜：走 local_time 前缀匹配，命中一次索引扫描。
pub fn today_summary() -> (i64, String) {
    let Some(db) = conn() else {
        return (0, String::new());
    };
    let Ok(guard) = db.lock() else {
        return (0, String::new());
    };
    let today = win::local_time();
    let today = &today[..10.min(today.len())];
    guard
        .query_row(
            "SELECT COUNT(*), IFNULL(MAX(local_time), '')
             FROM messages WHERE substr(local_time, 1, 10) = ?1",
            params![today],
            |r| Ok((r.get::<_, i64>(0)?, r.get::<_, String>(1)?)),
        )
        .unwrap_or((0, String::new()))
}

fn map_row(r: &rusqlite::Row<'_>) -> rusqlite::Result<Row> {
    Ok(Row {
        id: r.get(0)?,
        ts_ms: r.get(1)?,
        local_time: r.get(2)?,
        sender: r.get(3)?,
        content: r.get(4)?,
        rule_name: r.get(5)?,
        extracted: r.get(6)?,
        notify: r.get(7)?,
        auto_copy: r.get::<_, i32>(8)? != 0,
        copied: r.get::<_, i32>(9)? != 0,
    })
}

const SELECT: &str = "SELECT id, ts_ms, local_time, sender, content, rule_name, extracted,
                             notify, auto_copy, copied FROM messages";

pub fn recent(
    limit: usize,
    offset: usize,
    sender: &str,
    rule: &str,
    query: &str,
) -> Vec<Row> {
    let Some(db) = conn() else {
        return Vec::new();
    };
    let Ok(guard) = db.lock() else {
        return Vec::new();
    };
    let sql = format!(
        "{SELECT}
         WHERE (?1 = '' OR sender LIKE '%' || ?1 || '%')
           AND (?2 = '' OR rule_name = ?2)
           AND (?3 = '' OR content LIKE '%' || ?3 || '%' OR extracted LIKE '%' || ?3 || '%')
         ORDER BY id DESC LIMIT ?4 OFFSET ?5"
    );
    let mut stmt = match guard.prepare(&sql) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("[store] 查询失败: {e}");
            return Vec::new();
        }
    };
    let rows = stmt.query_map(
        params![sender, rule, query, limit as i64, offset as i64],
        map_row,
    );
    match rows {
        Ok(iter) => iter.filter_map(Result::ok).collect(),
        Err(e) => {
            eprintln!("[store] 查询失败: {e}");
            Vec::new()
        }
    }
}

fn pairs(guard: &Connection, sql: &str, since: i64) -> Vec<(String, i64)> {
    let mut stmt = match guard.prepare(sql) {
        Ok(s) => s,
        Err(_) => return Vec::new(),
    };
    let out = match stmt.query_map(params![since], |r| {
        Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)?))
    }) {
        Ok(iter) => iter.filter_map(Result::ok).collect(),
        Err(_) => Vec::new(),
    };
    out
}

pub fn stats(days: u32) -> serde_json::Value {
    let Some(db) = conn() else {
        return json!({"total": 0});
    };
    let Ok(guard) = db.lock() else {
        return json!({"total": 0});
    };
    let days = days.clamp(1, 3650) as i64;
    let since = win::now_ms() - days * 86_400_000;

    let scalar = |sql: &str| -> i64 {
        guard
            .query_row(sql, params![since], |r| r.get(0))
            .unwrap_or(0)
    };
    let total = scalar("SELECT count(*) FROM messages WHERE ts_ms >= ?1");
    let matched = scalar("SELECT count(*) FROM messages WHERE ts_ms >= ?1 AND rule_name <> ''");
    let copied = scalar("SELECT count(*) FROM messages WHERE ts_ms >= ?1 AND copied = 1");

    let by_day = pairs(
        &guard,
        "SELECT substr(local_time, 1, 10) d, count(*) FROM messages
         WHERE ts_ms >= ?1 GROUP BY d ORDER BY d",
        since,
    );
    let by_rule = pairs(
        &guard,
        "SELECT CASE WHEN rule_name = '' THEN '未命中' ELSE rule_name END r, count(*)
         FROM messages WHERE ts_ms >= ?1 GROUP BY r ORDER BY 2 DESC LIMIT 20",
        since,
    );
    let by_sender = pairs(
        &guard,
        "SELECT sender, count(*) FROM messages
         WHERE ts_ms >= ?1 GROUP BY sender ORDER BY 2 DESC LIMIT 20",
        since,
    );

    json!({
        "days": days,
        "total": total,
        "matched": matched,
        "unmatched": total - matched,
        "copied": copied,
        "by_day": by_day.iter().map(|(k,v)| json!({"label":k,"value":v})).collect::<Vec<_>>(),
        "by_rule": by_rule.iter().map(|(k,v)| json!({"label":k,"value":v})).collect::<Vec<_>>(),
        "by_sender": by_sender.iter().map(|(k,v)| json!({"label":k,"value":v})).collect::<Vec<_>>(),
    })
}

fn csv_cell(s: &str) -> String {
    if s.contains([',', '"', '\n', '\r']) {
        format!("\"{}\"", s.replace('"', "\"\""))
    } else {
        s.to_string()
    }
}

pub fn export_csv(sender: &str, rule: &str, query: &str) -> String {
    let rows = recent(100_000, 0, sender, rule, query);
    let mut out = String::from("\u{FEFF}ID,时间,来源,命中规则,提取值,通知方式,自动复制,已复制,原文\n");
    for r in rows {
        out.push_str(&format!(
            "{},{},{},{},{},{},{},{},{}\n",
            r.id,
            csv_cell(&r.local_time),
            csv_cell(&r.sender),
            csv_cell(&r.rule_name),
            csv_cell(&r.extracted),
            csv_cell(&r.notify),
            if r.auto_copy { "是" } else { "否" },
            if r.copied { "是" } else { "否" },
            csv_cell(&r.content),
        ));
    }
    out
}

pub fn purge(retention_days: u32) {
    if retention_days == 0 {
        return;
    }
    if let Some(db) = conn() {
        if let Ok(guard) = db.lock() {
            let cutoff = win::now_ms() - retention_days as i64 * 86_400_000;
            match guard.execute("DELETE FROM messages WHERE ts_ms < ?1", params![cutoff]) {
                Ok(n) if n > 0 => println!("[store] 已清理 {n} 条超期记录"),
                _ => {}
            }
        }
    }
}
