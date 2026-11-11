//! 全局共享状态：配置（可热重载）与加签重放去重集合。
use crate::config::Config;
use std::collections::HashSet;
use std::sync::{Arc, Mutex, OnceLock, RwLock};

pub struct AppState {
    pub cfg: RwLock<Config>,
    pub seen: Mutex<HashSet<String>>,
}

static APP: OnceLock<Arc<AppState>> = OnceLock::new();

pub fn init(cfg: Config) -> Arc<AppState> {
    let state = Arc::new(AppState {
        cfg: RwLock::new(cfg),
        seen: Mutex::new(HashSet::new()),
    });
    let _ = APP.set(Arc::clone(&state));
    state
}

pub fn get() -> Arc<AppState> {
    APP.get().expect("AppState 尚未初始化").clone()
}

pub fn reload_config() -> Result<(), String> {
    let fresh = crate::config::load();
    let state = get();
    let mut guard = state.cfg.write().map_err(|e| e.to_string())?;
    *guard = fresh;
    Ok(())
}

pub fn replace_rules(rules: Vec<crate::config::Rule>) -> Result<(), String> {
    let state = get();
    let mut guard = state.cfg.write().map_err(|e| e.to_string())?;
    guard.rules = rules;
    crate::config::save(&guard).map_err(|e| e.to_string())?;
    Ok(())
}
