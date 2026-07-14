//! Обход дерева файлов игры, фильтрация критичных файлов
//! и оценка потенциала сжатия по типам содержимого.

use serde::Serialize;
use std::path::{Path, PathBuf};
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
}

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
        });
    }

    // Сначала крупные файлы — равномернее загрузка пула потоков.
    result.files.sort_by(|a, b| b.size.cmp(&a.size));

    on_progress(result.total_files);
    Ok(result)
}
