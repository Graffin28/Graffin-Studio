//! A pinned runtime shared by app updates, kept outside the application bundle.
#[cfg(windows)]
pub fn shutdown() {}
#[cfg(windows)]
pub fn node(app: &tauri::AppHandle) -> Result<std::path::PathBuf, String> {
    use std::sync::atomic::Ordering;
    use tauri::Manager;
    if app.state::<crate::AppState>().closing.load(Ordering::SeqCst) { return Err("Приложение завершает работу.".into()); }
    crate::resource(app, "media-tools/node.exe")
}
#[cfg(not(windows))]
pub use native::{node, shutdown};
#[cfg(not(windows))]
mod native {
use serde::Deserialize;
use sha2::{Digest, Sha256};
use std::{fs, io::Read, path::{Path, PathBuf}, process::Command, sync::{Mutex, atomic::Ordering}, time::Duration};
use tauri::{AppHandle, Manager};

static INSTALL: Mutex<()> = Mutex::new(());
static CONTROL: Mutex<Option<crate::engine::RenderControl>> = Mutex::new(None);
struct ActiveInstall;
impl Drop for ActiveInstall { fn drop(&mut self) { if let Ok(mut guard) = CONTROL.lock() { *guard = None; } } }
pub fn shutdown() { if let Ok(guard) = CONTROL.lock() { if let Some(control) = guard.as_ref() { control.cancel(); } } }
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Manifest { directory: String, archive: String, url: String, archive_sha256: String, binary_sha256: String }
fn manifest() -> Result<Manifest, String> {
    serde_json::from_str(include_str!("../resources/media-tools/node-runtime.json")).map_err(|e| e.to_string())
}
fn digest(path: &Path) -> Result<String, String> {
    let mut file = fs::File::open(path).map_err(|e| e.to_string())?;
    let mut hash = Sha256::new(); let mut bytes = [0u8; 131_072];
    loop { let n = file.read(&mut bytes).map_err(|e| e.to_string())?; if n == 0 { break; } hash.update(&bytes[..n]); }
    Ok(format!("{:x}", hash.finalize()))
}
pub fn node(app: &AppHandle) -> Result<PathBuf, String> {
    let _guard = INSTALL.lock().map_err(|_| "Подготовка компонента загрузки видео занята.".to_string())?;
    if app.state::<crate::AppState>().closing.load(Ordering::SeqCst) { return Err("Приложение завершает работу.".into()); }
    let control = crate::engine::RenderControl::new();
    *CONTROL.lock().map_err(|_| "Подготовка компонента загрузки видео занята.".to_string())? = Some(control.clone());
    let _active = ActiveInstall;
    let m = manifest()?;
    let root = app.path().app_data_dir().map_err(|e| e.to_string())?.join("runtime");
    let node = root.join(&m.directory).join("bin/node");
    if node.is_file() && digest(&node)? == m.binary_sha256 { return Ok(node); }
    fs::create_dir_all(&root).map_err(|e| e.to_string())?;
    #[cfg(unix)] { use std::os::unix::fs::PermissionsExt; fs::set_permissions(&root, fs::Permissions::from_mode(0o700)).map_err(|e| e.to_string())?; }
    let archive = root.join(&m.archive);
    if !archive.is_file() || digest(&archive)? != m.archive_sha256 {
        let temporary = archive.with_extension("download");
        let mut command = Command::new("/usr/bin/curl"); command.args(["--fail", "--location", "--proto", "=https", "--tlsv1.2", "--silent", "--show-error", "--max-time", "75", "--max-filesize", "100000000", "--output"]).arg(&temporary).arg(&m.url);
        if control.run_cancellable(command, Duration::from_secs(80)).is_err() || digest(&temporary).ok().as_deref() != Some(&m.archive_sha256) {
            let _ = fs::remove_file(temporary); return Err("Не удалось подготовить загрузку видео. Проверьте интернет и повторите загрузку.".into());
        }
        fs::rename(temporary, &archive).map_err(|e| e.to_string())?;
    }
    let member = format!("{}/bin/node", m.directory);
    let mut command = Command::new("/usr/bin/tar"); command.arg("-xJf").arg(&archive).arg("-C").arg(&root).arg(member);
    if control.run_cancellable(command, Duration::from_secs(30)).is_err() || digest(&node).ok().as_deref() != Some(&m.binary_sha256) { return Err("Компонент загрузки видео не прошёл проверку. Повторите загрузку.".into()); }
    Ok(node)
}

}
