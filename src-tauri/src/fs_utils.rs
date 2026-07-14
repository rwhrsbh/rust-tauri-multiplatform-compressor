//! Определение файловой системы диска, на котором лежит папка игры,
//! и проверка поддержки прозрачного сжатия.

use serde::Serialize;
use std::path::Path;

/// Информация о диске/разделе, возвращаемая на фронтенд.
#[derive(Serialize, Clone, Debug)]
pub struct DiskInfo {
    /// Путь, который проверяли.
    pub path: String,
    /// Корень тома / точка монтирования.
    pub mount_point: String,
    /// Имя файловой системы (NTFS, APFS, btrfs, ext4, FAT32, ...).
    pub filesystem: String,
    pub total_bytes: u64,
    pub free_bytes: u64,
    /// Поддерживает ли ФС прозрачное сжатие.
    pub supported: bool,
    /// Человекочитаемое пояснение статуса (технический fallback).
    pub reason: String,
    /// Код причины для локализации на фронтенде:
    /// "ok.ntfs" | "ok.apfs" | "ok.btrfs" | "bad.fat" | "bad.generic"
    pub reason_code: String,
}

fn reason_code_for(fs: &str, supported: bool, ok_code: &str) -> String {
    if supported {
        return ok_code.to_string();
    }
    let fat = matches!(
        fs.to_ascii_uppercase().as_str(),
        "FAT32" | "FAT" | "FAT16" | "EXFAT" | "MSDOS" | "VFAT"
    );
    if fat { "bad.fat".into() } else { "bad.generic".into() }
}

fn unsupported_reason(fs: &str) -> String {
    let fat = matches!(
        fs.to_ascii_uppercase().as_str(),
        "FAT32" | "FAT" | "FAT16" | "EXFAT" | "MSDOS" | "VFAT"
    );
    if fat {
        format!(
            "Файловая система {fs} не поддерживает сжатие. Перенесите игру на диск с NTFS (Windows), APFS (macOS) или Btrfs (Linux)."
        )
    } else {
        format!(
            "Файловая система {fs} не поддерживает прозрачное сжатие на лету. Перенесите игру на диск с NTFS (Windows), APFS (macOS) или Btrfs (Linux)."
        )
    }
}

// ---------------------------------------------------------------------------
// Windows: GetVolumePathNameW + GetVolumeInformationW + GetDiskFreeSpaceExW
// ---------------------------------------------------------------------------
#[cfg(target_os = "windows")]
pub fn check_filesystem(path: &Path) -> Result<DiskInfo, String> {
    use std::os::windows::ffi::OsStrExt;
    use windows::core::PCWSTR;
    use windows::Win32::Storage::FileSystem::{
        GetDiskFreeSpaceExW, GetVolumeInformationW, GetVolumePathNameW,
    };

    if !path.exists() {
        return Err(format!("Путь не существует: {}", path.display()));
    }

    let wide: Vec<u16> = path.as_os_str().encode_wide().chain(Some(0)).collect();

    // Корень тома, на котором лежит папка (например, "D:\").
    let mut root_buf = [0u16; 512];
    unsafe { GetVolumePathNameW(PCWSTR(wide.as_ptr()), &mut root_buf) }
        .map_err(|e| format!("GetVolumePathNameW: {e}"))?;
    let root_len = root_buf.iter().position(|&c| c == 0).unwrap_or(0);
    let mount_point = String::from_utf16_lossy(&root_buf[..root_len]);

    // Имя файловой системы тома.
    let mut fs_buf = [0u16; 64];
    unsafe {
        GetVolumeInformationW(
            PCWSTR(root_buf.as_ptr()),
            None,
            None,
            None,
            None,
            Some(&mut fs_buf),
        )
    }
    .map_err(|e| format!("GetVolumeInformationW: {e}"))?;
    let fs_len = fs_buf.iter().position(|&c| c == 0).unwrap_or(0);
    let filesystem = String::from_utf16_lossy(&fs_buf[..fs_len]);

    // Свободное/общее место.
    let mut free: u64 = 0;
    let mut total: u64 = 0;
    unsafe {
        GetDiskFreeSpaceExW(
            PCWSTR(root_buf.as_ptr()),
            Some(&mut free),
            Some(&mut total),
            None,
        )
    }
    .map_err(|e| format!("GetDiskFreeSpaceExW: {e}"))?;

    let supported = filesystem.eq_ignore_ascii_case("NTFS");
    let reason = if supported {
        "Файловая система подходит. Прозрачное сжатие поддерживается (NTFS: WOF/LZX с откатом на LZNT1).".to_string()
    } else {
        unsupported_reason(&filesystem)
    };
    let reason_code = reason_code_for(&filesystem, supported, "ok.ntfs");

    Ok(DiskInfo {
        path: path.display().to_string(),
        mount_point,
        filesystem,
        total_bytes: total,
        free_bytes: free,
        supported,
        reason,
        reason_code,
    })
}

// ---------------------------------------------------------------------------
// macOS: statfs -> f_fstypename / f_mntonname
// ---------------------------------------------------------------------------
#[cfg(target_os = "macos")]
pub fn check_filesystem(path: &Path) -> Result<DiskInfo, String> {
    use std::ffi::{CStr, CString};
    use std::os::unix::ffi::OsStrExt;

    if !path.exists() {
        return Err(format!("Путь не существует: {}", path.display()));
    }

    let c_path = CString::new(path.as_os_str().as_bytes())
        .map_err(|_| "Некорректный путь".to_string())?;
    let mut st: libc::statfs = unsafe { std::mem::zeroed() };
    if unsafe { libc::statfs(c_path.as_ptr(), &mut st) } != 0 {
        return Err(format!(
            "statfs: {}",
            std::io::Error::last_os_error()
        ));
    }

    let filesystem = unsafe { CStr::from_ptr(st.f_fstypename.as_ptr()) }
        .to_string_lossy()
        .to_string();
    let mount_point = unsafe { CStr::from_ptr(st.f_mntonname.as_ptr()) }
        .to_string_lossy()
        .to_string();

    let bsize = st.f_bsize as u64;
    let total = st.f_blocks * bsize;
    let free = st.f_bavail * bsize;

    let fs_lower = filesystem.to_ascii_lowercase();
    let supported = fs_lower == "apfs" || fs_lower == "hfs";
    let display_fs = match fs_lower.as_str() {
        "apfs" => "APFS".to_string(),
        "hfs" => "HFS+".to_string(),
        "msdos" => "FAT32".to_string(),
        "exfat" => "exFAT".to_string(),
        _ => filesystem.clone(),
    };

    let reason = if supported {
        "Файловая система подходит. Прозрачное сжатие поддерживается (APFS/HFS+: decmpfs).".to_string()
    } else {
        unsupported_reason(&display_fs)
    };
    let reason_code = reason_code_for(&display_fs, supported, "ok.apfs");

    Ok(DiskInfo {
        path: path.display().to_string(),
        mount_point,
        filesystem: display_fs,
        total_bytes: total,
        free_bytes: free,
        supported,
        reason,
        reason_code,
    })
}

// ---------------------------------------------------------------------------
// Linux: statfs -> магические числа f_type, точка монтирования из /proc/mounts
// ---------------------------------------------------------------------------
#[cfg(target_os = "linux")]
pub fn check_filesystem(path: &Path) -> Result<DiskInfo, String> {
    use std::ffi::CString;
    use std::os::unix::ffi::OsStrExt;

    const BTRFS_SUPER_MAGIC: i64 = 0x9123_683E;
    const EXT4_SUPER_MAGIC: i64 = 0xEF53;
    const XFS_SUPER_MAGIC: i64 = 0x5846_5342;
    const MSDOS_SUPER_MAGIC: i64 = 0x4D44;
    const EXFAT_SUPER_MAGIC: i64 = 0x2011_BAB0;
    const NTFS_SB_MAGIC: i64 = 0x5346_544E;
    const NTFS3_MAGIC: i64 = 0x7366_746E;
    const F2FS_SUPER_MAGIC: i64 = 0xF2F5_2010;
    const ZFS_SUPER_MAGIC: i64 = 0x2FC1_2FC1;
    const TMPFS_MAGIC: i64 = 0x0102_1994;
    const OVERLAYFS_MAGIC: i64 = 0x794C_7630;
    const FUSE_SUPER_MAGIC: i64 = 0x6573_5546;

    if !path.exists() {
        return Err(format!("Путь не существует: {}", path.display()));
    }

    let c_path = CString::new(path.as_os_str().as_bytes())
        .map_err(|_| "Некорректный путь".to_string())?;
    let mut st: libc::statfs = unsafe { std::mem::zeroed() };
    if unsafe { libc::statfs(c_path.as_ptr(), &mut st) } != 0 {
        return Err(format!("statfs: {}", std::io::Error::last_os_error()));
    }

    let magic = st.f_type as i64;
    let filesystem = match magic {
        BTRFS_SUPER_MAGIC => "Btrfs",
        EXT4_SUPER_MAGIC => "ext4",
        XFS_SUPER_MAGIC => "XFS",
        MSDOS_SUPER_MAGIC => "FAT32",
        EXFAT_SUPER_MAGIC => "exFAT",
        NTFS_SB_MAGIC | NTFS3_MAGIC => "NTFS (Linux)",
        F2FS_SUPER_MAGIC => "F2FS",
        ZFS_SUPER_MAGIC => "ZFS",
        TMPFS_MAGIC => "tmpfs",
        OVERLAYFS_MAGIC => "overlayfs",
        FUSE_SUPER_MAGIC => "FUSE",
        _ => "неизвестная",
    }
    .to_string();

    let bsize = st.f_bsize as u64;
    let total = st.f_blocks as u64 * bsize;
    let free = st.f_bavail as u64 * bsize;

    let mount_point = find_mount_point(path);

    let supported = magic == BTRFS_SUPER_MAGIC;
    let reason = if supported {
        "Файловая система подходит. Прозрачное сжатие поддерживается (Btrfs: zstd через defrag).".to_string()
    } else {
        unsupported_reason(&filesystem)
    };
    let reason_code = reason_code_for(&filesystem, supported, "ok.btrfs");

    Ok(DiskInfo {
        path: path.display().to_string(),
        mount_point,
        filesystem,
        total_bytes: total,
        free_bytes: free,
        supported,
        reason,
        reason_code,
    })
}

/// Ищет самую длинную точку монтирования из /proc/mounts, являющуюся префиксом пути.
#[cfg(target_os = "linux")]
fn find_mount_point(path: &Path) -> String {
    let canonical = path.canonicalize().unwrap_or_else(|_| path.to_path_buf());
    let mounts = std::fs::read_to_string("/proc/mounts").unwrap_or_default();
    let mut best = "/".to_string();
    for line in mounts.lines() {
        if let Some(raw) = line.split_whitespace().nth(1) {
            // В /proc/mounts пробелы экранированы как \040
            let mp = raw.replace("\\040", " ");
            if canonical.starts_with(&mp) && mp.len() > best.len() {
                best = mp;
            }
        }
    }
    best
}
