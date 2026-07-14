//! Персистентная история сжатий: какие папки были сжаты, когда и
//! сколько места освобождено. Хранится в history.json в каталоге
//! данных приложения; используется для отката (декомпрессии).

use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use tauri::{AppHandle, Manager};

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct HistoryEntry {
    /// Папка игры.
    pub root: String,
    /// Unix-время завершения сжатия (секунды).
    pub date: u64,
    /// Сколько файлов было обработано.
    pub files: u64,
    /// Логический размер обработанных файлов.
    pub original_bytes: u64,
    /// Сэкономлено места.
    pub saved_bytes: i64,
    /// true, если сжатие было прервано отменой (сжата часть файлов).
    pub partial: bool,
}

fn store_path(app: &AppHandle) -> Result<PathBuf, String> {
    let dir = app
        .path()
        .app_data_dir()
        .map_err(|e| format!("app_data_dir: {e}"))?;
    std::fs::create_dir_all(&dir).map_err(|e| format!("create_dir_all: {e}"))?;
    Ok(dir.join("history.json"))
}

pub fn load(app: &AppHandle) -> Vec<HistoryEntry> {
    store_path(app)
        .ok()
        .and_then(|p| std::fs::read_to_string(p).ok())
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default()
}

fn store(app: &AppHandle, list: &[HistoryEntry]) {
    if let (Ok(path), Ok(json)) = (store_path(app), serde_json::to_string_pretty(list)) {
        let _ = std::fs::write(path, json);
    }
}

/// Добавляет запись (или заменяет существующую для той же папки).
pub fn upsert(app: &AppHandle, entry: HistoryEntry) {
    let mut list = load(app);
    list.retain(|e| e.root != entry.root);
    list.insert(0, entry);
    store(app, &list);
}

/// Удаляет запись после успешной декомпрессии.
pub fn remove(app: &AppHandle, root: &str) {
    let mut list = load(app);
    list.retain(|e| e.root != root);
    store(app, &list);
}
