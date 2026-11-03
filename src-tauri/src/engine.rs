//! Local, shell-free video assembly. Every exported foreground interval is unique
//! within a batch; only the background and silent banner may loop.
use rand::Rng;
use regex::Regex;
use serde::{Deserialize, Serialize};
use std::{
    collections::{hash_map::DefaultHasher, HashSet},
    fs,
    hash::{Hash, Hasher},
    io::{BufRead, BufReader, Read},
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
    sync::{atomic::{AtomicBool, AtomicU64, Ordering}, mpsc, Arc, Mutex},
    thread,
    time::{Duration, Instant, UNIX_EPOCH},
};

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Media {
    pub id: String,
    pub path: String,
    pub name: String,
    pub duration: f64,
    pub width: u32,
    pub height: u32,
    pub has_audio: bool,
    pub thumbnail: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BannerAsset {
    pub id: String,
    pub name: String,
    pub path: String,
    pub preview: String,
    pub kind: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Settings {
    pub count: u32,
    pub duration: f64,
    pub resolution: u32,
    pub top_split: f64,
    pub banner_height: u32,
    #[serde(default = "default_banner_bottom")]
    pub banner_bottom: u32,
    pub banner_enabled: bool,
    #[serde(default)]
    pub banner_path: String,
    #[serde(default = "default_banner_name")]
    pub banner_name: String,
    #[serde(default)]
    pub banner_library: Vec<BannerAsset>,
    #[serde(default)]
    pub subtitles_enabled: bool,
    #[serde(default = "default_subtitle_size")]
    pub subtitle_size: u32,
    #[serde(default)]
    pub subtitle_offset: i32,
    #[serde(default = "default_subtitle_language")]
    pub subtitle_language: String,
    #[serde(default = "default_subtitle_font")]
    pub subtitle_font: String,
    #[serde(default = "default_subtitle_color")]
    pub subtitle_color: String,
    #[serde(default = "default_subtitle_words_per_line")]
    pub subtitle_words_per_line: u32,
    #[serde(default = "default_subtitle_animation")]
    pub subtitle_animation: String,
    #[serde(default)]
    pub subtitle_highlight_enabled: bool,
    #[serde(default = "default_subtitle_highlight_color")]
    pub subtitle_highlight_color: String,
    #[serde(default = "default_cut_mode")]
    pub cut_mode: String,
    #[serde(default = "default_layout_mode")]
    pub layout_mode: String,
    #[serde(default = "default_single_scale")]
    pub single_scale: f64,
    #[serde(default)]
    pub music_path: String,
    #[serde(default)]
    pub music_name: String,
    #[serde(default)]
    pub music_duration: f64,
    #[serde(default)]
    pub music_start: f64,
    #[serde(default)]
    pub music_delay: f64,
    #[serde(default = "default_music_volume")]
    pub music_volume: f64,
    #[serde(default = "default_true")]
    pub music_loop: bool,
    #[serde(default = "default_fade")]
    pub music_fade_in: f64,
    #[serde(default = "default_fade")]
    pub music_fade_out: f64,
    #[serde(default)]
    pub interruption_enabled: bool,
    #[serde(default)]
    pub interruption_path: String,
    #[serde(default = "default_interruption_duration")]
    pub interruption_duration: f64,
    #[serde(default)]
    pub interruption_fade: f64,
    #[serde(default = "default_interruption_timing")]
    pub interruption_timing: String,
    #[serde(default = "default_interruption_start")]
    pub interruption_start: f64,
    #[serde(default = "default_interruption_end")]
    pub interruption_end: f64,
    pub fit_mode: String,
    pub top_focus: f64,
    pub bottom_focus: f64,
    pub top_volume: f64,
    pub bottom_volume: f64,
    pub order: String,
    pub output_dir: String,
    #[serde(default)]
    pub import_dir: String,
    #[serde(default)]
    pub top_source_dir: String,
    #[serde(default)]
    pub bottom_source_dir: String,
}

impl Default for Settings {
    fn default() -> Self {
        Self { count: 10, duration: 30.0, resolution: 1080, top_split: 0.5,
            banner_height: 240, banner_bottom: default_banner_bottom(), banner_enabled: true, fit_mode: "crop".into(),
            banner_path: String::new(), banner_name: default_banner_name(), banner_library: Vec::new(),
            subtitles_enabled: false, subtitle_size: default_subtitle_size(), subtitle_offset: 0, subtitle_language: default_subtitle_language(),
            subtitle_font: default_subtitle_font(), subtitle_color: default_subtitle_color(), cut_mode: default_cut_mode(),
            subtitle_words_per_line: default_subtitle_words_per_line(), subtitle_animation: default_subtitle_animation(),
            subtitle_highlight_enabled: false, subtitle_highlight_color: default_subtitle_highlight_color(),
            top_focus: 0.5, bottom_focus: 0.5, top_volume: 1.0, bottom_volume: 0.0,
            layout_mode: default_layout_mode(), single_scale: default_single_scale(),
            music_path: String::new(), music_name: String::new(), music_duration: 0.0, music_start: 0.0, music_delay: 0.0,
            music_volume: default_music_volume(), music_loop: true, music_fade_in: default_fade(), music_fade_out: default_fade(),
            interruption_enabled: false, interruption_path: String::new(), interruption_duration: default_interruption_duration(), interruption_fade: 0.0,
            interruption_timing: default_interruption_timing(), interruption_start: default_interruption_start(), interruption_end: default_interruption_end(),
            order: "random".into(), output_dir: String::new(), import_dir: String::new(), top_source_dir: String::new(), bottom_source_dir: String::new() }
    }
}

fn default_banner_bottom() -> u32 { 240 }
fn default_banner_name() -> String { "Barsik VPN".into() }
fn default_subtitle_size() -> u32 { 56 }
fn default_subtitle_language() -> String { "ru".into() }
fn default_subtitle_font() -> String { "manrope".into() }
fn default_subtitle_color() -> String { "#FFFFFF".into() }
fn default_subtitle_words_per_line() -> u32 { 1 }
fn default_subtitle_animation() -> String { "pop".into() }
fn default_subtitle_highlight_color() -> String { "#A6FF52".into() }
fn default_cut_mode() -> String { "fixed".into() }
fn default_layout_mode() -> String { "split".into() }
fn default_single_scale() -> f64 { 0.94 }
fn default_music_volume() -> f64 { 0.18 }
fn default_true() -> bool { true }
fn default_fade() -> f64 { 1.0 }
fn default_interruption_duration() -> f64 { 3.0 }
fn default_interruption_timing() -> String { "middle".into() }
fn default_interruption_start() -> f64 { 5.0 }
fn default_interruption_end() -> f64 { 20.0 }

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BatchRequest {
    pub top: Vec<Media>,
    pub bottom: Vec<Media>,
    pub settings: Settings,
    pub banner_path: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ClipPlan {
    pub index: u32,
    pub top_path: String,
    pub top_start: f64,
    pub bottom_path: String,
    pub bottom_start: f64,
    pub duration: f64,
    pub output_path: String,
}

#[derive(Clone, Default)]
pub struct RenderControl {
    cancelled: Arc<AtomicBool>,
    child: Arc<Mutex<Option<Child>>>,
}

impl RenderControl {
    pub fn new() -> Self { Self::default() }
    pub fn cancel(&self) {
        self.cancelled.store(true, Ordering::SeqCst);
        if let Ok(mut guard) = self.child.lock() {
            if let Some(child) = guard.as_mut() { crate::platform::terminate_child(child); }
        }
    }
    pub fn is_cancelled(&self) -> bool { self.cancelled.load(Ordering::SeqCst) }

    /// Auxiliary encoders and speech recognition share the export's cancellation
    /// handle. Both pipes are drained concurrently so a verbose child cannot
    /// block, and every termination path reaps the process.
    pub(crate) fn run_cancellable(&self, mut command: Command, timeout: Duration) -> Result<String, String> {
        if self.is_cancelled() { return Err("Экспорт отменён".into()); }
        crate::platform::hide_console(&mut command);
        let mut child = command.stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::piped())
            .spawn().map_err(|e| format!("Не удалось запустить обработку: {e}"))?;
        let stdout = child.stdout.take().ok_or("Нет канала вывода обработки")?;
        let stderr = child.stderr.take().ok_or("Нет диагностического канала обработки")?;
        let output_reader = thread::spawn(move || read_tail(stdout, 65_536));
        let error_reader = thread::spawn(move || read_tail(stderr, 65_536));
        {
            let mut guard = self.child.lock().map_err(|_| "Не удалось управлять обработкой")?;
            if guard.is_some() {
                crate::platform::terminate_child(&mut child); let _ = child.wait();
                let _ = output_reader.join(); let _ = error_reader.join();
                return Err("Другая обработка уже выполняется".into());
            }
            if self.is_cancelled() { crate::platform::terminate_child(&mut child); }
            *guard = Some(child);
        }
        let deadline = Instant::now() + timeout;
        let status = loop {
            let mut guard = self.child.lock().map_err(|_| "Не удалось прочитать процесс обработки")?;
            let child = guard.as_mut().ok_or("Процесс обработки потерян")?;
            if self.is_cancelled() || Instant::now() >= deadline {
                crate::platform::terminate_child(child); let _ = child.wait(); *guard = None;
                break Err(if self.is_cancelled() { "Экспорт отменён".to_string() } else { "Обработка превысила время ожидания".into() });
            }
            match child.try_wait() {
                Ok(Some(status)) => { *guard = None; break Ok(status); }
                Ok(None) => { drop(guard); thread::sleep(Duration::from_millis(40)); }
                Err(e) => { crate::platform::terminate_child(child); let _ = child.wait(); *guard = None; break Err(format!("Ошибка обработки: {e}")); }
            }
        };
        let output = output_reader.join().unwrap_or_default();
        let diagnostic = error_reader.join().unwrap_or_default();
        let status = status?;
        if self.is_cancelled() { return Err("Экспорт отменён".into()); }
        if !status.success() { return Err(format!("Ошибка обработки: {}", concise_error(&diagnostic))); }
        Ok(format!("{output}\n{diagnostic}"))
    }
}

#[derive(Clone, Debug)]
struct StreamInfo { duration: f64, width: u32, height: u32, has_audio: bool }

fn media_path(path: &Path) -> Result<PathBuf, String> {
    if !path.is_absolute() { return Err("Нужен абсолютный путь к видео".into()); }
    let path = fs::canonicalize(path).map_err(|e| format!("Файл недоступен: {} ({e})", path.display()))?;
    if !path.is_file() { return Err(format!("Это не файл: {}", path.display())); }
    let ext = path.extension().and_then(|s| s.to_str()).unwrap_or("").to_ascii_lowercase();
    if !["mp4", "mov", "mkv", "webm", "m4v", "avi", "mpg", "mpeg", "mts", "m2ts", "ts", "ogv"].contains(&ext.as_str()) {
        return Err("Выберите видео MP4, MOV, MKV, WebM или AVI".into());
    }
    Ok(path)
}

// FFmpeg diagnostics are used because this distribution bundles the static
// FFmpeg executable, not a second ffprobe binary. Input protocols remain local.
fn parse_diagnostics(text: &str) -> Result<StreamInfo, String> {
    let duration_re = Regex::new(r"Duration:\s*(\d+):(\d+):(\d+(?:\.\d+)?)").unwrap();
    let dims_re = Regex::new(r"(?:,\s*)(\d{1,6})x(\d{1,6})(?:[\s,\[]|$)").unwrap();
    let rotation_re = Regex::new(r"rotation of\s*([-\d.]+)").unwrap();
    let caps = duration_re.captures(text).ok_or("Не удалось определить длительность видео")?;
    let number = |i| caps.get(i).unwrap().as_str().parse::<f64>().unwrap_or(0.0);
    let duration = number(1) * 3600.0 + number(2) * 60.0 + number(3);
    let mut width = 0;
    let mut height = 0;
    let mut selected_video = false;
    for line in text.lines() {
        if line.contains("Stream #") {
            if width > 0 && line.contains("Video:") { selected_video = false; }
            if width == 0 && line.contains("Video:") && !line.contains("attached pic") {
                if let Some(c) = dims_re.captures(line) {
                    width = c[1].parse().unwrap_or(0);
                    height = c[2].parse().unwrap_or(0);
                    selected_video = true;
                }
            }
        }
        if selected_video {
            if let Some(c) = rotation_re.captures(line) {
                let degrees: f64 = c[1].parse().unwrap_or(0.0);
                if (degrees.abs() % 180.0 - 90.0).abs() < 1.0 { std::mem::swap(&mut width, &mut height); }
            }
        }
    }
    if !duration.is_finite() || duration <= 0.0 || duration > 1_209_600.0 || width == 0 || height == 0 {
        return Err("У файла нет доступной видеодорожки или корректной длительности".into());
    }
    Ok(StreamInfo { duration, width, height, has_audio: text.lines().any(|l| l.contains("Stream #") && l.contains("Audio:")) })
}

fn read_tail(mut reader: impl Read, max: usize) -> String {
    let mut buffer = [0_u8; 4096];
    let mut tail = Vec::new();
    while let Ok(n) = reader.read(&mut buffer) {
        if n == 0 { break; }
        tail.extend_from_slice(&buffer[..n]);
        if tail.len() > max { tail.drain(..tail.len() - max); }
    }
    String::from_utf8_lossy(&tail).into_owned()
}

fn bounded_command(mut command: Command, timeout: Duration) -> Result<(bool, String), String> {
    crate::platform::hide_console(&mut command);
    let mut child = command.stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::piped())
        .spawn().map_err(|e| format!("Не удалось запустить видеодвижок: {e}"))?;
    let stderr = child.stderr.take().ok_or("Нет диагностического канала FFmpeg")?;
    let reader = thread::spawn(move || read_tail(stderr, 65_536));
    let deadline = Instant::now() + timeout;
    loop {
        match child.try_wait() {
            Ok(Some(status)) => return Ok((status.success(), reader.join().unwrap_or_default())),
            Err(e) => { crate::platform::terminate_child(&mut child); let _ = child.wait(); let _ = reader.join(); return Err(format!("Ошибка процесса FFmpeg: {e}")); }
            Ok(None) if Instant::now() >= deadline => {
                crate::platform::terminate_child(&mut child); let _ = child.wait(); let _ = reader.join();
                return Err("Видео не ответило вовремя. Проверьте, что файл полностью скачан и доступен локально".into());
            }
            Ok(None) => thread::sleep(Duration::from_millis(25)),
        }
    }
}

fn inspect_stream(ffmpeg: &Path, path: &Path) -> Result<StreamInfo, String> {
    let mut command = Command::new(ffmpeg);
    command.args(["-hide_banner", "-nostdin", "-protocol_whitelist", "file,pipe", "-i"]).arg(path);
    let (_, diagnostic) = bounded_command(command, Duration::from_secs(15))?;
    parse_diagnostics(&diagnostic)
        .map_err(|e| format!("{}: {e}", path.file_name().unwrap_or_default().to_string_lossy()))
}

/// Read-only validation of a local audio asset; no remote FFmpeg protocols.
pub(crate) fn inspect_audio_duration(ffmpeg: &Path, path: &Path) -> Result<f64, String> {
    let path = audio_path(path)?;
    let mut command = Command::new(ffmpeg);
    command.args(["-hide_banner", "-nostdin", "-protocol_whitelist", "file,pipe", "-i"]).arg(path);
    let (_, text) = bounded_command(command, Duration::from_secs(15))?;
    let re = Regex::new(r"Duration:\s*(\d+):(\d+):(\d+(?:\.\d+)?)").unwrap();
    let caps = re.captures(&text).ok_or("Не удалось определить длительность музыки")?;
    let part = |i| caps.get(i).unwrap().as_str().parse::<f64>().unwrap_or(0.0);
    let duration = part(1) * 3600.0 + part(2) * 60.0 + part(3);
    if !text.lines().any(|line| line.contains("Stream #") && line.contains("Audio:")) || !duration.is_finite() || duration <= 0.0 || duration > 86_400.0 {
        return Err("Выберите аудиофайл длительностью до 24 часов".into());
    }
    Ok(duration)
}

fn audio_path(path: &Path) -> Result<PathBuf, String> {
    if !path.is_absolute() { return Err("Нужен абсолютный путь к музыке".into()); }
    let path = path.canonicalize().map_err(|_| "Фоновая музыка недоступна. Загрузите файл ещё раз.".to_string())?;
    let meta = path.metadata().map_err(|e| e.to_string())?;
    if !meta.is_file() || meta.len() == 0 || meta.len() > 1_073_741_824 { return Err("Выберите аудиофайл размером до 1 ГБ".into()); }
    let ext = path.extension().and_then(|s| s.to_str()).unwrap_or("").to_ascii_lowercase();
    if !["mp3", "wav", "m4a", "aac", "flac", "ogg", "opus", "aiff", "aif", "mp4", "webm"].contains(&ext.as_str()) { return Err("Музыка: MP3, WAV, M4A, AAC, FLAC, OGG, Opus или AIFF".into()); }
    Ok(path)
}

pub fn probe(ffmpeg: &Path, path: &Path, cache: &Path) -> Result<Media, String> {
    let path = media_path(path)?;
    let info = inspect_stream(ffmpeg, &path)?;
    fs::create_dir_all(cache).map_err(|e| format!("Не создать кэш миниатюр: {e}"))?;
    let meta = fs::metadata(&path).map_err(|e| e.to_string())?;
    let mut hasher = DefaultHasher::new();
    "retina-thumbnail-v2".hash(&mut hasher);
    path.hash(&mut hasher);
    meta.len().hash(&mut hasher);
    meta.modified().ok().and_then(|t| t.duration_since(UNIX_EPOCH).ok()).map(|d| d.as_nanos()).hash(&mut hasher);
    let id = format!("{:016x}", hasher.finish());
    let thumbnail = cache.join(format!("{id}.jpg"));
    if !thumbnail.is_file() {
        let mut command = Command::new(ffmpeg);
        command.args(["-hide_banner", "-loglevel", "error", "-nostdin", "-y", "-threads", "2", "-protocol_whitelist", "file,pipe", "-ss"])
            .arg(format!("{:.4}", (info.duration * 0.1).min(2.0)))
            .arg("-i").arg(&path)
            .args(["-map", "0:V:0", "-an", "-sn", "-vf", "scale=1080:1080:force_original_aspect_ratio=decrease,setsar=1", "-frames:v", "1", "-q:v", "3", "-update", "1"])
            .arg(&thumbnail);
        let (ok, diagnostic) = bounded_command(command, Duration::from_secs(20))?;
        if !ok || !thumbnail.is_file() { let _ = fs::remove_file(&thumbnail); return Err(format!("Не удалось прочитать кадр видео: {}", concise_error(&diagnostic))); }
    }
    Ok(Media { id, path: path.to_string_lossy().into_owned(),
        name: path.file_name().unwrap_or_default().to_string_lossy().into_owned(),
        duration: info.duration, width: info.width, height: info.height, has_audio: info.has_audio,
        thumbnail: thumbnail.to_string_lossy().into_owned() })
}

pub fn validate_settings(settings: &Settings) -> Result<(), String> {
    let bounded = |v: f64, low, high| v.is_finite() && v >= low && v <= high;
    if !(1..=200).contains(&settings.count) { return Err("Количество роликов: от 1 до 200".into()); }
    if !bounded(settings.duration, 5.0, 180.0) { return Err("Длительность: от 5 до 180 секунд".into()); }
    if ![720, 1080].contains(&settings.resolution) { return Err("Выберите ширину 720 или 1080 пикселей".into()); }
    if !bounded(settings.top_split, 0.3, 0.7) || settings.banner_height > 360 || settings.banner_bottom > 600 { return Err("Некорректные размеры блоков композиции".into()); }
    if !bounded(settings.top_focus, 0.0, 1.0) || !bounded(settings.bottom_focus, 0.0, 1.0) { return Err("Некорректное положение кадра".into()); }
    if !bounded(settings.top_volume, 0.0, 1.5) || !bounded(settings.bottom_volume, 0.0, 1.5) { return Err("Громкость должна быть от 0 до 150%".into()); }
    if !["split", "single"].contains(&settings.layout_mode.as_str()) || !bounded(settings.single_scale, 0.5, 1.0) { return Err("Некорректный режим или размер основного видео".into()); }
    if !bounded(settings.music_duration, 0.0, 86_400.0) || !bounded(settings.music_start, 0.0, 86_400.0) || !bounded(settings.music_delay, 0.0, 180.0)
        || !bounded(settings.music_volume, 0.0, 1.5) || !bounded(settings.music_fade_in, 0.0, 15.0) || !bounded(settings.music_fade_out, 0.0, 15.0) {
        return Err("Проверьте начало, громкость и плавность фоновой музыки".into());
    }
    if settings.music_name.chars().count() > 240 || settings.music_path.len() > 4096 || settings.interruption_path.len() > 4096 { return Err("Слишком длинное имя медиафайла".into()); }
    if !bounded(settings.interruption_duration, 0.5, 30.0) || !bounded(settings.interruption_start, 0.0, 180.0) || !bounded(settings.interruption_end, 0.0, 180.0)
        || !bounded(settings.interruption_fade, 0.0, 2.0) || !["middle", "fixed", "random", "quiet"].contains(&settings.interruption_timing.as_str()) { return Err("Проверьте длительность и момент появления прерывающего баннера".into()); }
    if settings.interruption_enabled && settings.interruption_timing == "random" && settings.interruption_start > settings.interruption_end { return Err("Конец диапазона баннера должен быть после начала".into()); }
    if settings.interruption_enabled && settings.interruption_path.trim().is_empty() { return Err("Загрузите прерывающий баннер".into()); }
    if settings.fit_mode != "crop" && settings.fit_mode != "contain" { return Err("Неизвестный способ заполнения кадра".into()); }
    if settings.order != "random" && settings.order != "sequential" { return Err("Неизвестный порядок нарезки".into()); }
    if !(28..=96).contains(&settings.subtitle_size) || !(-600..=600).contains(&settings.subtitle_offset) { return Err("Некорректные размеры субтитров".into()); }
    if !["ru", "en", "auto"].contains(&settings.subtitle_language.as_str()) { return Err("Выберите русский, английский или автоматический язык субтитров".into()); }
    if !["fixed", "smart"].contains(&settings.cut_mode.as_str()) { return Err("Неизвестный способ нарезки".into()); }
    if subtitle_font(&settings.subtitle_font).is_none() { return Err("Шрифт субтитров недоступен. Выберите встроенный шрифт или загрузите свой TTF/OTF.".into()); }
    if !valid_subtitle_color(&settings.subtitle_color) { return Err("Цвет субтитров должен быть в формате #RRGGBB".into()); }
    if !(1..=5).contains(&settings.subtitle_words_per_line) || !["none", "fade", "pop", "slide"].contains(&settings.subtitle_animation.as_str()) { return Err("Выберите от 1 до 5 слов и доступную анимацию субтитров".into()); }
    if !valid_subtitle_color(&settings.subtitle_highlight_color) { return Err("Цвет подсветки должен быть в формате #RRGGBB".into()); }
    if settings.banner_name.chars().count() > 80 || settings.banner_library.len() > 100 { return Err("Слишком много данных в библиотеке баннеров".into()); }
    if !Path::new(&settings.output_dir).is_absolute() { return Err("Выберите папку экспорта".into()); }
    Ok(())
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Geometry { pub width: u32, pub height: u32, pub top: u32, pub bottom: u32, pub banner: u32, pub footer: u32 }

pub fn geometry(settings: &Settings) -> Geometry {
    let width = settings.resolution;
    let height = width * 16 / 9;
    let even = |v: f64| ((v / 2.0).round() as u32) * 2;
    // The reserved bottom strip is part of the composition even when its
    // artwork is temporarily hidden; toggling it never moves the video seam.
    let banner = even(f64::from(settings.banner_height) * f64::from(width) / 1080.0);
    let footer = even(f64::from(settings.banner_bottom) * f64::from(width) / 1080.0);
    let available = height - banner - footer;
    let top = even(f64::from(available) * settings.top_split);
    Geometry { width, height, top, bottom: available - top, banner, footer }
}

/// The caller must use freshly probed metadata, never the frontend's duration.
pub fn plan(request: &BatchRequest) -> Result<Vec<ClipPlan>, String> {
    validate_settings(&request.settings)?;
    if request.top.is_empty() || request.settings.layout_mode != "single" && request.bottom.is_empty() { return Err("Добавьте основное и фоновое видео (или отключите фон)".into()); }
    if request.top.len() > 1000 || request.bottom.len() > 1000 { return Err("Можно выбрать не более 1000 файлов в каждом блоке".into()); }
    if !Path::new(&request.settings.output_dir).is_dir() { return Err("Папка экспорта недоступна".into()); }
    let mut seen = HashSet::new();
    let mut sources = Vec::new();
    let mut capacity = 0_usize;
    for media in &request.top {
        let path = media_path(Path::new(&media.path))?;
        if !seen.insert(path.clone()) { return Err("В верхнем блоке один и тот же файл выбран дважды".into()); }
        if !media.duration.is_finite() || media.duration <= 0.0 || media.duration > 1_209_600.0 { return Err("Перепроверьте длительность исходных видео".into()); }
        let slots = ((media.duration + 0.000_001) / request.settings.duration).floor() as usize;
        sources.push((path, slots, capacity));
        capacity = capacity.checked_add(slots).ok_or("Слишком много фрагментов")?;
    }
    let mut bottoms = Vec::new();
    for media in request.bottom.iter().filter(|_| request.settings.layout_mode != "single") {
        let path = media_path(Path::new(&media.path))?;
        if !media.duration.is_finite() || media.duration <= 0.0 { return Err("Перепроверьте длительность нижних видео".into()); }
        bottoms.push((path, media.duration));
    }
    if capacity < request.settings.count as usize {
        return Err(format!("Без повторов получится {capacity} роликов. Добавьте верхние видео, уменьшите длительность или количество"));
    }
    let mut rng = rand::rng();
    let selections: Vec<usize> = if request.settings.order == "random" {
        rand::seq::index::sample(&mut rng, capacity, request.settings.count as usize).into_vec()
    } else { (0..request.settings.count as usize).collect() };
    let mut clips = Vec::with_capacity(selections.len());
    for (i, selection) in selections.into_iter().enumerate() {
        let (top, _, offset) = sources.iter().find(|(_, slots, offset)| selection >= *offset && selection < *offset + *slots).ok_or("Не удалось распределить исходники")?;
        let fallback = (top.clone(), request.settings.duration);
        let bottom_index = if bottoms.is_empty() { 0 } else if request.settings.order == "random" { rng.random_range(0..bottoms.len()) } else { i % bottoms.len() };
        let (bottom, bottom_duration) = bottoms.get(bottom_index).unwrap_or(&fallback);
        let max_start = (bottom_duration - request.settings.duration).max(0.0);
        let bottom_start = if request.settings.order == "random" && max_start > 0.0 { rng.random_range(0.0..max_start) } else { 0.0 };
        let output = Path::new(&request.settings.output_dir).join(format!("Graffin_{:03}.mp4", i + 1));
        if output.exists() { return Err(format!("Файл {} уже существует. Выберите новую папку", output.display())); }
        clips.push(ClipPlan { index: (i + 1) as u32, top_path: top.to_string_lossy().into_owned(),
            top_start: (selection - offset) as f64 * request.settings.duration,
            bottom_path: bottom.to_string_lossy().into_owned(), bottom_start,
            duration: request.settings.duration, output_path: output.to_string_lossy().into_owned() });
    }
    Ok(clips)
}

fn fit_filter(g: Geometry, height: u32, mode: &str, focus: f64) -> String {
    if mode == "contain" {
        format!("scale={}:{}:force_original_aspect_ratio=decrease:force_divisible_by=2,pad={}:{}:(ow-iw)/2:(oh-ih)/2:color=0x080d15,setsar=1", g.width, height, g.width, height)
    } else {
        format!("scale={}:{}:force_original_aspect_ratio=increase:force_divisible_by=2,crop={}:{}:(iw-ow)*{:.6}:(ih-oh)/2,setsar=1", g.width, height, g.width, height, focus)
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct SubtitleCue { pub start: f64, pub end: f64, pub text: String }

pub(crate) fn subtitle_cues(srt: &str, duration: f64) -> Vec<SubtitleCue> {
    let timing = Regex::new(r"^(\d{1,3}):(\d{2}):(\d{2})[,.](\d{1,3})\s*-->\s*(\d{1,3}):(\d{2}):(\d{2})[,.](\d{1,3})").unwrap();
    let markup = Regex::new(r"<[^>]*>").unwrap();
    let normalized = srt.trim_start_matches('\u{feff}').replace("\r\n", "\n").replace('\r', "\n");
    let mut cues = Vec::new();
    for block in normalized.split("\n\n") {
        let mut lines = block.lines();
        let mut times = None;
        for line in lines.by_ref() {
            if let Some(c) = timing.captures(line.trim()) {
                let seconds = |offset: usize| {
                    let value = |index| c.get(index).unwrap().as_str().parse::<f64>().unwrap_or(0.0);
                    value(offset) * 3600.0 + value(offset + 1) * 60.0 + value(offset + 2)
                        + value(offset + 3) / 10_f64.powi(c.get(offset + 3).unwrap().as_str().len() as i32)
                };
                times = Some((seconds(1), seconds(5))); break;
            }
        }
        if let Some((start, end)) = times {
            let plain = markup.replace_all(&lines.collect::<Vec<_>>().join(" "), "").into_owned();
            let text = plain.split_whitespace().collect::<Vec<_>>().join(" ");
            let start = start.clamp(0.0, duration); let end = end.clamp(0.0, duration);
            if end > start && !text.is_empty() { cues.push(SubtitleCue { start, end, text }); }
        }
    }
    cues.sort_by(|a, b| a.start.total_cmp(&b.start));
    // Speech recognition may overlap adjacent segments slightly. Trim the
    // earlier cue so only one caption can occupy the seam at a time.
    for i in 0..cues.len().saturating_sub(1) { cues[i].end = cues[i].end.min(cues[i + 1].start); }
    cues.retain(|cue| cue.end - cue.start >= 0.01);
    cues
}

// Fresh Whisper transcripts use -ml 1 -sow and already contain measured word
// boundaries. The fallback only handles legacy/externally supplied phrase SRTs.
pub(crate) fn single_word_cues(cues: Vec<SubtitleCue>) -> Vec<SubtitleCue> {
    let mut words: Vec<SubtitleCue> = Vec::new();
    for cue in cues {
        let tokens = cue.text.split_whitespace().collect::<Vec<_>>();
        if tokens.len() == 1 {
            if tokens[0].chars().any(char::is_alphanumeric) { words.push(cue); }
            else if tokens[0].chars().all(|c| ",.!?:;…".contains(c)) {
                if let Some(previous) = words.last_mut() { previous.text.push_str(tokens[0]); }
            }
            continue;
        }
        let spoken = tokens.into_iter().filter(|word| word.chars().any(char::is_alphanumeric)).collect::<Vec<_>>();
        let total = spoken.iter().map(|word| word.chars().count()).sum::<usize>().max(1) as f64;
        let mut elapsed = 0_usize;
        for word in spoken {
            let start = cue.start + (cue.end - cue.start) * elapsed as f64 / total;
            elapsed += word.chars().count();
            let end = cue.start + (cue.end - cue.start) * elapsed as f64 / total;
            if end - start >= 0.01 { words.push(SubtitleCue { start, end, text: word.into() }); }
        }
    }
    words
}

fn ass_time(seconds: f64) -> String {
    let centiseconds = (seconds.max(0.0) * 100.0).round() as u64;
    format!("{}:{:02}:{:02}.{:02}", centiseconds / 360_000, centiseconds / 6000 % 60, centiseconds / 100 % 60, centiseconds % 100)
}

pub(crate) fn subtitle_font(slug: &str) -> Option<(String, String)> {
    let bundled = match slug {
        "manrope" => Some(("Manrope ExtraBold", "Manrope-ExtraBold.ttf")),
        "montserrat" => Some(("Montserrat ExtraBold", "Montserrat-ExtraBold.ttf")),
        "rubik" => Some(("Rubik Bold", "Rubik-Bold.ttf")),
        "oswald" => Some(("Oswald Bold", "Oswald-Bold.ttf")),
        "russo" => Some(("Russo One", "RussoOne-Regular.ttf")),
        _ => None,
    };
    if let Some((family, filename)) = bundled { return Some((family.into(), filename.into())); }
    crate::font_library::resolve(slug).ok().map(|font| (font.family, font.path))
}
pub(crate) fn valid_subtitle_color(color: &str) -> bool {
    color.len() == 7 && color.starts_with('#') && color.as_bytes()[1..].iter().all(u8::is_ascii_hexdigit)
}
fn ass_color(color: &str) -> String {
    if !valid_subtitle_color(color) { return "&H00FFFFFF".into(); }
    format!("&H00{}{}{}", &color[5..7], &color[3..5], &color[1..3]).to_ascii_uppercase()
}
fn subtitle_word_width_for(font: &str, text: &str) -> f64 {
    if font.starts_with("custom-") {
        return crate::font_library::word_width(font, text).unwrap_or_else(|| (text.chars().count() as f64 * 2.0).max(0.1));
    }
    static ADVANCES: std::sync::OnceLock<std::collections::HashMap<String, std::collections::HashMap<char, f64>>> = std::sync::OnceLock::new();
    let fonts = ADVANCES.get_or_init(|| {
        [
            ("manrope", include_str!("../resources/fonts/Manrope-ExtraBold-widths.json")),
            ("montserrat", include_str!("../resources/fonts/Montserrat-ExtraBold-widths.json")),
            ("rubik", include_str!("../resources/fonts/Rubik-Bold-widths.json")),
            ("oswald", include_str!("../resources/fonts/Oswald-Bold-widths.json")),
            ("russo", include_str!("../resources/fonts/RussoOne-Regular-widths.json")),
        ].into_iter().map(|(key, data)| (key.into(), serde_json::from_str(data).expect("bundled font metrics"))).collect()
    });
    let advances = fonts.get(font).unwrap_or_else(|| fonts.get("manrope").unwrap());
    text.chars().map(|letter| advances.get(&letter).copied().unwrap_or(1.1)).sum::<f64>().max(0.1)
}
#[cfg(test)]
fn subtitle_word_width(text: &str) -> f64 { subtitle_word_width_for("manrope", text) }

// Group measured words while retaining their timing; never bridge a long pause
// or a sentence boundary just to fill the user's maximum word count.
fn subtitle_groups(cues: Vec<SubtitleCue>, maximum: usize) -> Vec<Vec<SubtitleCue>> {
    let mut groups: Vec<Vec<SubtitleCue>> = Vec::new();
    let mut current: Vec<SubtitleCue> = Vec::new();
    for cue in cues {
        let break_before = current.last().is_some_and(|previous| {
            current.len() >= maximum || cue.start - previous.end >= 0.25
                || previous.text.trim_end_matches(['"', '»', '”', ')', ']', '｝']).ends_with(['.', '!', '?', '…'])
        });
        if break_before { groups.push(std::mem::take(&mut current)); }
        current.push(cue);
    }
    if !current.is_empty() { groups.push(current); }
    groups
}
fn caption_text(text: &str) -> String { text.replace('\\', "＼").replace('{', "｛").replace('}', "｝") }

fn subtitle_ass(settings: &Settings, duration: f64, srt: &str) -> Option<String> {
    let cues = single_word_cues(subtitle_cues(srt, duration));
    if cues.is_empty() { return None; }
    let g = geometry(settings); let scale = f64::from(g.width) / 1080.0;
    let size = f64::from(settings.subtitle_size) * scale;
    let base_y = if settings.layout_mode == "single" { f64::from(g.height) * 0.72 } else { f64::from(g.top) };
    let center_y = (base_y + f64::from(settings.subtitle_offset) * scale).clamp(size * 1.3, f64::from(g.height) - size * 1.3);
    let (family, _) = subtitle_font(&settings.subtitle_font)?;
    let color = ass_color(&settings.subtitle_color);
    let highlight = ass_color(&settings.subtitle_highlight_color);
    let mut ass = format!("[Script Info]\nScriptType: v4.00+\nPlayResX: {}\nPlayResY: {}\nWrapStyle: 2\nScaledBorderAndShadow: yes\n\n[V4+ Styles]\nFormat: Name, Fontname, Fontsize, PrimaryColour, SecondaryColour, OutlineColour, BackColour, Bold, Italic, Underline, StrikeOut, ScaleX, ScaleY, Spacing, Angle, BorderStyle, Outline, Shadow, Alignment, MarginL, MarginR, MarginV, Encoding\nStyle: Seam,{family},{size:.2},{color},{color},&H00000000,&H90000000,0,0,0,0,100,100,0,0,1,{:.2},{:.2},5,32,32,0,1\n\n[Events]\nFormat: Layer, Start, End, Style, Name, MarginL, MarginR, MarginV, Effect, Text\n", g.width, g.height, 3.6 * scale, 1.0 * scale);
    for group in subtitle_groups(cues, settings.subtitle_words_per_line.clamp(1, 5) as usize) {
        let start = group.first()?.start; let end = group.last()?.end;
        if ass_time(start) == ass_time(end) { continue; }
        let plain = group.iter().map(|word| word.text.as_str()).collect::<Vec<_>>().join(" ");
        let overshoot = if settings.subtitle_animation == "pop" { 1.08 } else { 1.0 };
        let word_size = size.min(f64::from(g.width) * 0.82 / (subtitle_word_width_for(&settings.subtitle_font, &plain) * overshoot));
        let milliseconds = ((end - start) * 1000.0).round().max(10.0) as u32;
        let x = f64::from(g.width) / 2.0;
        let mut tags = format!("\\an5\\q2\\fs{word_size:.2}");
        if settings.subtitle_animation == "slide" {
            let slide = 170.min(milliseconds / 3).max(1);
            tags.push_str(&format!("\\move({x:.2},{:.2},{x:.2},{center_y:.2},0,{slide})\\fad({},{})", center_y + 24.0 * scale, 65.min(milliseconds / 5), 60.min(milliseconds / 5)));
        } else {
            tags.push_str(&format!("\\pos({x:.2},{center_y:.2})"));
            match settings.subtitle_animation.as_str() {
                "fade" => tags.push_str(&format!("\\fad({},{})", 90.min(milliseconds / 4), 70.min(milliseconds / 4))),
                "pop" => {
                    let pop = 65.min((milliseconds * 3 / 10).max(1));
                    let settle = 135.min((milliseconds * 6 / 10).max(pop + 1));
                    tags.push_str(&format!("\\fscx86\\fscy86\\fad({},{})\\t(0,{pop},\\fscx108\\fscy108)\\t({pop},{settle},\\fscx100\\fscy100)", 28.min(milliseconds / 5), 35.min(milliseconds / 5)));
                }
                _ => {}
            }
        }
        let text = if settings.subtitle_highlight_enabled {
            group.iter().map(|word| {
                let on = ((word.start - start) * 1000.0).round().max(0.0) as u32;
                let off = ((word.end - start) * 1000.0).round().max(1.0) as u32;
                let initial = if on == 0 { &highlight } else { &color };
                let activate = if on == 0 { String::new() } else { format!("\\t({on},{},\\1c{highlight})", on + 1) };
                format!("{{\\1c{initial}{activate}\\t({off},{},\\1c{color})}}{}", off + 1, caption_text(&word.text))
            }).collect::<Vec<_>>().join(" ")
        } else { caption_text(&plain) };
        // One event per phrase means appearance motion runs once, independent
        // of the color transforms on individual measured words.
        ass.push_str(&format!("Dialogue: 0,{},{},Seam,,0,0,0,,{{{tags}}}{text}\n", ass_time(start), ass_time(end)));
    }
    Some(ass)
}

struct StyledSubtitles { path: PathBuf }
impl Drop for StyledSubtitles { fn drop(&mut self) { let _ = fs::remove_file(&self.path); } }

fn subtitle_font_dir(slug: &str) -> String {
    let safe = !slug.is_empty() && slug.len() <= 80 && slug.bytes().all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-');
    format!("graffin-caption-fonts-v2/{}", if safe { slug } else { "invalid" })
}

fn style_subtitles(settings: &Settings, duration: f64, srt: &Path, cache: &Path, font: &Path) -> Result<Option<StyledSubtitles>, String> {
    if fs::metadata(srt).map_err(|e| format!("Не прочитать субтитры: {e}"))?.len() > 2_000_000 { return Err("Файл субтитров слишком большой".into()); }
    let text = fs::read_to_string(srt).map_err(|e| format!("Не прочитать субтитры: {e}"))?;
    let Some(ass) = subtitle_ass(settings, duration, &text) else { return Ok(None); };
    fs::create_dir_all(cache).map_err(|e| format!("Не создать кэш субтитров: {e}"))?;
    // Each selected face gets an isolated directory: two custom weights with
    // the same internal family cannot cause libass to pick the other font.
    // Relative, ASCII-only paths also work in non-ASCII home directories.
    let fonts = cache.join(subtitle_font_dir(&settings.subtitle_font));
    fs::create_dir_all(&fonts).map_err(|e| format!("Не подготовить шрифт субтитров: {e}"))?;
    let bytes = fs::read(font).map_err(|e| format!("Не найден шрифт субтитров: {e}"))?;
    let cached_font = fonts.join(font.file_name().ok_or("Не найдено имя шрифта")?);
    if fs::read(&cached_font).ok().as_deref() != Some(bytes.as_slice()) {
        let temporary_font = fonts.join(format!(".font-{}-{}.partial", std::process::id(), PARTIAL_SEQUENCE.fetch_add(1, Ordering::Relaxed)));
        fs::write(&temporary_font, &bytes).map_err(|e| format!("Не сохранить шрифт субтитров: {e}"))?;
        if let Err(error) = fs::rename(&temporary_font, &cached_font) { let _ = fs::remove_file(&temporary_font); return Err(format!("Не сохранить шрифт субтитров: {error}")); }
    }
    let path = cache.join(format!("graffin-subtitles-{}-{}.ass", std::process::id(), PARTIAL_SEQUENCE.fetch_add(1, Ordering::Relaxed)));
    let mut options = fs::OpenOptions::new(); options.write(true).create_new(true);
    #[cfg(unix)] { use std::os::unix::fs::OpenOptionsExt; options.mode(0o600); }
    let mut file = options.open(&path).map_err(|e| format!("Не подготовить субтитры: {e}"))?;
    use std::io::Write;
    if let Err(error) = file.write_all(ass.as_bytes()) { let _ = fs::remove_file(&path); return Err(format!("Не сохранить субтитры: {error}")); }
    Ok(Some(StyledSubtitles { path }))
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum BannerInput { Still, Animation }

fn banner_input(path: &Path) -> Result<(PathBuf, BannerInput), String> {
    if !path.is_absolute() { return Err("Нужен абсолютный путь к баннеру".into()); }
    let path = fs::canonicalize(path).map_err(|e| format!("Баннер недоступен: {e}"))?;
    let metadata = path.metadata().map_err(|e| e.to_string())?;
    if !metadata.is_file() || metadata.len() == 0 || metadata.len() > 536_870_912 { return Err("Баннер должен быть файлом размером до 512 МБ".into()); }
    let ext = path.extension().and_then(|s| s.to_str()).unwrap_or("").to_ascii_lowercase();
    if !["mp4", "mov", "mkv", "webm", "m4v", "gif", "apng", "png", "jpg", "jpeg", "webp"].contains(&ext.as_str()) { return Err("Баннер: выберите видео, GIF, PNG, JPG или WebP".into()); }
    let animated_png = if ext == "png" {
        // APNG may use a .png extension, including the built-in Barsik loop.
        // Read real chunk boundaries rather than mistaking pixel data for acTL.
        let mut file = fs::File::open(&path).map_err(|e| e.to_string())?;
        let mut header = [0_u8; 8]; let mut found = false;
        if file.read_exact(&mut header).is_ok() && header == *b"\x89PNG\r\n\x1a\n" {
            use std::io::{Seek, SeekFrom};
            for _ in 0..128 {
                if file.read_exact(&mut header).is_err() { break; }
                if &header[4..] == b"acTL" { found = true; break; }
                if &header[4..] == b"IDAT" || &header[4..] == b"IEND" { break; }
                let size = u32::from_be_bytes(header[..4].try_into().unwrap());
                if file.seek(SeekFrom::Current(i64::from(size) + 4)).is_err() { break; }
            }
        }
        found
    } else { false };
    let kind = if ["jpg", "jpeg", "webp"].contains(&ext.as_str()) || ext == "png" && !animated_png { BannerInput::Still } else { BannerInput::Animation };
    Ok((path, kind))
}

fn verify_banner(ffmpeg: &Path, path: &Path) -> Result<(), String> {
    let mut command = Command::new(ffmpeg);
    command.args(["-hide_banner", "-loglevel", "error", "-nostdin", "-threads", "2", "-protocol_whitelist", "file,pipe", "-i"]).arg(path)
        .args(["-map", "0:V:0", "-frames:v", "1", "-an", "-sn", "-f", "null", "-"]);
    let (ok, text) = bounded_command(command, Duration::from_secs(15))?;
    if !ok { return Err(format!("Не удалось прочитать баннер: {}", concise_error(&text))); }
    Ok(())
}

fn append_banner_input(command: &mut Command, path: &Path, kind: BannerInput) {
    match kind {
        BannerInput::Still => { command.args(["-loop", "1", "-framerate", "30"]); }
        BannerInput::Animation => { command.args(["-stream_loop", "-1"]); }
    }
    command.args(["-threads", "2", "-protocol_whitelist", "file,pipe", "-i"]).arg(path);
}

#[derive(Clone, Copy, Debug)]
struct Interruption { at: f64, duration: f64 }

fn interruption_time(settings: &Settings, clip: &ClipPlan) -> Option<Interruption> {
    if !settings.interruption_enabled { return None; }
    let final_frame = ((clip.duration * 30.0).floor() as u64).saturating_sub(1).max(1);
    let latest = final_frame as f64 / 30.0;
    let requested = match settings.interruption_timing.as_str() {
        "fixed" => settings.interruption_start,
        "random" => {
            // A chosen moment stays stable when editing/re-rendering a clip.
            let mut hash = DefaultHasher::new();
            clip.top_path.hash(&mut hash); clip.top_start.to_bits().hash(&mut hash); clip.index.hash(&mut hash);
            let low = settings.interruption_start.min(latest);
            let high = settings.interruption_end.min(latest).max(low);
            low + (high - low) * (hash.finish() as f64 / u64::MAX as f64)
        }
        _ => clip.duration / 2.0,
    };
    let frame = ((requested * 30.0).round() as u64).clamp(1, final_frame);
    Some(Interruption { at: frame as f64 / 30.0, duration: (settings.interruption_duration * 30.0).round() / 30.0 })
}

// FFmpeg's silencedetect produces clip-relative timestamps after input seek.
// Select a complete quiet interval, including a trailing silence at EOF.
fn quiet_interruption_at(diagnostic: &str, duration: f64) -> f64 {
    if !duration.is_finite() || duration <= 0.0 { return 0.0; }
    let middle = duration / 2.0;
    let events = Regex::new(r"silence_(start|end):\s*(-?\d+(?:\.\d+)?)").unwrap();
    let mut start = None; let mut intervals = Vec::new();
    for event in events.captures_iter(diagnostic) {
        let Ok(value) = event[2].parse::<f64>() else { continue; };
        if !value.is_finite() { continue; }
        let value = value.clamp(0.0, duration);
        if &event[1] == "start" { start = Some(value); }
        else if let Some(begin) = start.take() {
            if value - begin >= 0.249 { intervals.push((begin, value)); }
        }
    }
    if let Some(begin) = start { if duration - begin >= 0.249 { intervals.push((begin, duration)); } }
    let chosen = intervals.into_iter().map(|(begin,end)| (begin+end)/2.0)
        .min_by(|a,b| (a-middle).abs().total_cmp(&(b-middle).abs())).unwrap_or(middle);
    // The actual insertion is subsequently rounded to a real video frame.
    chosen.clamp((1.0_f64/30.0).min(middle), (duration-1.0/30.0).max(middle))
}

fn detect_quiet_interruption(ffmpeg: &Path, source: &Path, clip: &ClipPlan, has_audio: bool, control: &RenderControl) -> Result<f64,String> {
    if !has_audio { return Ok(clip.duration / 2.0); }
    // A bounded minute around the middle keeps both work and diagnostic size
    // predictable, even for extremely fragmented audio in a three-minute clip.
    let window_duration=clip.duration.min(60.0);
    let window_start=(clip.duration-window_duration)/2.0;
    let mut command = Command::new(ffmpeg);
    command.args(["-hide_banner","-loglevel","info","-nostdin","-threads","2","-protocol_whitelist","file,pipe","-ss"])
        .arg(format!("{:.6}",clip.top_start+window_start)).arg("-i").arg(source)
        .args(["-t"]).arg(format!("{:.6}",window_duration))
        .args(["-map","0:a:0","-vn","-sn","-dn","-af","silencedetect=noise=-32dB:d=0.25","-f","null","-"]);
    match control.run_cancellable(command,Duration::from_secs(30)) {
        Ok(diagnostic) => Ok(window_start+quiet_interruption_at(&diagnostic,window_duration)),
        Err(_) if control.is_cancelled() => Err("Экспорт отменён".into()),
        // Quiet placement is optional; a failed analysis never prevents a valid
        // clip from using the documented middle-of-video fallback.
        Err(_) => Ok(clip.duration/2.0),
    }
}

pub(crate) fn content_pause(settings:&Settings,clip:&ClipPlan)->Option<(f64,f64)> {
    interruption_time(settings,clip).map(|pause|(pause.at,pause.duration))
}

struct RenderInputs { banner: Option<usize>, interruption: Option<usize>, music: Option<(usize, f64)>, pause: Option<Interruption> }

fn filter_graph(settings: &Settings, clip: &ClipPlan, top: &StreamInfo, bottom: &StreamInfo, subtitle_filename: Option<&str>, inputs: &RenderInputs) -> String {
    let g = geometry(settings);
    let duration = format!("{:.6}", clip.duration);
    let mut graph = Vec::new();
    if settings.layout_mode == "single" {
        let main_height = ((f64::from(g.height) * settings.single_scale / 2.0).round() as u32) * 2;
        graph.push(format!("[0:V:0]trim=duration={duration},setpts=PTS-STARTPTS,fps=30,{},format=yuv420p,pad={}:{}:0:(oh-ih)/2:color=black[canvas]", fit_filter(g, main_height, &settings.fit_mode, settings.top_focus), g.width, g.height));
    } else {
        graph.push(format!("[0:V:0]trim=duration={duration},setpts=PTS-STARTPTS,fps=30,{},format=yuv420p[top]", fit_filter(g, g.top, &settings.fit_mode, settings.top_focus)));
        graph.push(format!("[1:V:0]trim=duration={duration},setpts=PTS-STARTPTS,fps=30,{},format=yuv420p[bottom]", fit_filter(g, g.bottom, &settings.fit_mode, settings.bottom_focus)));
        graph.push(format!("[top][bottom]vstack=inputs=2:shortest=1,pad={}:{}:0:0:color=0x080d15[canvas]", g.width, g.height));
    }
    if let Some(index) = inputs.banner {
        graph.push(format!("[{index}:v:0]trim=duration={duration},setpts=PTS-STARTPTS,fps=30,scale={}:{}:force_original_aspect_ratio=decrease:force_divisible_by=2,format=rgba[banner]", g.width, g.banner));
        graph.push(format!("[canvas][banner]overlay=x=(W-w)/2:y={}+({}-h)/2:eof_action=repeat:shortest=0:format=auto,format=yuv420p[composite]", g.height - g.footer - g.banner, g.banner));
    } else { graph.push("[canvas]null[composite]".into()); }
    if let Some(filename) = subtitle_filename {
        graph.push(format!("[composite]ass=filename={filename}:fontsdir={}:shaping=complex[captioned]", subtitle_font_dir(&settings.subtitle_font)));
    } else { graph.push("[composite]null[captioned]".into()); }
    let mut audio_count = 0;
    for (index, has_audio, volume) in [(0, top.has_audio, settings.top_volume), (1, settings.layout_mode != "single" && bottom.has_audio, settings.bottom_volume)] {
        if has_audio && volume > 0.0 {
            graph.push(format!("[{index}:a:0]atrim=duration={duration},asetpts=PTS-STARTPTS,aresample=48000,aformat=sample_fmts=fltp:channel_layouts=stereo,loudnorm=I=-18:TP=-2:LRA=11,aresample=48000,volume={volume:.6},apad,atrim=duration={duration}[a{audio_count}]"));
            audio_count += 1;
        }
    }
    if let Some((index, remaining)) = inputs.music {
        let play = (clip.duration - settings.music_delay).min(if settings.music_loop { f64::MAX } else { remaining });
        let fade_in = settings.music_fade_in.min(play / 2.0);
        let fade_out = settings.music_fade_out.min(play / 2.0);
        let delay = (settings.music_delay * 48_000.0).round() as u64;
        // Buffer only a tail shorter than this clip (at most 180 seconds),
        // never a complete long track. Repeat starts at the selected offset.
        let looping = if settings.music_loop && remaining < play {
            format!(",aloop=loop=-1:size={},asetpts=N/SR/TB", (remaining * 48_000.0).ceil().max(1.0) as u64)
        } else { String::new() };
        graph.push(format!("[{index}:a:0]asetpts=PTS-STARTPTS,aresample=48000,aformat=sample_fmts=fltp:channel_layouts=stereo{looping},atrim=duration={play:.6},volume={:.6},afade=t=in:d={fade_in:.6},afade=t=out:st={:.6}:d={fade_out:.6},adelay={delay}S:all=1,apad,atrim=duration={duration}[a{audio_count}]", settings.music_volume, play - fade_out));
        audio_count += 1;
    }
    match audio_count {
        0 => graph.push(format!("anullsrc=r=48000:cl=stereo,atrim=duration={duration}[mixed]")),
        1 => graph.push("[a0]alimiter=limit=0.95:level=0:latency=1[mixed]".into()),
        _ => graph.push(format!("{}amix=inputs={audio_count}:normalize=0:duration=longest,alimiter=limit=0.95:level=0:latency=1[mixed]", (0..audio_count).map(|i|format!("[a{i}]")).collect::<String>())),
    }
    if let (Some(pause), Some(index)) = (inputs.pause, inputs.interruption) {
        let at = pause.at; let hold = pause.duration; let frame=(at*30.0).round() as u64;
        graph.push("[captioned]settb=AVTB,split=3[vpre][vfreeze][vpost]".into());
        graph.push(format!("[vpre]trim=end_frame={frame},setpts=PTS-STARTPTS[before]"));
        graph.push(format!("[vfreeze]trim=start_frame={frame}:end_frame={},setpts=PTS-STARTPTS,tpad=stop_mode=clone:stop_duration={hold:.6},trim=duration={hold:.6}[frozen]", frame+1));
        graph.push(format!("[vpost]trim=start_frame={frame},setpts=PTS-STARTPTS[after]"));
        let fade = settings.interruption_fade.min(hold / 2.0);
        let transition = if fade > 0.0 { format!(",fade=t=in:st=0:d={fade:.6}:alpha=1,fade=t=out:st={:.6}:d={fade:.6}:alpha=1",hold-fade) } else { String::new() };
        graph.push(format!("[{index}:v:0]trim=duration={hold:.6},setpts=PTS-STARTPTS,fps=30,scale={}:{}:force_original_aspect_ratio=decrease:force_divisible_by=2,format=rgba{transition}[interruption]", (g.width * 94 / 100) / 2 * 2, (g.height * 70 / 100) / 2 * 2));
        graph.push(format!("[frozen][interruption]overlay=(W-w)/2:(H-h)/2:eof_action=repeat:shortest=0:format=auto,format=yuv420p,trim=duration={hold:.6},setpts=PTS-STARTPTS[advert]"));
        graph.push("[before][advert][after]concat=n=3:v=1:a=0[v]".into());
        graph.push("[mixed]asplit=2[apre][apost]".into());
        graph.push(format!("[apre]atrim=end={at:.6},asetpts=PTS-STARTPTS[abefore]"));
        graph.push(format!("anullsrc=r=48000:cl=stereo,atrim=duration={hold:.6},asetpts=PTS-STARTPTS[asilence]"));
        graph.push(format!("[apost]atrim=start={at:.6},asetpts=PTS-STARTPTS[aafter]"));
        graph.push("[abefore][asilence][aafter]concat=n=3:v=0:a=1[a]".into());
    } else {
        graph.push("[captioned]null[v]".into()); graph.push("[mixed]anull[a]".into());
    }
    graph.join(";")
}

fn concise_error(text: &str) -> String {
    let lines = text.lines().filter(|l| !l.trim().is_empty()).rev().take(5).collect::<Vec<_>>();
    lines.into_iter().rev().collect::<Vec<_>>().join(" ").chars().take(900).collect()
}

static PARTIAL_SEQUENCE: AtomicU64 = AtomicU64::new(0);

#[cfg(target_os = "macos")]
pub(crate) fn publish_output(partial: &Path, output: &Path) -> Result<(), String> {
    use std::{ffi::CString, os::unix::ffi::OsStrExt};
    unsafe extern "C" {
        fn renamex_np(from: *const std::os::raw::c_char, to: *const std::os::raw::c_char, flags: u32) -> std::os::raw::c_int;
    }
    let from = CString::new(partial.as_os_str().as_bytes()).map_err(|_| "Некорректный путь временного файла")?;
    let to = CString::new(output.as_os_str().as_bytes()).map_err(|_| "Некорректный путь экспорта")?;
    // RENAME_EXCL, from the macOS SDK sys/stdio.h. Unlike hard links, rename
    // works on exFAT export disks. Both paths share the same parent/volume.
    let result = unsafe { renamex_np(from.as_ptr(), to.as_ptr(), 0x0000_0004) };
    if result == 0 { Ok(()) } else { Err(format!("Не сохранить готовый ролик: {}", std::io::Error::last_os_error())) }
}

#[cfg(windows)]
pub(crate) fn publish_output(partial: &Path, output: &Path) -> Result<(), String> {
    use std::os::windows::ffi::OsStrExt;
    #[link(name = "kernel32")]
    unsafe extern "system" { fn MoveFileW(existing: *const u16, destination: *const u16) -> i32; }
    let from: Vec<u16> = partial.as_os_str().encode_wide().chain(Some(0)).collect();
    let to: Vec<u16> = output.as_os_str().encode_wide().chain(Some(0)).collect();
    if from[..from.len()-1].contains(&0) || to[..to.len()-1].contains(&0) { return Err("Некорректный путь экспорта.".into()); }
    // MoveFileW refuses an existing destination and works on NTFS and exFAT.
    if unsafe { MoveFileW(from.as_ptr(), to.as_ptr()) } != 0 { Ok(()) }
    else { Err(format!("Не сохранить готовый ролик: {}", std::io::Error::last_os_error())) }
}

#[cfg(all(not(target_os = "macos"), not(windows)))]
pub(crate) fn publish_output(partial: &Path, output: &Path) -> Result<(), String> {
    // This application ships for macOS. The conservative fallback keeps the
    // standalone engine testable on other platforms without replacing files.
    fs::hard_link(partial, output).map_err(|e| format!("Не сохранить готовый ролик: {e}"))?;
    fs::remove_file(partial).map_err(|e| e.to_string())
}

pub fn render_clip(
    ffmpeg: &Path, banner: &Path, settings: &Settings, clip: &ClipPlan,
    control: &RenderControl, on_progress: impl FnMut(f64),
) -> Result<(), String> {
    render_clip_inner(ffmpeg, banner, settings, clip, control, None, on_progress)
}

/// Render caption words already verified by the moment-analysis pipeline.
/// This bypasses a second ASR pass and preserves the reviewed transcript.
pub(crate) fn render_clip_with_subtitles(
    ffmpeg: &Path, banner: &Path, settings: &Settings, clip: &ClipPlan,
    control: &RenderControl, subtitle_file: &Path, on_progress: impl FnMut(f64),
) -> Result<(), String> {
    render_clip_inner(ffmpeg, banner, settings, clip, control, Some(subtitle_file), on_progress)
}

pub(crate) fn render_clip_inner(
    ffmpeg: &Path, banner: &Path, settings: &Settings, clip: &ClipPlan,
    control: &RenderControl, subtitle_fixture: Option<&Path>, mut on_progress: impl FnMut(f64),
) -> Result<(), String> {
    validate_settings(settings)?;
    if control.is_cancelled() { return Err("Экспорт отменён".into()); }
    if !clip.duration.is_finite() || clip.duration < 0.05 || clip.duration > 180.0
        || !clip.top_start.is_finite() || clip.top_start < 0.0
        || !clip.bottom_start.is_finite() || clip.bottom_start < 0.0 {
        return Err("Некорректные границы фрагмента".into());
    }
    let mut normalized_clip = clip.clone();
    if settings.layout_mode == "single" { normalized_clip.bottom_path = clip.top_path.clone(); normalized_clip.bottom_start = 0.0; }
    let clip = &normalized_clip;
    let top_path = media_path(Path::new(&clip.top_path))?;
    let bottom_path = if settings.layout_mode == "single" { top_path.clone() } else { media_path(Path::new(&clip.bottom_path))? };
    let top = inspect_stream(ffmpeg, &top_path)?;
    let bottom = if settings.layout_mode == "single" { top.clone() } else { inspect_stream(ffmpeg, &bottom_path)? };
    if clip.top_start + clip.duration > top.duration + 0.005 { return Err("Верхнее видео короче выбранного фрагмента".into()); }
    if clip.bottom_start >= bottom.duration { return Err("Начало нижнего фрагмента находится за концом видео".into()); }
    let output = Path::new(&clip.output_path);
    if !output.is_absolute() || output.extension().and_then(|v| v.to_str()) != Some("mp4") { return Err("Нужен абсолютный путь экспорта MP4".into()); }
    if output.exists() { return Err("Файл экспорта уже существует; он не будет перезаписан".into()); }
    let parent = output.parent().ok_or("Не выбрана папка экспорта")?;
    if !parent.is_dir() { return Err("Папка экспорта недоступна".into()); }
    let g = geometry(settings);
    let banner = if g.banner > 0 && settings.banner_enabled {
        Some(banner_input(if settings.banner_path.is_empty() { banner } else { Path::new(&settings.banner_path) })?)
    } else { None };
    if let Some((path, _)) = &banner { verify_banner(ffmpeg, path)?; }
    let interruption = if settings.interruption_enabled { Some(banner_input(Path::new(&settings.interruption_path))?) } else { None };
    if let Some((path, _)) = &interruption { verify_banner(ffmpeg, path)?; }
    // Persist the actual chosen time in this video's recipe. App settings keep
    // the user's quiet policy, while later caption edits replay the same frame.
    let mut resolved_settings = settings.clone();
    if interruption.is_some() && settings.interruption_timing == "quiet" {
        on_progress(0.0);
        resolved_settings.interruption_start = detect_quiet_interruption(ffmpeg,&top_path,clip,top.has_audio,control)?;
        resolved_settings.interruption_timing = "fixed".into();
    }
    let settings = &resolved_settings;
    let music = if !settings.music_path.trim().is_empty() {
        let path = audio_path(Path::new(&settings.music_path))?;
        let track_duration = inspect_audio_duration(ffmpeg, &path)?;
        if settings.music_start >= track_duration { return Err("Начало музыки находится за концом трека".into()); }
        if settings.music_volume > 0.0 && settings.music_delay < clip.duration { Some((path, track_duration - settings.music_start)) } else { None }
    } else { None };
    let pause = interruption_time(settings, clip);
    let output_duration = clip.duration + pause.map_or(0.0, |p|p.duration);
    let mut subtitle_text = String::new();
    let source_identities = crate::subtitle_editor::render_source_identities(clip, settings, banner.as_ref().map(|(path,_)|path.as_path()))?;
    let subtitles = if settings.subtitles_enabled && (top.has_audio || subtitle_fixture.is_some()) {
        on_progress(0.0);
        let cache = if subtitle_fixture.is_some() { parent.join(".graffin-subtitle-cache") } else { crate::media_tools::subtitle_cache_dir()? };
        let srt = match subtitle_fixture {
            Some(path) => path.to_path_buf(),
            None => crate::media_tools::transcribe_clip(ffmpeg, &top_path, clip.top_start, clip.duration, &settings.subtitle_language, &cache, control)?,
        };
        if control.is_cancelled() { return Err("Экспорт отменён".into()); }
        subtitle_text = fs::read_to_string(&srt).map_err(|e| format!("Не прочитать слова субтитров: {e}"))?;
        let (_, filename) = subtitle_font(&settings.subtitle_font).ok_or("Неизвестный шрифт")?;
        style_subtitles(settings, clip.duration, &srt, &cache, &ffmpeg.parent().ok_or("Не найдены ресурсы экспорта")?.join("fonts").join(filename))?
    } else { None };
    let seq = PARTIAL_SEQUENCE.fetch_add(1, Ordering::Relaxed);
    let partial = parent.join(format!(".{}.{}.{}.partial.mp4", output.file_stem().unwrap_or_default().to_string_lossy(), std::process::id(), seq));
    let result = (|| {
        let mut command = Command::new(ffmpeg);
        command.args(["-hide_banner", "-loglevel", "warning", "-nostdin", "-n", "-filter_complex_threads", "2", "-threads", "2", "-protocol_whitelist", "file,pipe", "-ss"])
            .arg(format!("{:.6}", clip.top_start)).arg("-i").arg(&top_path);
        let mut next_index = 1;
        if settings.layout_mode != "single" {
            command.args(["-stream_loop", "-1", "-threads", "2", "-protocol_whitelist", "file,pipe", "-ss"])
                .arg(format!("{:.6}", clip.bottom_start)).arg("-i").arg(&bottom_path);
            next_index += 1;
        }
        let mut inputs = RenderInputs { banner: None, interruption: None, music: None, pause };
        if let Some((path, kind)) = &banner {
            append_banner_input(&mut command, path, *kind); inputs.banner = Some(next_index); next_index += 1;
        }
        if let Some((path, kind)) = &interruption {
            append_banner_input(&mut command, path, *kind); inputs.interruption = Some(next_index); next_index += 1;
        }
        if let Some((path, remaining)) = &music {
            command.args(["-threads", "2", "-protocol_whitelist", "file,pipe", "-ss"])
                .arg(format!("{:.6}", settings.music_start)).arg("-i").arg(path);
            inputs.music = Some((next_index, *remaining));
        }
        if let Some(subtitles) = &subtitles {
            command.current_dir(subtitles.path.parent().ok_or("Не найдена папка субтитров")?);
        }
        let subtitle_filename = subtitles.as_ref().and_then(|subtitles| subtitles.path.file_name()).and_then(|name| name.to_str());
        command.arg("-filter_complex").arg(filter_graph(settings, clip, &top, &bottom, subtitle_filename, &inputs))
            .args(["-map", "[v]", "-map", "[a]", "-t"]).arg(format!("{output_duration:.6}"))
            .args(["-r", "30", "-c:v", "libx264", "-preset", "veryfast", "-crf", "20", "-threads", "4", "-pix_fmt", "yuv420p", "-c:a", "aac", "-b:a", "160k", "-ar", "48000", "-ac", "2", "-movflags", "+faststart", "-map_metadata", "-1", "-sn", "-dn", "-progress", "pipe:1", "-nostats"])
            .arg(&partial).stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::piped());
        crate::platform::hide_console(&mut command);
        let mut child = command.spawn().map_err(|e| format!("Не удалось запустить экспорт: {e}"))?;
        let stdout = child.stdout.take().ok_or("Не удалось получить прогресс экспорта")?;
        let stderr = child.stderr.take().ok_or("Не удалось получить журнал экспорта")?;
        let (tx, rx) = mpsc::channel::<f64>();
        let duration = output_duration;
        let progress_reader = thread::spawn(move || {
            let mut final_frame = 0_u64;
            for line in BufReader::new(stdout).lines().map_while(Result::ok) {
                if let Some(value) = line.strip_prefix("frame=") { if let Ok(frame) = value.trim().parse::<u64>() { final_frame = frame; } }
                if let Some(value) = line.strip_prefix("out_time_us=") {
                    if let Ok(us) = value.parse::<f64>() { let _ = tx.send((us / 1_000_000.0 / duration).clamp(0.0, 1.0)); }
                }
            }
            final_frame
        });
        let error_reader = thread::spawn(move || read_tail(stderr, 16_384));
        {
            let mut guard = control.child.lock().map_err(|_| "Не удалось управлять процессом экспорта")?;
            if guard.is_some() { crate::platform::terminate_child(&mut child); let _ = child.wait(); return Err("Другой экспорт уже выполняется".into()); }
            if control.is_cancelled() { crate::platform::terminate_child(&mut child); }
            *guard = Some(child);
        }
        on_progress(0.0);
        let status = loop {
            let mut last = None;
            for progress in rx.try_iter() { last = Some(progress); }
            if let Some(progress) = last { on_progress(progress.min(0.995)); }
            let poll = {
                let mut guard = control.child.lock().map_err(|_| "Не удалось прочитать процесс экспорта")?;
                let child = guard.as_mut().ok_or("Процесс экспорта потерян")?;
                if control.is_cancelled() { crate::platform::terminate_child(child); }
                child.try_wait()
            };
            match poll {
                Ok(Some(status)) => break status,
                Ok(None) => thread::sleep(Duration::from_millis(40)),
                Err(e) => {
                    if let Ok(mut guard) = control.child.lock() { if let Some(child) = guard.as_mut() { crate::platform::terminate_child(child); let _ = child.wait(); } *guard = None; }
                    let _ = progress_reader.join(); let _ = error_reader.join();
                    return Err(format!("Не удалось дождаться экспорта: {e}"));
                }
            }
        };
        if let Ok(mut guard) = control.child.lock() { *guard = None; }
        let final_frame = progress_reader.join().unwrap_or(0);
        let diagnostic = error_reader.join().unwrap_or_default();
        if control.is_cancelled() { return Err("Экспорт отменён".into()); }
        if !status.success() { return Err(format!("Ошибка экспорта: {}", concise_error(&diagnostic))); }
        if final_frame + 1 < (output_duration * 30.0).floor() as u64 {
            return Err("Видеодорожка исходника закончилась раньше выбранного фрагмента. Уменьшите длительность или выберите другой файл".into());
        }
        if fs::metadata(&partial).map(|m| m.len()).unwrap_or(0) < 1024 { return Err("Видеодвижок не создал готовый файл".into()); }
        if control.is_cancelled() { return Err("Экспорт отменён".into()); }
        crate::subtitle_editor::verify_source_identities(&source_identities)?;
        crate::subtitle_editor::record_render(&partial, output, banner.as_ref().map(|(p, _)| p.as_path()), settings, clip, &subtitle_text, source_identities)?;
        if let Err(error) = publish_output(&partial, output) { crate::subtitle_editor::remove_record(output); return Err(error); }
        on_progress(1.0);
        Ok(())
    })();
    if result.is_err() { let _ = fs::remove_file(&partial); }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    fn settings() -> Settings { Settings { count: 2, duration: 5.0, banner_bottom: 0, order: "sequential".into(), ..Settings::default() } }
    fn tempdir(name: &str) -> PathBuf {
        let path = std::env::temp_dir().join(format!("barsik-engine-{name}-{}-{}", std::process::id(), PARTIAL_SEQUENCE.fetch_add(1, Ordering::Relaxed)));
        fs::create_dir_all(&path).unwrap(); path
    }
    fn media(path: &Path, duration: f64) -> Media {
        Media { id: "test".into(), path: path.to_string_lossy().into_owned(), name: "test".into(), duration, width: 320, height: 180, has_audio: true, thumbnail: String::new() }
    }

    #[test]
    fn banner_quiet_silence_parser_chooses_nearest_complete_interval_and_falls_back() {
        let log="[silencedetect] silence_start: 1\n[silencedetect] silence_end: 1.5 | silence_duration: 0.5\n[silencedetect] silence_start: 4.0\n[silencedetect] silence_end: 4.4 | silence_duration: 0.4\n[silencedetect] silence_start: 9";
        assert!((quiet_interruption_at(log,10.0)-4.2).abs()<0.0001);
        assert_eq!(quiet_interruption_at("",10.0),5.0);
        assert_eq!(quiet_interruption_at("silence_start: 4.0\nsilence_end: 4.1",10.0),5.0);
        assert_eq!(quiet_interruption_at("silence_start: NaN\nsilence_end: inf",10.0),5.0);
        assert_eq!(quiet_interruption_at("silence_start: 100\nsilence_end: 200",10.0),5.0);
        assert_eq!(quiet_interruption_at("silence_start: 8",10.0),9.0);
        assert!(quiet_interruption_at("silence_start: -10\nsilence_end: 0.3",10.0)>0.0);
        assert_eq!(quiet_interruption_at("",f64::NAN),0.0);
    }

    #[test]
    fn banner_quiet_and_fade_settings_are_compatible_bounded_and_cancelled() {
        let mut value=serde_json::to_value(Settings{output_dir:std::env::temp_dir().to_string_lossy().into_owned(),..settings()}).unwrap();value.as_object_mut().unwrap().remove("interruptionFade");
        let restored:Settings=serde_json::from_value(value).unwrap();assert_eq!(restored.interruption_fade,0.0);
        for bad in [f64::NAN,-0.1,2.1] {assert!(validate_settings(&Settings{interruption_fade:bad,..restored.clone()}).is_err());}
        assert!(validate_settings(&Settings{interruption_timing:"quiet".into(),interruption_fade:2.0,..restored}).is_ok());
        let clip=ClipPlan{index:1,top_path:"/tmp/source.mp4".into(),top_start:50.,bottom_path:String::new(),bottom_start:0.,duration:6.,output_path:"/tmp/out.mp4".into()};
        assert_eq!(detect_quiet_interruption(Path::new("/unused"),Path::new("/unused"),&clip,false,&RenderControl::new()).unwrap(),3.0);
        let control=RenderControl::new();control.cancel();assert!(detect_quiet_interruption(Path::new("/unused"),Path::new("/unused"),&clip,true,&control).unwrap_err().contains("отменён"));
        let value=Settings{interruption_enabled:true,interruption_path:"/tmp/banner.png".into(),interruption_duration:0.5,interruption_fade:2.0,..settings()};
        let info=StreamInfo{duration:6.,width:320,height:240,has_audio:false};
        let graph=filter_graph(&value,&clip,&info,&info,None,&RenderInputs{banner:None,interruption:Some(1),music:None,pause:interruption_time(&value,&clip)});
        assert!(graph.contains("fade=t=in:st=0:d=0.250000:alpha=1"));assert!(graph.contains("fade=t=out:st=0.250000:d=0.250000:alpha=1"));
    }

    #[test]
    #[ignore = "real FFmpeg verifies quiet placement, banner fade, silent freeze and persisted edit recipe"]
    fn ffmpeg_quiet_banner_fade_and_recipe_smoke() {
        let ff=Path::new(env!("CARGO_MANIFEST_DIR")).join("resources/ffmpeg");
        let retained=std::env::var_os("GRAFFIN_BANNER_QA_DIR").map(PathBuf::from);let dir=retained.clone().unwrap_or_else(||tempdir("quiet-banner-v24"));fs::create_dir_all(&dir).unwrap();
        let source=dir.join("source.mp4");let banner=dir.join("banner.png");
        assert!(Command::new(&ff).args(["-hide_banner","-loglevel","error","-y","-f","lavfi","-i","color=c=0x202020:s=320x240:r=30:d=6","-f","lavfi","-i",r"aevalsrc=if(between(t\,2.0\,2.6)\,0\,0.3*sin(2*PI*440*t)):s=48000:d=6","-c:v","libx264","-pix_fmt","yuv420p","-c:a","aac","-b:a","160k"]).arg(&source).status().unwrap().success());
        assert!(Command::new(&ff).args(["-hide_banner","-loglevel","error","-y","-f","lavfi","-i","color=c=white:s=64x20","-frames:v","1","-update","1"]).arg(&banner).status().unwrap().success());
        let output=dir.join("quiet-fade.mp4");let _=fs::remove_file(&output);
        let clip=ClipPlan{index:1,top_path:source.to_string_lossy().into(),top_start:0.,bottom_path:source.to_string_lossy().into(),bottom_start:0.,duration:6.,output_path:output.to_string_lossy().into()};
        let settings=Settings{count:1,duration:6.,resolution:720,layout_mode:"single".into(),banner_enabled:false,banner_height:0,banner_bottom:0,interruption_enabled:true,interruption_path:banner.to_string_lossy().into(),interruption_duration:1.0,interruption_timing:"quiet".into(),interruption_fade:0.4,output_dir:dir.to_string_lossy().into(),..settings()};
        let quiet=detect_quiet_interruption(&ff,&source,&clip,true,&RenderControl::new()).unwrap();assert!((quiet-2.3).abs()<0.05,"quiet point {quiet}");
        let offset_clip=ClipPlan{top_start:1.0,duration:4.0,..clip.clone()};let relative=detect_quiet_interruption(&ff,&source,&offset_clip,true,&RenderControl::new()).unwrap();assert!((relative-1.3).abs()<0.05,"quiet timestamp is clip-relative: {relative}");
        render_clip(&ff,&banner,&settings,&clip,&RenderControl::new(),|_|{}).unwrap();
        assert_eq!(settings.interruption_timing,"quiet");
        let recipe:serde_json::Value=serde_json::from_slice(&fs::read(dir.join(".graffin-edit/quiet-fade.mp4.json")).unwrap()).unwrap();
        assert_eq!(recipe["settings"]["interruptionTiming"],"fixed");let stored=recipe["settings"]["interruptionStart"].as_f64().unwrap();assert!((stored-quiet).abs()<0.001);
        let resolved:Settings=serde_json::from_value(recipe["settings"].clone()).unwrap();let (at,hold)=content_pause(&resolved,&clip).unwrap();assert!((at-2.3).abs()<0.05);assert_eq!(hold,1.0);
        let intensity=|time:f64|{let out=Command::new(&ff).args(["-hide_banner","-loglevel","error","-ss",&format!("{time:.6}"),"-i"]).arg(&output).args(["-frames:v","1","-vf","crop=2:2:360:640","-pix_fmt","rgb24","-f","rawvideo","pipe:1"]).output().unwrap();assert!(out.status.success());assert_eq!(out.stdout.len(),12);out.stdout.iter().map(|v|f64::from(*v)).sum::<f64>()/12.0};
        let early=intensity(at+0.04);let peak=intensity(at+0.5);let late=intensity(at+0.93);let after=intensity(at+1.1);
        eprintln!("quiet={at:.3} fade pixels early={early:.1} peak={peak:.1} late={late:.1} after={after:.1}");
        assert!(peak>230.0&&early<peak*0.7&&late<peak*0.7&&after<60.0);
        let rms=|time:f64|{let out=Command::new(&ff).args(["-hide_banner","-loglevel","error","-ss",&format!("{time:.6}"),"-i"]).arg(&output).args(["-t","0.1","-vn","-f","f32le","-ac","1","-ar","48000","pipe:1"]).output().unwrap();assert!(out.status.success());let samples=out.stdout.chunks_exact(4).map(|b|f32::from_le_bytes(b.try_into().unwrap())as f64).collect::<Vec<_>>();assert!(!samples.is_empty());(samples.iter().map(|v|v*v).sum::<f64>()/samples.len()as f64).sqrt()};
        assert!(rms(at+0.2)<0.005,"inserted pause is silent");assert!(rms(3.9)>0.01,"source sound resumes after the inserted pause");
        let metadata=inspect_stream(&ff,&output).unwrap();assert!((metadata.duration-7.0).abs()<0.1);
        assert!(Command::new(&ff).args(["-hide_banner","-loglevel","error","-i"]).arg(&output).args(["-f","null","-"]).status().unwrap().success());
        if retained.is_none(){fs::remove_dir_all(dir).unwrap();}
    }

    #[test]
    fn caption_group_settings_migrate_and_validate() {
        let mut json = serde_json::to_value(Settings { output_dir:std::env::temp_dir().to_string_lossy().into_owned(), ..settings() }).unwrap();
        for key in ["subtitleWordsPerLine", "subtitleAnimation", "subtitleHighlightEnabled", "subtitleHighlightColor"] { json.as_object_mut().unwrap().remove(key); }
        let restored: Settings = serde_json::from_value(json).unwrap();
        assert_eq!(restored.subtitle_words_per_line, 1); assert_eq!(restored.subtitle_animation, "pop");
        assert!(!restored.subtitle_highlight_enabled); assert_eq!(restored.subtitle_highlight_color, "#A6FF52");
        assert!(validate_settings(&restored).is_ok());
        for maximum in [0, 6, u32::MAX] { assert!(validate_settings(&Settings { subtitle_words_per_line:maximum, ..restored.clone() }).is_err()); }
        assert!(validate_settings(&Settings { subtitle_animation:"unknown".into(), ..restored.clone() }).is_err());
        assert!(validate_settings(&Settings { subtitle_highlight_color:"green".into(), ..restored.clone() }).is_err());
        for animation in ["none", "fade", "pop", "slide"] { assert!(validate_settings(&Settings { subtitle_words_per_line:5, subtitle_animation:animation.into(), subtitle_highlight_enabled:true, ..restored.clone() }).is_ok()); }
    }

    #[test]
    fn caption_groups_keep_measured_words_and_respect_sentences_and_pauses() {
        let cues = vec![(0.1,0.3,"Вот"),(0.35,0.6,"момент!"),(0.65,0.9,"Следующая"),(1.0,1.2,"мысль"),(1.6,1.8,"После"),(1.85,2.1,"паузы"),(2.15,2.4,"три"),(2.45,2.7,"четыре")].into_iter().map(|(start,end,text)|SubtitleCue {start,end,text:text.into()}).collect();
        let groups=subtitle_groups(cues,3);
        assert_eq!(groups.iter().map(|group|group.iter().map(|w|w.text.as_str()).collect::<Vec<_>>()).collect::<Vec<_>>(),vec![vec!["Вот","момент!"],vec!["Следующая","мысль"],vec!["После","паузы","три"],vec!["четыре"]]);
        assert_eq!((groups[0][0].start,groups[0][1].end),(0.1,0.6));
        assert_eq!((groups[2][0].start,groups[2][2].end),(1.6,2.4));
    }

    #[test]
    fn caption_group_ass_has_one_animation_and_exact_active_word_timing() {
        let srt="1\n00:00:00,200 --> 00:00:00,600\nКаждое\n\n2\n00:00:00,700 --> 00:00:01,100\nслово\n\n3\n00:00:01,200 --> 00:00:01,800\nважно\n";
        let value=Settings {subtitle_words_per_line:3,subtitle_highlight_enabled:true,..settings()};
        let ass=subtitle_ass(&value,3.0,srt).unwrap();
        let lines=ass.lines().filter(|line|line.starts_with("Dialogue:")).collect::<Vec<_>>();
        assert_eq!(lines.len(),1);let event=lines[0];
        assert!(event.contains("0:00:00.20,0:00:01.80"));
        assert_eq!(event.matches("\\fscx86").count(),1); assert_eq!(event.matches("\\pos(").count(),1);
        assert!(event.contains("\\t(500,501,\\1c&H0052FFA6)"));
        assert!(event.contains("\\t(900,901,\\1c&H00FFFFFF)"));
        assert!(event.contains("\\t(1000,1001,\\1c&H0052FFA6)"));
        for animation in ["none","fade","pop","slide"] {
            let ass=subtitle_ass(&Settings{subtitle_animation:animation.into(),..value.clone()},3.0,srt).unwrap();
            let event=ass.lines().find(|l|l.starts_with("Dialogue:")).unwrap();
            assert_eq!(event.contains("\\move("),animation=="slide");
            assert_eq!(event.contains("\\fscx86"),animation=="pop");
            assert_eq!(event.contains("\\fad("),animation!="none");
        }
    }

    #[test]
    fn caption_group_width_fits_complete_phrase_and_escapes_text() {
        let word="Ш".repeat(60);
        let srt=format!("1\n00:00:00,100 --> 00:00:00,500\n{word}\n\n2\n00:00:00,550 --> 00:00:01,000\n{word}\n");
        let value=Settings {subtitle_words_per_line:5,subtitle_size:96,..settings()};
        let ass=subtitle_ass(&value,2.,&srt).unwrap();
        let event=ass.lines().find(|l|l.starts_with("Dialogue:")).unwrap();
        let size=Regex::new(r"\\fs(\d+(?:\.\d+)?)").unwrap().captures(event).unwrap()[1].parse::<f64>().unwrap();
        let phrase=format!("{word} {word}");
        assert!(size < 20.0); assert!(size*subtitle_word_width_for(&value.subtitle_font,&phrase)*1.08 <= 1080.0*0.82+1.0);
        assert_eq!(caption_text("{x}\\evil"),"｛x｝＼evil");
    }

    #[test]
    #[ignore = "real FFmpeg verifies grouped active-word pixels, animations and freeze/resume"]
    fn ffmpeg_caption_groups_animations_and_pause_smoke() {
        let ff=Path::new(env!("CARGO_MANIFEST_DIR")).join("resources/ffmpeg");
        let retained=std::env::var_os("GRAFFIN_CAPTION_QA_DIR").map(PathBuf::from);
        let dir=retained.clone().unwrap_or_else(||tempdir("caption-groups-v24"));fs::create_dir_all(&dir).unwrap();
        let words=dir.join("words.srt");
        fs::write(&words,"1\n00:00:00,200 --> 00:00:00,600\nКаждое\n\n2\n00:00:00,700 --> 00:00:01,100\nслово\n\n3\n00:00:01,200 --> 00:00:01,800\nважно\n").unwrap();
        let base=Settings{resolution:720,layout_mode:"single".into(),banner_enabled:false,banner_height:0,banner_bottom:0,subtitles_enabled:true,subtitle_size:76,subtitle_words_per_line:3,subtitle_highlight_color:"#00FF00".into(),subtitle_highlight_enabled:true,subtitle_animation:"none".into(),output_dir:dir.to_string_lossy().into(),..settings()};
        let (_,font)=subtitle_font(&base.subtitle_font).unwrap();let font=ff.parent().unwrap().join("fonts").join(font);
        let frame=|path:&Path,second:f64| {
            let output=Command::new(&ff).args(["-hide_banner","-loglevel","error","-ss",&format!("{second:.6}"),"-i"]).arg(path).args(["-frames:v","1","-vf","crop=720:200:0:820","-pix_fmt","rgb24","-f","rawvideo","pipe:1"]).output().unwrap();
            assert!(output.status.success());assert_eq!(output.stdout.len(),720*200*3);output.stdout
        };
        let green=|pixels:&[u8]| {let points=pixels.chunks_exact(3).enumerate().filter(|(_,p)|p[1]>130&&p[0]<80&&p[2]<80).map(|(i,_)|(i%720)as f64).collect::<Vec<_>>();(points.len(),points.iter().sum::<f64>()/points.len().max(1)as f64)};
        let bbox=|pixels:&[u8]| {let mut x0=720;let mut x1=0;let mut y0=200;let mut y1=0;let mut count=0;for(i,p)in pixels.chunks_exact(3).enumerate(){if p[0]>190&&p[1]>190&&p[2]>190{let x=i%720;let y=i/720;x0=x0.min(x);x1=x1.max(x);y0=y0.min(y);y1=y1.max(y);count+=1;}}(count,x1.saturating_sub(x0),(y0+y1)as f64/2.0)};
        for animation in ["none","fade","pop","slide"] {
            let value=Settings{subtitle_animation:animation.into(),subtitle_highlight_enabled:false,..base.clone()};
            let styled=style_subtitles(&value,2.2,&words,&dir,&font).unwrap().unwrap();
            let output=dir.join(format!("{animation}.mp4"));
            let status=Command::new(&ff).args(["-hide_banner","-loglevel","error","-y","-f","lavfi","-i","color=c=black:s=720x1280:r=30:d=2.2","-vf",&format!("ass=filename={}:fontsdir={}",styled.path.file_name().unwrap().to_string_lossy(),subtitle_font_dir(&value.subtitle_font)),"-c:v","libx264","-preset","ultrafast","-crf","16","-pix_fmt","yuv420p"]).arg(&output).current_dir(&dir).status().unwrap();assert!(status.success());
            let early=bbox(&frame(&output,if animation=="slide" {0.27} else {0.215}));let settled=bbox(&frame(&output,0.4));assert!(settled.0>500,"{animation}: settled caption pixels");eprintln!("{animation} early={early:?} settled={settled:?}");
            match animation {"none"=>assert!((early.1 as f64-settled.1 as f64).abs()<3.0),"fade"=>assert!(early.0<settled.0/2,"fade has lower early opacity"),"pop"=>assert!(early.1<settled.1,"pop starts smaller"),"slide"=>assert!(early.2>settled.2+3.0,"slide starts lower"),_=>{}}
        }
        let source=dir.join("source.mp4");let banner=dir.join("pause.png");
        assert!(Command::new(&ff).args(["-hide_banner","-loglevel","error","-y","-f","lavfi","-i","color=c=black:s=320x240:r=30:d=2.3","-c:v","libx264","-pix_fmt","yuv420p"]).arg(&source).status().unwrap().success());
        assert!(Command::new(&ff).args(["-hide_banner","-loglevel","error","-y","-f","lavfi","-i","color=c=white:s=64x20","-frames:v","1","-update","1"]).arg(&banner).status().unwrap().success());
        let clip=ClipPlan{index:1,top_path:source.to_string_lossy().into(),top_start:0.,bottom_path:source.to_string_lossy().into(),bottom_start:0.,duration:2.2,output_path:dir.join("highlight-pause.mp4").to_string_lossy().into()};
        let value=Settings{interruption_enabled:true,interruption_path:banner.to_string_lossy().into(),interruption_duration:0.8,interruption_timing:"fixed".into(),interruption_start:0.8,..base};
        let _=fs::remove_file(&clip.output_path);
        render_clip_inner(&ff,&banner,&value,&clip,&RenderControl::new(),Some(&words),|_|{}).unwrap();
        let output=Path::new(&clip.output_path);
        let first=green(&frame(output,0.4));let hold1=green(&frame(output,1.0));let hold2=green(&frame(output,1.4));let third=green(&frame(output,2.2));
        assert!(first.0>50&&hold1.0>50&&third.0>50,"active word has green pixels");
        assert!(hold1.1>first.1+40.0&&third.1>hold1.1+40.0,"highlight follows measured word positions");
        assert!((hold1.1-hold2.1).abs()<2.0,"highlight freezes during banner pause");
        let empty=green(&frame(output,1.94));assert!(empty.0<20,"no highlight between measured words after resume");
        assert!(Command::new(&ff).args(["-hide_banner","-loglevel","error","-i"]).arg(output).args(["-f","null","-"]).status().unwrap().success());
        if retained.is_none(){fs::remove_dir_all(dir).unwrap();}
    }

    #[test]
    fn legacy_settings_and_single_source_planning_remain_compatible() {
        let mut old = serde_json::to_value(settings()).unwrap();
        for key in ["layoutMode", "singleScale", "musicPath", "musicName", "musicDuration", "musicStart", "musicDelay", "musicVolume", "musicLoop", "musicFadeIn", "musicFadeOut", "interruptionEnabled", "interruptionPath", "interruptionDuration", "interruptionTiming", "interruptionStart", "interruptionEnd"] { old.as_object_mut().unwrap().remove(key); }
        let restored: Settings = serde_json::from_value(old).unwrap();
        assert_eq!(restored.layout_mode, "split"); assert_eq!(restored.single_scale, 0.94);
        assert!(restored.music_loop); assert!(restored.music_path.is_empty()); assert!(!restored.interruption_enabled);
        let dir = tempdir("single-plan"); let top = dir.join("top.mp4"); fs::write(&top, b"fixture").unwrap();
        let request = BatchRequest { top: vec![media(&top, 10.0)], bottom: vec![], banner_path: String::new(), settings: Settings {layout_mode:"single".into(), output_dir:dir.to_string_lossy().into_owned(), ..settings()} };
        let clips = plan(&request).unwrap(); assert_eq!(clips.len(), 2); assert!(clips.iter().all(|clip| clip.bottom_path == clip.top_path));
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn interruption_is_bounded_and_stable_for_subtitle_rerenders() {
        let clip = ClipPlan {index:1,top_path:"/source.mp4".into(),bottom_path:String::new(),top_start:10.0,bottom_start:0.0,duration:5.0,output_path:"/tmp/output.mp4".into()};
        let mut settings = Settings {interruption_enabled:true,interruption_path:"/banner.png".into(),interruption_timing:"random".into(),interruption_start:1.0,interruption_end:3.0,..settings()};
        let first = interruption_time(&settings, &clip).unwrap(); assert!((1.0..=3.0).contains(&first.at));
        for _ in 0..10 {assert_eq!(interruption_time(&settings, &clip).unwrap().at, first.at);}
        settings.interruption_start=100.0;settings.interruption_end=150.0; let bounded=interruption_time(&settings,&clip).unwrap(); assert!(bounded.at<clip.duration);
        settings.interruption_timing="fixed".into();settings.interruption_start=0.0;assert!(interruption_time(&settings,&clip).unwrap().at>0.0);
        settings.interruption_timing="middle".into(); assert_eq!(interruption_time(&settings,&clip).unwrap().at,2.5);
        assert_eq!(content_pause(&settings,&clip),Some((2.5,3.0)));settings.interruption_enabled=false;assert!(content_pause(&settings,&clip).is_none());settings.interruption_enabled=true;
        settings.output_dir=std::env::temp_dir().to_string_lossy().into_owned(); settings.music_start=f64::NAN; assert!(validate_settings(&settings).is_err());
        settings.music_start=0.0; settings.single_scale=0.49;assert!(validate_settings(&settings).is_err());
    }

    fn decoded_audio(ffmpeg: &Path, path: &Path) -> Vec<f32> {
        let output=Command::new(ffmpeg).args(["-v","error","-i"]).arg(path).args(["-vn","-ac","1","-ar","48000","-f","f32le","pipe:1"]).output().unwrap();
        assert!(output.status.success(),"{}",String::from_utf8_lossy(&output.stderr));
        output.stdout.chunks_exact(4).map(|bytes|f32::from_le_bytes(bytes.try_into().unwrap())).collect()
    }
    fn audio_window(audio:&[f32],at:f64)->(f64,f64) {
        let begin=(at*48000.0) as usize; let end=begin+9600; assert!(end<=audio.len());let slice=&audio[begin..end];
        let rms=(slice.iter().map(|sample|f64::from(*sample).powi(2)).sum::<f64>()/slice.len() as f64).sqrt();
        let crossings=slice.windows(2).filter(|pair|pair[0]*pair[1]<0.0).count() as f64;
        (rms,crossings/0.4)
    }
    fn decoded_pixel(ffmpeg:&Path,path:&Path,at:f64,x:u32,y:u32)->[u8;3] {
        let output=Command::new(ffmpeg).args(["-v","error","-ss",&format!("{at:.4}"),"-i"]).arg(path).args(["-frames:v","1","-vf",&format!("crop=2:2:{x}:{y}"),"-f","rawvideo","-pix_fmt","rgb24","pipe:1"]).output().unwrap();assert!(output.status.success());assert!(output.stdout.len()>=3);output.stdout[..3].try_into().unwrap()
    }

    #[test]
    #[ignore = "real FFmpeg integration: pause/resume, music offsets and all audio combinations"]
    fn ffmpeg_single_music_and_interruption_resume_smoke() {
        let ff=Path::new(env!("CARGO_MANIFEST_DIR")).join("resources/ffmpeg");
        let retained=std::env::var_os("GRAFFIN_RENDER_QA_DIR").map(PathBuf::from);let dir=retained.clone().unwrap_or_else(||tempdir("render-v23"));fs::create_dir_all(&dir).unwrap();
        let top=dir.join("top.mp4");let bottom=dir.join("bottom.mp4");let image=dir.join("interrupt.png");let ad_video=dir.join("interrupt.mp4");let music=dir.join("music.wav");
        fixture(&ff,&["-f","lavfi","-i","color=c=red:s=320x240:r=30:d=5.2,drawbox=x=0:y=0:w=iw:h=ih:color=blue:t=fill:enable='gte(t,2.5)'","-f","lavfi","-i",r"aevalsrc=0.15*sin(2*PI*if(lt(t\,2.5)\,440\,880)*t):s=48000:d=5.2","-c:v","libx264","-pix_fmt","yuv420p","-c:a","aac","-shortest"],&top);
        fixture(&ff,&["-f","lavfi","-i","color=c=yellow:s=320x240:r=30:d=5.2","-f","lavfi","-i","sine=frequency=220:sample_rate=48000:duration=5.2","-c:v","libx264","-pix_fmt","yuv420p","-c:a","aac","-shortest"],&bottom);
        fixture(&ff,&["-f","lavfi","-i","color=c=lime:s=320x120:r=30","-frames:v","1","-update","1"],&image);
        fixture(&ff,&["-f","lavfi","-i","color=c=lime:s=320x120:r=30:d=0.6","-f","lavfi","-i","sine=frequency=3000:sample_rate=48000:duration=0.6","-c:v","libx264","-pix_fmt","yuv420p","-c:a","aac","-shortest"],&ad_video);
        fixture(&ff,&["-f","lavfi","-i",r"aevalsrc=0.2*sin(2*PI*if(lt(t\,1)\,1000\,2000)*t):s=48000:d=2","-c:a","pcm_s16le"],&music);
        let mut settings=Settings {count:1,duration:5.0,resolution:720,layout_mode:"single".into(),single_scale:0.8,banner_enabled:false,banner_height:0,banner_bottom:0,interruption_enabled:true,interruption_path:image.to_string_lossy().into(),interruption_duration:1.0,interruption_timing:"fixed".into(),interruption_start:2.0,output_dir:dir.to_string_lossy().into(),..Settings::default()};
        let mut clip=ClipPlan {index:1,top_path:top.to_string_lossy().into(),bottom_path:String::new(),top_start:0.0,bottom_start:0.0,duration:5.0,output_path:dir.join("single-pause.mp4").to_string_lossy().into()};
        render_clip(&ff,&image,&settings,&clip,&RenderControl::new(),|_|{}).unwrap();
        let output=Path::new(&clip.output_path);let info=inspect_stream(&ff,output).unwrap();assert_eq!((info.width,info.height),(720,1280));assert!((info.duration-6.0).abs()<0.08);
        let audio=decoded_audio(&ff,output);assert!(audio_window(&audio,2.2).0<0.0001,"all content audio is muted during banner");assert!((audio_window(&audio,1.0).1-440.0).abs()<15.0);assert!((audio_window(&audio,3.1).1-440.0).abs()<15.0,"resumes source time, not advanced time");assert!((audio_window(&audio,3.8).1-880.0).abs()<20.0);
        let before=decoded_pixel(&ff,output,1.8,360,250);let frozen=decoded_pixel(&ff,output,2.6,360,250);let resumed=decoded_pixel(&ff,output,3.2,360,250);let later=decoded_pixel(&ff,output,3.8,360,250);
        assert!(before[0]>200 && frozen[0]>200 && resumed[0]>200 && later[2]>200,"source video freezes and resumes");
        assert!(decoded_pixel(&ff,output,2.5,360,640)[1]>200,"interruption is visible");assert!(decoded_pixel(&ff,output,1.0,360,20).iter().all(|v|*v<8),"single scale leaves black canvas");
        settings.top_volume=0.0;settings.music_path=music.to_string_lossy().into();settings.music_start=1.0;settings.music_delay=0.5;settings.music_volume=0.7;settings.music_fade_in=0.0;settings.music_fade_out=0.0;settings.music_loop=false;settings.interruption_enabled=false;
        clip.output_path=dir.join("music-offset-no-loop.mp4").to_string_lossy().into();render_clip(&ff,&image,&settings,&clip,&RenderControl::new(),|_|{}).unwrap();let audio=decoded_audio(&ff,Path::new(&clip.output_path));assert!(audio_window(&audio,0.1).0<0.0001);assert!((audio_window(&audio,0.8).1-2000.0).abs()<25.0);assert!(audio_window(&audio,2.0).0<0.0001,"non-loop track ends without changing video length");
        settings.music_loop=true;settings.interruption_enabled=true;settings.interruption_path=ad_video.to_string_lossy().into();clip.output_path=dir.join("music-loop-video-pause.mp4").to_string_lossy().into();render_clip(&ff,&image,&settings,&clip,&RenderControl::new(),|_|{}).unwrap();let audio=decoded_audio(&ff,Path::new(&clip.output_path));
        assert!(audio_window(&audio,2.3).0<0.0001,"video banner audio is never mixed");for at in [1.1,3.5,5.4] {assert!((audio_window(&audio,at).1-2000.0).abs()<25.0,"loop repeats selected tail at {at}");}
        settings.interruption_enabled=false;settings.music_fade_in=0.8;settings.music_fade_out=0.8;clip.output_path=dir.join("music-fades.mp4").to_string_lossy().into();render_clip(&ff,&image,&settings,&clip,&RenderControl::new(),|_|{}).unwrap();let audio=decoded_audio(&ff,Path::new(&clip.output_path));
        let steady=audio_window(&audio,1.5).0;assert!(audio_window(&audio,0.52).0<steady*0.4,"music fades in after delay");assert!(audio_window(&audio,4.75).0<steady*0.3,"music fades out before clip end");
        settings.interruption_enabled=true;settings.interruption_start=100.0;settings.interruption_path=image.to_string_lossy().into();settings.music_path=String::new();settings.top_volume=1.0;clip.output_path=dir.join("preview-last-frame-interrupt.mp4").to_string_lossy().into();render_clip(&ff,&image,&settings,&clip,&RenderControl::new(),|_|{}).unwrap();
        assert!((inspect_stream(&ff,Path::new(&clip.output_path)).unwrap().duration-6.0).abs()<0.08);assert!(decoded_pixel(&ff,Path::new(&clip.output_path),5.4,360,640)[1]>200,"out-of-preview fixed time clamps visibly to final source frame");
        settings.interruption_enabled=false;settings.layout_mode="split".into();settings.music_start=0.0;settings.music_delay=0.0;clip.bottom_path=bottom.to_string_lossy().into();
        // Exercise every top/background/music presence combination with the
        // actual graph, decoding all streams through FFmpeg without encoding.
        for mask in 0..8 {
            settings.top_volume=if mask&1!=0 {1.0}else{0.0};settings.bottom_volume=if mask&2!=0 {0.5}else{0.0};
            let info=StreamInfo {duration:5.2,width:320,height:240,has_audio:true};
            let inputs=RenderInputs {banner:None,interruption:None,pause:None,music:if mask&4!=0 {Some((2,2.0))}else{None}};
            let graph=filter_graph(&settings,&clip,&info,&info,None,&inputs);
            let result=Command::new(&ff).args(["-v","error","-filter_complex_threads","2","-i"]).arg(&top).arg("-i").arg(&bottom).arg("-i").arg(&music).arg("-filter_complex").arg(graph).args(["-map","[v]","-map","[a]","-t","5","-f","null","-"]).output().unwrap();assert!(result.status.success(),"audio combination {mask}: {}",String::from_utf8_lossy(&result.stderr));
        }
        if retained.is_none(){fs::remove_dir_all(dir).unwrap();}
    }

    #[test]
    fn geometry_touches_and_sums_exactly() {
        for resolution in [720, 1080] { for top_split in [0.3, 0.5, 0.7] { for banner_height in [0, 1, 239, 240, 360] { for banner_bottom in [0, 1, 239, 240, 599, 600] {
            let g = geometry(&Settings { resolution, top_split, banner_height, banner_bottom, ..settings() });
            assert_eq!(g.top + g.bottom + g.banner + g.footer, g.height);
            assert_eq!(g.width * 16, g.height * 9);
            assert!([g.top, g.bottom, g.banner, g.footer].iter().all(|v| v % 2 == 0));
        } } } }
        let g = geometry(&Settings::default());
        assert_eq!((g.top, g.bottom, g.banner, g.footer), (720, 720, 240, 240));
        assert_eq!(geometry(&Settings { banner_enabled: false, ..Settings::default() }), g);
    }

    #[test]
    fn old_settings_restore_default_footer() {
        let mut json = serde_json::to_value(Settings::default()).unwrap();
        json.as_object_mut().unwrap().remove("bannerBottom");
        for name in ["bannerPath", "bannerName", "bannerLibrary", "subtitlesEnabled", "subtitleSize", "subtitleOffset", "subtitleLanguage"] { json.as_object_mut().unwrap().remove(name); }
        let restored: Settings = serde_json::from_value(json).unwrap();
        assert_eq!(restored.banner_bottom, 240);
        assert_eq!(restored.banner_path, ""); assert_eq!(restored.banner_name, "Barsik VPN"); assert!(restored.banner_library.is_empty());
        assert!(!restored.subtitles_enabled); assert_eq!((restored.subtitle_size, restored.subtitle_offset), (56, 0)); assert_eq!(restored.subtitle_language, "ru");
        let explicit: Settings = serde_json::from_value(serde_json::to_value(settings()).unwrap()).unwrap();
        assert_eq!(explicit.banner_bottom, 0);
    }

    #[test]
    fn diagnostic_duration_audio_and_rotation() {
        let info = parse_diagnostics("Duration: 01:02:03.75, start: 0.000, bitrate: 1 kb/s\nStream #0:0: Video: h264 (High), yuv420p, 1920x1080 [SAR 1:1 DAR 16:9], 30 fps\nSide data:\n displaymatrix: rotation of -90.00 degrees\nStream #0:1: Audio: aac, 48000 Hz, stereo").unwrap();
        assert_eq!((info.width, info.height), (1080, 1920));
        assert_eq!(info.duration, 3723.75); assert!(info.has_audio);
        assert!(parse_diagnostics("Duration: N/A\nStream #0:0: Audio: mp3").is_err());
    }

    #[test]
    fn plans_never_overlap_even_random_and_refuse_duplicates() {
        let dir = tempdir("plan"); let top = dir.join("top.mp4"); let bottom = dir.join("bottom.mp4");
        fs::write(&top, b"fixture").unwrap(); fs::write(&bottom, b"fixture").unwrap();
        let mut request = BatchRequest { top: vec![media(&top, 19.99)], bottom: vec![media(&bottom, 1.0)], banner_path: String::new(), settings: Settings { count: 3, output_dir: dir.to_string_lossy().into_owned(), ..settings() } };
        let sequential = plan(&request).unwrap();
        assert_eq!(sequential.iter().map(|c| c.top_start).collect::<Vec<_>>(), vec![0.0, 5.0, 10.0]);
        request.settings.order = "random".into();
        for _ in 0..20 {
            let mut starts = plan(&request).unwrap().into_iter().map(|c| c.top_start as u32).collect::<Vec<_>>(); starts.sort_unstable(); starts.dedup(); assert_eq!(starts.len(), 3);
        }
        request.settings.count = 4; assert!(plan(&request).unwrap_err().contains("3 роликов"));
        request.settings.count = 2; request.top.push(media(&top, 20.0)); assert!(plan(&request).unwrap_err().contains("дважды"));
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn refuses_nonfinite_settings_and_invalid_bounds() {
        let mut s = Settings { output_dir: std::env::temp_dir().to_string_lossy().into_owned(), ..settings() };
        assert!(validate_settings(&s).is_ok()); s.top_volume = f64::NAN; assert!(validate_settings(&s).is_err());
        s.top_volume = 1.0; s.top_split = 0.0; assert!(validate_settings(&s).is_err());
        s.top_split = 0.5; s.banner_bottom = 601; assert!(validate_settings(&s).is_err());
    }

    #[test]
    #[ignore = "requires bundled FFmpeg; verifies all faces and real source re-render"]
    fn ffmpeg_editor_rerender_and_five_faces_smoke() {
        let ff=Path::new(env!("CARGO_MANIFEST_DIR")).join("resources/ffmpeg");
        let retained=std::env::var_os("GRAFFIN_EDITOR_QA_DIR").map(PathBuf::from);
        let dir=retained.clone().unwrap_or_else(||tempdir("editor-v22"));fs::create_dir_all(&dir).unwrap();
        let top=dir.join("top.mp4");let bottom=dir.join("bottom.mp4");
        for (path,color) in [(&top,"0x1e2430"),(&bottom,"0x121820")] {
            assert!(Command::new(&ff).args(["-hide_banner","-loglevel","error","-f","lavfi","-i",&format!("color=c={color}:s=320x240:r=30:d=5.1"),"-c:v","libx264","-pix_fmt","yuv420p"]).arg(path).status().unwrap().success());
        }
        let words=dir.join("words.srt");fs::write(&words,"1\n00:00:00,200 --> 00:00:02,000\nОшибка\n").unwrap();
        let mut settings=Settings {count:1,duration:5.,resolution:720,banner_enabled:false,banner_height:0,banner_bottom:0,subtitles_enabled:true,subtitle_size:86,output_dir:dir.to_string_lossy().into(),..Settings::default()};
        let mut clip=ClipPlan {index:1,top_path:top.to_string_lossy().into(),bottom_path:bottom.to_string_lossy().into(),top_start:0.,bottom_start:0.,duration:5.,output_path:dir.join("original.mp4").to_string_lossy().into()};
        render_clip_inner(&ff,Path::new("/unused.png"),&settings,&clip,&RenderControl::new(),Some(&words),|_|{}).unwrap();
        let original_hash=fs::metadata(&clip.output_path).unwrap().len();assert!(original_hash>1024);
        fs::write(&words,"").unwrap();clip.output_path=dir.join("without-caption.mp4").to_string_lossy().into();
        render_clip_inner(&ff,Path::new("/unused.png"),&settings,&clip,&RenderControl::new(),Some(&words),|_|{}).unwrap();
        // Edited video is rebuilt from source: the previous white caption must
        // disappear completely when its word is removed.
        let frame=Command::new(&ff).args(["-hide_banner","-loglevel","error","-ss","1","-i",&clip.output_path,"-frames:v","1","-vf","crop=720:200:0:540","-f","rawvideo","-pix_fmt","rgb24","pipe:1"]).output().unwrap();assert!(frame.status.success());assert!(frame.stdout.chunks_exact(3).all(|pixel|pixel.iter().copied().max().unwrap_or(0)<100));
        for slug in ["manrope","montserrat","rubik","oswald","russo"] {
            settings.subtitle_font=slug.into();settings.subtitle_color="#F5D547".into();
            fs::write(&words,"1\n00:00:00,200 --> 00:00:02,000\nИсправлено\n").unwrap();
            let (_,filename)=subtitle_font(slug).unwrap();let font=ff.parent().unwrap().join("fonts").join(filename);
            let cache=dir.join("font-probe");let styled=style_subtitles(&settings,5.,&words,&cache,&font).unwrap().unwrap();
            let checked=Command::new(&ff).args(["-hide_banner","-loglevel","verbose","-f","lavfi","-i","color=s=720x1280:r=30:d=0.6","-vf",&format!("ass=filename={}:fontsdir={}",styled.path.file_name().unwrap().to_string_lossy(),subtitle_font_dir(&settings.subtitle_font)),"-f","null","-"]).current_dir(&cache).output().unwrap();
            let log=String::from_utf8_lossy(&checked.stderr);assert!(checked.status.success(),"{slug}: {log}");
            let expected=subtitle_font(slug).unwrap().0.chars().filter(char::is_ascii_alphanumeric).collect::<String>().to_ascii_lowercase();
            assert!(log.lines().filter(|line|line.contains("fontselect:")).filter_map(|line|line.split_once("->").map(|(_,v)|v)).any(|face|face.chars().filter(char::is_ascii_alphanumeric).collect::<String>().to_ascii_lowercase().contains(&expected)),"bundledface {slug} selected: {log}");
        }
        assert!(Command::new(&ff).args(["-hide_banner","-loglevel","error","-i",&clip.output_path,"-f","null","-"]).status().unwrap().success());if retained.is_none(){fs::remove_dir_all(dir).unwrap();}
    }

    #[test]
    fn legacy_settings_preserve_caption_appearance_and_fixed_cut_mode() {
        let mut value=serde_json::to_value(settings()).unwrap();
        for key in ["subtitleFont","subtitleColor","cutMode"]{value.as_object_mut().unwrap().remove(key);}
        let old:Settings=serde_json::from_value(value).unwrap();
        assert_eq!(old.subtitle_font,"manrope");assert_eq!(old.subtitle_color,"#FFFFFF");assert_eq!(old.cut_mode,"fixed");
    }

    #[test]
    fn five_caption_fonts_and_hex_colors_are_validated_and_preserved() {
        let srt="1\n00:00:00,100 --> 00:00:01,000\nСлово\n";
        let mut families=HashSet::new();
        for slug in ["manrope","montserrat","rubik","oswald","russo"] {
            let (family,file)=subtitle_font(slug).unwrap();families.insert(family.clone());
            let value=Settings {subtitle_font:slug.into(),subtitle_color:"#12ABEF".into(),output_dir:std::env::temp_dir().to_string_lossy().into_owned(),..settings()};
            assert!(validate_settings(&value).is_ok());
            let ass=subtitle_ass(&value,2.,srt).unwrap();assert!(ass.contains(&format!("Style: Seam,{family},")));assert!(ass.contains("&H00EFAB12"));
            let path=Path::new(env!("CARGO_MANIFEST_DIR")).join("resources/fonts").join(file);assert!(path.is_file());
            assert!(subtitle_word_width_for(slug,"Широкое")>subtitle_word_width_for(slug,"iii"));
        }
        assert_eq!(families.len(),5);
        for color in ["red","#FFF","#FF00GG","#FFFFFF\n","#FFFFFF,evil"] {assert!(validate_settings(&Settings{subtitle_color:color.into(),..settings()}).is_err());}
        assert!(validate_settings(&Settings{subtitle_font:"injected,font".into(),..settings()}).is_err());
    }

    #[test]
    fn subtitle_cues_use_clip_relative_times_and_never_inject_styles() {
        let srt = "\u{feff}1\r\n00:00:00,250 --> 00:00:03,000\r\n<b>Привет, мир!</b> {\\pos(0,0)}\r\n\r\n2\r\n00:00:02,500 --> 00:00:06,000\r\nВторой текст\r\n";
        let cues = subtitle_cues(srt, 5.0);
        assert_eq!(cues.len(), 2); assert_eq!((cues[0].start, cues[0].end), (0.25, 2.5)); assert_eq!((cues[1].start, cues[1].end), (2.5, 5.0));
        assert!(!cues[0].text.contains("<b>"));
        let ass = subtitle_ass(&settings(), 5.0, srt).unwrap();
        assert!(ass.contains("WrapStyle: 2")); assert!(ass.contains("&H00000000"));
        assert!(!ass.contains("{\\pos(0,0)}")); assert!(ass.contains("｛＼pos(0,0)｝"));
        assert!(ass.contains("\\pos(540.00,840.00)"));
    }

    #[test]
    fn captions_center_on_shared_seam_scale_offset_and_keep_one_word() {
        for resolution in [720, 1080] {
            let settings = Settings { resolution, subtitle_offset: 60, ..settings() };
            let g = geometry(&settings);
            let srt = "1\n00:00:00,000 --> 00:00:05,000\nОчень длинная фраза для проверки правильного переноса субтитров между двумя видео без зазора и без пропущенных слов в готовом ролике.\n";
            let ass = subtitle_ass(&settings, 5.0, srt).unwrap();
            let expected_y = f64::from(g.top) + 60.0 * f64::from(resolution) / 1080.0;
            assert!(ass.contains(&format!("\\pos({:.2},{expected_y:.2})", f64::from(resolution) / 2.0)));
            let dialogues = ass.lines().filter(|line| line.starts_with("Dialogue:")).collect::<Vec<_>>();
            assert!(dialogues.len() >= 2);
            assert!(dialogues.iter().all(|line| !line.contains("\\N") && line.split_once("}").unwrap().1.split_whitespace().count() == 1));
            let plain = dialogues.iter().map(|line| line.split_once("}").unwrap().1).collect::<Vec<_>>().join(" ").replace("\\N", " ");
            for word in ["Очень", "правильного", "зазора", "ролике."] { assert!(plain.contains(word)); }
            assert!(dialogues.last().unwrap().contains(",0:00:05.00,"));
        }
        assert!(subtitle_ass(&settings(), 5.0, "").is_none());
    }

    #[test]
    fn measured_words_preserve_pauses_punctuation_and_clip_bounds() {
        let srt = "1\n00:00:00,070 --> 00:00:00,560\nИнтернет\n\n2\n00:00:00,560 --> 00:00:00,770\nбез\n\n3\n00:00:01,200 --> 00:00:01,800\nограничений\n\n4\n00:00:01,800 --> 00:00:01,900\n!\n\n5\n00:00:01,900 --> 00:00:02,000\n–\n\n6\n00:00:02,100 --> 00:00:03,500\nBarsik-VPN\n";
        let words = single_word_cues(subtitle_cues(srt, 3.0));
        assert_eq!(words.iter().map(|c| c.text.as_str()).collect::<Vec<_>>(), ["Интернет", "без", "ограничений!", "Barsik-VPN"]);
        assert_eq!((words[0].start, words[0].end), (0.07, 0.56));
        assert_eq!((words[2].start, words[2].end), (1.2, 1.8));
        assert_eq!((words[3].start, words[3].end), (2.1, 3.0));
        for time in [0.0, 0.9, 1.95, 3.1] { assert!(!words.iter().any(|c| c.start <= time && time < c.end), "no caption during pause {time}"); }
        let ass = subtitle_ass(&settings(), 3.0, srt).unwrap();
        assert!(ass.contains("Style: Seam,Manrope ExtraBold,"));
        let dialogues = ass.lines().filter(|line| line.starts_with("Dialogue:")).collect::<Vec<_>>();
        assert_eq!(dialogues.len(), 4);
        assert!(dialogues.iter().all(|line| line.contains("\\fscx86") && line.contains("\\fscx108") && line.contains("\\fscx100") && line.contains("\\fad(")));
    }

    #[test]
    fn long_single_word_shrinks_without_wrapping_or_losing_characters() {
        let word = "электроэнцефалографический";
        assert!(subtitle_word_width(&"W".repeat(25)) > subtitle_word_width(&"i".repeat(25)) * 2.0);
        let srt = format!("1\n00:00:00,000 --> 00:00:02,000\n{word}\n");
        let ass = subtitle_ass(&Settings { subtitle_size: 96, ..settings() }, 2.0, &srt).unwrap();
        let dialogue = ass.lines().find(|line| line.starts_with("Dialogue:")).unwrap();
        assert!(dialogue.ends_with(word)); assert!(!dialogue.contains("\\N")); assert!(!dialogue.contains("\\fs96.00"));
    }

    #[test]
    fn banner_selection_distinguishes_stills_apng_and_local_video() {
        let dir = tempdir("banners");
        let png = dir.join("still.png"); fs::write(&png, b"\x89PNG\r\n\x1a\n\0\0\0\0IEND\0\0\0\0").unwrap();
        let apng = dir.join("loop.png"); fs::write(&apng, b"\x89PNG\r\n\x1a\n\0\0\0\x08acTL\0\0\0\x02\0\0\0\0\0\0\0\0").unwrap();
        let video = dir.join("loop.webm"); fs::write(&video, b"fixture").unwrap();
        assert_eq!(banner_input(&png).unwrap().1, BannerInput::Still);
        assert_eq!(banner_input(&apng).unwrap().1, BannerInput::Animation);
        assert_eq!(banner_input(&video).unwrap().1, BannerInput::Animation);
        assert!(banner_input(Path::new("https://invalid.test/banner.mp4")).is_err());
        let _ = fs::remove_dir_all(dir);
    }

    #[cfg(unix)]
    #[test]
    fn auxiliary_process_is_cancelled_and_reaped_before_export_can_continue() {
        let control = RenderControl::new(); let cancel = control.clone();
        let trigger = thread::spawn(move || { thread::sleep(Duration::from_millis(100)); cancel.cancel(); });
        let mut command = Command::new("/bin/sleep"); command.arg("10");
        let started = Instant::now();
        assert!(control.run_cancellable(command, Duration::from_secs(15)).unwrap_err().contains("отменён"));
        trigger.join().unwrap(); assert!(started.elapsed() < Duration::from_secs(3)); assert!(control.child.lock().unwrap().is_none());
        let control = RenderControl::new(); let mut command = Command::new("/bin/sleep"); command.arg("10");
        assert!(control.run_cancellable(command, Duration::from_millis(100)).unwrap_err().contains("время ожидания")); assert!(control.child.lock().unwrap().is_none());
    }

    #[cfg(windows)]
    #[test]
    fn windows_auxiliary_process_is_cancelled_and_reaped() {
        let root = PathBuf::from(std::env::var_os("SystemRoot").unwrap()).join("System32/ping.exe");
        let control = RenderControl::new(); let cancel = control.clone();
        let trigger = thread::spawn(move || { thread::sleep(Duration::from_millis(100)); cancel.cancel(); });
        let mut command = Command::new(root); command.args(["-n", "12", "127.0.0.1"]);
        let started = Instant::now();
        assert!(control.run_cancellable(command, Duration::from_secs(15)).unwrap_err().contains("отменён"));
        trigger.join().unwrap(); assert!(started.elapsed() < Duration::from_secs(5)); assert!(control.child.lock().unwrap().is_none());
    }

    #[test]
    fn publication_is_atomic_and_never_replaces_existing_output() {
        let dir = tempdir("publish"); let partial = dir.join("partial.mp4"); let output = dir.join("final.mp4");
        fs::write(&partial, b"complete").unwrap(); fs::write(&output, b"existing").unwrap();
        assert!(publish_output(&partial, &output).is_err()); assert_eq!(fs::read(&output).unwrap(), b"existing");
        fs::remove_file(&output).unwrap(); publish_output(&partial, &output).unwrap();
        assert_eq!(fs::read(&output).unwrap(), b"complete"); assert!(!partial.exists());
        fs::remove_dir_all(dir).unwrap();
    }

    fn fixture(ffmpeg: &Path, arguments: &[&str], output: &Path) {
        let status = Command::new(ffmpeg).args(["-hide_banner", "-loglevel", "error", "-y"]).args(arguments).arg(output).status().unwrap(); assert!(status.success());
    }

    /// Exercises the actual selected custom animation, silent overlay, image
    /// alpha, subtitle pixels at the shared seam, and repeated loops. Retain
    /// visual artifacts by setting GRAFFIN_SMOKE_OUTPUT to an absolute folder.
    #[test]
    #[ignore = "requires the bundled FFmpeg executable"]
    fn ffmpeg_custom_banner_and_subtitles_smoke() {
        let ffmpeg = PathBuf::from(std::env::var("BARSIK_FFMPEG").expect("set BARSIK_FFMPEG"));
        let retained = std::env::var_os("GRAFFIN_SMOKE_OUTPUT").map(PathBuf::from);
        let dir = retained.clone().unwrap_or_else(|| tempdir("graffin-smoke")); fs::create_dir_all(&dir).unwrap();
        let top = dir.join("top.mp4"); let bottom = dir.join("bottom.mp4"); let banner = dir.join("custom-loop.mp4"); let still = dir.join("transparent-banner.png");
        fixture(&ffmpeg, &["-f", "lavfi", "-i", "color=c=red:s=320x180:r=30:d=10", "-f", "lavfi", "-i", "sine=frequency=440:sample_rate=48000:duration=10", "-c:v", "libx264", "-pix_fmt", "yuv420p", "-c:a", "aac", "-shortest"], &top);
        fixture(&ffmpeg, &["-f", "lavfi", "-i", "color=c=blue:s=320x180:r=30:d=1", "-c:v", "libx264", "-pix_fmt", "yuv420p", "-an"], &bottom);
        fixture(&ffmpeg, &["-f", "lavfi", "-i", "color=c=lime:s=320x60:r=30:d=1,drawbox=x=0:y=0:w=iw:h=ih:color=magenta:t=fill:enable='gte(t,0.5)'", "-f", "lavfi", "-i", "sine=frequency=1000:sample_rate=48000:duration=1", "-c:v", "libx264", "-pix_fmt", "yuv420p", "-c:a", "aac", "-shortest"], &banner);
        fixture(&ffmpeg, &["-f", "lavfi", "-i", "color=c=0x00ff00@0.5:s=320x60:r=30,format=rgba", "-frames:v", "1", "-c:v", "png", "-update", "1"], &still);
        let srt = dir.join("fixture.srt"); fs::write(&srt, "1\n00:00:00,500 --> 00:00:01,250\nЧёткие\n\n2\n00:00:01,350 --> 00:00:02,600\nсубтитры\n\n3\n00:00:02,700 --> 00:00:03,150\nна\n\n4\n00:00:03,250 --> 00:00:04,500\nстыке\n").unwrap();
        let settings = Settings { count: 1, resolution: 720, banner_height: 240, banner_bottom: 180, banner_path: banner.to_string_lossy().into_owned(), subtitles_enabled: true, output_dir: dir.to_string_lossy().into_owned(), ..settings() };
        let clip = ClipPlan { index: 1, top_path: top.to_string_lossy().into_owned(), top_start: 0.0, bottom_path: bottom.to_string_lossy().into_owned(), bottom_start: 0.0, duration: 5.0, output_path: dir.join("custom-loop-subtitles.mp4").to_string_lossy().into_owned() };
        let _ = fs::remove_file(&clip.output_path);
        render_clip_inner(&ffmpeg, Path::new("/missing-fallback.png"), &settings, &clip, &RenderControl::new(), Some(&srt), |_| {}).unwrap();
        let info = inspect_stream(&ffmpeg, Path::new(&clip.output_path)).unwrap(); assert_eq!((info.width, info.height), (720, 1280)); assert!((info.duration - 5.0).abs() < 0.1);
        let g = geometry(&settings);
        let frame = |video: &Path, time: &str| {
            let output = Command::new(&ffmpeg).args(["-hide_banner", "-loglevel", "error", "-ss", time, "-i"]).arg(video).args(["-frames:v", "1", "-f", "rawvideo", "-pix_fmt", "rgb24", "pipe:1"]).output().unwrap(); assert!(output.status.success()); assert_eq!(output.stdout.len(), (g.width * g.height * 3) as usize); output.stdout
        };
        for (time, green) in [("0.25", true), ("0.75", false), ("1.25", true), ("1.75", false), ("4.25", true)] {
            let bytes = frame(Path::new(&clip.output_path), time); let offset = (((g.top + g.bottom + g.banner / 2) * g.width + g.width / 2) * 3) as usize; let color = &bytes[offset..offset + 3];
            if green { assert!(color[1] > 170 && color[0] < 60 && color[2] < 60, "loop at {time}: {color:?}"); }
            else { assert!(color[0] > 170 && color[1] < 60 && color[2] > 170, "loop at {time}: {color:?}"); }
        }
        let bytes = frame(Path::new(&clip.output_path), "2.25");
        let white_count = |start: u32, end: u32| (start..end).flat_map(|y| (0..g.width).map(move |x| ((y * g.width + x) * 3) as usize)).filter(|offset| bytes[*offset..*offset + 3].iter().all(|value| *value > 190)).count();
        assert!(white_count(g.top - 60, g.top) > 70, "caption reaches above seam"); assert!(white_count(g.top, g.top + 60) > 70, "caption reaches below seam");
        let still_settings = Settings { banner_path: still.to_string_lossy().into_owned(), subtitles_enabled: false, top_volume: 0.0, bottom_volume: 0.0, ..settings.clone() };
        let still_clip = ClipPlan { output_path: dir.join("transparent-still.mp4").to_string_lossy().into_owned(), ..clip.clone() }; let _ = fs::remove_file(&still_clip.output_path);
        render_clip(&ffmpeg, Path::new("/missing-fallback.png"), &still_settings, &still_clip, &RenderControl::new(), |_| {}).unwrap();
        let bytes = frame(Path::new(&still_clip.output_path), "4.25"); let offset = (((g.top + g.bottom + g.banner / 2) * g.width + g.width / 2) * 3) as usize; let color = &bytes[offset..offset + 3];
        assert!(color[1] > 100 && color[1] < 180 && color[0] < 30 && color[2] < 30, "transparent image stays visible and blended through end: {color:?}");
        let silence = Command::new(&ffmpeg).args(["-hide_banner", "-i"]).arg(&still_clip.output_path).args(["-af", "volumedetect", "-vn", "-f", "null", "-"]).output().unwrap();
        assert!(silence.status.success()); assert!(String::from_utf8_lossy(&silence.stderr).contains("max_volume: -91.0 dB"), "custom banner audio is never mixed");
        for output in [&clip.output_path, &still_clip.output_path] { let decoded = Command::new(&ffmpeg).args(["-hide_banner", "-loglevel", "error", "-i", output, "-f", "null", "-"]).status().unwrap(); assert!(decoded.success()); }
        assert!(!fs::read_dir(dir.join(".graffin-subtitle-cache")).unwrap().any(|entry| entry.unwrap().path().extension().and_then(|v| v.to_str()) == Some("ass")));
        if retained.is_none() { fs::remove_dir_all(&dir).unwrap(); }
    }

    #[test]
    #[ignore = "requires genuine Whisper word SRT, Russian audio and bundled FFmpeg"]
    fn ffmpeg_measured_word_animation_smoke() {
        let ffmpeg = PathBuf::from(std::env::var("BARSIK_FFMPEG").unwrap());
        let dir = PathBuf::from(std::env::var("GRAFFIN_WORD_OUTPUT").unwrap()); fs::create_dir_all(&dir).unwrap();
        let srt = PathBuf::from(std::env::var("GRAFFIN_WORD_SRT").unwrap());
        let audio = PathBuf::from(std::env::var("GRAFFIN_WORD_AUDIO").unwrap());
        let top = dir.join("speech-top.mp4"); let bottom = dir.join("speech-bottom.mp4");
        fixture(&ffmpeg, &["-f", "lavfi", "-i", "color=c=0x21313b:s=360x320:r=30:d=3.1", "-i", audio.to_str().unwrap(), "-c:v", "libx264", "-c:a", "aac", "-af", "apad", "-t", "3.1", "-pix_fmt", "yuv420p"], &top);
        fixture(&ffmpeg, &["-f", "lavfi", "-i", "color=c=0x33343b:s=360x320:r=30:d=3.1", "-c:v", "libx264", "-pix_fmt", "yuv420p", "-an"], &bottom);
        let settings = Settings { resolution: 720, duration: 5.0, subtitles_enabled: true, subtitle_size: 72, banner_enabled: false, banner_height: 0, banner_bottom: 0, output_dir: dir.to_string_lossy().into_owned(), ..settings() };
        let clip = ClipPlan { index: 1, top_path: top.to_string_lossy().into_owned(), top_start: 0.0, bottom_path: bottom.to_string_lossy().into_owned(), bottom_start: 0.0, duration: 3.1, output_path: dir.join("Russian_Animated_Words.mp4").to_string_lossy().into_owned() };
        let _ = fs::remove_file(&clip.output_path);
        render_clip_inner(&ffmpeg, Path::new("/unused.png"), &settings, &clip, &RenderControl::new(), Some(&srt), |_| {}).unwrap();
        let cache = dir.join(".graffin-subtitle-cache");
        let styled = style_subtitles(&settings, 3.1, &srt, &cache, &ffmpeg.parent().unwrap().join("fonts/Manrope-ExtraBold.ttf")).unwrap().unwrap();
        fs::copy(&styled.path, dir.join("measured-words.ass")).unwrap();
        let probe = Command::new(&ffmpeg).args(["-hide_banner", "-loglevel", "verbose", "-f", "lavfi", "-i", "color=s=720x1280:r=30:d=0.4", "-vf", &format!("ass=filename={}:fontsdir={}", styled.path.file_name().unwrap().to_str().unwrap(), subtitle_font_dir(&settings.subtitle_font)), "-f", "null", "-"]).current_dir(&cache).output().unwrap();
        let font_log = String::from_utf8_lossy(&probe.stderr); fs::write(dir.join("font-select.log"), font_log.as_bytes()).unwrap();
        assert!(probe.status.success()); assert!(font_log.contains("Manrope-ExtraBold"), "bundled font must be selected: {font_log}");
        let g = geometry(&settings);
        let bbox = |time: &str| {
            let output = Command::new(&ffmpeg).args(["-hide_banner", "-loglevel", "error", "-ss", time, "-i", &clip.output_path, "-frames:v", "1", "-f", "rawvideo", "-pix_fmt", "rgb24", "pipe:1"]).output().unwrap(); assert!(output.status.success());
            let mut xs = Vec::new(); let mut ys = Vec::new();
            for y in g.top - 80..g.top + 80 { for x in 0..g.width { let offset = ((y * g.width + x) * 3) as usize; if output.stdout[offset..offset+3].iter().all(|v| *v > 190) { xs.push(x); ys.push(y); } } }
            if xs.is_empty() { return (0, 0, 0); }
            (xs.iter().max().unwrap() - xs.iter().min().unwrap() + 1, ys.iter().max().unwrap() - ys.iter().min().unwrap() + 1, xs.len())
        };
        let rising = bbox("0.100"); let peak = bbox("0.134"); let settled = bbox("0.267");
        fs::write(dir.join("animation-bounds.json"), serde_json::json!({"rising":rising,"peak":peak,"settled":settled,"silence":bbox("3.050")}).to_string()).unwrap();
        assert!(peak.0 > settled.0 && peak.1 >= settled.1 && peak.2 > settled.2, "pop settles back: {rising:?} {peak:?} {settled:?}");
        assert!(rising.0 < peak.0, "word grows into overshoot"); assert_eq!(bbox("3.050"), (0,0,0));
        let decoded = Command::new(&ffmpeg).args(["-hide_banner", "-loglevel", "error", "-i", &clip.output_path, "-f", "null", "-"]).status().unwrap(); assert!(decoded.success());
    }

    /// Run explicitly with BARSIK_FFMPEG=/absolute/ffmpeg cargo test
    /// --lib engine::tests::ffmpeg_smoke -- --ignored --nocapture.
    #[test]
    #[ignore = "requires the bundled FFmpeg executable"]
    fn ffmpeg_smoke() {
        let ffmpeg = PathBuf::from(std::env::var("BARSIK_FFMPEG").expect("set BARSIK_FFMPEG"));
        let dir = tempdir("smoke");
        let top = dir.join("top.mp4"); let bottom = dir.join("bottom.mp4"); let banner = dir.join("banner.png");
        fixture(&ffmpeg, &["-f", "lavfi", "-i", "color=c=red:s=320x180:r=30:d=10", "-f", "lavfi", "-i", "sine=frequency=440:sample_rate=48000:duration=10", "-c:v", "libx264", "-pix_fmt", "yuv420p", "-c:a", "aac", "-shortest"], &top);
        fixture(&ffmpeg, &["-f", "lavfi", "-i", "color=c=blue:s=320x180:r=30:d=1", "-c:v", "libx264", "-pix_fmt", "yuv420p", "-an"], &bottom);
        fixture(&ffmpeg, &["-f", "lavfi", "-i", "color=c=green@0.8:s=300x60:r=10:d=0.4,format=rgba", "-c:v", "apng", "-plays", "0", "-f", "apng"], &banner);
        let request = BatchRequest { top: vec![probe(&ffmpeg, &top, &dir.join("cache")).unwrap()], bottom: vec![probe(&ffmpeg, &bottom, &dir.join("cache")).unwrap()], banner_path: banner.to_string_lossy().into_owned(), settings: Settings { resolution: 720, output_dir: dir.to_string_lossy().into_owned(), ..settings() } };
        let clips = plan(&request).unwrap();
        for clip in &clips {
            let mut progress = vec![];
            render_clip(&ffmpeg, &banner, &request.settings, clip, &RenderControl::new(), |p| progress.push(p)).unwrap();
            assert_eq!(progress.last(), Some(&1.0));
            let info = inspect_stream(&ffmpeg, Path::new(&clip.output_path)).unwrap();
            assert_eq!((info.width, info.height), (720, 1280)); assert!(info.has_audio); assert!((info.duration - 5.0).abs() < 0.1);
            let decoded = Command::new(&ffmpeg).args(["-hide_banner", "-loglevel", "error", "-i"]).arg(&clip.output_path).args(["-f", "null", "-"]).status().unwrap(); assert!(decoded.success());
            let output = Command::new(&ffmpeg).args(["-hide_banner", "-loglevel", "error", "-i"]).arg(&clip.output_path).args(["-frames:v", "1", "-vf", "scale=72:128", "-f", "rawvideo", "-pix_fmt", "rgb24", "pipe:1"]).output().unwrap(); assert!(output.status.success());
            let pixel = |x: usize, y: usize| &output.stdout[(y*72+x)*3..(y*72+x)*3+3];
            let a = pixel(36, 55); let b = pixel(36, 56); let c = pixel(36, 120);
            assert!(a[0] > 150 && a[2] < 50, "top reaches seam: {a:?}");
            assert!(b[2] > 150 && b[0] < 50, "bottom begins at seam: {b:?}");
            assert!(c[1] > c[0] + 40 && c[1] > c[2] + 40, "banner occupies its own region: {c:?}");
        }
        // Both completely silent sources still export an explicit silent AAC
        // track; no missing-stream errors and no unexpected banner audio.
        let silent = ClipPlan { top_path: bottom.to_string_lossy().into_owned(), top_start: 0.0, bottom_path: bottom.to_string_lossy().into_owned(), bottom_start: 0.0, duration: 0.5, output_path: dir.join("silent.mp4").to_string_lossy().into_owned(), index: 3 };
        render_clip(&ffmpeg, &banner, &request.settings, &silent, &RenderControl::new(), |_| {}).unwrap();
        assert!(inspect_stream(&ffmpeg, Path::new(&silent.output_path)).unwrap().has_audio);
        // Both audio tracks mixed, and hiding the banner preserves its strip.
        let mixed = ClipPlan { top_path: top.to_string_lossy().into_owned(), bottom_path: top.to_string_lossy().into_owned(), output_path: dir.join("mixed.mp4").to_string_lossy().into_owned(), ..silent.clone() };
        let mix_settings = Settings { bottom_volume: 0.4, banner_enabled: false, fit_mode: "contain".into(), ..request.settings.clone() };
        render_clip(&ffmpeg, Path::new("/missing-hidden-banner.png"), &mix_settings, &mixed, &RenderControl::new(), |_| {}).unwrap();
        let info = inspect_stream(&ffmpeg, Path::new(&mixed.output_path)).unwrap();
        assert_eq!((info.width, info.height), (720, 1280)); assert!(info.has_audio);
        let with_footer = ClipPlan { top_path: top.to_string_lossy().into_owned(), bottom_path: bottom.to_string_lossy().into_owned(), output_path: dir.join("with_footer.mp4").to_string_lossy().into_owned(), ..silent.clone() };
        let footer_settings = Settings { banner_bottom: 240, ..request.settings.clone() };
        render_clip(&ffmpeg, &banner, &footer_settings, &with_footer, &RenderControl::new(), |_| {}).unwrap();
        let frame = Command::new(&ffmpeg).args(["-hide_banner", "-loglevel", "error", "-i"]).arg(&with_footer.output_path).args(["-frames:v", "1", "-vf", "scale=72:128", "-f", "rawvideo", "-pix_fmt", "rgb24", "pipe:1"]).output().unwrap(); assert!(frame.status.success());
        let pixel = |x: usize, y: usize| &frame.stdout[(y*72+x)*3..(y*72+x)*3+3];
        let top_end = pixel(36, 47); let bottom_start = pixel(36, 48); let art = pixel(36, 104); let footer = pixel(36, 120);
        assert!(top_end[0] > 150 && top_end[2] < 50); assert!(bottom_start[2] > 150 && bottom_start[0] < 50);
        assert!(art[1] > art[0] + 40 && art[1] > art[2] + 40);
        assert!(footer.iter().all(|v| *v < 30), "footer remains blank: {footer:?}");
        // A zero-height banner still leaves the independent footer untouched.
        let no_art = ClipPlan { output_path: dir.join("footer_only.mp4").to_string_lossy().into_owned(), ..with_footer.clone() };
        render_clip(&ffmpeg, Path::new("/missing-zero-height-banner.png"), &Settings { banner_height: 0, ..footer_settings }, &no_art, &RenderControl::new(), |_| {}).unwrap();
        assert_eq!(inspect_stream(&ffmpeg, Path::new(&no_art.output_path)).unwrap().height, 1280);
        let cancelled = ClipPlan { output_path: dir.join("cancelled.mp4").to_string_lossy().into_owned(), ..clips[0].clone() };
        let control = RenderControl::new(); let handle = control.clone();
        let result = render_clip(&ffmpeg, &banner, &request.settings, &cancelled, &control, move |_| handle.cancel());
        assert!(result.is_err()); assert!(!Path::new(&cancelled.output_path).exists());
        assert!(!fs::read_dir(&dir).unwrap().any(|p| p.unwrap().file_name().to_string_lossy().contains("partial")));
        fs::remove_dir_all(&dir).unwrap();
    }
}
