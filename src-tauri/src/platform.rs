//! OS boundaries for the desktop bundle. Never resolve tool executables through PATH.
use std::{path::{Path, PathBuf}, process::{Child, Command}};

pub(crate) fn resource_name(name: &str) -> &str {
    #[cfg(windows)]
    { return match name { "ffmpeg" => "ffmpeg.exe", "media-tools/whisper-cli" => "media-tools/whisper-cli.exe", other => other }; }
    #[cfg(not(windows))]
    { name }
}

pub(crate) fn data_dir() -> Result<PathBuf, String> {
    #[cfg(windows)]
    let path = std::env::var_os("APPDATA").or_else(|| std::env::var_os("LOCALAPPDATA"))
        .map(|root| PathBuf::from(root).join("com.barsik.studio")).ok_or("Папка данных приложения недоступна.")?;
    #[cfg(not(windows))]
    let path = PathBuf::from(std::env::var_os("HOME").ok_or("Домашняя папка недоступна.")?).join("Library/Application Support/com.barsik.studio");
    if !path.is_absolute() { return Err("Папка данных приложения недоступна.".into()); }
    Ok(path)
}

#[cfg(windows)]
fn system_directory() -> Result<PathBuf, String> {
    let root = PathBuf::from(std::env::var_os("SystemRoot").ok_or("Системная папка Windows недоступна.")?);
    if !root.is_absolute() { return Err("Системная папка Windows недоступна.".into()); }
    Ok(root.join("System32"))
}

pub(crate) fn curl() -> Result<PathBuf, String> {
    #[cfg(windows)]
    { let path = system_directory()?.join("curl.exe"); if !path.is_file() { return Err("Для загрузки нужен Windows 10 версии 1803 или новее либо Windows 11.".into()); } Ok(path) }
    #[cfg(not(windows))]
    { Ok(PathBuf::from("/usr/bin/curl")) }
}

pub(crate) fn ensure_speech_cpu() -> Result<(), String> {
    #[cfg(all(windows, target_arch = "x86_64"))]
    if !std::is_x86_feature_detected!("avx2") || !std::is_x86_feature_detected!("fma") || !std::is_x86_feature_detected!("f16c") {
        return Err("Для распознавания речи нужен процессор с AVX2, FMA и F16C. Монтаж по времени и загрузка видео доступны без распознавания; отключите автосубтитры.".into());
    }
    Ok(())
}

pub(crate) fn sync_file(path: &Path) -> std::io::Result<()> {
    // FlushFileBuffers requires a writable Windows handle, even when the
    // caller only needs to flush a file produced by an external encoder.
    #[cfg(windows)]
    { std::fs::OpenOptions::new().read(true).write(true).open(path)?.sync_all() }
    #[cfg(not(windows))]
    { std::fs::File::open(path)?.sync_all() }
}

pub(crate) fn hide_console(command: &mut Command) {
    #[cfg(windows)]
    { use std::os::windows::process::CommandExt; command.creation_flags(0x0800_0000); }
    #[cfg(not(windows))]
    { let _ = command; }
}

pub(crate) fn terminate_child(child: &mut Child) {
    // yt-dlp may launch FFmpeg or Node. Terminate descendants too, otherwise
    // they can retain the output pipes after the tracked parent is cancelled.
    #[cfg(windows)]
    if let Ok(system) = system_directory() {
        let mut command = Command::new(system.join("taskkill.exe"));
        command.args(["/PID", &child.id().to_string(), "/T", "/F"])
            .stdin(std::process::Stdio::null()).stdout(std::process::Stdio::null()).stderr(std::process::Stdio::null());
        hide_console(&mut command);
        let _ = command.status();
    }
    let _ = child.kill();
}

pub(crate) fn open_folder(folder: &Path) -> Result<(), String> {
    #[cfg(windows)]
    let mut command = {
        let root = PathBuf::from(std::env::var_os("SystemRoot").ok_or("Системная папка Windows недоступна.")?);
        if !root.is_absolute() { return Err("Системная папка Windows недоступна.".into()); }
        let mut command = Command::new(root.join("explorer.exe"));
        // canonicalize() adds the Win32 extended path prefix. Explorer expects
        // a normal drive/UNC spelling; do not pass its comma-based switches.
        let raw = folder.to_string_lossy();
        let display = raw.strip_prefix(r"\\?\UNC\").map(|p| format!(r"\\{p}")).unwrap_or_else(|| raw.strip_prefix(r"\\?\").unwrap_or(&raw).to_string());
        command.arg(display);
        command
    };
    #[cfg(not(windows))]
    let mut command = { let mut command = Command::new("/usr/bin/open"); command.arg(folder); command };
    hide_console(&mut command);
    // Explorer may hand the request to an existing process and return a
    // nonzero status. A successful spawn is the correct success boundary.
    #[cfg(windows)]
    { command.spawn().map_err(|e| format!("Не открыть папку: {e}"))?; Ok(()) }
    #[cfg(not(windows))]
    { let status = command.status().map_err(|e| e.to_string())?; if status.success() { Ok(()) } else { Err("Не удалось открыть папку.".into()) } }
}

#[cfg(test)]
mod tests {
    #[test]
    fn completed_files_can_be_flushed_with_a_writable_windows_handle() {
        let path = std::env::temp_dir().join(format!("graffin-flush-{}-{}.tmp", std::process::id(), crate::stamp()));
        std::fs::write(&path, b"complete").unwrap();
        super::sync_file(&path).unwrap();
        assert_eq!(std::fs::read(&path).unwrap(), b"complete");
        std::fs::remove_file(path).unwrap();
    }

    #[test]
    fn executable_resources_match_the_target_platform() {
        #[cfg(windows)]
        { assert_eq!(super::resource_name("ffmpeg"), "ffmpeg.exe"); assert_eq!(super::resource_name("media-tools/whisper-cli"), "media-tools/whisper-cli.exe"); }
        #[cfg(not(windows))]
        { assert_eq!(super::resource_name("ffmpeg"), "ffmpeg"); }
        assert_eq!(super::resource_name("banner.png"), "banner.png");
    }
}
