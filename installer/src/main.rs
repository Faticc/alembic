//! Установщик Alembic одним файлом, без сторонних инструментов.
//!
//! Игра и steam_api64.dll вшиты в этот exe. Установка идёт в профиль пользователя
//! (%LOCALAPPDATA%\Alembic), поэтому права администратора не нужны.
//!
//! Ключи: `/S` - тихо, без окон; `/NOSHORTCUTS` - не создавать ярлыки;
//! `/D=<папка>` - установить в другую папку.
//!
//! Перед сборкой установщика нужно собрать саму игру: `cargo build --release` в корне.

#![windows_subsystem = "windows"]

use std::os::windows::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::Command;

const GAME_EXE: &[u8] = include_bytes!("../../target/release/alembic.exe");
const STEAM_DLL: &[u8] = include_bytes!("../../target/release/steam_api64.dll");

const TITLE: &str = "Alembic - установка";
const CREATE_NO_WINDOW: u32 = 0x0800_0000;

const UNINSTALL_CMD: &str = "@echo off\r\n\
powershell -NoProfile -Command \"foreach($f in 'Desktop','Programs'){Remove-Item (Join-Path ([Environment]::GetFolderPath($f)) 'Alembic.lnk') -ErrorAction SilentlyContinue}\"\r\n\
cd /d \"%TEMP%\"\r\n\
start \"\" /b cmd /c \"timeout /t 1 >nul & rmdir /s /q \"\"%~dp0\"\"\"\r\n";

const SHORTCUT_PS: &str = "$s=New-Object -ComObject WScript.Shell;\
foreach($f in 'Desktop','Programs'){\
$l=$s.CreateShortcut((Join-Path ([Environment]::GetFolderPath($f)) 'Alembic.lnk'));\
$l.TargetPath=(Join-Path $env:ALEMBIC_DIR 'alembic.exe');\
$l.WorkingDirectory=$env:ALEMBIC_DIR;\
$l.Description='Alembic';\
$l.Save()}";

#[link(name = "user32")]
extern "system" {
    fn MessageBoxW(hwnd: *mut core::ffi::c_void, text: *const u16, caption: *const u16, flags: u32) -> i32;
}

const MB_YESNO: u32 = 0x4;
const MB_ICONERROR: u32 = 0x10;
const MB_ICONQUESTION: u32 = 0x20;
const MB_ICONINFORMATION: u32 = 0x40;
const IDYES: i32 = 6;

fn msg(text: &str, flags: u32) -> i32 {
    let wide = |s: &str| s.encode_utf16().chain(Some(0)).collect::<Vec<u16>>();
    unsafe { MessageBoxW(std::ptr::null_mut(), wide(text).as_ptr(), wide(TITLE).as_ptr(), flags) }
}

fn install(dir: &Path, shortcuts: bool) -> Result<(), String> {
    let step = |what: &str, r: std::io::Result<()>| r.map_err(|e| format!("{}: {}", what, e));
    step("не удалось создать папку", std::fs::create_dir_all(dir))?;
    step("не удалось записать alembic.exe (игра сейчас запущена?)", std::fs::write(dir.join("alembic.exe"), GAME_EXE))?;
    step("не удалось записать steam_api64.dll", std::fs::write(dir.join("steam_api64.dll"), STEAM_DLL))?;
    step("не удалось записать uninstall.cmd", std::fs::write(dir.join("uninstall.cmd"), UNINSTALL_CMD))?;
    if shortcuts {
        // Путь передаётся через переменную окружения, чтобы не ломаться на пробелах и кириллице.
        let status = Command::new("powershell")
            .args(["-NoProfile", "-ExecutionPolicy", "Bypass", "-Command", SHORTCUT_PS])
            .env("ALEMBIC_DIR", dir)
            .creation_flags(CREATE_NO_WINDOW)
            .status();
        if !matches!(status, Ok(s) if s.success()) {
            return Err("игра установлена, но ярлыки создать не удалось".to_string());
        }
    }
    Ok(())
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let flag = |f: &str| args.iter().any(|a| a.eq_ignore_ascii_case(f));
    let silent = flag("/S");
    let dir = args
        .iter()
        .find_map(|a| a.strip_prefix("/D=").map(PathBuf::from))
        .or_else(|| std::env::var_os("LOCALAPPDATA").map(|p| PathBuf::from(p).join("Alembic")))
        .unwrap_or_else(|| PathBuf::from("Alembic"));

    if !silent {
        let q = format!(
            "Установить игру Alembic?\n\nПапка: {}\nЯрлыки появятся на рабочем столе и в меню «Пуск».\n\n\
             Для сетевой игры нужен запущенный Steam.",
            dir.display()
        );
        if msg(&q, MB_YESNO | MB_ICONQUESTION) != IDYES {
            return;
        }
    }
    match install(&dir, !flag("/NOSHORTCUTS")) {
        Ok(()) => {
            if silent {
                return;
            }
            let q = "Alembic установлен.\n\nУдалить игру можно файлом uninstall.cmd в её папке.\n\nЗапустить сейчас?";
            if msg(q, MB_YESNO | MB_ICONINFORMATION) == IDYES {
                let _ = Command::new(dir.join("alembic.exe")).current_dir(&dir).spawn();
            }
        }
        Err(e) => {
            if !silent {
                msg(&format!("Установка не удалась: {}", e), MB_ICONERROR);
            }
            std::process::exit(1);
        }
    }
}
