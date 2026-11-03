//! Public URL import and offline Whisper subtitles. No browser sessions or shell.
use crate::engine::{self, RenderControl};
use serde::Serialize;
use sha2::{Digest, Sha256};
use std::{fs::{self, File}, io::Read, net::{IpAddr, ToSocketAddrs}, path::{Path, PathBuf}, process::Command, sync::{atomic::{AtomicBool, Ordering}, Mutex}, time::{Duration, UNIX_EPOCH}};
use tauri::{AppHandle, Url};

const MODEL_NAME: &str = "ggml-small-q5_1.bin";
const MODEL_URL: &str = "https://huggingface.co/ggerganov/whisper.cpp/resolve/5359861c739e955e79d9a303bcbc70fb988958b1/ggml-small-q5_1.bin";
const MODEL_HASH: &str = "ae85e4a935d7a567bd102fe55afc16bb595bdb618e11b2fc7591bc08120411bb";
const MODEL_BYTES: u64 = 190_085_487;
const SPEECH_CONTEXT: f64 = 1.5;
#[cfg(windows)]
const WHISPER_VERSION: &str = "v1.8.2";
#[cfg(not(windows))]
const WHISPER_VERSION: &str = "v1.9.5";
#[cfg(not(windows))]
const DOWNLOADER_URL: &str = "https://github.com/yt-dlp/yt-dlp/releases/download/2026.08.19/yt-dlp_macos";
#[cfg(not(windows))]
const DOWNLOADER_HASH: &str = "0f192b7ec147ab6288885d6351d9ab67367640029b4377576ef46dd79cf7b202";
#[cfg(not(windows))]
const DOWNLOADER_BYTES: u64 = 37_146_048;
static INSTALL: Mutex<()> = Mutex::new(());
static IMPORT_BUSY: AtomicBool = AtomicBool::new(false);
static IMPORT_CONTROL: Mutex<Option<RenderControl>> = Mutex::new(None);
static PREPARE_CONTROL: Mutex<Option<RenderControl>> = Mutex::new(None);

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MediaToolsStatus {
    pub downloader_ready: bool,
    pub subtitles_ready: bool,
    pub model_ready: bool,
    pub message: String,
}

fn data_dir() -> Result<PathBuf, String> { crate::platform::data_dir() }
#[cfg(windows)]
static BUNDLED_WHISPER: std::sync::OnceLock<PathBuf> = std::sync::OnceLock::new();
pub(crate) fn configure(app: &AppHandle) -> Result<(), String> {
    #[cfg(windows)]
    { let path = crate::resource(app, "media-tools/whisper-cli")?; let _ = BUNDLED_WHISPER.set(path); }
    #[cfg(not(windows))]
    { let _ = app; }
    Ok(())
}
fn whisper_path(app: &AppHandle) -> Result<PathBuf, String> {
    #[cfg(windows)]
    { crate::resource(app, "media-tools/whisper-cli") }
    #[cfg(not(windows))]
    { let _ = app; Ok(tools_dir()?.join("whisper-cli")) }
}
fn downloader_path(app: &AppHandle) -> Result<PathBuf, String> {
    #[cfg(windows)]
    { crate::resource(app, "media-tools/yt-dlp.exe") }
    #[cfg(not(windows))]
    { let _ = app; Ok(tools_dir()?.join("yt-dlp_macos")) }
}

fn tools_dir() -> Result<PathBuf, String> { Ok(data_dir()?.join("media-tools")) }
pub fn subtitle_cache_dir() -> Result<PathBuf, String> {
    let path = data_dir()?.join("cache/subtitles");
    fs::create_dir_all(&path).map_err(|_| "Не удалось открыть кэш субтитров.")?;
    Ok(path)
}
fn executable(path: &Path) -> bool {
    #[cfg(unix)] { use std::os::unix::fs::PermissionsExt; path.metadata().is_ok_and(|m| m.is_file() && m.permissions().mode() & 0o111 != 0) }
    #[cfg(not(unix))] { path.is_file() }
}
fn hash_file(path: &Path) -> Result<String, String> {
    let mut file = File::open(path).map_err(|_| "Файл инструмента недоступен.")?;
    let mut hash = Sha256::new(); let mut buffer = [0_u8; 131_072];
    loop { let n = file.read(&mut buffer).map_err(|_| "Не удалось проверить файл инструмента.")?; if n == 0 { break; } hash.update(&buffer[..n]); }
    Ok(format!("{:x}", hash.finalize()))
}
#[cfg(not(windows))]
fn set_executable(path: &Path) -> Result<(), String> {
    #[cfg(unix)] { use std::os::unix::fs::PermissionsExt; fs::set_permissions(path, fs::Permissions::from_mode(0o755)).map_err(|_| "Не удалось подготовить инструмент.")?; }
    Ok(())
}
fn status(app: &AppHandle) -> MediaToolsStatus {
    let path = tools_dir().unwrap_or_default();
    let downloader_ready = downloader_path(app).is_ok_and(|path| executable(&path));
    let speech_cpu = crate::platform::ensure_speech_cpu();
    let subtitles_ready = speech_cpu.is_ok() && whisper_path(app).is_ok_and(|path| executable(&path));
    let model_ready = path.join(MODEL_NAME).metadata().is_ok_and(|m| m.is_file() && m.len() == MODEL_BYTES);
    let message = match speech_cpu { Err(error) => error, Ok(()) => if downloader_ready && subtitles_ready && model_ready { "Загрузка и субтитры готовы.".into() } else { "Подготовьте загрузчик и модель речи один раз.".into() } };
    MediaToolsStatus { downloader_ready, subtitles_ready, model_ready, message }
}
#[tauri::command]
pub fn media_tools_status(app: AppHandle) -> MediaToolsStatus { status(&app) }

fn install_asset(url: &str, path: &Path, expected: &str, size: u64, control: &RenderControl) -> Result<(), String> {
    if path.metadata().is_ok_and(|m| m.len() == size) && hash_file(path).ok().as_deref() == Some(expected) { return Ok(()); }
    let temp = path.with_extension(format!("{:016x}.partial", rand::random::<u64>()));
    let mut curl = Command::new(crate::platform::curl()?);
    curl.args(["--disable", "--fail", "--location", "--silent", "--show-error", "--proto", "=https", "--proto-redir", "=https", "--connect-timeout", "20", "--max-time", "900", "--retry", "2", "--max-filesize", &size.to_string(), "--output"]).arg(&temp).arg(url);
    let result = (|| {
        control.run_cancellable(curl, Duration::from_secs(950)).map_err(|_| "Не удалось скачать инструмент. Проверьте интернет и повторите.")?;
        if temp.metadata().map_err(|_| "Загрузка не завершена.")?.len() != size || hash_file(&temp)? != expected { return Err("Проверка скачанного файла не прошла. Повторите подготовку.".into()); }
        crate::platform::sync_file(&temp).map_err(|_| "Не удалось сохранить инструмент.")?;
        fs::rename(&temp, path).map_err(|_| "Не удалось установить инструмент.")?;
        Ok(())
    })();
    if result.is_err() { let _ = fs::remove_file(temp); }
    result
}
#[cfg(not(windows))]
fn copy_whisper(app: &AppHandle, folder: &Path) -> Result<(), String> {
    let source = crate::resource(app, "media-tools/whisper-cli")?;
    let destination = folder.join("whisper-cli");
    // Source is protected by the app's resource signature; the model and downloader have pinned hashes.
    if hash_file(&source)? != hash_file(&destination).unwrap_or_default() {
        let temporary = folder.join("whisper-cli.partial");
        fs::copy(&source, &temporary).map_err(|_| "Не удалось установить распознавание речи.")?;
        set_executable(&temporary)?;
        fs::rename(temporary, destination).map_err(|_| "Не удалось сохранить распознавание речи.")?;
    }
    Ok(())
}
fn prepare(app: &AppHandle, model: bool, control: &RenderControl) -> Result<MediaToolsStatus, String> {
    if model { crate::platform::ensure_speech_cpu()?; }
    let _guard = INSTALL.lock().map_err(|_| "Подготовка инструментов занята.")?;
    let folder = tools_dir()?; fs::create_dir_all(&folder).map_err(|_| "Не удалось открыть папку инструментов.")?;
    #[cfg(not(windows))]
    {
        copy_whisper(app, &folder)?;
        install_asset(DOWNLOADER_URL, &folder.join("yt-dlp_macos"), DOWNLOADER_HASH, DOWNLOADER_BYTES, control)?;
        set_executable(&folder.join("yt-dlp_macos"))?;
    }
    #[cfg(windows)]
    { configure(app)?; let _ = whisper_path(app)?; let _ = downloader_path(app)?; }
    if model { install_asset(MODEL_URL, &folder.join(MODEL_NAME), MODEL_HASH, MODEL_BYTES, control)?; }
    Ok(status(app))
}
#[tauri::command]
pub async fn prepare_media_tools(app: AppHandle) -> Result<MediaToolsStatus, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let control = RenderControl::new();
        let mut guard = PREPARE_CONTROL.lock().map_err(|_| "Подготовка инструментов занята.")?;
        if guard.is_some() { return Err("Подготовка инструментов уже выполняется.".into()); }
        *guard = Some(control.clone()); drop(guard);
        let result = prepare(&app, true, &control);
        if let Ok(mut guard) = PREPARE_CONTROL.lock() { *guard = None; }
        result
    }).await.map_err(|_| "Подготовка инструментов прервана.")?
}

fn public_ip(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(v) => { let [a,b,c,_] = v.octets();
            !(v.is_private() || v.is_loopback() || v.is_link_local() || v.is_multicast() || v.is_unspecified() || v.is_broadcast() ||
                a == 0 || a >= 224 || a == 100 && (64..=127).contains(&b) || a == 192 && b == 0 ||
                a == 192 && b == 0 && c == 2 || a == 198 && (b == 18 || b == 19 || b == 51 && c == 100) || a == 203 && b == 0 && c == 113)
        },
        IpAddr::V6(v) => if let Some(v4) = v.to_ipv4_mapped() { public_ip(IpAddr::V4(v4)) } else {
            let s = v.segments(); (s[0] & 0xe000) == 0x2000 && !(s[0] == 0x2001 && s[1] == 0x0db8)
        },
    }
}
fn validated_url(raw: &str) -> Result<Url, String> {
    if raw.len() > 4096 || raw.chars().any(char::is_control) { return Err("Некорректная ссылка на видео.".into()); }
    let url = Url::parse(raw).map_err(|_| "Вставьте полную HTTPS-ссылку на общедоступное видео.")?;
    if url.scheme() != "https" || !url.username().is_empty() || url.password().is_some() || url.port().is_some_and(|p| p != 443) { return Err("Нужна HTTPS-ссылка без логина, пароля и нестандартного порта.".into()); }
    let host = url.host_str().ok_or("В ссылке не указан сайт.")?.to_ascii_lowercase();
    if host == "localhost" || !host.contains('.') && host.parse::<IpAddr>().is_err() || [".localhost", ".local", ".internal", ".lan", ".test", ".invalid", ".example", ".onion", ".home", ".home.arpa"].iter().any(|s| host.ends_with(s)) { return Err("Локальные и внутренние адреса недоступны для импорта.".into()); }
    if let Ok(ip) = host.trim_matches(['[', ']']).parse::<IpAddr>() { if !public_ip(ip) { return Err("Локальные и внутренние адреса недоступны для импорта.".into()); } }
    Ok(url)
}
fn resolved_public(url: &Url) -> Result<IpAddr, String> {
    let host = url.host_str().ok_or("Не указан сайт.")?;
    let ips: Vec<_> = (host.trim_matches(['[', ']']), 443).to_socket_addrs().map_err(|_| "Не удалось найти сайт. Проверьте ссылку и интернет.")?.map(|s| s.ip()).collect();
    if ips.is_empty() || ips.iter().any(|ip| !public_ip(*ip)) { return Err("Локальные и внутренние адреса недоступны для импорта.".into()); }
    Ok(*ips.iter().find(|ip| ip.is_ipv4()).unwrap_or(&ips[0]))
}
fn website_supported(url: &Url) -> bool {
    let h = url.host_str().unwrap_or("");
    ["youtube.com", "youtu.be", "tiktok.com", "vk.com", "vkvideo.ru", "rutube.ru", "twitch.tv", "vimeo.com", "dailymotion.com"].iter().any(|site| h == *site || h.ends_with(&format!(".{site}")))
}
fn direct_video(url: &Url) -> bool {
    Path::new(url.path()).extension().and_then(|s| s.to_str()).is_some_and(|s| ["mp4", "mov", "mkv", "webm", "m4v", "avi"].contains(&s.to_ascii_lowercase().as_str()))
}
fn parse_headers(text: &str) -> Result<(u16, Option<String>), String> {
    let mut status = 0; let mut location = None;
    for line in text.lines() {
        if line.starts_with("HTTP/") { status = line.split_whitespace().nth(1).and_then(|v| v.parse().ok()).unwrap_or(0); location = None; }
        else if let Some((key, value)) = line.split_once(':') { if key.eq_ignore_ascii_case("location") { location = Some(value.trim().to_owned()); } }
    }
    if status == 0 { Err("Сайт не вернул корректный ответ.".into()) } else { Ok((status, location)) }
}
fn download_direct(mut url: Url, folder: &Path, control: &RenderControl) -> Result<PathBuf, String> {
    let temporary = folder.join("video.partial"); let headers = folder.join("response.headers");
    for _ in 0..6 {
        let ip = resolved_public(&url)?;
        let host = url.host_str().ok_or("Не указан сайт.")?;
        let address = match ip { IpAddr::V4(v) => v.to_string(), IpAddr::V6(v) => format!("[{v}]") };
        let resolve = format!("{host}:443:{address}");
        let mut curl = Command::new(crate::platform::curl()?);
        curl.args(["--disable", "--silent", "--show-error", "--proto", "=https", "--noproxy", "*", "--resolve", &resolve, "--max-redirs", "0", "--connect-timeout", "20", "--max-time", "600", "--max-filesize", "2147483648", "--dump-header"]).arg(&headers).arg("--output").arg(&temporary).arg(url.as_str());
        control.run_cancellable(curl, Duration::from_secs(620)).map_err(|_| "Не удалось скачать видео. Проверьте общедоступную ссылку.")?;
        let metadata = fs::metadata(&headers).map_err(|_| "Нет ответа сайта.")?;
        if metadata.len() > 65_536 { return Err("Сайт вернул слишком большой ответ.".into()); }
        let (code, location) = parse_headers(&fs::read_to_string(&headers).map_err(|_| "Нет ответа сайта.")?)?;
        if (300..400).contains(&code) {
            let next = url.join(&location.ok_or("Сайт не указал адрес видео.")?).map_err(|_| "Некорректное перенаправление.")?;
            url = validated_url(next.as_str())?; continue;
        }
        if !(200..300).contains(&code) { return Err("Видео недоступно. Нужна общедоступная ссылка без входа в аккаунт.".into()); }
        let extension = Path::new(url.path()).extension().and_then(|s| s.to_str()).filter(|s| ["mp4", "mov", "mkv", "webm", "m4v", "avi"].contains(&s.to_ascii_lowercase().as_str())).unwrap_or("mp4");
        let path = folder.join(format!("Импорт.{extension}")); fs::rename(&temporary, &path).map_err(|_| "Не удалось сохранить видео.")?;
        let _ = fs::remove_file(headers); return Ok(path);
    }
    Err("Сайт перенаправляет слишком много раз.".into())
}
fn import_root(raw:&str)->Result<PathBuf,String>{
    let path=Path::new(raw);
    if raw.trim().is_empty()||raw.len()>4096||!path.is_absolute(){return Err("Выберите папку для видео по ссылкам.".into());}
    fs::create_dir_all(path).map_err(|_|"Не удалось создать выбранную папку загрузки. Проверьте доступ и свободное место.")?;
    let path=path.canonicalize().map_err(|_|"Папка загрузки недоступна.")?;
    if !path.is_dir(){return Err("Выберите папку для видео по ссылкам.".into());}
    Ok(path)
}
struct ImportCleanup { folder: PathBuf, keep: bool }
impl Drop for ImportCleanup { fn drop(&mut self) { if !self.keep && !self.folder.as_os_str().is_empty() { let _ = fs::remove_dir_all(&self.folder); } if let Ok(mut guard) = IMPORT_CONTROL.lock() { *guard = None; } IMPORT_BUSY.store(false, Ordering::SeqCst); } }
pub fn shutdown() { for slot in [&IMPORT_CONTROL, &PREPARE_CONTROL] { if let Ok(guard) = slot.lock() { if let Some(control) = guard.as_ref() { control.cancel(); } } } }
fn import_url(app: &AppHandle, raw: &str, import_dir: &str) -> Result<engine::Media, String> {
    let url = validated_url(raw)?;
    resolved_public(&url)?;
    if !website_supported(&url) && !direct_video(&url) { return Err("Поддерживаются публичные видео TikTok, YouTube, VK, RuTube и прямые ссылки MP4/MOV/WebM.".into()); }
    let root=import_root(import_dir)?;
    if IMPORT_BUSY.swap(true, Ordering::SeqCst) { return Err("Дождитесь текущей загрузки видео.".into()); }
    let folder = root.join(format!("Импорт-{:016x}", rand::random::<u64>()));
    let mut cleanup = ImportCleanup { folder: PathBuf::new(), keep: false };
    fs::create_dir(&folder).map_err(|_| "Не удалось создать папку загрузки видео.")?;
    cleanup.folder=folder.clone();
    let control = RenderControl::new(); *IMPORT_CONTROL.lock().map_err(|_| "Загрузка занята.")? = Some(control.clone());
    let ff = crate::resource(app, "ffmpeg")?;
    let path = if direct_video(&url) { download_direct(url, &folder, &control)? } else {
        prepare(app, false, &control)?;
        let node = crate::download_runtime::node(app)?;
        let mut command = Command::new(downloader_path(app)?);
        command.args(["--ignore-config", "--no-plugin-dirs", "--no-remote-components", "--no-js-runtimes", "--js-runtimes"]).arg(format!("node:{}", node.display()))
            .args(["--no-cookies", "--no-playlist", "--playlist-items", "1", "--max-filesize", "2G", "--socket-timeout", "20", "--retries", "2", "--fragment-retries", "2", "--restrict-filenames", "--no-write-info-json", "--no-write-thumbnail", "--no-write-subs", "--no-write-auto-subs", "--no-progress", "--newline", "--ffmpeg-location"]).arg(&ff)
            .args(["-f", "bv*[height<=1920][vcodec^=avc1]+ba[ext=m4a]/b[ext=mp4]/b", "--merge-output-format", "mp4", "-o"]).arg(folder.join("%(title).80B.%(ext)s")).arg("--").arg(url.as_str());
        control.run_cancellable(command, Duration::from_secs(1200)).map_err(|_| "Сайт не отдал видео. Проверьте, что оно общедоступно; закрытые ролики и вход в аккаунт не поддерживаются.")?;
        let files = fs::read_dir(&folder).map_err(|_| "Загруженное видео недоступно.")?.filter_map(Result::ok).map(|e| e.path()).filter(|p| p.is_file() && p.extension().and_then(|s| s.to_str()).is_some_and(|s| ["mp4", "mov", "mkv", "webm", "m4v", "avi"].contains(&s.to_ascii_lowercase().as_str()))).collect::<Vec<_>>();
        if files.len() != 1 { return Err("Загрузка не создала один готовый видеофайл.".into()); } files[0].clone()
    };
    if control.is_cancelled() { return Err("Загрузка отменена.".into()); }
    let media = engine::probe(&ff, &path, &folder)?;
    crate::authorize(app, &path)?; if !media.thumbnail.is_empty() { crate::authorize(app, Path::new(&media.thumbnail))?; }
    cleanup.keep = true; Ok(media)
}
#[tauri::command]
pub async fn import_media_url(app: AppHandle, url: String, import_dir: String) -> Result<engine::Media, String> {
    tauri::async_runtime::spawn_blocking(move || import_url(&app, &url, &import_dir)).await.map_err(|_| "Загрузка видео прервана.")?
}

struct TemporaryAudio { wav: PathBuf, prefix: PathBuf }
impl Drop for TemporaryAudio { fn drop(&mut self) { let _ = fs::remove_file(&self.wav); for extension in ["srt", "txt", "json"] { let _ = fs::remove_file(self.prefix.with_extension(extension)); } } }

fn speech_window(start: f64, duration: f64) -> (f64, f64, f64) {
    let from = (start - SPEECH_CONTEXT).max(0.0);
    let offset = start - from;
    (from, duration + offset + SPEECH_CONTEXT, offset)
}

// Recognition sees surrounding speech, while the exported words remain on the
// exact clip timeline. Context-only words never escape into the visible clip.
fn clip_word_srt(srt: &str, offset: f64, duration: f64) -> String {
    let timing = regex::Regex::new(r"^(\d{1,3}):(\d{2}):(\d{2})[,.](\d{3})\s*-->\s*(\d{1,3}):(\d{2}):(\d{2})[,.](\d{3})\s*$").unwrap();
    let normalized = srt.trim_start_matches('\u{feff}').replace("\r\n", "\n").replace('\r', "\n");
    let mut result = String::new(); let mut sequence = 0;
    let milliseconds = |value: f64| -> u64 { (value.max(0.0) * 1000.0).round() as u64 };
    let timestamp = |value: u64| format!("{:02}:{:02}:{:02},{:03}",value/3_600_000,value/60_000%60,value/1000%60,value%1000);
    for block in normalized.split("\n\n") {
        let mut lines = block.lines(); let mut span = None;
        for line in lines.by_ref() {
            if let Some(c) = timing.captures(line.trim()) {
                let seconds = |base: usize| {
                    let n = |index| c.get(index).unwrap().as_str().parse::<u64>().unwrap_or(0);
                    (n(base)*3_600_000+n(base+1)*60_000+n(base+2)*1000+n(base+3)) as f64 / 1000.0
                };
                span = Some((seconds(1), seconds(5))); break;
            }
        }
        let Some((from, to)) = span else { continue };
        let text = lines.collect::<Vec<_>>().join(" ").split_whitespace().collect::<Vec<_>>().join(" ");
        if text.is_empty() || to <= offset || from >= offset + duration { continue; }
        let from = milliseconds((from-offset).clamp(0.0,duration));
        let to = milliseconds((to-offset).clamp(0.0,duration));
        if to <= from { continue; }
        sequence += 1;
        result.push_str(&format!("{sequence}\n{} --> {}\n{text}\n\n",timestamp(from),timestamp(to)));
    }
    result
}

pub fn transcribe_clip(ff: &Path, input: &Path, start: f64, duration: f64, language: &str, cache: &Path, control: &RenderControl) -> Result<PathBuf, String> {
    if !start.is_finite() || start < 0.0 || !duration.is_finite() || duration <= 0.0 || duration > 600.0 || !["ru", "en", "auto"].contains(&language) { return Err("Некорректный фрагмент для субтитров.".into()); }
    if control.is_cancelled() { return Err("Операция отменена.".into()); }
    let tools = tools_dir()?;
    #[cfg(windows)]
    let whisper = BUNDLED_WHISPER.get().cloned().ok_or("Встроенное распознавание речи недоступно. Перенесите папку приложения целиком.")?;
    #[cfg(not(windows))]
    let whisper = tools.join("whisper-cli");
    let model = tools.join(MODEL_NAME);
    if !executable(&whisper) || !model.is_file() { return Err("Сначала нажмите «Подготовить инструменты» для бесплатных автосубтитров.".into()); }
    let source = fs::canonicalize(input).map_err(|_| "Исходник субтитров недоступен.")?;
    let metadata = source.metadata().map_err(|_| "Исходник субтитров недоступен.")?;
    let modified = metadata.modified().ok().and_then(|v| v.duration_since(UNIX_EPOCH).ok()).map(|v| v.as_nanos()).unwrap_or_default();
    let key = format!("whisper-small-q5_1-{WHISPER_VERSION}-words-context-v3|{}|{}|{modified}|{start:.6}|{duration:.6}|{language}|{MODEL_HASH}", source.display(), metadata.len());
    let digest = format!("{:x}", Sha256::digest(key.as_bytes()));
    fs::create_dir_all(cache).map_err(|_| "Кэш субтитров недоступен.")?;
    let destination = cache.join(format!("{digest}.srt"));
    if destination.is_file() { return Ok(destination); }
    crate::platform::ensure_speech_cpu()?;
    let prefix = cache.join(format!(".{digest}-{:016x}", rand::random::<u64>()));
    let temporary = TemporaryAudio { wav: prefix.with_extension("wav"), prefix: prefix.clone() };
    let (context_start, context_duration, offset) = speech_window(start, duration);
    let mut extract = Command::new(ff);
    extract.args(["-hide_banner", "-loglevel", "error", "-nostdin", "-y", "-protocol_whitelist", "file,pipe", "-ss", &format!("{context_start:.6}"), "-i"]).arg(&source).args(["-t", &format!("{context_duration:.6}"), "-map", "0:a:0", "-vn", "-ar", "16000", "-ac", "1", "-c:a", "pcm_s16le"]).arg(&temporary.wav);
    control.run_cancellable(extract, Duration::from_secs(120)).map_err(|error| if control.is_cancelled() { "Операция отменена.".into() } else { format!("Не удалось извлечь речь для субтитров: {error}") })?;
    let mut command = Command::new(whisper);
    command.arg("-m").arg(model).arg("-f").arg(&temporary.wav).args(["-l", language, "-osrt", "-of"]).arg(&prefix).args(["-t", "4", "-bs", "5", "-bo", "5", "-tp", "0", "-ml", "1", "-sow", "-np"]);
    control.run_cancellable(command, Duration::from_secs((context_duration * 20.0 + 90.0).min(1800.0) as u64)).map_err(|_| if control.is_cancelled() { "Операция отменена." } else { "Не удалось распознать речь. Повторите или отключите автосубтитры." })?;
    let srt = prefix.with_extension("srt");
    if !srt.is_file() { return Err("Распознавание не создало субтитры.".into()); }
    if srt.metadata().map_err(|_| "Субтитры недоступны.")?.len() > 2_000_000 { return Err("Получены слишком большие субтитры.".into()); }
    if control.is_cancelled() { return Err("Операция отменена.".into()); }
    let text = fs::read_to_string(&srt).map_err(|_| "Не прочитать распознанные слова.")?;
    fs::write(&srt, clip_word_srt(&text, offset, duration)).map_err(|_| "Не подготовить слова для фрагмента.")?;
    crate::platform::sync_file(&srt).map_err(|_| "Не удалось сохранить субтитры.")?;
    if control.is_cancelled() { return Err("Операция отменена.".into()); }
    fs::rename(&srt, &destination).map_err(|_| "Не удалось сохранить кэш субтитров.")?;
    Ok(destination)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test] fn download_folder_is_explicit_and_failure_cleanup_owns_only_child(){
        assert!(import_root("").is_err());assert!(import_root("relative").is_err());
        let root=std::env::temp_dir().join(format!("graffin-import-root-{}-{:016x}",std::process::id(),rand::random::<u64>()));
        let actual=import_root(root.to_str().unwrap()).unwrap();let protected=actual.join("user.mp4");fs::write(&protected,b"keep").unwrap();
        let child=actual.join("owned");fs::create_dir(&child).unwrap();fs::write(child.join("partial"),b"partial").unwrap();
        {let _cleanup=ImportCleanup{folder:child.clone(),keep:false};}
        assert!(!child.exists());assert_eq!(fs::read(protected).unwrap(),b"keep");fs::remove_dir_all(root).unwrap();
    }
    #[test] fn speech_context_is_clamped_at_source_start() {
        assert_eq!(speech_window(10.0,5.0),(8.5,8.0,1.5));
        assert_eq!(speech_window(0.0,5.0),(0.0,6.5,0.0));
        assert_eq!(speech_window(0.3,5.0),(0.0,6.8,0.3));
    }
    #[test] fn context_words_are_rebased_clipped_and_reindexed_without_filling_pauses() {
        let srt="\u{feff}1\r\n00:00:00,000 --> 00:00:01,000\r\nПеред\r\n\r\n2\r\n00:00:01,000 --> 00:00:01,700\r\nПервое\r\n\r\n3\r\n00:00:02,000 --> 00:00:02,400\r\nСлово\r\n\r\n4\r\n00:00:06,300 --> 00:00:07,000\r\nПоследнее\r\n\r\n5\r\n00:00:07,000 --> 00:00:07,200\r\nПосле\r\n";
        assert_eq!(clip_word_srt(srt,1.5,5.0),"1\n00:00:00,000 --> 00:00:00,200\nПервое\n\n2\n00:00:00,500 --> 00:00:00,900\nСлово\n\n3\n00:00:04,800 --> 00:00:05,000\nПоследнее\n\n");
        let near_zero="1\n00:00:00,200 --> 00:00:00,600\nНачало\n\n2\n00:00:00,900 --> 00:00:01,100\nДальше\n";
        assert_eq!(clip_word_srt(near_zero,0.3,1.0),"1\n00:00:00,000 --> 00:00:00,300\nНачало\n\n2\n00:00:00,600 --> 00:00:00,800\nДальше\n\n");
    }
    #[test] fn empty_or_invalid_recognition_never_invents_caption_text() {
        for srt in ["", "invalid", "1\n00:00:00,200 --> 00:00:00,200\nслово\n", "1\n00:00:00,000 --> 00:00:00,100\n \n", "1\n00:00:03,000 --> 00:00:04,000\nвне\n"] {
            assert_eq!(clip_word_srt(srt,0.0,1.0),"");
        }
    }
    #[test] fn urls_reject_local_unsafe_and_authenticated_addresses() {
        for url in ["file:///etc/passwd", "http://example.com/a.mp4", "https://user:pass@example.com/a.mp4", "https://example.com:444/a.mp4", "https://127.0.0.1/a.mp4", "https://2130706433/a.mp4", "https://192.168.1.2/a.mp4", "https://[::1]/a.mp4", "https://foo.local/a.mp4", "https://localhost/a.mp4"] { assert!(validated_url(url).is_err(), "{url}"); }
        assert!(validated_url("https://www.youtube.com/watch?v=public").is_ok());
        assert!(validated_url("https://media.example.org/clip.mp4?token=public").is_ok());
    }
    #[test] fn only_public_ips_are_allowed() {
        for ip in ["127.0.0.1", "0.0.0.0", "10.0.0.1", "172.20.0.1", "169.254.169.254", "100.64.0.1", "192.168.1.1", "198.18.0.1", "::1", "fe80::1", "fc00::1", "::ffff:127.0.0.1", "2001:db8::1"] { assert!(!public_ip(ip.parse().unwrap()), "{ip}"); }
        for ip in ["1.1.1.1", "8.8.8.8", "2606:4700:4700::1111"] { assert!(public_ip(ip.parse().unwrap())); }
    }
    #[test] fn redirect_parser_resets_proxy_and_interim_headers() {
        let result = parse_headers("HTTP/1.1 200 Connection established\r\n\r\nHTTP/2 302\r\nLocation: /video.mp4\r\n\r\n").unwrap();
        assert_eq!(result, (302, Some("/video.mp4".into())));
        assert!(parse_headers("invalid").is_err());
    }
    #[test] fn platform_and_direct_detection_do_not_accept_lookalikes() {
        assert!(website_supported(&Url::parse("https://www.tiktok.com/@x/video/1").unwrap()));
        assert!(!website_supported(&Url::parse("https://tiktok.com.evil.test/").unwrap()));
        assert!(direct_video(&Url::parse("https://example.org/a.MP4?x=1").unwrap()));
        assert!(!direct_video(&Url::parse("https://example.org/a.mp4.exe").unwrap()));
    }
    #[test] #[ignore = "Explicit public-network smoke; no authentication"]
    fn actual_public_https_video_download() {
        let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).parent().unwrap().join("qa/montage-selected-download-folder");
        let root=import_root(root.to_str().unwrap()).unwrap();
        let protected=root.join("existing-user-file.txt");fs::write(&protected,b"keep").unwrap();
        let folder=root.join(format!("Импорт-{:016x}",rand::random::<u64>()));fs::create_dir(&folder).unwrap();
        let mut cleanup=ImportCleanup{folder:folder.clone(),keep:false};
        let control = RenderControl::new();
        let file = download_direct(validated_url("https://media.w3.org/2010/05/sintel/trailer.mp4").unwrap(), &folder, &control).unwrap();
        let ff = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("resources/ffmpeg");
        let media = engine::probe(&ff, &file, &folder).unwrap();
        assert!(media.duration > 1.0 && media.width > 0 && media.height > 0);
        assert!(file.starts_with(&root));assert_eq!(fs::read(protected).unwrap(),b"keep");
        cleanup.keep=true;
    }
    #[test] #[ignore = "Explicit user-supplied Russian speech regression; needs GRAFFIN_ASR_SOURCE"]
    fn actual_contextual_russian_speech_regression() {
        let source = PathBuf::from(std::env::var_os("GRAFFIN_ASR_SOURCE").expect("set GRAFFIN_ASR_SOURCE"));
        let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        let cache = root.parent().unwrap().join("qa/graffin-asr-accuracy/production-asr");
        let result = transcribe_clip(&root.join("resources/ffmpeg"), &source, 208.0, 26.0, "ru", &cache, &RenderControl::new()).unwrap();
        let text = fs::read_to_string(&result).unwrap();
        let lower = text.to_lowercase();
        assert!(lower.contains("гитхаб"), "technical name stays recognizable");
        assert!(lower.contains("крипту"), "word is separated from the following address");
        assert!(text.matches("-->").count() > 35, "speech after the opening is retained");
        assert!(text.contains("00:00:2"), "recognition continues into final third");
        for file in fs::read_dir(&cache).unwrap().filter_map(Result::ok) {
            assert!(!file.file_name().to_string_lossy().starts_with('.'), "no temporary ASR output remains");
        }
    }
    #[test] #[ignore = "Explicit real Russian audio recognition smoke"]
    fn actual_russian_speech_recognition() {
        let downloads = PathBuf::from(std::env::var_os("HOME").unwrap()).join("Downloads");
        let source = fs::read_dir(&downloads).unwrap().filter_map(Result::ok).map(|e| e.path()).find(|p| p.file_name().unwrap().to_string_lossy().contains("Голос-2026-10-17-20-46") && p.extension().is_some_and(|e|e=="mp3")).expect("Russian sample");
        let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        let cache = root.parent().unwrap().join("qa/media-tools-asr");
        let result = transcribe_clip(&root.join("resources/ffmpeg"), &source, 0.0, 6.0, "ru", &cache, &RenderControl::new()).unwrap();
        let text = fs::read_to_string(result).unwrap();
        assert!(text.contains("-->"));
        assert!(text.to_lowercase().contains("интернет"));
        assert!(text.to_lowercase().contains("ограничен"));
        for block in text.split("\n\n") {
            let lines = block.lines().collect::<Vec<_>>();
            if let Some(index) = lines.iter().position(|line| line.contains("-->")) {
                assert!(lines[index + 1..].join(" ").split_whitespace().count() <= 1, "Whisper must retain complete words, not phrases: {block}");
            }
        }
    }
}
