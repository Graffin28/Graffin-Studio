use crate::engine::{self, Settings};
use serde::{Deserialize, Serialize};
use std::{fs::{self, File, OpenOptions}, io::Write, path::Path, sync::{Mutex, atomic::{AtomicU64, Ordering}}, time::{SystemTime, UNIX_EPOCH}};
use tauri::{AppHandle, Manager};

static LOCK: Mutex<()> = Mutex::new(());
static SEQUENCE: AtomicU64 = AtomicU64::new(0);
fn now() -> u64 { SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_millis() as u64 }
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all="camelCase")]
pub struct StudioPreset { pub id: String, pub name: String, pub settings: Settings, pub updated_at: u64 }
#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all="camelCase")]
pub struct PresetSaveRequest { pub id: Option<String>, pub name: String, pub settings: Settings }
fn path(app: &AppHandle) -> Result<std::path::PathBuf,String> {
    let dir=app.path().app_config_dir().map_err(|e|e.to_string())?;
    fs::create_dir_all(&dir).map_err(|_|"Не создать папку пресетов")?; Ok(dir.join("studio-presets-v2.json"))
}
fn load(path: &Path) -> Result<Vec<StudioPreset>,String> {
    if !path.exists() { return Ok(vec![]); }
    if fs::metadata(path).map_err(|_|"Не прочитать пресеты")?.len()>8_000_000 { return Err("Файл пресетов слишком большой".into()); }
    let mut entries:Vec<StudioPreset>=serde_json::from_slice(&fs::read(path).map_err(|_|"Не прочитать пресеты")?).map_err(|_|"Файл пресетов повреждён. Сохранённые данные оставлены на месте.")?;
    entries.sort_by_key(|item|std::cmp::Reverse(item.updated_at)); Ok(entries)
}
fn persist(path: &Path, entries: &[StudioPreset]) -> Result<(),String> {
    let temporary=path.with_extension(format!("{}.{}.tmp",std::process::id(),SEQUENCE.fetch_add(1,Ordering::Relaxed)));
    let result=(|| {
        let mut file=OpenOptions::new().write(true).create_new(true).open(&temporary).map_err(|e|e.to_string())?;
        file.write_all(&serde_json::to_vec_pretty(entries).map_err(|e|e.to_string())?).map_err(|e|e.to_string())?; file.sync_all().map_err(|e|e.to_string())?;
        fs::rename(&temporary,path).map_err(|e|e.to_string())?;
        if let Some(parent)=path.parent() { if let Ok(dir)=File::open(parent) { let _=dir.sync_all(); } } Ok(())
    })();
    if result.is_err() { let _=fs::remove_file(&temporary); } result.map_err(|_:String|"Не сохранить пресеты. Предыдущая версия сохранена.".into())
}
fn validate(request: &PresetSaveRequest) -> Result<(),String> {
    let name=request.name.trim();
    if name.is_empty() || name.chars().count()>60 || name.chars().any(char::is_control) { return Err("Название пресета: от 1 до 60 символов".into()); }
    if request.id.as_ref().is_some_and(|id| id.is_empty()||id.len()>80||!id.bytes().all(|b|b.is_ascii_alphanumeric()||b==b'-')) { return Err("Некорректный пресет".into()); }
    engine::validate_settings(&request.settings)?;
    Ok(())
}
#[tauri::command]
pub fn studio_presets(app: AppHandle) -> Result<Vec<StudioPreset>,String> { let _guard=LOCK.lock().map_err(|_|"Пресеты заняты")?; load(&path(&app)?) }
#[tauri::command]
pub fn studio_save_preset(app: AppHandle, request: PresetSaveRequest) -> Result<Vec<StudioPreset>,String> {
    validate(&request)?; let _guard=LOCK.lock().map_err(|_|"Пресеты заняты")?; let path=path(&app)?; let mut entries=load(&path)?;
    let id=request.id.unwrap_or_else(||format!("preset-{}-{}",now(),SEQUENCE.fetch_add(1,Ordering::Relaxed))); let name=request.name.trim().to_string();
    if entries.iter().any(|item|item.id!=id&&item.name.to_lowercase()==name.to_lowercase()) { return Err("Такое название уже есть. Выберите другое.".into()); }
    entries.retain(|item|item.id!=id); if entries.len()>=40 { return Err("Можно сохранить до 40 пресетов".into()); }
    entries.insert(0,StudioPreset{id,name,settings:request.settings,updated_at:now()}); persist(&path,&entries)?; Ok(entries)
}
#[tauri::command]
pub fn studio_delete_preset(app: AppHandle, id: String) -> Result<Vec<StudioPreset>,String> {
    let _guard=LOCK.lock().map_err(|_|"Пресеты заняты")?; let path=path(&app)?; let mut entries=load(&path)?; entries.retain(|item|item.id!=id); persist(&path,&entries)?; Ok(entries)
}
#[cfg(test)]
mod tests {
    use super::*;
    fn request()->PresetSaveRequest { PresetSaveRequest{id:None,name:"Юмор".into(),settings:Settings{output_dir:std::env::temp_dir().to_string_lossy().into_owned(),..Settings::default()}} }
    #[test] fn settings_validation(){let mut req=request();assert!(validate(&req).is_ok());req.settings.subtitle_color="red".into();assert!(validate(&req).is_err());req=request();req.name=" \n".into();assert!(validate(&req).is_err());}
    #[test] fn presets_roundtrip_does_not_store_account_or_schedule(){let dir=std::env::temp_dir().join(format!("graffin-preset-test-{}-{}",std::process::id(),now()));fs::create_dir(&dir).unwrap();let p=dir.join("presets.json");let req=request();let entry=StudioPreset{id:"preset-test".into(),name:req.name,settings:req.settings,updated_at:now()};persist(&p,&[entry]).unwrap();assert_eq!(load(&p).unwrap().len(),1);let text=fs::read_to_string(&p).unwrap();for forbidden in ["accountId","slots","cookies","automationEnabled"]{assert!(!text.contains(forbidden));}fs::remove_dir_all(dir).unwrap();}
    #[test] fn old_presets_ignore_removed_publication_fields(){let req=request();let old=serde_json::json!({"id":"preset-old","name":"Старое оформление","settings":req.settings,"caption":{"caption":"old","hashtags":"#old"},"updatedAt":1});let item:StudioPreset=serde_json::from_value(old).unwrap();assert_eq!(item.id,"preset-old");assert!(!serde_json::to_string(&item).unwrap().contains("hashtags"));}
}
