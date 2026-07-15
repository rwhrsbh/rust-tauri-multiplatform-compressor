mod compressor;
mod crawler;
mod fs_utils;
mod history;
mod job;
mod library;

use compressor::CompressionAlgo;
use crawler::{AnalysisSummary, ScanResult};
use fs_utils::DiskInfo;
use history::HistoryEntry;
use job::{JobControl, JobMode};
use serde::Serialize;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use tauri::{AppHandle, Emitter, Manager, State};

/// Глобальное состояние приложения.
struct AppState {
    /// Результат последнего сканирования (список файлов для сжатия).
    scan: Mutex<Option<ScanResult>>,
    /// Управление текущей задачей.
    control: Arc<JobControl>,
    /// Идёт фоновая проверка актуальности сжатия папок из истории.
    staleness_running: Arc<AtomicBool>,
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
    algorithm: Option<CompressionAlgo>,
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
        algorithm.unwrap_or_default(),
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
        CompressionAlgo::default(), // при декомпрессии не используется
    );
    Ok(())
}

/// Событие с результатом проверки одной папки из истории.
pub const EVENT_STALENESS: &str = "history://staleness";
/// Событие «проверка всех папок завершена».
pub const EVENT_STALENESS_DONE: &str = "history://staleness-done";

/// Порог «сжатие устарело»: есть несжатые файлы и потенциальная
/// экономия заметна (либо файлов достаточно много).
const STALE_MIN_SAVED_BYTES: u64 = 10 * 1024 * 1024;
const STALE_MIN_FILES: u64 = 25;

#[derive(Serialize, Clone)]
struct StalenessPayload {
    root: String,
    /// "ok" | "stale" | "missing"
    status: &'static str,
    uncompressed_files: u64,
    potential_saved_bytes: u64,
}

/// Фоновая проверка папок из истории: не появились ли несжатые файлы
/// после обновления игры. Результаты уходят событиями по мере проверки.
#[tauri::command]
fn check_history_staleness(app: AppHandle, state: State<'_, AppState>) -> Result<(), String> {
    if state.staleness_running.swap(true, Ordering::SeqCst) {
        return Ok(()); // уже идёт — не запускаем вторую
    }
    let running = Arc::clone(&state.staleness_running);
    // Один поток и последовательные сканы: не грузим диск,
    // пока пользователь, возможно, запускает сжатие.
    std::thread::spawn(move || {
        for entry in history::load(&app) {
            let root = PathBuf::from(&entry.root);
            let payload = if !root.is_dir() {
                StalenessPayload {
                    root: entry.root,
                    status: "missing",
                    uncompressed_files: 0,
                    potential_saved_bytes: 0,
                }
            } else {
                match crawler::scan_folder(&root, |_| {}) {
                    Ok(scan) => {
                        // Считаем только несжатые файлы, ИЗМЕНЁННЫЕ после даты
                        // сжатия: старые несжатые (WOF счёл сжатие невыгодным)
                        // не должны давать ложное «нужно дожать».
                        let mut new_files = 0u64;
                        let mut potential = 0u64;
                        for f in &scan.files {
                            if f.is_compressed {
                                continue;
                            }
                            let Some(mtime) = f.modified else { continue };
                            if mtime <= entry.date {
                                continue;
                            }
                            new_files += 1;
                            let est = (f.size as f64 * crawler::estimate_ratio(&f.path)) as u64;
                            potential += f.size.saturating_sub(est);
                        }
                        let stale = new_files > 0
                            && (potential >= STALE_MIN_SAVED_BYTES
                                || new_files >= STALE_MIN_FILES);
                        StalenessPayload {
                            root: entry.root,
                            status: if stale { "stale" } else { "ok" },
                            uncompressed_files: new_files,
                            potential_saved_bytes: potential,
                        }
                    }
                    Err(_) => StalenessPayload {
                        root: entry.root,
                        status: "missing",
                        uncompressed_files: 0,
                        potential_saved_bytes: 0,
                    },
                }
            };
            let _ = app.emit(EVENT_STALENESS, payload);
        }
        let _ = app.emit(EVENT_STALENESS_DONE, ());
        running.store(false, Ordering::SeqCst);
    });
    Ok(())
}

/// Удаляет запись из истории (файлы не трогаются).
#[tauri::command]
fn remove_history_entry(app: AppHandle, root: String) {
    history::remove(&app, &root);
}

/// Открывает папку в системном файловом менеджере.
#[tauri::command]
fn open_in_explorer(path: String) -> Result<(), String> {
    if !Path::new(&path).is_dir() {
        return Err("Папка не найдена".into());
    }
    #[cfg(target_os = "windows")]
    let cmd = "explorer";
    #[cfg(target_os = "macos")]
    let cmd = "open";
    #[cfg(target_os = "linux")]
    let cmd = "xdg-open";
    std::process::Command::new(cmd)
        .arg(&path)
        .spawn()
        .map(|_| ())
        .map_err(|e| e.to_string())
}

/// Проверка «путь — это папка» (для drag & drop на фронтенде).
#[tauri::command]
fn is_directory(path: String) -> bool {
    Path::new(&path).is_dir()
}

/// Библиотека установленных игр из лаунчеров (Steam / Epic / GOG).
#[tauri::command]
async fn get_game_library() -> Result<Vec<library::GameEntry>, String> {
    tauri::async_runtime::spawn_blocking(library::scan_all)
        .await
        .map_err(|e| e.to_string())
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
                staleness_running: Arc::new(AtomicBool::new(false)),
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
            get_history,
            check_history_staleness,
            remove_history_entry,
            open_in_explorer,
            is_directory,
            get_game_library
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
