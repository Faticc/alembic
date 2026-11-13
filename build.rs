//! Кладёт steam_api64.dll рядом с собранным exe: без неё игра не запустится.
//! Библиотеку распаковывает крейт steamworks-sys в свою папку сборки - берём её оттуда.

use std::path::PathBuf;

fn main() {
    // OUT_DIR = target/<profile>/build/alembic-<hash>/out
    let out = PathBuf::from(std::env::var("OUT_DIR").unwrap());
    let build_dir = out.parent().and_then(|p| p.parent()).map(PathBuf::from);
    let profile_dir = build_dir.as_ref().and_then(|p| p.parent()).map(PathBuf::from);
    if let (Some(build_dir), Some(profile_dir)) = (build_dir, profile_dir) {
        if let Ok(entries) = std::fs::read_dir(&build_dir) {
            for e in entries.flatten() {
                if e.file_name().to_string_lossy().starts_with("steamworks-sys-") {
                    let dll = e.path().join("out").join("steam_api64.dll");
                    if dll.exists() {
                        let _ = std::fs::copy(&dll, profile_dir.join("steam_api64.dll"));
                    }
                }
            }
        }
    }
    println!("cargo:rerun-if-changed=build.rs");
}
