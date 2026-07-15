//! Обнаружение установленных игр из лаунчеров: Steam (все платформы),
//! Epic Games и GOG Galaxy (Windows). Обложки берутся из локального
//! кэша Steam и отдаются фронтенду как data:-URI.

use base64::Engine;
use serde::Serialize;
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

#[derive(Serialize, Clone)]
pub struct GameEntry {
    pub name: String,
    /// Папка с файлами игры (то, что сжимаем).
    pub path: String,
    /// "steam" | "epic" | "gog"
    pub launcher: &'static str,
    /// Обложка как data:-URI (если нашлась в локальном кэше лаунчера).
    pub cover: Option<String>,
}

/// Сканирует все известные лаунчеры и возвращает список игр.
pub fn scan_all() -> Vec<GameEntry> {
    let mut games = steam_games();
    #[cfg(windows)]
    {
        games.extend(epic_games());
        games.extend(gog_games());
    }
    let mut seen = HashSet::new();
    games.retain(|g| seen.insert(g.path.to_lowercase()));
    games.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));
    games
}

// ===========================================================================
// Общие помощники
// ===========================================================================

/// Извлекает пары "ключ" "значение" из VDF/ACF (формат Valve).
/// Вложенность игнорируется — нам нужны только листовые пары.
fn vdf_pairs(text: &str) -> Vec<(String, String)> {
    let mut out = Vec::new();
    for line in text.lines() {
        let mut parts: Vec<&str> = Vec::new();
        let mut rest = line.trim();
        while let Some(start) = rest.find('"') {
            let after = &rest[start + 1..];
            let Some(end) = after.find('"') else { break };
            parts.push(&after[..end]);
            rest = &after[end + 1..];
        }
        if parts.len() == 2 {
            out.push((parts[0].to_string(), parts[1].replace("\\\\", "\\")));
        }
    }
    out
}

/// Читает картинку и кодирует в data:-URI (лимит 1.5 МБ на обложку).
fn read_data_uri(path: &Path) -> Option<String> {
    let meta = std::fs::metadata(path).ok()?;
    if !meta.is_file() || meta.len() > 1_500_000 {
        return None;
    }
    let bytes = std::fs::read(path).ok()?;
    let mime = match path
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_ascii_lowercase()
        .as_str()
    {
        "png" => "image/png",
        "webp" => "image/webp",
        _ => "image/jpeg",
    };
    Some(format!(
        "data:{};base64,{}",
        mime,
        base64::engine::general_purpose::STANDARD.encode(bytes)
    ))
}

// ===========================================================================
// Steam
// ===========================================================================

fn steam_root() -> Option<PathBuf> {
    #[cfg(windows)]
    {
        if let Some(p) = win_reg::get_string(
            win_reg::CURRENT_USER,
            "Software\\Valve\\Steam",
            "SteamPath",
        ) {
            let pb = PathBuf::from(p.replace('/', "\\"));
            if pb.is_dir() {
                return Some(pb);
            }
        }
        for c in [
            "C:\\Program Files (x86)\\Steam",
            "C:\\Program Files\\Steam",
        ] {
            let pb = PathBuf::from(c);
            if pb.is_dir() {
                return Some(pb);
            }
        }
        None
    }
    #[cfg(target_os = "macos")]
    {
        let home = std::env::var("HOME").ok()?;
        let pb = PathBuf::from(home).join("Library/Application Support/Steam");
        pb.is_dir().then_some(pb)
    }
    #[cfg(target_os = "linux")]
    {
        let home = std::env::var("HOME").ok()?;
        for c in [".steam/steam", ".local/share/Steam"] {
            let pb = PathBuf::from(&home).join(c);
            if pb.is_dir() {
                return Some(pb);
            }
        }
        None
    }
}

fn steam_games() -> Vec<GameEntry> {
    let mut games = Vec::new();
    let Some(root) = steam_root() else {
        return games;
    };

    // Все библиотеки Steam: основная + из libraryfolders.vdf
    let mut libs = vec![root.join("steamapps")];
    if let Ok(text) = std::fs::read_to_string(root.join("steamapps").join("libraryfolders.vdf")) {
        for (k, v) in vdf_pairs(&text) {
            if k.eq_ignore_ascii_case("path") {
                let p = PathBuf::from(v).join("steamapps");
                if p.is_dir() && !libs.contains(&p) {
                    libs.push(p);
                }
            }
        }
    }

    for lib in libs {
        let Ok(rd) = std::fs::read_dir(&lib) else {
            continue;
        };
        for e in rd.flatten() {
            let fname = e.file_name().to_string_lossy().to_string();
            if !(fname.starts_with("appmanifest_") && fname.ends_with(".acf")) {
                continue;
            }
            let Ok(text) = std::fs::read_to_string(e.path()) else {
                continue;
            };
            let map: HashMap<String, String> = vdf_pairs(&text).into_iter().collect();
            let (Some(appid), Some(name), Some(installdir)) =
                (map.get("appid"), map.get("name"), map.get("installdir"))
            else {
                continue;
            };
            // Служебные пакеты Steam (redistributables) — не игры
            if name.contains("Steamworks") || name.contains("Redistributable") {
                continue;
            }
            let dir = lib.join("common").join(installdir);
            if !dir.is_dir() {
                continue;
            }
            games.push(GameEntry {
                name: name.clone(),
                path: dir.display().to_string(),
                launcher: "steam",
                cover: steam_cover(&root, appid),
            });
        }
    }
    games
}

/// Обложка из локального кэша Steam. Steam менял раскладку кэша,
/// поэтому проверяем оба варианта: плоские файлы и подпапку appid.
fn steam_cover(root: &Path, appid: &str) -> Option<String> {
    let cache = root.join("appcache").join("librarycache");
    for c in [
        cache.join(format!("{appid}_library_600x900.jpg")),
        cache.join(format!("{appid}_library_600x900.png")),
        cache.join(appid).join("library_600x900.jpg"),
        cache.join(appid).join("library_600x900.png"),
    ] {
        if let Some(d) = read_data_uri(&c) {
            return Some(d);
        }
    }
    // Новейшая раскладка: librarycache/<appid>/<hash>.jpg
    if let Ok(rd) = std::fs::read_dir(cache.join(appid)) {
        for e in rd.flatten() {
            let n = e.file_name().to_string_lossy().to_lowercase();
            if n.contains("library_600x900") {
                if let Some(d) = read_data_uri(&e.path()) {
                    return Some(d);
                }
            }
        }
    }
    None
}

// ===========================================================================
// Epic Games (Windows): манифесты установленных игр
// ===========================================================================

#[cfg(windows)]
fn epic_games() -> Vec<GameEntry> {
    let mut games = Vec::new();
    let pd = std::env::var("ProgramData").unwrap_or_else(|_| "C:\\ProgramData".into());
    let dir = PathBuf::from(pd).join("Epic\\EpicGamesLauncher\\Data\\Manifests");
    let Ok(rd) = std::fs::read_dir(&dir) else {
        return games;
    };
    for e in rd.flatten() {
        if e.path().extension().and_then(|x| x.to_str()) != Some("item") {
            continue;
        }
        let Ok(text) = std::fs::read_to_string(e.path()) else {
            continue;
        };
        let Ok(v) = serde_json::from_str::<serde_json::Value>(&text) else {
            continue;
        };
        let name = v["DisplayName"].as_str().unwrap_or_default().to_string();
        let path = v["InstallLocation"].as_str().unwrap_or_default().to_string();
        if name.is_empty() || path.is_empty() || !Path::new(&path).is_dir() {
            continue;
        }
        games.push(GameEntry {
            name,
            path,
            launcher: "epic",
            cover: None,
        });
    }
    games
}

// ===========================================================================
// GOG Galaxy (Windows): реестр установленных игр
// ===========================================================================

#[cfg(windows)]
fn gog_games() -> Vec<GameEntry> {
    let mut games = Vec::new();
    for view in [
        "SOFTWARE\\WOW6432Node\\GOG.com\\Games",
        "SOFTWARE\\GOG.com\\Games",
    ] {
        for sub in win_reg::subkeys(win_reg::LOCAL_MACHINE, view) {
            let key = format!("{view}\\{sub}");
            let (Some(name), Some(path)) = (
                win_reg::get_string(win_reg::LOCAL_MACHINE, &key, "gameName"),
                win_reg::get_string(win_reg::LOCAL_MACHINE, &key, "path"),
            ) else {
                continue;
            };
            if !Path::new(&path).is_dir() {
                continue;
            }
            games.push(GameEntry {
                name,
                path,
                launcher: "gog",
                cover: None,
            });
        }
    }
    games
}

// ===========================================================================
// Чтение реестра Windows (Unicode-корректно, без парсинга вывода reg.exe)
// ===========================================================================

#[cfg(windows)]
mod win_reg {
    use windows::core::{PCWSTR, PWSTR};
    use windows::Win32::System::Registry::{
        RegCloseKey, RegEnumKeyExW, RegGetValueW, RegOpenKeyExW, HKEY, HKEY_CURRENT_USER,
        HKEY_LOCAL_MACHINE, KEY_READ, RRF_RT_REG_SZ,
    };

    pub const CURRENT_USER: HKEY = HKEY_CURRENT_USER;
    pub const LOCAL_MACHINE: HKEY = HKEY_LOCAL_MACHINE;

    fn wide(s: &str) -> Vec<u16> {
        s.encode_utf16().chain(Some(0)).collect()
    }

    /// Строковое значение реестра (REG_SZ).
    pub fn get_string(root: HKEY, subkey: &str, value: &str) -> Option<String> {
        let sk = wide(subkey);
        let val = wide(value);
        let mut len: u32 = 0;
        unsafe {
            RegGetValueW(
                root,
                PCWSTR(sk.as_ptr()),
                PCWSTR(val.as_ptr()),
                RRF_RT_REG_SZ,
                None,
                None,
                Some(&mut len),
            )
            .ok()
            .ok()?;
            let mut buf = vec![0u16; len as usize / 2 + 1];
            RegGetValueW(
                root,
                PCWSTR(sk.as_ptr()),
                PCWSTR(val.as_ptr()),
                RRF_RT_REG_SZ,
                None,
                Some(buf.as_mut_ptr() as *mut _),
                Some(&mut len),
            )
            .ok()
            .ok()?;
            let chars = (len as usize / 2).saturating_sub(1);
            Some(String::from_utf16_lossy(&buf[..chars]))
        }
    }

    /// Имена подключей.
    pub fn subkeys(root: HKEY, subkey: &str) -> Vec<String> {
        let sk = wide(subkey);
        let mut out = Vec::new();
        let mut hkey = HKEY::default();
        unsafe {
            if RegOpenKeyExW(root, PCWSTR(sk.as_ptr()), 0, KEY_READ, &mut hkey)
                .ok()
                .is_err()
            {
                return out;
            }
            let mut index = 0u32;
            loop {
                let mut buf = [0u16; 256];
                let mut len = buf.len() as u32;
                let res = RegEnumKeyExW(
                    hkey,
                    index,
                    PWSTR(buf.as_mut_ptr()),
                    &mut len,
                    None,
                    PWSTR::null(),
                    None,
                    None,
                );
                if res.ok().is_err() {
                    break;
                }
                out.push(String::from_utf16_lossy(&buf[..len as usize]));
                index += 1;
            }
            let _ = RegCloseKey(hkey);
        }
        out
    }
}
