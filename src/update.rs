//! Автообновление через релизы GitHub.
//!
//! При запуске игра в фоне спрашивает у GitHub метку последнего релиза. Если она новее
//! текущей версии, рядом с игрой скачивается `alembic.new.exe`. Подмена происходит при
//! выходе из игры (или сразу по F5): работающий exe переименовывается в `alembic.old.exe`,
//! новый встаёт на его место. Сохранение не затрагивается.
//!
//! Скачивание идёт системным `curl.exe` (есть в Windows 10 и 11), без сетевых библиотек.
//! На GitHub не передаётся ничего, кроме самого запроса.

use std::os::windows::process::CommandExt;
use std::path::PathBuf;
use std::process::Command;
use std::sync::mpsc::{channel, Receiver};

pub const REPO: &str = "Faticc/alembic";
pub const VERSION: &str = env!("CARGO_PKG_VERSION");
const CREATE_NO_WINDOW: u32 = 0x0800_0000;

fn exe() -> Option<PathBuf> {
    std::env::current_exe().ok()
}

fn sibling(name: &str) -> Option<PathBuf> {
    exe().map(|e| e.with_file_name(name))
}

fn curl(args: &[&str]) -> Option<Vec<u8>> {
    let out = Command::new("curl.exe")
        .args(["-s", "-L", "-f", "-A", "alembic-updater"])
        .args(args)
        .creation_flags(CREATE_NO_WINDOW)
        .output()
        .ok()?;
    if out.status.success() {
        Some(out.stdout)
    } else {
        None
    }
}

/// "v1.2.3" или "1.2.3" -> (1, 2, 3).
pub fn parse_version(s: &str) -> Option<(u32, u32, u32)> {
    let mut it = s.trim().trim_start_matches('v').split('.').map(|p| p.parse::<u32>().ok());
    Some((it.next()??, it.next()??, it.next()??))
}

/// Версия, с которой сравниваем. Переменная окружения нужна только для проверки обновления.
fn current_version() -> (u32, u32, u32) {
    std::env::var("ALEMBIC_FAKE_VERSION")
        .ok()
        .and_then(|v| parse_version(&v))
        .or_else(|| parse_version(VERSION))
        .unwrap_or((0, 0, 0))
}

/// Спрашивает GitHub и, если вышла версия новее, скачивает её рядом с игрой.
/// Ok(Some(метка)) - обновление скачано; Ok(None) - обновлять нечего.
pub fn check_and_download() -> Result<Option<String>, String> {
    let api = format!("https://api.github.com/repos/{}/releases/latest", REPO);
    let body = curl(&["-m", "15", &api]).ok_or("не удалось связаться с GitHub")?;
    let body = String::from_utf8_lossy(&body);
    let tag = body
        .split("\"tag_name\"")
        .nth(1)
        .and_then(|rest| rest.split('"').nth(1))
        .ok_or("в ответе GitHub нет метки релиза")?
        .to_string();
    let latest = parse_version(&tag).ok_or("непонятная метка релиза")?;
    if latest <= current_version() {
        return Ok(None);
    }
    let new = sibling("alembic.new.exe").ok_or("неизвестен путь к игре")?;
    let url = format!("https://github.com/{}/releases/download/{}/alembic.exe", REPO, tag);
    let tmp = new.with_extension("part");
    curl(&["-m", "300", "-o", &tmp.to_string_lossy(), &url]).ok_or("не удалось скачать обновление")?;
    // Защита от обрывов и страниц-заглушек: это должен быть настоящий exe разумного размера.
    let data = std::fs::read(&tmp).map_err(|e| e.to_string())?;
    if data.len() < 200_000 || !data.starts_with(b"MZ") {
        let _ = std::fs::remove_file(&tmp);
        return Err("скачанный файл не похож на игру".to_string());
    }
    std::fs::rename(&tmp, &new).map_err(|e| e.to_string())?;
    Ok(Some(tag))
}

/// Запускает проверку в фоне. None - обновления отключены (сборка из исходников в target).
pub fn start() -> Option<Receiver<String>> {
    cleanup();
    let path = exe()?.to_string_lossy().to_lowercase();
    if cfg!(debug_assertions) || path.contains("\\target\\") {
        return None;
    }
    let (tx, rx) = channel();
    std::thread::spawn(move || {
        if let Ok(Some(tag)) = check_and_download() {
            let _ = tx.send(tag);
        }
    });
    Some(rx)
}

/// Убирает хвосты прошлого обновления.
pub fn cleanup() {
    for name in ["alembic.old.exe", "alembic.new.exe", "alembic.new.part"] {
        if let Some(p) = sibling(name) {
            let _ = std::fs::remove_file(p);
        }
    }
}

/// Подменяет работающий exe скачанным. Windows разрешает переименовать запущенный файл,
/// поэтому старый уходит в `alembic.old.exe` и удаляется при следующем запуске.
pub fn apply(relaunch: bool) -> Result<(), String> {
    let exe = exe().ok_or("неизвестен путь к игре")?;
    let new = exe.with_file_name("alembic.new.exe");
    let old = exe.with_file_name("alembic.old.exe");
    if !new.exists() {
        return Err("обновление не скачано".to_string());
    }
    let _ = std::fs::remove_file(&old);
    std::fs::rename(&exe, &old).map_err(|e| e.to_string())?;
    if let Err(e) = std::fs::rename(&new, &exe) {
        // Не получилось - возвращаем прежний файл на место.
        let _ = std::fs::rename(&old, &exe);
        return Err(e.to_string());
    }
    if relaunch {
        Command::new(&exe).spawn().map_err(|e| e.to_string())?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::parse_version;

    #[test]
    fn versions_compare() {
        assert_eq!(parse_version("v0.2.0"), Some((0, 2, 0)));
        assert_eq!(parse_version("1.10.3"), Some((1, 10, 3)));
        assert!(parse_version("v0.10.0") > parse_version("v0.9.9"));
        assert!(parse_version("nightly").is_none());
        assert!(parse_version("v1.2").is_none());
    }
}
