mod compressor;
mod crawler;
mod fs_utils;
mod history;
mod job;

use crawler::{AnalysisSummary, ScanResult};
use fs_utils::DiskInfo;
use history::HistoryEntry;
use job::{JobControl, JobMode};
use serde::Serialize;
use std::path::PathBuf;
use std::sync::atomic::Ordering;
use std::sync::{Arc, Mutex};
use tauri::{AppHandle, Emitter, Manager, State};

/// Глобальное состояние приложения.
struct AppState {
    /// Результат последнего сканирования (список файлов для сжатия).
    scan: Mutex<Option<ScanResult>>,
    /// Управление текущей задачей.
    control: Arc<JobControl>,
}

#[derive(Serialize, Clone)]
struct ScanProgressPayload {
    scanned_files: u64,
}

/// Проверяет файловую систему диска, на котором лежит выбранная папка.
#[tauri::command]
async fn check_filesystem(path: String) -> Result<DiskInfo, String> {
    tauri::async_runtime::spawn_blocking(move || {
        fs_utils::check_filesystem(&PathBuf::from(path))
    })
    .await
    .map_err(|e| e.to_string())?
}

/// Сканирует папку игры и возвращает оценку сжатия.
#[tauri::command]
async fn analyze_folder(
    app: AppHandle,
    state: State<'_, AppState>,
    path: String,
) -> Result<AnalysisSummary, String> {
    if state.control.running.load(Ordering::SeqCst) {
        return Err("Задача уже выполняется".into());
    }

    let root = PathBuf::from(&path);
    let app_for_events = app.clone();
    let scan = tauri::async_runtime::spawn_blocking(move || {
        crawler::scan_folder(&root, |scanned| {
            let _ = app_for_events.emit(
                job::EVENT_SCAN,
                ScanProgressPayload {
                    scanned_files: scanned,
                },
            );
        })
    })
    .await
    .map_err(|e| e.to_string())??;

    let summary = scan.summary();
    *state.scan.lock().unwrap() = Some(scan);
    Ok(summary)
}

/// Забирает файлы последнего анализа: для сжатия — только ещё не сжатые,
/// для декомпрессии — только уже сжатые.
fn take_files(
    state: &State<'_, AppState>,
    path: &str,
    compressed: bool,
) -> Result<Vec<crawler::FileEntry>, String> {
    let guard = state.scan.lock().unwrap();
    match guard.as_ref() {
        Some(scan) if scan.root == PathBuf::from(path) => Ok(scan
            .files
            .iter()
            .filter(|f| f.is_compressed == compressed)
            .cloned()
            .collect()),
        _ => Err("Сначала выполните анализ папки".into()),
    }
}

/// Запускает фоновое сжатие проанализированной папки.
#[tauri::command]
fn start_compression(
    app: AppHandle,
    state: State<'_, AppState>,
    path: String,
) -> Result<(), String> {
    if state.control.running.load(Ordering::SeqCst) {
        return Err("Задача уже выполняется".into());
    }
    let disk = fs_utils::check_filesystem(&PathBuf::from(&path))?;
    if !disk.supported {
        return Err(disk.reason);
    }
    let files = take_files(&state, &path, false)?;
    if files.is_empty() {
        return Err("Нет файлов для сжатия: папка уже сжата".into());
    }
    job::spawn_job(
        app,
        PathBuf::from(&path),
        files,
        Arc::clone(&state.control),
        JobMode::Compress,
    );
    Ok(())
}

/// Возвращает файлы в исходное (несжатое) состояние.
#[tauri::command]
fn start_decompression(
    app: AppHandle,
    state: State<'_, AppState>,
    path: String,
) -> Result<(), String> {
    if state.control.running.load(Ordering::SeqCst) {
        return Err("Задача уже выполняется".into());
    }
    let files = take_files(&state, &path, true)?;
    if files.is_empty() {
        // Нечего декомпрессировать — просто убираем запись из истории
        history::remove(&app, &path);
        return Err("Сжатых файлов не найдено".into());
    }
    job::spawn_job(
        app,
        PathBuf::from(&path),
        files,
        Arc::clone(&state.control),
        JobMode::Decompress,
    );
    Ok(())
}

/// История сжатий (для списка на главном экране и отката).
#[tauri::command]
fn get_history(app: AppHandle) -> Vec<HistoryEntry> {
    history::load(&app)
}

#[tauri::command]
fn pause_job(state: State<'_, AppState>) {
    state.control.paused.store(true, Ordering::SeqCst);
}

#[tauri::command]
fn resume_job(state: State<'_, AppState>) {
    state.control.paused.store(false, Ordering::SeqCst);
}

#[tauri::command]
fn cancel_job(state: State<'_, AppState>) {
    state.control.cancelled.store(true, Ordering::SeqCst);
    state.control.paused.store(false, Ordering::SeqCst);
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .setup(|app| {
            app.manage(AppState {
                scan: Mutex::new(None),
                control: Arc::new(JobControl::default()),
            });
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            check_filesystem,
            analyze_folder,
            start_compression,
            start_decompression,
            pause_job,
            resume_job,
            cancel_job,
            get_history
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
