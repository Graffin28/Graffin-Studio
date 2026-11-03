use crate::{engine::{BannerAsset, RenderControl}, resource};
use sha2::{Digest, Sha256};
use std::{fs, io::Read, path::Path, process::Command, time::Duration};
use tauri::{AppHandle, Manager};

pub fn authorize_saved(app: &AppHandle, settings: &crate::engine::Settings) {
    let Ok(root) = app.path().app_data_dir().map(|p| p.join("banner-assets")).and_then(|p| p.canonicalize().map_err(Into::into)) else { return; };
    for asset in &settings.banner_library {
        for raw in [&asset.path, &asset.preview] {
            if let Ok(path) = Path::new(raw).canonicalize() {
                if path.starts_with(&root) && path.is_file() { let _ = app.asset_protocol_scope().allow_file(path); }
            }
        }
    }
}

fn import(app: &AppHandle, raw: &str) -> Result<BannerAsset, String> {
    let source = Path::new(raw).canonicalize().map_err(|_| "Баннер недоступен. Выберите файл ещё раз.".to_string())?;
    let metadata = source.metadata().map_err(|e| e.to_string())?;
    if !metadata.is_file() || metadata.len() == 0 || metadata.len() > 536_870_912 { return Err("Выберите баннер размером до 512 МБ.".into()); }
    let ext = source.extension().and_then(|e| e.to_str()).unwrap_or("").to_ascii_lowercase();
    let kind = match ext.as_str() {
        "mp4" | "mov" | "webm" | "mkv" | "m4v" => "video",
        "png" | "jpg" | "jpeg" | "webp" | "gif" => "image",
        _ => return Err("Баннер: MP4, MOV, WebM, GIF, PNG, JPG или WebP.".into()),
    };
    let mut input = fs::File::open(&source).map_err(|e| e.to_string())?;
    let mut hash = Sha256::new(); let mut buffer = [0u8; 131_072];
    loop { let count = input.read(&mut buffer).map_err(|e| e.to_string())?; if count == 0 { break; } hash.update(&buffer[..count]); }
    let id = format!("{:x}", hash.finalize());
    let root = app.path().app_data_dir().map_err(|e| e.to_string())?.join("banner-assets");
    fs::create_dir_all(&root).map_err(|e| e.to_string())?;
    let target = root.join(format!("{id}.{ext}"));
    if !target.exists() {
        let staging = root.join(format!(".{id}.{}.tmp", crate::stamp()));
        fs::copy(&source, &staging).map_err(|e| e.to_string())?;
        fs::rename(&staging, &target).map_err(|e| e.to_string())?;
    }
    let preview = root.join(format!("{id}-preview.png"));
    if !preview.is_file() {
        let staging = root.join(format!(".{id}-preview-{}.png", crate::stamp()));
        let mut command = Command::new(resource(app, "ffmpeg")?);
        command.args(["-hide_banner", "-loglevel", "error", "-nostdin", "-y", "-protocol_whitelist", "file,pipe", "-i"]).arg(&target)
            .args(["-frames:v", "1", "-vf", "scale=960:-2:force_original_aspect_ratio=decrease", "-update", "1"]).arg(&staging);
        if let Err(error) = RenderControl::new().run_cancellable(command, Duration::from_secs(90)) {
            let _ = fs::remove_file(&staging); return Err(format!("Не удалось прочитать баннер: {error}"));
        }
        if !staging.is_file() { return Err("В баннере не найдена картинка или видеодорожка.".into()); }
        fs::rename(staging, &preview).map_err(|e| e.to_string())?;
    }
    app.asset_protocol_scope().allow_file(&target).map_err(|e| e.to_string())?;
    app.asset_protocol_scope().allow_file(&preview).map_err(|e| e.to_string())?;
    let name = source.file_stem().and_then(|s| s.to_str()).unwrap_or("Мой баннер").chars().take(80).collect();
    Ok(BannerAsset { id, name, path: target.to_string_lossy().into(), preview: preview.to_string_lossy().into(), kind: kind.into() })
}

#[tauri::command]
pub async fn import_banner(app: AppHandle, path: String) -> Result<BannerAsset, String> {
    tauri::async_runtime::spawn_blocking(move || import(&app, &path)).await.map_err(|e| e.to_string())?
}
