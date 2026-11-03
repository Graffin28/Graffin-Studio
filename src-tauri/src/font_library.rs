//! Persistent, content-addressed custom fonts. Settings contain an ID, never a
//! user-controlled path; only verified files inside the application library are
//! handed to the subtitle renderer or the asset protocol.
use serde::Serialize;
use sha2::{Digest, Sha256};
use std::{collections::HashSet, fs, io::{Read, Write}, path::{Path, PathBuf}, sync::{OnceLock, atomic::{AtomicU64, Ordering}}};
use tauri::{AppHandle, Manager};

const MAX_FONT_BYTES: u64 = 32 * 1024 * 1024;
static LIBRARY_ROOT: OnceLock<PathBuf> = OnceLock::new();
static IMPORT_SEQUENCE: AtomicU64 = AtomicU64::new(0);

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FontEntry { pub id: String, pub name: String, pub family: String, pub path: String }

struct FontInfo { family: String, name: String, extension: &'static str }
fn invalid() -> String { "Повреждённый шрифт. Выберите корректный файл TTF или OTF.".into() }
fn u16_at(bytes: &[u8], at: usize) -> Result<u16, String> {
    let value = bytes.get(at..at.checked_add(2).ok_or_else(invalid)?).ok_or_else(invalid)?;
    Ok(u16::from_be_bytes([value[0], value[1]]))
}
fn u32_at(bytes: &[u8], at: usize) -> Result<u32, String> {
    let value = bytes.get(at..at.checked_add(4).ok_or_else(invalid)?).ok_or_else(invalid)?;
    Ok(u32::from_be_bytes([value[0], value[1], value[2], value[3]]))
}

fn decode_name(platform: u16, bytes: &[u8]) -> Option<String> {
    let value = if platform == 0 || platform == 3 {
        if bytes.len() % 2 != 0 { return None; }
        String::from_utf16(&bytes.chunks_exact(2).map(|x| u16::from_be_bytes([x[0], x[1]])).collect::<Vec<_>>()).ok()?
    } else if platform == 1 && bytes.is_ascii() { String::from_utf8(bytes.to_vec()).ok()? } else { return None; };
    let value = value.trim().to_string();
    if value.is_empty() || value.chars().count() > 180 || value.chars().any(char::is_control) { None } else { Some(value) }
}

fn parse_font(bytes: &[u8]) -> Result<FontInfo, String> {
    if bytes.len() < 12 || bytes.len() as u64 > MAX_FONT_BYTES { return Err(invalid()); }
    let signature = bytes.get(..4).ok_or_else(invalid)?;
    let extension = if signature == b"OTTO" { "otf" } else if signature == [0, 1, 0, 0] || signature == b"true" { "ttf" } else { return Err(invalid()); };
    let count = usize::from(u16_at(bytes, 4)?);
    if count == 0 || count > 256 { return Err(invalid()); }
    let directory_end = 12 + count * 16;
    if directory_end > bytes.len() { return Err(invalid()); }
    let mut tags = HashSet::new();
    let mut tables = Vec::with_capacity(count);
    for i in 0..count {
        let at = 12 + i * 16;
        let tag = &bytes[at..at + 4];
        let offset = u32_at(bytes, at + 8)? as usize;
        let length = u32_at(bytes, at + 12)? as usize;
        if !tags.insert(tag) || offset < directory_end || offset % 4 != 0 { return Err(invalid()); }
        let end = offset.checked_add(length).ok_or_else(invalid)?;
        if end > bytes.len() || tables.iter().any(|(_, start, previous_end): &(&[u8], usize, usize)| length > 0 && *start < end && offset < *previous_end) { return Err(invalid()); }
        tables.push((tag, offset, end));
    }
    let table = |tag: &[u8]| -> Result<&[u8], String> {
        let (_, start, end) = tables.iter().find(|(key, _, _)| *key == tag).ok_or_else(invalid)?;
        Ok(&bytes[*start..*end])
    };
    let head = table(b"head")?;
    let units = u16_at(head, 18)?;
    if head.len() < 54 || u32_at(head, 12)? != 0x5f0f3cf5 || !(16..=16384).contains(&units) { return Err(invalid()); }
    let glyphs = usize::from(u16_at(table(b"maxp")?, 4)?);
    let metrics = usize::from(u16_at(table(b"hhea")?, 34)?);
    if glyphs == 0 || metrics == 0 || metrics > glyphs || table(b"hmtx")?.len() < metrics * 4 + (glyphs - metrics) * 2 { return Err(invalid()); }
    let cmap = table(b"cmap")?;
    let maps = usize::from(u16_at(cmap, 2)?);
    if u16_at(cmap, 0)? != 0 || maps == 0 || maps > 256 || 4 + maps * 8 > cmap.len() { return Err(invalid()); }
    for i in 0..maps {
        let offset = u32_at(cmap, 8 + i * 8)? as usize;
        let format = u16_at(cmap, offset)?;
        let length = match format { 0 | 2 | 4 | 6 => usize::from(u16_at(cmap, offset + 2)?), 8 | 10 | 12 | 13 => u32_at(cmap, offset + 4)? as usize, 14 => u32_at(cmap, offset + 2)? as usize, _ => return Err(invalid()) };
        if length < 6 || offset.checked_add(length).filter(|end| *end <= cmap.len()).is_none() { return Err(invalid()); }
    }
    if extension == "ttf" {
        let glyf = table(b"glyf")?; let loca = table(b"loca")?;
        let long = match u16_at(head, 50)? { 0 => false, 1 => true, _ => return Err(invalid()) };
        let stride = if long { 4 } else { 2 };
        if loca.len() < (glyphs + 1) * stride { return Err(invalid()); }
        let mut previous = 0;
        for i in 0..=glyphs {
            let offset = if long { u32_at(loca, i * 4)? as usize } else { usize::from(u16_at(loca, i * 2)?) * 2 };
            if offset < previous || offset > glyf.len() { return Err(invalid()); }
            previous = offset;
        }
    } else {
        let cff = table(b"CFF ").or_else(|_| table(b"CFF2"))?;
        if cff.len() < 4 || ![1, 2].contains(&cff[0]) || usize::from(cff[2]) > cff.len() { return Err(invalid()); }
    }
    let names = table(b"name")?;
    if u16_at(names, 0)? > 1 { return Err(invalid()); }
    let records = usize::from(u16_at(names, 2)?); let storage = usize::from(u16_at(names, 4)?);
    if records > 4096 || 6 + records * 12 > names.len() || storage < 6 + records * 12 || storage > names.len() { return Err(invalid()); }
    let mut decoded = Vec::new();
    for i in 0..records {
        let at = 6 + i * 12;
        let platform = u16_at(names, at)?; let language = u16_at(names, at + 4)?; let id = u16_at(names, at + 6)?;
        let length = usize::from(u16_at(names, at + 8)?); let start = storage + usize::from(u16_at(names, at + 10)?);
        let end = start.checked_add(length).ok_or_else(invalid)?;
        let raw = names.get(start..end).ok_or_else(invalid)?;
        if let Some(value) = decode_name(platform, raw) {
            let priority = if platform == 3 && [0, 0x409].contains(&language) { 0 } else if platform == 0 { 1 } else if platform == 3 { 2 } else { 3 };
            decoded.push((id, priority, value));
        }
    }
    decoded.sort_by_key(|(_, priority, _)| *priority);
    let get = |ids: &[u16]| ids.iter().find_map(|id| decoded.iter().find(|(key, _, _)| key == id).map(|(_, _, value)| value.clone()));
    // The legacy family is the exact face name used by libass/fontconfig for
    // static weights such as ExtraBold; use the typographic family as fallback.
    let family = get(&[1, 16]).ok_or_else(invalid)?;
    if family.contains([',', '{', '}', '\\']) { return Err("Имя семейства шрифта содержит неподдерживаемые символы.".into()); }
    let name = get(&[4, 1, 16]).unwrap_or_else(|| family.clone());
    Ok(FontInfo { family, name, extension })
}

fn valid_id(id: &str) -> bool {
    id.strip_prefix("custom-").is_some_and(|hash| hash.len() == 64 && hash.bytes().all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b)))
}
fn digest(bytes: &[u8]) -> String { format!("custom-{:x}", Sha256::digest(bytes)) }

// Widths are in em units, just like the bundled subtitle metrics. Keep only the
// two Unicode cmap formats used by modern TrueType/OpenType fonts; other maps
// safely use the widest advance instead of guessing from a different family.
struct FontMetrics {
    units: f64,
    advances: Vec<u16>,
    fallback: f64,
    cmap4: Option<Vec<u8>>,
    cmap12: Option<Vec<(u32, u32, u32)>>,
}
fn font_table<'a>(bytes: &'a [u8], tag: &[u8]) -> Result<&'a [u8], String> {
    for i in 0..usize::from(u16_at(bytes, 4)?) {
        let at = 12 + i * 16;
        if bytes.get(at..at + 4) == Some(tag) {
            let start = u32_at(bytes, at + 8)? as usize;
            let end = start.checked_add(u32_at(bytes, at + 12)? as usize).ok_or_else(invalid)?;
            return bytes.get(start..end).ok_or_else(invalid);
        }
    }
    Err(invalid())
}
impl FontMetrics {
    fn parse(bytes: &[u8]) -> Result<Self, String> {
        parse_font(bytes)?;
        let units = f64::from(u16_at(font_table(bytes, b"head")?, 18)?);
        let glyphs = usize::from(u16_at(font_table(bytes, b"maxp")?, 4)?);
        let hhea = font_table(bytes, b"hhea")?;
        let count = usize::from(u16_at(hhea, 34)?);
        let hmtx = font_table(bytes, b"hmtx")?;
        let mut advances = Vec::with_capacity(glyphs);
        for i in 0..glyphs { advances.push(u16_at(hmtx, i.min(count - 1) * 4)?); }
        let widest = advances.iter().copied().max().unwrap_or(0).max(u16_at(hhea, 10)?);
        let fallback = (f64::from(widest) / units).max(1.1);
        let cmap = font_table(bytes, b"cmap")?;
        let mut candidates = Vec::new();
        for i in 0..usize::from(u16_at(cmap, 2)?) {
            let platform = u16_at(cmap, 4 + i * 8)?;
            let encoding = u16_at(cmap, 6 + i * 8)?;
            if platform != 0 && !(platform == 3 && [1, 10].contains(&encoding)) { continue; }
            let offset = u32_at(cmap, 8 + i * 8)? as usize;
            let format = u16_at(cmap, offset)?;
            if ![4, 12].contains(&format) { continue; }
            let length = if format == 4 { usize::from(u16_at(cmap, offset + 2)?) } else { u32_at(cmap, offset + 4)? as usize };
            let end = offset.checked_add(length).ok_or_else(invalid)?;
            let map = cmap.get(offset..end).ok_or_else(invalid)?;
            candidates.push((if platform == 3 { 0 } else { 1 }, format, map));
        }
        candidates.sort_by_key(|(priority, _, _)| *priority);
        let mut cmap4 = None;
        let mut cmap12 = None;
        for (_, format, map) in candidates {
            if format == 4 && cmap4.is_none() {
                let twice = usize::from(u16_at(map, 6)?);
                let segments = twice / 2;
                if twice % 2 != 0 || segments == 0 || 16 + segments * 8 > map.len() { return Err(invalid()); }
                let mut previous = None;
                for i in 0..segments {
                    let end = u16_at(map, 14 + i * 2)?;
                    let start = u16_at(map, 16 + segments * 2 + i * 2)?;
                    let at = 16 + segments * 6 + i * 2;
                    let range = usize::from(u16_at(map, at)?);
                    if start > end || previous.is_some_and(|last| start <= last) { return Err(invalid()); }
                    if range != 0 && (range % 2 != 0 || at + range < 16 + segments * 8 || at + range + usize::from(end - start) * 2 + 2 > map.len()) { return Err(invalid()); }
                    previous = Some(end);
                }
                cmap4 = Some(map.to_vec());
            } else if format == 12 && cmap12.is_none() {
                let count = u32_at(map, 12)? as usize;
                if 16usize.checked_add(count.checked_mul(12).ok_or_else(invalid)?).ok_or_else(invalid)? > map.len() { return Err(invalid()); }
                // Bound memory even for an unusually fragmented or hostile cmap.
                // Larger maps retain the safe max-advance fallback.
                if count > 65_536 { continue; }
                let mut groups = Vec::with_capacity(count);
                let mut previous = None;
                for i in 0..count {
                    let at = 16 + i * 12;
                    let start = u32_at(map, at)?;
                    let end = u32_at(map, at + 4)?;
                    let glyph = u32_at(map, at + 8)?;
                    if start > end || end > 0x10ffff || previous.is_some_and(|last| start <= last) || glyph.checked_add(end - start).is_none() { return Err(invalid()); }
                    groups.push((start, end, glyph)); previous = Some(end);
                }
                cmap12 = Some(groups);
            }
        }
        Ok(Self { units, advances, fallback, cmap4, cmap12 })
    }
    fn glyph(&self, character: char) -> Option<usize> {
        let code = u32::from(character);
        if let Some(groups) = &self.cmap12 {
            let index = groups.partition_point(|(_, end, _)| *end < code);
            if let Some(&(start, end, first)) = groups.get(index) {
                if (start..=end).contains(&code) {
                    let glyph = (first + (code - start)) as usize;
                    if glyph != 0 && glyph < self.advances.len() { return Some(glyph); }
                }
            }
        }
        let code = u16::try_from(code).ok()?;
        let map = self.cmap4.as_ref()?;
        let segments = usize::from(u16_at(map, 6).ok()?) / 2;
        let mut low = 0;
        let mut high = segments;
        while low < high {
            let mid = low + (high - low) / 2;
            if u16_at(map, 14 + mid * 2).ok()? < code { low = mid + 1; } else { high = mid; }
        }
        if low == segments || code < u16_at(map, 16 + segments * 2 + low * 2).ok()? { return None; }
        let delta = u16_at(map, 16 + segments * 4 + low * 2).ok()?;
        let at = 16 + segments * 6 + low * 2;
        let range = usize::from(u16_at(map, at).ok()?);
        let glyph = if range == 0 { code.wrapping_add(delta) } else {
            let start = u16_at(map, 16 + segments * 2 + low * 2).ok()?;
            let glyph = u16_at(map, at + range + usize::from(code - start) * 2).ok()?;
            if glyph == 0 { return None; }
            glyph.wrapping_add(delta)
        };
        (glyph != 0 && usize::from(glyph) < self.advances.len()).then_some(usize::from(glyph))
    }
    fn word_width(&self, text: &str) -> f64 {
        text.chars().map(|character| self.glyph(character).map_or(self.fallback, |glyph| f64::from(self.advances[glyph]) / self.units)).sum::<f64>().max(0.1)
    }
}
fn metrics_in(root: &Path, id: &str) -> Result<FontMetrics, String> {
    let font = resolve_in(root, id)?;
    let bytes = read_font(Path::new(&font.path))?;
    if digest(&bytes) != id { return Err(invalid()); }
    FontMetrics::parse(&bytes)
}
pub(crate) fn word_width(id: &str, text: &str) -> Option<f64> {
    use std::{collections::HashMap, sync::Mutex};
    static METRICS: OnceLock<Mutex<HashMap<String, FontMetrics>>> = OnceLock::new();
    if !valid_id(id) { return None; }
    let mut cache = METRICS.get_or_init(|| Mutex::new(HashMap::new())).lock().ok()?;
    if let Some(metrics) = cache.get(id) { return Some(metrics.word_width(text)); }
    let metrics = metrics_in(LIBRARY_ROOT.get()?, id).ok()?;
    let width = metrics.word_width(text);
    // Each entry retains at most one 64 KB cmap4 and 65,536 cmap12 groups.
    // Content-addressed IDs make cached metrics immutable across imports.
    if cache.len() >= 8 { if let Some(key) = cache.keys().next().cloned() { cache.remove(&key); } }
    cache.insert(id.into(), metrics);
    Some(width)
}
fn read_font(path: &Path) -> Result<Vec<u8>, String> {
    let metadata = fs::symlink_metadata(path).map_err(|_| "Шрифт недоступен. Загрузите его ещё раз.".to_string())?;
    if !metadata.is_file() || metadata.len() < 12 || metadata.len() > MAX_FONT_BYTES { return Err("Выберите шрифт TTF или OTF размером до 32 МБ.".into()); }
    let mut file = fs::File::open(path).map_err(|e| e.to_string())?;
    let mut bytes = Vec::with_capacity(metadata.len() as usize);
    Read::by_ref(&mut file).take(MAX_FONT_BYTES + 1).read_to_end(&mut bytes).map_err(|e| e.to_string())?;
    if bytes.len() as u64 > MAX_FONT_BYTES { return Err("Шрифт больше 32 МБ.".into()); }
    Ok(bytes)
}
fn root_dir(root: &Path) -> Result<PathBuf, String> {
    fs::create_dir_all(root).map_err(|e| format!("Не создать библиотеку шрифтов: {e}"))?;
    if fs::symlink_metadata(root).map_err(|e| e.to_string())?.file_type().is_symlink() { return Err("Библиотека шрифтов недоступна.".into()); }
    root.canonicalize().map_err(|e| e.to_string())
}
fn resolve_in(root: &Path, id: &str) -> Result<FontEntry, String> {
    if !valid_id(id) { return Err("Неизвестный шрифт. Выберите шрифт из библиотеки.".into()); }
    let root = root_dir(root)?;
    for extension in ["ttf", "otf"] {
        let path = root.join(format!("{id}.{extension}"));
        if !path.exists() { continue; }
        let bytes = read_font(&path)?;
        if digest(&bytes) != id { return Err("Шрифт в библиотеке повреждён. Загрузите его ещё раз.".into()); }
        let info = parse_font(&bytes)?;
        if extension != info.extension { return Err(invalid()); }
        let canonical = path.canonicalize().map_err(|e| e.to_string())?;
        if canonical.parent() != Some(root.as_path()) { return Err("Шрифт находится за пределами библиотеки.".into()); }
        return Ok(FontEntry { id: id.into(), name: info.name, family: info.family, path: canonical.to_string_lossy().into_owned() });
    }
    Err("Шрифт не найден в библиотеке. Загрузите его ещё раз.".into())
}
fn import_in(root: &Path, source: &Path) -> Result<FontEntry, String> {
    let extension = source.extension().and_then(|v| v.to_str()).unwrap_or("").to_ascii_lowercase();
    if !["ttf", "otf"].contains(&extension.as_str()) { return Err("Поддерживаются шрифты TTF и OTF.".into()); }
    let bytes = read_font(source)?; let info = parse_font(&bytes)?;
    let id = digest(&bytes); let root = root_dir(root)?; let target = root.join(format!("{id}.{}", info.extension));
    // Reimport repairs a damaged library copy. Atomic replacement also replaces
    // a malicious symlink itself rather than following its destination.
    if resolve_in(&root, &id).is_err() {
        let temporary = root.join(format!(".{id}-{}-{}-{}.partial", std::process::id(), crate::stamp(), IMPORT_SEQUENCE.fetch_add(1, Ordering::Relaxed)));
        let mut options = fs::OpenOptions::new(); options.write(true).create_new(true);
        #[cfg(unix)] { use std::os::unix::fs::OpenOptionsExt; options.mode(0o600); }
        let mut file = options.open(&temporary).map_err(|e| e.to_string())?;
        if let Err(error) = file.write_all(&bytes).and_then(|_| file.sync_all()).and_then(|_| fs::rename(&temporary, &target)) {
            let _ = fs::remove_file(&temporary); return Err(format!("Не сохранить шрифт: {error}"));
        }
    }
    resolve_in(&root, &id)
}
fn library_in(root: &Path) -> Result<Vec<FontEntry>, String> {
    let root = root_dir(root)?; let mut fonts = Vec::new(); let mut seen = HashSet::new();
    for item in fs::read_dir(&root).map_err(|e| e.to_string())? {
        let Ok(item) = item else { continue; }; let path = item.path();
        let Some(id) = path.file_stem().and_then(|v| v.to_str()) else { continue; };
        if valid_id(id) && seen.insert(id.to_string()) { if let Ok(font) = resolve_in(&root, id) { fonts.push(font); } }
    }
    fonts.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()).then_with(|| a.id.cmp(&b.id)));
    Ok(fonts)
}

pub(crate) fn configure(app: &AppHandle) -> Result<(), String> {
    let root = app.path().app_data_dir().map_err(|e| e.to_string())?.join("fonts");
    let _ = LIBRARY_ROOT.set(root);
    Ok(())
}
pub(crate) fn resolve(id: &str) -> Result<FontEntry, String> {
    resolve_in(LIBRARY_ROOT.get().ok_or("Библиотека шрифтов ещё не открыта.")?, id)
}
fn authorize(app: &AppHandle, fonts: &[FontEntry]) -> Result<(), String> {
    for font in fonts { app.asset_protocol_scope().allow_file(&font.path).map_err(|e| e.to_string())?; }
    Ok(())
}
#[tauri::command]
pub async fn font_library(app: AppHandle) -> Result<Vec<FontEntry>, String> {
    tauri::async_runtime::spawn_blocking(move || {
        configure(&app)?;
        let fonts = library_in(LIBRARY_ROOT.get().ok_or("Библиотека шрифтов ещё не открыта.")?)?;
        authorize(&app, &fonts)?; Ok(fonts)
    }).await.map_err(|e| e.to_string())?
}
#[tauri::command]
pub async fn import_font(app: AppHandle, path: String) -> Result<FontEntry, String> {
    tauri::async_runtime::spawn_blocking(move || {
        configure(&app)?;
        let font = import_in(LIBRARY_ROOT.get().ok_or("Библиотека шрифтов ещё не открыта.")?, Path::new(&path))?;
        authorize(&app, std::slice::from_ref(&font))?; Ok(font)
    }).await.map_err(|e| e.to_string())?
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};
    static SEQ: AtomicUsize = AtomicUsize::new(0);
    struct Scratch(PathBuf);
    impl Scratch { fn new() -> Self { let path = std::env::temp_dir().join(format!("graffin-font-test-{}-{}", std::process::id(), SEQ.fetch_add(1, Ordering::Relaxed))); fs::create_dir_all(&path).unwrap(); Self(path) } }
    impl Drop for Scratch { fn drop(&mut self) { let _ = fs::remove_dir_all(&self.0); } }
    fn fixture() -> &'static [u8] { include_bytes!("../resources/fonts/Manrope-ExtraBold.ttf") }
    #[test] fn bundled_fonts_parse() {
        for font in [fixture(), include_bytes!("../resources/fonts/Montserrat-ExtraBold.ttf"), include_bytes!("../resources/fonts/Rubik-Bold.ttf"), include_bytes!("../resources/fonts/Oswald-Bold.ttf"), include_bytes!("../resources/fonts/RussoOne-Regular.ttf")] { assert!(!parse_font(font).unwrap().family.is_empty()); }
        assert_eq!(parse_font(fixture()).unwrap().family, "Manrope ExtraBold");
    }
    #[test] fn font_metrics_match_bundled_width_catalogs() {
        for (bytes, catalog) in [
            (fixture(), include_str!("../resources/fonts/Manrope-ExtraBold-widths.json")),
            (include_bytes!("../resources/fonts/Montserrat-ExtraBold.ttf").as_slice(), include_str!("../resources/fonts/Montserrat-ExtraBold-widths.json")),
            (include_bytes!("../resources/fonts/Rubik-Bold.ttf").as_slice(), include_str!("../resources/fonts/Rubik-Bold-widths.json")),
            (include_bytes!("../resources/fonts/Oswald-Bold.ttf").as_slice(), include_str!("../resources/fonts/Oswald-Bold-widths.json")),
            (include_bytes!("../resources/fonts/RussoOne-Regular.ttf").as_slice(), include_str!("../resources/fonts/RussoOne-Regular-widths.json")),
        ] {
            let metrics = FontMetrics::parse(bytes).unwrap();
            let advances: std::collections::HashMap<char, f64> = serde_json::from_str(catalog).unwrap();
            for (character, expected) in advances {
                if character.is_control() { continue; }
                let Some(glyph) = metrics.glyph(character) else { continue; };
                let actual = f64::from(metrics.advances[glyph]) / metrics.units;
                assert!((actual - expected).abs() < 0.0006, "width differs for {character:?}");
            }
            assert!(metrics.glyph('Ш').is_some());
            assert!(metrics.word_width("Широкое") > metrics.word_width("iii"));
            assert_eq!(metrics.word_width(""), 0.1);
            assert!(metrics.word_width("\u{10ffff}") >= 1.1);
        }
    }
    fn with_cmap(cmap: &[u8]) -> Vec<u8> {
        let mut bytes = fixture().to_vec();
        while bytes.len() % 4 != 0 { bytes.push(0); }
        let offset = bytes.len(); bytes.extend_from_slice(cmap);
        for i in 0..usize::from(u16_at(&bytes, 4).unwrap()) {
            let at = 12 + i * 16;
            if &bytes[at..at + 4] == b"cmap" {
                bytes[at + 8..at + 12].copy_from_slice(&(offset as u32).to_be_bytes());
                bytes[at + 12..at + 16].copy_from_slice(&(cmap.len() as u32).to_be_bytes());
                break;
            }
        }
        bytes
    }
    fn cmap12(groups: &[(u32, u32, u32)]) -> Vec<u8> {
        let mut cmap = vec![0, 0, 0, 1, 0, 3, 0, 10, 0, 0, 0, 12, 0, 12, 0, 0];
        cmap.extend_from_slice(&((16 + groups.len() * 12) as u32).to_be_bytes());
        cmap.extend_from_slice(&0u32.to_be_bytes());
        cmap.extend_from_slice(&(groups.len() as u32).to_be_bytes());
        for &(start, end, glyph) in groups { for value in [start, end, glyph] { cmap.extend_from_slice(&value.to_be_bytes()); } }
        cmap
    }
    #[test] fn metrics_support_non_bmp_and_reject_bad_cmaps() {
        let original = FontMetrics::parse(fixture()).unwrap();
        let wide = original.glyph('W').unwrap() as u32;
        let bytes = with_cmap(&cmap12(&[(0x1f600, 0x1f601, wide)]));
        let metrics = FontMetrics::parse(&bytes).unwrap();
        assert_eq!(metrics.word_width("😀"), original.word_width("W"));
        assert_eq!(metrics.glyph('😁'), Some(wide as usize + 1));
        assert!(metrics.word_width("А") >= original.word_width("W"));
        for groups in [vec![(10, 9, 1)], vec![(0x110000, 0x110001, 1)], vec![(10, 20, 1), (20, 30, 2)], vec![(1, 2, u32::MAX)]] {
            assert!(FontMetrics::parse(&with_cmap(&cmap12(&groups))).is_err());
        }
        // An out-of-range glyph must take the fallback without arithmetic overflow.
        let metrics = FontMetrics::parse(&with_cmap(&cmap12(&[(0x1f600, 0x1f600, u32::MAX)]))).unwrap();
        assert_eq!(metrics.word_width("😀"), metrics.fallback);
        let mut broken = cmap12(&[(0x1f600, 0x1f600, wide)]);
        broken[24..28].copy_from_slice(&u32::MAX.to_be_bytes());
        assert!(FontMetrics::parse(&with_cmap(&broken)).is_err());
    }
    #[test] fn custom_metrics_resolve_from_library_and_reject_missing_or_unsafe_ids() {
        let temp = Scratch::new(); let source = temp.0.join("wide.ttf"); fs::write(&source, fixture()).unwrap();
        let root = temp.0.join("library"); let font = import_in(&root, &source).unwrap(); fs::remove_file(source).unwrap();
        let metrics = metrics_in(&root, &font.id).unwrap();
        assert_eq!(metrics.word_width("Широкое"), FontMetrics::parse(fixture()).unwrap().word_width("Широкое"));
        for id in ["../font", "/tmp/font.ttf", "custom-../font", "custom-", "custom-0000000000000000000000000000000000000000000000000000000000000000"] { assert!(metrics_in(&root, id).is_err()); }
        assert!(word_width("custom-../font", "test").is_none());
        fs::write(&font.path, b"tampered font").unwrap(); assert!(metrics_in(&root, &font.id).is_err());
    }
    #[cfg(target_os = "macos")]
    #[test] fn cff_otf_roundtrip() {
        let source = Path::new("/System/Library/Fonts/Supplemental/STIXGeneral.otf");
        // Supplemental system fonts are optional on a minimal macOS image.
        if !source.is_file() { return; }
        let temp = Scratch::new(); let font = import_in(&temp.0.join("library"), source).unwrap();
        assert!(font.path.ends_with(".otf")); assert!(!font.family.is_empty());
        assert_eq!(resolve_in(&temp.0.join("library"), &font.id).unwrap().name, font.name);
    }
    #[test] fn rejects_corruption_without_panicking() {
        for length in [0, 1, 4, 12, 50, 512, fixture().len() / 2] { assert!(parse_font(&fixture()[..length]).is_err()); }
        let mut damaged = fixture().to_vec(); damaged[20..24].copy_from_slice(&u32::MAX.to_be_bytes()); assert!(parse_font(&damaged).is_err());
        let mut damaged = fixture().to_vec(); damaged[0..4].copy_from_slice(b"wOF2"); assert!(parse_font(&damaged).is_err());
        let mut damaged = fixture().to_vec(); damaged[4..6].copy_from_slice(&u16::MAX.to_be_bytes()); assert!(parse_font(&damaged).is_err());
    }
    #[test] fn persistent_roundtrip_deduplicates_and_does_not_need_source() {
        let temp = Scratch::new(); let source = temp.0.join("My Font.ttf"); fs::write(&source, fixture()).unwrap();
        let root = temp.0.join("library"); let font = import_in(&root, &source).unwrap();
        assert!(valid_id(&font.id)); assert_eq!(font.family, "Manrope ExtraBold"); assert_eq!(import_in(&root, &source).unwrap().id, font.id);
        fs::remove_file(source).unwrap(); assert_eq!(library_in(&root).unwrap().len(), 1); assert_eq!(resolve_in(&root, &font.id).unwrap().path, font.path);
        fs::write(&font.path, b"tampered font").unwrap(); assert!(resolve_in(&root, &font.id).is_err()); assert!(library_in(&root).unwrap().is_empty());
        let source = temp.0.join("repair.ttf"); fs::write(&source, fixture()).unwrap();
        assert_eq!(import_in(&root, &source).unwrap().id, font.id); assert_eq!(library_in(&root).unwrap().len(), 1);
    }
    #[test] fn rejects_paths_and_invalid_ids() {
        let temp = Scratch::new();
        for id in ["../font", "/tmp/font.ttf", "custom-../font", "custom-", "manrope", "custom-AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA", "custom-0000000000000000000000000000000000000000000000000000000000000000/../x"] { assert!(resolve_in(&temp.0, id).is_err()); }
        let source = temp.0.join("bad.ttf"); fs::write(&source, b"not a font at all").unwrap(); assert!(import_in(&temp.0.join("library"), &source).is_err());
    }
    #[cfg(unix)]
    #[test] fn rejects_library_symlinks() {
        use std::os::unix::fs::symlink;
        let temp = Scratch::new(); let outside = temp.0.join("outside.ttf"); fs::write(&outside, fixture()).unwrap();
        let root = temp.0.join("library"); fs::create_dir(&root).unwrap(); let id = digest(fixture()); symlink(&outside, root.join(format!("{id}.ttf"))).unwrap();
        assert!(resolve_in(&root, &id).is_err()); assert!(library_in(&root).unwrap().is_empty());
    }
    #[test]
    #[ignore = "renders a real captioned MP4 with bundled FFmpeg"]
    fn custom_font_native_export() {
        use crate::engine::{self, ClipPlan, RenderControl, Settings};
        use std::process::Command;
        let temp = Scratch::new(); let source = temp.0.join("custom.ttf"); fs::write(&source, fixture()).unwrap();
        let root = temp.0.join("library"); let font = import_in(&root, &source).unwrap();
        LIBRARY_ROOT.set(root).unwrap(); fs::remove_file(source).unwrap();
        let resolved = engine::subtitle_font(&font.id).unwrap(); assert_eq!(resolved.0, font.family); assert_eq!(resolved.1, font.path);
        let ffmpeg = Path::new(env!("CARGO_MANIFEST_DIR")).join("resources/ffmpeg");
        let video = temp.0.join("source.mp4");
        assert!(Command::new(&ffmpeg).args(["-hide_banner", "-loglevel", "error", "-f", "lavfi", "-i", "color=c=0x172033:s=160x240:r=30:d=5.1", "-c:v", "libx264", "-pix_fmt", "yuv420p"]).arg(&video).status().unwrap().success());
        let words = temp.0.join("words.srt"); fs::write(&words, "1\n00:00:00,100 --> 00:00:01,500\nПривет!\n").unwrap();
        let settings = Settings { count: 1, duration: 5., resolution: 720, banner_enabled: false, banner_height: 0, banner_bottom: 0, subtitles_enabled: true, subtitle_font: font.id, output_dir: temp.0.to_string_lossy().into_owned(), layout_mode: "single".into(), ..Settings::default() };
        let output = temp.0.join("custom-font.mp4");
        let clip = ClipPlan { index: 1, top_path: video.to_string_lossy().into_owned(), bottom_path: String::new(), top_start: 0., bottom_start: 0., duration: 5., output_path: output.to_string_lossy().into_owned() };
        engine::render_clip_inner(&ffmpeg, Path::new("/unused.png"), &settings, &clip, &RenderControl::new(), Some(&words), |_| {}).unwrap();
        assert!(fs::metadata(&output).unwrap().len() > 1024);
        assert!(Command::new(&ffmpeg).args(["-hide_banner", "-loglevel", "error", "-i"]).arg(output).args(["-f", "null", "-"]).status().unwrap().success());
    }
}
