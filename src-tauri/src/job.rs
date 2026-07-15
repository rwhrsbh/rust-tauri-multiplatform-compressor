//! Выполнение задачи сжатия/декомпрессии: пул потоков rayon,
//! пауза/возобновление/отмена, события прогресса в реальном времени.

use crate::compressor::{physical_size, platform_compressor, CompressOutcome, CompressionAlgo};
use crate::crawler::FileEntry;
use crate::history::{self, HistoryEntry};
use rayon::prelude::*;
use serde::Serialize;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use tauri::{AppHandle, Emitter};

pub const EVENT_PROGRESS: &str = "compression://progress";
pub const EVENT_DONE: &str = "compression://done";
pub const EVENT_SCAN: &str = "compression://scan-progress";

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum JobMode {
    Compress,
    Decompress,
}

impl JobMode {
    fn as_str(&self) -> &'static str {
        match self {
            JobMode::Compress => "compress",
            JobMode::Decompress => "decompress",
        }
    }
}

/// Флаги управления задачей, разделяемые между потоками.
#[derive(Default)]
pub struct JobControl {
    pub paused: AtomicBool,
    pub cancelled: AtomicBool,
    pub running: AtomicBool,
}

impl JobControl {
    /// Блокирует рабочий поток, пока задача на паузе.
    /// Возвращает false, если задача отменена.
    fn wait_if_paused(&self) -> bool {
        while self.paused.load(Ordering::Relaxed) {
            if self.cancelled.load(Ordering::Relaxed) {
                return false;
            }
            std::thread::sleep(Duration::from_millis(100));
        }
        !self.cancelled.load(Ordering::Relaxed)
    }
}

#[derive(Serialize, Clone)]
struct ProgressPayload {
    mode: &'static str,
    state: &'static str,
    processed_files: u64,
    total_files: u64,
    percent: f64,
    bytes_processed: u64,
    total_bytes: u64,
    saved_bytes: i64,
    speed_bps: f64,
    current_file: String,
    elapsed_secs: f64,
}

#[derive(Serialize, Clone)]
pub struct DonePayload {
    mode: &'static str,
    cancelled: bool,
    processed_files: u64,
    total_files: u64,
    failed_files: u64,
    not_beneficial_files: u64,
    original_bytes: u64,
    final_physical_bytes: u64,
    saved_bytes: i64,
    elapsed_secs: f64,
    errors: Vec<String>,
}

struct Shared {
    processed: AtomicU64,
    failed: AtomicU64,
    not_beneficial: AtomicU64,
    bytes_processed: AtomicU64,
    physical_after: AtomicU64,
    logical_done: AtomicU64,
    current_file: Mutex<String>,
    errors: Mutex<Vec<String>>,
}

/// Запускает задачу в отдельном потоке; прогресс уходит событиями на фронтенд.
pub fn spawn_job(
    app: AppHandle,
    root: PathBuf,
    files: Vec<FileEntry>,
    control: Arc<JobControl>,
    mode: JobMode,
    algo: CompressionAlgo,
) {
    control.running.store(true, Ordering::SeqCst);
    control.paused.store(false, Ordering::SeqCst);
    control.cancelled.store(false, Ordering::SeqCst);

    std::thread::spawn(move || {
        run_job(app, root, files, control, mode, algo);
    });
}

fn run_job(
    app: AppHandle,
    root: PathBuf,
    files: Vec<FileEntry>,
    control: Arc<JobControl>,
    mode: JobMode,
    algo: CompressionAlgo,
) {
    let total_files = files.len() as u64;
    let total_bytes: u64 = files.iter().map(|f| f.size).sum();
    let started = Instant::now();

    let shared = Arc::new(Shared {
        processed: AtomicU64::new(0),
        failed: AtomicU64::new(0),
        not_beneficial: AtomicU64::new(0),
        bytes_processed: AtomicU64::new(0),
        physical_after: AtomicU64::new(0),
        logical_done: AtomicU64::new(0),
        current_file: Mutex::new(String::new()),
        errors: Mutex::new(Vec::new()),
    });

    // Поток-монитор: шлёт события прогресса каждые 250 мс,
    // считает мгновенную скорость по дельте байт.
    let monitor_stop = Arc::new(AtomicBool::new(false));
    let monitor = {
        let app = app.clone();
        let shared = Arc::clone(&shared);
        let control = Arc::clone(&control);
        let stop = Arc::clone(&monitor_stop);
        let mode_str = mode.as_str();
        std::thread::spawn(move || {
            let mut last_bytes = 0u64;
            let mut last_tick = Instant::now();
            let mut speed = 0.0f64;
            while !stop.load(Ordering::Relaxed) {
                std::thread::sleep(Duration::from_millis(250));
                let bytes = shared.bytes_processed.load(Ordering::Relaxed);
                let now = Instant::now();
                let dt = now.duration_since(last_tick).as_secs_f64();
                if dt > 0.0 {
                    // Сглаживание скорости (EMA), чтобы цифра не дёргалась
                    let inst = (bytes - last_bytes) as f64 / dt;
                    speed = if speed == 0.0 { inst } else { speed * 0.7 + inst * 0.3 };
                }
                last_bytes = bytes;
                last_tick = now;

                let processed = shared.processed.load(Ordering::Relaxed);
                let logical_done = shared.logical_done.load(Ordering::Relaxed);
                let physical_after = shared.physical_after.load(Ordering::Relaxed);
                let payload = ProgressPayload {
                    mode: mode_str,
                    state: if control.cancelled.load(Ordering::Relaxed) {
                        "cancelling"
                    } else if control.paused.load(Ordering::Relaxed) {
                        "paused"
                    } else {
                        "running"
                    },
                    processed_files: processed,
                    total_files,
                    percent: if total_bytes > 0 {
                        bytes as f64 / total_bytes as f64 * 100.0
                    } else {
                        100.0
                    },
                    bytes_processed: bytes,
                    total_bytes,
                    saved_bytes: logical_done as i64 - physical_after as i64,
                    speed_bps: if control.paused.load(Ordering::Relaxed) { 0.0 } else { speed },
                    current_file: shared.current_file.lock().unwrap().clone(),
                    elapsed_secs: started.elapsed().as_secs_f64(),
                };
                let _ = app.emit(EVENT_PROGRESS, payload);
            }
        })
    };

    let compressor = platform_compressor(algo);

    files.par_iter().for_each(|entry| {
        if !control.wait_if_paused() {
            return; // отменено
        }

        {
            let mut cur = shared.current_file.lock().unwrap();
            *cur = entry.path.display().to_string();
        }

        let result = match mode {
            JobMode::Compress => compressor.compress_file(&entry.path, &control.cancelled),
            JobMode::Decompress => {
                compressor.decompress_file(&entry.path, &control.cancelled)
            }
        };

        match result {
            Ok(CompressOutcome::Done) => {}
            Ok(CompressOutcome::Cancelled) => {
                // Файл не обработан (или обработан частично) из-за отмены —
                // не учитываем его в счётчиках.
                return;
            }
            Ok(CompressOutcome::NotBeneficial) => {
                shared.not_beneficial.fetch_add(1, Ordering::Relaxed);
            }
            Err(e) => {
                shared.failed.fetch_add(1, Ordering::Relaxed);
                let mut errors = shared.errors.lock().unwrap();
                if errors.len() < 100 {
                    errors.push(e);
                }
            }
        }

        shared
            .physical_after
            .fetch_add(physical_size(&entry.path), Ordering::Relaxed);
        shared.logical_done.fetch_add(entry.size, Ordering::Relaxed);
        shared.bytes_processed.fetch_add(entry.size, Ordering::Relaxed);
        shared.processed.fetch_add(1, Ordering::Relaxed);
    });

    monitor_stop.store(true, Ordering::Relaxed);
    let _ = monitor.join();

    let cancelled = control.cancelled.load(Ordering::SeqCst);
    control.running.store(false, Ordering::SeqCst);
    control.paused.store(false, Ordering::SeqCst);

    let logical_done = shared.logical_done.load(Ordering::Relaxed);
    let physical_after = shared.physical_after.load(Ordering::Relaxed);
    let processed = shared.processed.load(Ordering::Relaxed);

    // Обновляем историю: успешное (или частичное) сжатие — записываем,
    // полная декомпрессия — убираем запись.
    let root_str = root.display().to_string();
    match mode {
        JobMode::Compress if processed > 0 => {
            history::upsert(
                &app,
                HistoryEntry {
                    root: root_str,
                    date: SystemTime::now()
                        .duration_since(UNIX_EPOCH)
                        .map(|d| d.as_secs())
                        .unwrap_or(0),
                    files: processed,
                    original_bytes: logical_done,
                    saved_bytes: logical_done as i64 - physical_after as i64,
                    partial: cancelled,
                    // Алгоритм выбирается на Windows (WOF) и Linux (Btrfs);
                    // на macOS ditto всегда zlib — не записываем.
                    algorithm: cfg!(any(windows, target_os = "linux")).then_some(algo),
                },
            );
        }
        JobMode::Decompress if !cancelled => {
            history::remove(&app, &root_str);
        }
        _ => {}
    }

    let payload = DonePayload {
        mode: mode.as_str(),
        cancelled,
        processed_files: shared.processed.load(Ordering::Relaxed),
        total_files,
        failed_files: shared.failed.load(Ordering::Relaxed),
        not_beneficial_files: shared.not_beneficial.load(Ordering::Relaxed),
        original_bytes: logical_done,
        final_physical_bytes: physical_after,
        saved_bytes: logical_done as i64 - physical_after as i64,
        elapsed_secs: started.elapsed().as_secs_f64(),
        errors: shared.errors.lock().unwrap().clone(),
    };
    let _ = app.emit(EVENT_DONE, payload);
}
