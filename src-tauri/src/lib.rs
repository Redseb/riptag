mod library;

use serde::Serialize;
use std::path::PathBuf;
use tauri::ipc::Channel;

#[derive(Serialize)]
struct AppInfo {
    version: &'static str,
    os: &'static str,
    arch: &'static str,
}

fn app_info() -> AppInfo {
    AppInfo {
        version: env!("CARGO_PKG_VERSION"),
        os: std::env::consts::OS,
        arch: std::env::consts::ARCH,
    }
}

#[tauri::command]
fn get_app_info() -> AppInfo {
    app_info()
}

// Async so a large library doesn't block the window while it's read.
#[tauri::command]
async fn scan_library(path: PathBuf) -> Result<library::ScanResult, String> {
    tauri::async_runtime::spawn_blocking(move || library::scan_library(&path, &library::ECHO_MINI))
        .await
        .map_err(|e| e.to_string())?
}

#[tauri::command]
async fn repair_library(
    source: PathBuf,
    output: PathBuf,
    on_progress: Channel<library::RepairProgress>,
) -> Result<library::RepairReport, String> {
    tauri::async_runtime::spawn_blocking(move || {
        library::repair_library(&source, &output, &library::ECHO_MINI, |progress| {
            let _ = on_progress.send(progress);
        })
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
async fn fetch_lyrics(
    source: PathBuf,
    output: PathBuf,
    on_progress: Channel<library::RepairProgress>,
) -> Result<library::LyricsReport, String> {
    tauri::async_runtime::spawn_blocking(move || {
        library::fetch_lyrics(
            &source,
            &output,
            &library::ECHO_MINI,
            library::lrclib_lookup,
            |progress| {
                let _ = on_progress.send(progress);
            },
        )
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
fn find_player_volumes() -> Result<Vec<library::PlayerVolume>, String> {
    library::find_player_volumes(&library::ECHO_MINI)
}

// The volume is looked up again, so only a detected player can be written to.
#[tauri::command]
async fn sync_to_device(
    output: PathBuf,
    volume: PathBuf,
    on_progress: Channel<library::RepairProgress>,
) -> Result<library::SyncReport, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let found = library::find_player_volumes(&library::ECHO_MINI)?
            .into_iter()
            .find(|player| player.path == volume)
            .ok_or("That player isn't connected")?;
        library::sync_to_device(&output, &found, |progress| {
            let _ = on_progress.send(progress);
        })
    })
    .await
    .map_err(|e| e.to_string())?
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .invoke_handler(tauri::generate_handler![
            get_app_info,
            scan_library,
            repair_library,
            fetch_lyrics,
            find_player_volumes,
            sync_to_device
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn app_info_reports_crate_version() {
        assert_eq!(app_info().version, env!("CARGO_PKG_VERSION"));
    }
}
