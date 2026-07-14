//! Платформо-зависимое прозрачное сжатие файлов.
//!
//! - Windows: WOF (Windows Overlay Filter, алгоритм LZX) через
//!   `FSCTL_SET_EXTERNAL_BACKING`; при недоступности WOF — откат на
//!   классическое NTFS-сжатие `FSCTL_SET_COMPRESSION` (LZNT1).
//! - macOS: decmpfs (`com.apple.decmpfs`) через `ditto --hfsCompression`
//!   с атомарной заменой файла; процесс убивается при отмене.
//! - Linux (Btrfs): `FS_IOC_SETFLAGS` + `FS_COMPR_FL`, затем
//!   `BTRFS_IOC_DEFRAG_RANGE` со сжатием zstd чанками по 256 МБ,
//!   чтобы отмена срабатывала быстро даже на огромных файлах.

use std::path::Path;
use std::sync::atomic::AtomicBool;

/// Результат обработки одного файла.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CompressOutcome {
    /// Файл обработан.
    Done,
    /// ФС решила, что сжатие не даёт выгоды — файл оставлен как есть.
    /// Сейчас его возвращает только WOF на Windows.
    #[cfg_attr(not(target_os = "windows"), allow(dead_code))]
    NotBeneficial,
    /// Обработка прервана отменой задачи (файл в безопасном состоянии).
    Cancelled,
}

pub trait Compressor: Send + Sync {
    /// Применяет прозрачное сжатие к файлу. Реализация обязана
    /// периодически проверять `cancelled` и прерываться безопасно.
    fn compress_file(
        &self,
        path: &Path,
        cancelled: &AtomicBool,
    ) -> Result<CompressOutcome, String>;

    /// Возвращает файл в исходное (несжатое) состояние.
    fn decompress_file(
        &self,
        path: &Path,
        cancelled: &AtomicBool,
    ) -> Result<CompressOutcome, String>;
}

/// Компрессор для текущей платформы.
pub fn platform_compressor() -> Box<dyn Compressor> {
    #[cfg(target_os = "windows")]
    {
        Box::new(windows_impl::WofCompressor)
    }
    #[cfg(target_os = "macos")]
    {
        Box::new(macos_impl::DittoCompressor)
    }
    #[cfg(target_os = "linux")]
    {
        Box::new(linux_impl::BtrfsCompressor)
    }
}

/// Фактически занимаемое на диске место (после прозрачного сжатия
/// оно меньше логического размера файла).
pub fn physical_size(path: &Path) -> u64 {
    #[cfg(windows)]
    {
        windows_impl::compressed_size(path)
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        std::fs::metadata(path).map(|m| m.blocks() * 512).unwrap_or(0)
    }
}

// ===========================================================================
// Windows: WOF (LZX) + откат на NTFS LZNT1
// ===========================================================================
#[cfg(target_os = "windows")]
mod windows_impl {
    use super::{CompressOutcome, Compressor};
    use std::os::windows::ffi::OsStrExt;
    use std::path::Path;
    use std::sync::atomic::{AtomicBool, Ordering};
    use windows::core::PCWSTR;
    use windows::Win32::Foundation::{CloseHandle, ERROR_COMPRESSION_NOT_BENEFICIAL, HANDLE};
    use windows::Win32::Storage::FileSystem::{
        CreateFileW, GetCompressedFileSizeW, FILE_ATTRIBUTE_NORMAL, FILE_GENERIC_READ,
        FILE_GENERIC_WRITE, FILE_SHARE_READ, OPEN_EXISTING,
    };
    use windows::Win32::System::Ioctl::{
        FSCTL_DELETE_EXTERNAL_BACKING, FSCTL_SET_COMPRESSION, FSCTL_SET_EXTERNAL_BACKING,
    };
    use windows::Win32::System::IO::DeviceIoControl;

    // Структуры WOF (wofapi.h). Определены вручную, чтобы не зависеть
    // от наличия биндингов в конкретной версии крейта `windows`.
    const WOF_CURRENT_VERSION: u32 = 1;
    const WOF_PROVIDER_FILE: u32 = 2;
    const FILE_PROVIDER_CURRENT_VERSION: u32 = 1;
    const FILE_PROVIDER_COMPRESSION_LZX: u32 = 3;

    const COMPRESSION_FORMAT_DEFAULT: u16 = 1; // LZNT1
    const COMPRESSION_FORMAT_NONE: u16 = 0;

    #[repr(C)]
    struct WofFileCompressionInfo {
        // WOF_EXTERNAL_INFO
        wof_version: u32,
        wof_provider: u32,
        // FILE_PROVIDER_EXTERNAL_INFO_V1
        provider_version: u32,
        algorithm: u32,
        flags: u32,
    }

    fn wide(path: &Path) -> Vec<u16> {
        path.as_os_str().encode_wide().chain(Some(0)).collect()
    }

    fn open_rw(path: &Path) -> Result<HANDLE, String> {
        let w = wide(path);
        unsafe {
            CreateFileW(
                PCWSTR(w.as_ptr()),
                (FILE_GENERIC_READ | FILE_GENERIC_WRITE).0,
                FILE_SHARE_READ,
                None,
                OPEN_EXISTING,
                FILE_ATTRIBUTE_NORMAL,
                None,
            )
        }
        .map_err(|e| format!("Не удалось открыть файл {}: {e}", path.display()))
    }

    struct HandleGuard(HANDLE);
    impl Drop for HandleGuard {
        fn drop(&mut self) {
            unsafe {
                let _ = CloseHandle(self.0);
            }
        }
    }

    /// Сжатие через WOF (как `compact.exe /EXE:LZX`).
    fn wof_compress(handle: HANDLE) -> windows::core::Result<()> {
        let info = WofFileCompressionInfo {
            wof_version: WOF_CURRENT_VERSION,
            wof_provider: WOF_PROVIDER_FILE,
            provider_version: FILE_PROVIDER_CURRENT_VERSION,
            algorithm: FILE_PROVIDER_COMPRESSION_LZX,
            flags: 0,
        };
        unsafe {
            DeviceIoControl(
                handle,
                FSCTL_SET_EXTERNAL_BACKING,
                Some(&info as *const _ as *const core::ffi::c_void),
                std::mem::size_of::<WofFileCompressionInfo>() as u32,
                None,
                0,
                None,
                None,
            )
        }
    }

    /// Классическое NTFS-сжатие (атрибут "Сжимать содержимое").
    fn ntfs_set_compression(handle: HANDLE, format: u16) -> windows::core::Result<()> {
        let mut returned = 0u32;
        unsafe {
            DeviceIoControl(
                handle,
                FSCTL_SET_COMPRESSION,
                Some(&format as *const _ as *const core::ffi::c_void),
                std::mem::size_of::<u16>() as u32,
                None,
                0,
                Some(&mut returned),
                None,
            )
        }
    }

    fn wof_remove(handle: HANDLE) -> windows::core::Result<()> {
        unsafe {
            DeviceIoControl(
                handle,
                FSCTL_DELETE_EXTERNAL_BACKING,
                None,
                0,
                None,
                0,
                None,
                None,
            )
        }
    }

    pub fn compressed_size(path: &Path) -> u64 {
        let w = wide(path);
        let mut high: u32 = 0;
        let low = unsafe { GetCompressedFileSizeW(PCWSTR(w.as_ptr()), Some(&mut high)) };
        if low == u32::MAX {
            // INVALID_FILE_SIZE может быть и валидным значением; при ошибке вернём логический размер
            return std::fs::metadata(path).map(|m| m.len()).unwrap_or(0);
        }
        ((high as u64) << 32) | low as u64
    }

    pub struct WofCompressor;

    impl Compressor for WofCompressor {
        fn compress_file(
            &self,
            path: &Path,
            cancelled: &AtomicBool,
        ) -> Result<CompressOutcome, String> {
            // Сам вызов WOF атомарен и непрерываем, поэтому проверяем
            // флаг отмены непосредственно перед стартом.
            if cancelled.load(Ordering::Relaxed) {
                return Ok(CompressOutcome::Cancelled);
            }
            let handle = open_rw(path)?;
            let guard = HandleGuard(handle);

            match wof_compress(guard.0) {
                Ok(()) => Ok(CompressOutcome::Done),
                Err(e) if e.code() == ERROR_COMPRESSION_NOT_BENEFICIAL.into() => {
                    Ok(CompressOutcome::NotBeneficial)
                }
                Err(_) => {
                    // WOF недоступен (старая Windows, не-NTFS и т.п.) — откат на LZNT1
                    ntfs_set_compression(guard.0, COMPRESSION_FORMAT_DEFAULT)
                        .map(|_| CompressOutcome::Done)
                        .map_err(|e| {
                            format!("NTFS-сжатие не удалось для {}: {e}", path.display())
                        })
                }
            }
        }

        fn decompress_file(
            &self,
            path: &Path,
            cancelled: &AtomicBool,
        ) -> Result<CompressOutcome, String> {
            if cancelled.load(Ordering::Relaxed) {
                return Ok(CompressOutcome::Cancelled);
            }
            let handle = open_rw(path)?;
            let guard = HandleGuard(handle);
            // Снимаем WOF-подложку (ошибка = подложки не было, это нормально)
            let _ = wof_remove(guard.0);
            // И выключаем классическое NTFS-сжатие
            ntfs_set_compression(guard.0, COMPRESSION_FORMAT_NONE)
                .map(|_| CompressOutcome::Done)
                .map_err(|e| format!("Декомпрессия не удалась для {}: {e}", path.display()))
        }
    }
}

// ===========================================================================
// macOS: decmpfs через ditto --hfsCompression (с убийством процесса при отмене)
// ===========================================================================
#[cfg(target_os = "macos")]
mod macos_impl {
    use super::{CompressOutcome, Compressor};
    use std::path::{Path, PathBuf};
    use std::process::Command;
    use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
    use std::time::Duration;

    static TMP_COUNTER: AtomicU64 = AtomicU64::new(0);

    fn temp_sibling(path: &Path) -> PathBuf {
        let n = TMP_COUNTER.fetch_add(1, Ordering::Relaxed);
        let name = path
            .file_name()
            .map(|s| s.to_string_lossy().to_string())
            .unwrap_or_else(|| "file".into());
        path.with_file_name(format!(".gc_tmp_{}_{}_{}", std::process::id(), n, name))
    }

    /// Пересоздаёт файл через ditto (с/без hfsCompression) и атомарно
    /// подменяет оригинал, сохраняя права доступа. При отмене убивает
    /// дочерний процесс — оригинал остаётся нетронутым.
    fn rewrite_with_ditto(
        path: &Path,
        compress: bool,
        cancelled: &AtomicBool,
    ) -> Result<CompressOutcome, String> {
        if cancelled.load(Ordering::Relaxed) {
            return Ok(CompressOutcome::Cancelled);
        }

        let tmp = temp_sibling(path);
        let mut cmd = Command::new("/usr/bin/ditto");
        if compress {
            cmd.arg("--hfsCompression");
        } else {
            cmd.arg("--nohfsCompression");
        }
        cmd.arg("--noqtn").arg(path).arg(&tmp);

        let mut child = cmd
            .spawn()
            .map_err(|e| format!("Не удалось запустить ditto: {e}"))?;

        // Ждём завершения, реагируя на отмену
        let status = loop {
            match child.try_wait() {
                Ok(Some(status)) => break status,
                Ok(None) => {
                    if cancelled.load(Ordering::Relaxed) {
                        let _ = child.kill();
                        let _ = child.wait();
                        let _ = std::fs::remove_file(&tmp);
                        return Ok(CompressOutcome::Cancelled);
                    }
                    std::thread::sleep(Duration::from_millis(30));
                }
                Err(e) => {
                    let _ = std::fs::remove_file(&tmp);
                    return Err(format!("Ошибка ожидания ditto: {e}"));
                }
            }
        };

        if !status.success() {
            let _ = std::fs::remove_file(&tmp);
            return Err(format!(
                "ditto завершился с ошибкой ({}) для {}",
                status,
                path.display()
            ));
        }

        // Права оригинала переносим на новый файл
        if let Ok(meta) = std::fs::metadata(path) {
            let _ = std::fs::set_permissions(&tmp, meta.permissions());
        }

        std::fs::rename(&tmp, path)
            .map(|_| CompressOutcome::Done)
            .map_err(|e| {
                let _ = std::fs::remove_file(&tmp);
                format!("Не удалось заменить файл {}: {e}", path.display())
            })
    }

    pub struct DittoCompressor;

    impl Compressor for DittoCompressor {
        fn compress_file(
            &self,
            path: &Path,
            cancelled: &AtomicBool,
        ) -> Result<CompressOutcome, String> {
            rewrite_with_ditto(path, true, cancelled)
        }

        fn decompress_file(
            &self,
            path: &Path,
            cancelled: &AtomicBool,
        ) -> Result<CompressOutcome, String> {
            rewrite_with_ditto(path, false, cancelled)
        }
    }
}

// ===========================================================================
// Linux: Btrfs — FS_COMPR_FL + BTRFS_IOC_DEFRAG_RANGE (zstd), чанками
// ===========================================================================
#[cfg(target_os = "linux")]
mod linux_impl {
    use super::{CompressOutcome, Compressor};
    use std::os::fd::AsRawFd;
    use std::path::Path;
    use std::sync::atomic::{AtomicBool, Ordering};

    // linux/fs.h: _IOR('f', 1, long) / _IOW('f', 2, long)
    const FS_IOC_GETFLAGS: libc::c_ulong = 0x8008_6601;
    const FS_IOC_SETFLAGS: libc::c_ulong = 0x4008_6602;
    const FS_COMPR_FL: libc::c_long = 0x0000_0004;
    const FS_NOCOMP_FL: libc::c_long = 0x0000_0400;

    // linux/btrfs.h: _IOW(BTRFS_IOCTL_MAGIC=0x94, 16, btrfs_ioctl_defrag_range_args)
    const BTRFS_IOC_DEFRAG_RANGE: libc::c_ulong = 0x4030_9410;
    const BTRFS_DEFRAG_RANGE_COMPRESS: u64 = 1;
    const BTRFS_COMPRESS_ZSTD: u32 = 3;

    /// Размер чанка дефрагментации: между чанками проверяется флаг отмены,
    /// поэтому отмена срабатывает быстро даже на файлах в десятки ГБ.
    const DEFRAG_CHUNK: u64 = 256 * 1024 * 1024;

    #[repr(C)]
    #[derive(Default)]
    struct BtrfsDefragRangeArgs {
        start: u64,
        len: u64,
        flags: u64,
        extent_thresh: u32,
        compress_type: u32,
        unused: [u32; 4],
    }

    fn ioctl_err(op: &str, path: &Path) -> String {
        format!(
            "{op} не удался для {}: {}",
            path.display(),
            std::io::Error::last_os_error()
        )
    }

    fn get_flags(fd: i32) -> Result<libc::c_long, std::io::Error> {
        let mut flags: libc::c_long = 0;
        if unsafe { libc::ioctl(fd, FS_IOC_GETFLAGS, &mut flags) } != 0 {
            return Err(std::io::Error::last_os_error());
        }
        Ok(flags)
    }

    fn set_flags(fd: i32, flags: libc::c_long) -> Result<(), std::io::Error> {
        if unsafe { libc::ioctl(fd, FS_IOC_SETFLAGS, &flags) } != 0 {
            return Err(std::io::Error::last_os_error());
        }
        Ok(())
    }

    fn defrag_range(
        fd: i32,
        start: u64,
        len: u64,
        compress: bool,
    ) -> Result<(), std::io::Error> {
        let args = BtrfsDefragRangeArgs {
            start,
            len,
            flags: if compress { BTRFS_DEFRAG_RANGE_COMPRESS } else { 0 },
            extent_thresh: 0,
            compress_type: if compress { BTRFS_COMPRESS_ZSTD } else { 0 },
            unused: [0; 4],
        };
        if unsafe { libc::ioctl(fd, BTRFS_IOC_DEFRAG_RANGE, &args) } != 0 {
            return Err(std::io::Error::last_os_error());
        }
        Ok(())
    }

    /// Дефрагментирует файл чанками, реагируя на отмену между чанками.
    fn defrag_chunked(
        fd: i32,
        path: &Path,
        size: u64,
        compress: bool,
        cancelled: &AtomicBool,
    ) -> Result<CompressOutcome, String> {
        let mut start = 0u64;
        loop {
            if cancelled.load(Ordering::Relaxed) {
                // Часть файла уже пережата — это безопасное состояние
                return Ok(CompressOutcome::Cancelled);
            }
            defrag_range(fd, start, DEFRAG_CHUNK, compress)
                .map_err(|_| ioctl_err("BTRFS_IOC_DEFRAG_RANGE", path))?;
            start = start.saturating_add(DEFRAG_CHUNK);
            if start >= size {
                return Ok(CompressOutcome::Done);
            }
        }
    }

    pub struct BtrfsCompressor;

    impl Compressor for BtrfsCompressor {
        fn compress_file(
            &self,
            path: &Path,
            cancelled: &AtomicBool,
        ) -> Result<CompressOutcome, String> {
            if cancelled.load(Ordering::Relaxed) {
                return Ok(CompressOutcome::Cancelled);
            }
            let file = std::fs::OpenOptions::new()
                .read(true)
                .write(true)
                .open(path)
                .map_err(|e| format!("Не удалось открыть {}: {e}", path.display()))?;
            let fd = file.as_raw_fd();
            let size = file
                .metadata()
                .map(|m| m.len())
                .map_err(|e| format!("metadata {}: {e}", path.display()))?;

            // Помечаем файл флагом 'c', чтобы будущие записи тоже сжимались.
            let flags = get_flags(fd).map_err(|_| ioctl_err("FS_IOC_GETFLAGS", path))?;
            set_flags(fd, (flags | FS_COMPR_FL) & !FS_NOCOMP_FL)
                .map_err(|_| ioctl_err("FS_IOC_SETFLAGS", path))?;

            // Пережимаем существующие данные (аналог btrfs filesystem defrag -czstd)
            let outcome = defrag_chunked(fd, path, size, true, cancelled)?;

            // Сбрасываем на диск, чтобы физический размер стал актуальным
            unsafe {
                libc::fsync(fd);
            }
            Ok(outcome)
        }

        fn decompress_file(
            &self,
            path: &Path,
            cancelled: &AtomicBool,
        ) -> Result<CompressOutcome, String> {
            if cancelled.load(Ordering::Relaxed) {
                return Ok(CompressOutcome::Cancelled);
            }
            let file = std::fs::OpenOptions::new()
                .read(true)
                .write(true)
                .open(path)
                .map_err(|e| format!("Не удалось открыть {}: {e}", path.display()))?;
            let fd = file.as_raw_fd();
            let size = file
                .metadata()
                .map(|m| m.len())
                .map_err(|e| format!("metadata {}: {e}", path.display()))?;

            let flags = get_flags(fd).map_err(|_| ioctl_err("FS_IOC_GETFLAGS", path))?;
            // Запрещаем сжатие и перезаписываем экстенты без компрессии
            set_flags(fd, (flags & !FS_COMPR_FL) | FS_NOCOMP_FL)
                .map_err(|_| ioctl_err("FS_IOC_SETFLAGS", path))?;
            let outcome = defrag_chunked(fd, path, size, false, cancelled)?;
            unsafe {
                libc::fsync(fd);
            }
            // Возвращаем флаги в нейтральное состояние
            let flags = get_flags(fd).map_err(|_| ioctl_err("FS_IOC_GETFLAGS", path))?;
            set_flags(fd, flags & !FS_NOCOMP_FL)
                .map_err(|_| ioctl_err("FS_IOC_SETFLAGS", path))?;
            Ok(outcome)
        }
    }
}
