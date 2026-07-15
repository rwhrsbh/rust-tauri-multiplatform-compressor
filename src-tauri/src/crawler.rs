//! Обход дерева файлов игры, фильтрация критичных файлов
//! и оценка потенциала сжатия по типам содержимого.

use serde::Serialize;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::UNIX_EPOCH;
use walkdir::WalkDir;

/// Расширения, которые никогда не трогаем: исполняемые файлы и библиотеки.
/// Их сжатие может ломать античиты, подписи и загрузчики.
const SKIP_EXTENSIONS: &[&str] = &["exe", "dll", "dylib", "so", "sys"];

/// Файлы меньше этого размера не сжимаем — накладные расходы превышают выгоду.
const MIN_FILE_SIZE: u64 = 4096;

#[derive(Clone, Debug)]
pub struct FileEntry {
    pub path: PathBuf,
    pub size: u64,
    /// Файл уже прозрачно сжат (физический размер заметно меньше логического).
    pub is_compressed: bool,
    /// Unix-время последнего изменения (секунды); None, если ФС не отдала.
    pub modified: Option<u64>,
}

#[derive(Clone, Debug, Default)]
pub struct ScanResult {
    pub root: PathBuf,
    /// Все кандидаты (без пропущенных): и сжатые, и несжатые.
    pub files: Vec<FileEntry>,
    pub total_files: u64,
    pub skipped_files: u64,
    pub total_bytes: u64,
    /// Логический размер уже сжатых файлов.
    pub already_compressed_bytes: u64,
    /// Физический размер уже сжатых файлов (сколько они реально занимают).
    pub already_physical_bytes: u64,
    pub already_compressed_files: u64,
    /// Логический размер файлов, которые предстоит сжать.
    pub todo_bytes: u64,
    /// Оценка размера todo-файлов после сжатия.
    pub todo_estimated_bytes: u64,
    /// Найдены Windows-бинарники (.exe/.dll) — признак Wine/Proton-игры на Linux/macOS.
    pub windows_binaries: u64,
}

/// Сводка анализа для фронтенда.
#[derive(Serialize, Clone, Debug)]
pub struct AnalysisSummary {
    pub root: String,
    pub total_files: u64,
    pub compressible_files: u64,
    pub skipped_files: u64,
    pub total_bytes: u64,
    pub estimated_bytes: u64,
    /// Ожидаемая экономия, 0..1
    pub estimated_savings_ratio: f64,
    /// Уже сжатые ранее файлы — повторно не сжимаются.
    pub already_compressed_files: u64,
    /// Сколько места уже сэкономлено ранее сжатыми файлами.
    pub already_saved_bytes: u64,
    /// true, если это Windows-игра, запускаемая через Proton/Wine
    /// (Windows-бинарники в папке на Linux/macOS). Сжатие работает так же:
    /// файлы лежат в обычной ФС хоста, а .exe/.dll мы не трогаем.
    pub proton_hint: bool,
    /// Прогноз итогового размера для каждого WOF-алгоритма (Windows),
    /// чтобы фронтенд пересчитывал оценку без повторного сканирования.
    pub estimated_bytes_by_algo: HashMap<String, u64>,
}

/// Множитель экономии относительно базовой таблицы коэффициентов
/// (база откалибрована под XPRESS8K; zstd на Btrfs сопоставим).
fn algo_savings_factor(algo: &str) -> f64 {
    match algo {
        // WOF (Windows/NTFS)
        "xpress4k" => 0.85,
        "xpress8k" => 1.0,
        "xpress16k" => 1.08,
        "lzx" => 1.35,
        // Btrfs (Linux)
        "zstd" => 1.05,
        "zlib" => 1.0,
        "lzo" => 0.65,
        _ => 1.0,
    }
}

pub const ALGO_IDS: &[&str] = &[
    "xpress4k", "xpress8k", "xpress16k", "lzx", "zstd", "zlib", "lzo",
];

impl ScanResult {
    pub fn summary(&self) -> AnalysisSummary {
        // Ожидаемый размер на диске после сжатия:
        // пропущенные файлы как есть + уже сжатые (их физический размер)
        // + оценка для тех, что предстоит сжать.
        let skipped_bytes = self
            .total_bytes
            .saturating_sub(self.todo_bytes)
            .saturating_sub(self.already_compressed_bytes);
        let estimated_total =
            skipped_bytes + self.already_physical_bytes + self.todo_estimated_bytes;
        let ratio = if self.total_bytes > 0 {
            1.0 - estimated_total as f64 / self.total_bytes as f64
        } else {
            0.0
        };
        let is_steam_path = self
            .root
            .components()
            .any(|c| c.as_os_str().eq_ignore_ascii_case("steamapps"));
        // Прогноз для каждого алгоритма: масштабируем базовую экономию
        let base_savings = self.todo_bytes.saturating_sub(self.todo_estimated_bytes);
        let estimated_bytes_by_algo = ALGO_IDS
            .iter()
            .map(|&algo| {
                let savings = ((base_savings as f64) * algo_savings_factor(algo)) as u64;
                let todo_est = self.todo_bytes.saturating_sub(savings.min(self.todo_bytes));
                (
                    algo.to_string(),
                    skipped_bytes + self.already_physical_bytes + todo_est,
                )
            })
            .collect();
        AnalysisSummary {
            root: self.root.display().to_string(),
            total_files: self.total_files,
            compressible_files: self.files.len() as u64 - self.already_compressed_files,
            skipped_files: self.skipped_files,
            total_bytes: self.total_bytes,
            estimated_bytes: estimated_total,
            estimated_savings_ratio: ratio.max(0.0),
            already_compressed_files: self.already_compressed_files,
            already_saved_bytes: self
                .already_compressed_bytes
                .saturating_sub(self.already_physical_bytes),
            proton_hint: cfg!(not(target_os = "windows"))
                && (self.windows_binaries > 0 || is_steam_path),
            estimated_bytes_by_algo,
        }
    }
}

/// Предустановленный коэффициент сжатия (ожидаемый размер / исходный)
/// для разных категорий игровых файлов.
fn compression_ratio(ext: &str) -> f64 {
    match ext {
        // Текст, конфиги, скрипты, шейдеры — жмутся отлично
        "txt" | "json" | "xml" | "ini" | "cfg" | "yaml" | "yml" | "lua" | "js" | "py"
        | "csv" | "log" | "html" | "md" | "shader" | "hlsl" | "glsl" | "fx" | "mtl"
        | "sql" | "vdf" | "acf" => 0.35,
        // Несжатые текстуры и изображения
        "dds" | "tga" | "bmp" | "psd" | "tif" | "tiff" | "raw" | "hdr" | "exr" => 0.55,
        // Несжатый звук
        "wav" | "aiff" | "aif" | "pcm" => 0.65,
        // Модели, анимации, генерируемые данные
        "fbx" | "obj" | "dae" | "mesh" | "anim" | "skel" | "nif" | "gltf" | "glb" => 0.70,
        // Бинарные данные/архивы игр — часто частично сжаты
        "pak" | "bin" | "dat" | "assets" | "resource" | "bundle" | "arc" | "big"
        | "bsa" | "ba2" | "vpk" | "forge" | "cache" | "db" => 0.85,
        // Уже сжатые форматы — почти не жмутся
        "zip" | "rar" | "7z" | "gz" | "zst" | "lz4" | "cab" | "ogg" | "mp3" | "mp4"
        | "mkv" | "webm" | "avi" | "jpg" | "jpeg" | "png" | "webp" | "bik" | "bk2"
        | "wem" | "fsb" | "bnk" | "astc" | "ktx2" => 0.97,
        // Всё остальное — консервативная средняя оценка
        _ => 0.75,
    }
}

/// Ожидаемый коэффициент сжатия для файла (по расширению).
pub fn estimate_ratio(path: &Path) -> f64 {
    compression_ratio(&extension_of(path))
}

fn extension_of(path: &Path) -> String {
    path.extension()
        .map(|e| e.to_string_lossy().to_ascii_lowercase())
        .unwrap_or_default()
}

/// Физический размер файла (сколько он реально занимает на диске).
/// На Unix берётся из уже полученных метаданных без лишнего syscall.
#[cfg(unix)]
fn physical_of(meta: &std::fs::Metadata, _path: &Path) -> u64 {
    use std::os::unix::fs::MetadataExt;
    meta.blocks() * 512
}

#[cfg(windows)]
fn physical_of(_meta: &std::fs::Metadata, path: &Path) -> u64 {
    crate::compressor::physical_size(path)
}

/// Эвристика "файл уже прозрачно сжат": физический размер заметно
/// меньше логического (с запасом на кластеры/сектора).
fn detect_compressed(size: u64, physical: u64) -> bool {
    physical > 0 && physical < size * 95 / 100 && size - physical > 4096
}

/// Обходит папку игры, собирает файлы для сжатия и считает оценку.
/// `on_progress` вызывается периодически с количеством просканированных файлов.
pub fn scan_folder(
    root: &Path,
    mut on_progress: impl FnMut(u64),
) -> Result<ScanResult, String> {
    if !root.is_dir() {
        return Err(format!("Не папка: {}", root.display()));
    }

    let mut result = ScanResult {
        root: root.to_path_buf(),
        ..Default::default()
    };

    for entry in WalkDir::new(root).follow_links(false) {
        let entry = match entry {
            Ok(e) => e,
            Err(_) => continue, // нет прав / битая ссылка — пропускаем
        };
        if !entry.file_type().is_file() {
            continue;
        }
        let meta = match entry.metadata() {
            Ok(m) => m,
            Err(_) => continue,
        };
        let size = meta.len();
        result.total_files += 1;
        result.total_bytes += size;

        if result.total_files % 500 == 0 {
            on_progress(result.total_files);
        }

        let ext = extension_of(entry.path());
        if ext == "exe" || ext == "dll" {
            result.windows_binaries += 1;
        }

        if SKIP_EXTENSIONS.contains(&ext.as_str()) || size < MIN_FILE_SIZE {
            result.skipped_files += 1;
            continue;
        }

        let physical = physical_of(&meta, entry.path());
        let is_compressed = detect_compressed(size, physical);
        if is_compressed {
            result.already_compressed_files += 1;
            result.already_compressed_bytes += size;
            result.already_physical_bytes += physical;
        } else {
            let ratio = compression_ratio(&ext);
            result.todo_bytes += size;
            result.todo_estimated_bytes += (size as f64 * ratio) as u64;
        }
        result.files.push(FileEntry {
            path: entry.path().to_path_buf(),
            size,
            is_compressed,
            modified: meta
                .modified()
                .ok()
                .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
                .map(|d| d.as_secs()),
        });
    }

    // Сначала крупные файлы — равномернее загрузка пула потоков.
    result.files.sort_by(|a, b| b.size.cmp(&a.size));

    on_progress(result.total_files);
    Ok(result)
}
