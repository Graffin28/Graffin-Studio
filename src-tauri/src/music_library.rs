//! Imported music lives in the application library, independent of the source file.
use crate::resource;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{fs, io::{Read, Write}, path::{Path, PathBuf}, sync::Mutex};
use tauri::{AppHandle, Manager};

const MAX_BYTES: u64 = 209_715_200;
static LOCK: Mutex<()> = Mutex::new(());

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MusicAsset { pub id: String, pub name: String, pub path: String, pub duration: f64 }

fn root(app: &AppHandle) -> Result<PathBuf, String> {
    let path = app.path().app_data_dir().map_err(|e| e.to_string())?.join("music-assets");
    fs::create_dir_all(&path).map_err(|_| "Не создать библиотеку музыки.")?;
    path.canonicalize().map_err(|_| "Библиотека музыки недоступна.".into())
}

fn safe_entry(root: &Path, entry: &MusicAsset) -> bool {
    entry.id.len() == 64 && entry.id.bytes().all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase())
        && entry.duration.is_finite() && entry.duration > 0.0 && entry.duration <= 86_400.0
        && entry.name.chars().count() <= 120
        && Path::new(&entry.path).file_stem().and_then(|v| v.to_str()) == Some(entry.id.as_str())
        && Path::new(&entry.path).canonicalize().ok().is_some_and(|path| path.parent() == Some(root) && path.is_file())
}

fn list(app: &AppHandle) -> Result<Vec<MusicAsset>, String> {
    let root = root(app)?;
    let mut entries = Vec::new();
    for file in fs::read_dir(&root).map_err(|_| "Не прочитать библиотеку музыки.")?.take(500) {
        let path = file.map_err(|_| "Не прочитать библиотеку музыки.")?.path();
        if path.extension().and_then(|v| v.to_str()) != Some("json") { continue; }
        let Ok(metadata) = fs::symlink_metadata(&path) else { continue; };
        if !metadata.is_file() || metadata.file_type().is_symlink() || metadata.len() > 4096 { continue; }
        let Ok(bytes) = fs::read(&path) else { continue; };
        let Ok(entry) = serde_json::from_slice::<MusicAsset>(&bytes) else { continue; };
        if safe_entry(&root, &entry) {
            app.asset_protocol_scope().allow_file(&entry.path).map_err(|e| e.to_string())?;
            entries.push(entry);
        }
    }
    entries.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));
    Ok(entries)
}


fn import(app: &AppHandle, raw: &str) -> Result<MusicAsset, String> {
    let _guard = LOCK.lock().map_err(|_| "Библиотека музыки занята.")?;
    let source = Path::new(raw).canonicalize().map_err(|_| "Трек недоступен. Выберите файл ещё раз.")?;
    let metadata = source.metadata().map_err(|_| "Не прочитать трек.")?;
    if !metadata.is_file() || metadata.len() == 0 || metadata.len() > MAX_BYTES { return Err("Выберите музыкальный файл размером до 200 МБ.".into()); }
    let ext = source.extension().and_then(|v| v.to_str()).unwrap_or("").to_ascii_lowercase();
    if !["mp3", "wav", "m4a", "aac", "flac", "ogg", "opus", "aiff", "aif"].contains(&ext.as_str()) { return Err("Музыка: MP3, WAV, M4A, AAC, FLAC, OGG, Opus или AIFF.".into()); }
    let root = root(app)?;
    if list(app)?.len() >= 100 { return Err("В библиотеке уже 100 треков.".into()); }
    // Hash exactly the copied bytes so changing sources cannot poison the library.
    let staging = root.join(format!(".import-{}.{}.{}", std::process::id(), crate::stamp(), ext));
    let result = (|| {
        let mut input = fs::File::open(&source).map_err(|_| "Не прочитать музыку.")?;
        let mut output = fs::OpenOptions::new().write(true).create_new(true).open(&staging).map_err(|_| "Не сохранить музыку.")?;
        let mut hash = Sha256::new(); let mut buffer = [0u8; 131_072]; let mut count = 0u64;
        loop {
            let n = input.read(&mut buffer).map_err(|_| "Не прочитать музыку.")?;
            if n == 0 { break; } count += n as u64;
            if count > MAX_BYTES { return Err("Музыкальный файл стал больше 200 МБ.".to_string()); }
            output.write_all(&buffer[..n]).map_err(|_| "Не сохранить музыку.")?; hash.update(&buffer[..n]);
        }
        output.sync_all().map_err(|_| "Не сохранить музыку.")?;
        let duration = crate::engine::inspect_audio_duration(&resource(app, "ffmpeg")?, &staging)?;
        let id = format!("{:x}", hash.finalize());
        let target = root.join(format!("{id}.{ext}"));
        let name = source.file_stem().and_then(|v| v.to_str()).unwrap_or("Моя музыка").chars().filter(|c| !c.is_control()).take(120).collect();
        let asset = MusicAsset { id: id.clone(), name, path: target.to_string_lossy().into(), duration };
        fs::rename(&staging, &target).map_err(|_| "Не сохранить музыку.")?;
        let json = root.join(format!(".{id}.{}.json", crate::stamp()));
        fs::write(&json, serde_json::to_vec(&asset).map_err(|e| e.to_string())?).map_err(|_| "Не сохранить описание музыки.")?;
        fs::rename(&json, root.join(format!("{id}.json"))).map_err(|_| "Не сохранить описание музыки.")?;
        app.asset_protocol_scope().allow_file(&target).map_err(|e| e.to_string())?;
        Ok(asset)
    })();
    let _ = fs::remove_file(staging); result
}

#[tauri::command]
pub async fn import_music(app: AppHandle, path: String) -> Result<MusicAsset, String> {
    tauri::async_runtime::spawn_blocking(move || import(&app, &path)).await.map_err(|e| e.to_string())?
}
#[tauri::command]
pub async fn music_library(app: AppHandle) -> Result<Vec<MusicAsset>, String> {
    tauri::async_runtime::spawn_blocking(move || list(&app)).await.map_err(|e| e.to_string())?
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test] fn library_rejects_external_paths_and_bad_ids() {
        let entry = MusicAsset { id: "../x".into(), name: "x".into(), path: "/etc/passwd".into(), duration: 1.0 };
        assert!(!safe_entry(Path::new("/tmp/music"), &entry));
    }
}
