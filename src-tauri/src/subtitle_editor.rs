//! Editable captions retain the original source recipe, never paint over burned text.
use crate::engine::{self, ClipPlan, RenderControl, Settings, SubtitleCue};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{fs::{self, File, OpenOptions}, io::{Read, Write}, path::{Path, PathBuf}, sync::atomic::{AtomicU64, Ordering}, time::{SystemTime, UNIX_EPOCH}};
use tauri::{AppHandle, Manager};

static SEQUENCE: AtomicU64 = AtomicU64::new(0);
// A document read can recover an interrupted pair commit. Serialize it with
// saves so it cannot mistake a currently active commit for an interrupted one.
static EDIT_ACCESS: std::sync::Mutex<()> = std::sync::Mutex::new(());
fn token() -> String { format!("{}-{}-{}", std::process::id(), SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_millis(), SEQUENCE.fetch_add(1, Ordering::Relaxed)) }

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all="camelCase")]
pub(crate) struct FileIdentity {
    path: String, bytes: u64, modified: u128, device: u64, inode: u64, sha256: String,
}
fn regular(path: &Path) -> Result<PathBuf, String> {
    if !path.is_absolute() { return Err("Нужен абсолютный путь к ролику.".into()); }
    let attributes=fs::symlink_metadata(path).map_err(|_|"Файл больше недоступен.")?;
    if !attributes.file_type().is_file() || attributes.file_type().is_symlink() { return Err("Выберите обычный файл, а не ссылку.".into()); }
    path.canonicalize().map_err(|_|"Файл больше недоступен.".into())
}
fn hash(path: &Path) -> Result<String,String> {
    let mut file=File::open(path).map_err(|_|"Не прочитать видео.")?; let mut digest=Sha256::new(); let mut buffer=[0;262144];
    loop {let count=file.read(&mut buffer).map_err(|_|"Не проверить содержимое видео.")?; if count==0 {break;} digest.update(&buffer[..count]);}
    Ok(format!("{:x}",digest.finalize()))
}
fn stamp(path: &Path) -> Result<(u64,u128,u64,u64),String> {
    let m=fs::symlink_metadata(path).map_err(|_|"Файл больше недоступен.")?;
    if !m.is_file() || m.file_type().is_symlink() { return Err("Файл изменился во время обработки.".into()); }
    #[cfg(unix)] let (device,inode)={use std::os::unix::fs::MetadataExt;(m.dev(),m.ino())};
    #[cfg(not(unix))] let (device,inode)=(0,0);
    Ok((m.len(),m.modified().ok().and_then(|v|v.duration_since(UNIX_EPOCH).ok()).map(|v|v.as_nanos()).unwrap_or_default(),device,inode))
}
fn identify(path: &Path) -> Result<FileIdentity,String> {
    let path=regular(path)?;let before=stamp(&path)?;let sha256=hash(&path)?;
    if before!=stamp(&path)? { return Err("Исходник изменился во время проверки.".into()); }
    Ok(FileIdentity {path:path.to_string_lossy().into_owned(),bytes:before.0,modified:before.1,device:before.2,inode:before.3,sha256})
}
pub(crate) fn source_identities(clip: &ClipPlan) -> Result<Vec<FileIdentity>,String> {
    let mut result=vec![identify(Path::new(&clip.top_path))?];
    if clip.bottom_path!=clip.top_path {result.push(identify(Path::new(&clip.bottom_path))?);} Ok(result)
}
pub(crate) fn render_source_identities(clip:&ClipPlan,settings:&Settings,banner:Option<&Path>)->Result<Vec<FileIdentity>,String>{
    let mut identities=source_identities(clip)?;
    let mut assets=Vec::new();
    if let Some(path)=banner {assets.push(path);}
    if settings.interruption_enabled {assets.push(Path::new(&settings.interruption_path));}
    if !settings.music_path.is_empty() {assets.push(Path::new(&settings.music_path));}
    for asset in assets {let identity=identify(asset)?;if !identities.iter().any(|source|source.path==identity.path){identities.push(identity);}}
    Ok(identities)
}
pub(crate) fn verify_source_identities(identities: &[FileIdentity]) -> Result<(),String> {
    for value in identities { if stamp(Path::new(&value.path))? != (value.bytes,value.modified,value.device,value.inode) {return Err("Исходник изменился. Повторите сборку из выбранных файлов.".into());} } Ok(())
}
fn verify_source_content(identities: &[FileIdentity]) -> Result<(),String> {
    verify_source_identities(identities)?;
    for value in identities {if hash(Path::new(&value.path))? != value.sha256 {return Err("Исходник был изменён. Сохранённый монтаж не будет подменён.".into());}} Ok(())
}

#[derive(Clone,Debug,Serialize,Deserialize)]
#[serde(rename_all="camelCase")]
struct EditRecord {version:u32,clip:ClipPlan,settings:Settings,banner_path:String,words:Vec<SubtitleCue>,sources:Vec<FileIdentity>,output_hash:String}
fn record_path(output:&Path,create:bool)->Result<PathBuf,String> {
    let parent=output.parent().ok_or("Не найдена папка ролика.")?;
    let directory=parent.join(".graffin-edit");
    if directory.exists() && (fs::symlink_metadata(&directory).map_err(|_|"Папка монтажа недоступна.")?.file_type().is_symlink() || !directory.is_dir()) {return Err("Папка монтажа не должна быть ссылкой.".into());}
    if create {fs::create_dir_all(&directory).map_err(|_|"Не создать данные редактирования.")?;}
    let name=output.file_name().and_then(|v|v.to_str()).ok_or("Не найдено имя ролика.")?;
    Ok(directory.join(format!("{name}.json")))
}
fn atomic_json(path:&Path,value:&impl Serialize)->Result<(),String> {
    let temp=path.with_extension(format!("{}.tmp",token()));
    let result=(|| {
        let mut options=OpenOptions::new();options.write(true).create_new(true);
        #[cfg(unix)]{use std::os::unix::fs::OpenOptionsExt;options.mode(0o600);}
        let mut file=options.open(&temp).map_err(|_|"Не подготовить данные монтажа.")?;
        file.write_all(&serde_json::to_vec(value).map_err(|_|"Не собрать данные монтажа.")?).map_err(|_|"Не сохранить данные монтажа.")?;file.sync_all().map_err(|_|"Не сохранить данные монтажа.")?;
        fs::rename(&temp,path).map_err(|_|"Не сохранить данные монтажа.")?;
        if let Some(parent)=path.parent(){if let Ok(dir)=File::open(parent){let _=dir.sync_all();}} Ok(())
    })(); if result.is_err(){let _=fs::remove_file(&temp);} result
}
fn persist(path:&Path,record:&EditRecord)->Result<(),String>{atomic_json(path,record)}
#[derive(Serialize,Deserialize)]
#[serde(rename_all="camelCase")]
struct EditTransaction {old_record:EditRecord,new_record:EditRecord,backup_name:String,temporary_name:String}
fn transaction_path(path:&Path)->Result<PathBuf,String>{Ok(record_path(path,false)?.with_extension("transaction.json"))}
fn owned_recipe(path:&Path,record:&EditRecord)->bool {
    let recipe=Path::new(&record.clip.output_path);
    recipe.file_name()==path.file_name() && recipe.parent().and_then(|v|v.canonicalize().ok())==path.parent().and_then(|v|v.canonicalize().ok())
}
fn recover_transaction(path:&Path)->Result<(),String>{
    let journal=transaction_path(path)?;if !journal.exists(){return Ok(());}
    let m=fs::symlink_metadata(&journal).map_err(|_|"Не проверить сохранение монтажа.")?;
    if !m.is_file()||m.file_type().is_symlink()||m.len()>8_000_000{return Err("Журнал сохранения монтажа повреждён.".into());}
    let entry:EditTransaction=serde_json::from_slice(&fs::read(&journal).map_err(|_|"Не прочитать журнал монтажа.")?).map_err(|_|"Журнал сохранения монтажа повреждён.")?;
    let basename=|value:&str,prefix:&str|value.starts_with(prefix)&&value.ends_with(".mp4")&&Path::new(value).file_name().and_then(|v|v.to_str())==Some(value);
    if !owned_recipe(path,&entry.old_record)||!owned_recipe(path,&entry.new_record)||!basename(&entry.backup_name,".graffin-caption-backup-")||!basename(&entry.temporary_name,".graffin-caption-edit-"){return Err("Журнал принадлежит другому монтажу.".into());}
    let current=hash(path)?;
    let record=if current==entry.new_record.output_hash{&entry.new_record}else if current==entry.old_record.output_hash{&entry.old_record}else{return Err("Видео изменилось после прерванного сохранения. Резервная копия сохранена в папке ролика.".into());};
    persist(&record_path(path,true)?,record)?;
    let parent=path.parent().unwrap();
    for name in [&entry.backup_name,&entry.temporary_name]{let candidate=parent.join(name);if candidate.exists(){let _=fs::remove_file(&candidate);remove_record(&candidate);}}
    fs::remove_file(journal).map_err(|_|"Не завершить восстановление монтажа.")?;Ok(())
}

pub(crate) fn record_render(partial:&Path,output:&Path,banner:Option<&Path>,settings:&Settings,clip:&ClipPlan,srt:&str,mut sources:Vec<FileIdentity>)->Result<(),String> {
    let mut clip=clip.clone();clip.output_path=output.to_string_lossy().into_owned();
    let banner_path=if let Some(banner)=banner {let identity=identify(banner)?;let value=identity.path.clone();if !sources.iter().any(|s|s.path==value){sources.push(identity);}value}else{String::new()};
    for asset in [if settings.interruption_enabled {settings.interruption_path.as_str()} else {""}, settings.music_path.as_str()] {
        if !asset.is_empty() {
            let identity=identify(Path::new(asset))?;
            if !sources.iter().any(|source|source.path==identity.path) {sources.push(identity);}
        }
    }
    let words=engine::single_word_cues(engine::subtitle_cues(srt,clip.duration));
    let value=EditRecord {version:1,clip,settings:settings.clone(),banner_path,words,sources,output_hash:hash(partial)?};
    persist(&record_path(output,true)?,&value)
}
pub(crate) fn remove_record(output:&Path){if let Ok(path)=record_path(output,false){let _=fs::remove_file(path);}}
fn load_record(path:&Path)->Result<EditRecord,String> {
    recover_transaction(path)?;
    let sidecar=record_path(path,false)?;
    let size=fs::symlink_metadata(&sidecar).map_err(|_|"Создано в старой версии. Для редактирования соберите ролик заново.")?;
    if !size.is_file() || size.file_type().is_symlink() || size.len()>4_000_000 {return Err("Данные монтажа повреждены.".into());}
    let record:EditRecord=serde_json::from_slice(&fs::read(&sidecar).map_err(|_|"Не прочитать данные монтажа.")?).map_err(|_|"Данные монтажа повреждены.")?;
    if record.version!=1 || !record.clip.duration.is_finite() || !(0.05..=180.).contains(&record.clip.duration) {return Err("Версия данных монтажа не поддерживается.".into());}
    let old=Path::new(&record.clip.output_path);
    let same_parent=old.parent().and_then(|p|p.canonicalize().ok())==path.parent().and_then(|p|p.canonicalize().ok());
    if old.file_name()!=path.file_name() || !same_parent || record.sources.len()>5 {return Err("Данные принадлежат другому ролику.".into());}
    engine::validate_settings(&record.settings)?;
    validate_words(&record.words,record.clip.duration)?;
    if hash(path)?!=record.output_hash {return Err("Ролик изменён вне редактора. Сохранённый монтаж не будет подменён.".into());}
    Ok(record)
}
fn revision(record:&EditRecord)->String {format!("{:x}",Sha256::digest(serde_json::to_vec(record).unwrap_or_default()))}

#[derive(Clone,Debug,Serialize)]
#[serde(rename_all="camelCase")]
pub struct SubtitleDocument {pub path:String,pub name:String,pub duration:f64,pub pause_at:Option<f64>,pub pause_duration:f64,pub words:Vec<SubtitleCue>,pub settings:Settings,pub preview_path:String,pub editable:bool,pub reason:Option<String>,pub revision:String}
#[derive(Clone,Debug,Deserialize)]
#[serde(rename_all="camelCase")]
pub struct SubtitleEditRequest {pub path:String,pub revision:String,pub words:Vec<SubtitleCue>,pub subtitle_font:String,pub subtitle_color:String,pub subtitle_size:u32,pub subtitle_offset:i32,
    #[serde(default)] pub subtitle_words_per_line:Option<u32>,
    #[serde(default)] pub subtitle_animation:Option<String>,
    #[serde(default)] pub subtitle_highlight_enabled:Option<bool>,
    #[serde(default)] pub subtitle_highlight_color:Option<String>}
fn document(app:&AppHandle,path:&str)->Result<SubtitleDocument,String> {
    let path=regular(Path::new(path))?;
    if path.extension().and_then(|v|v.to_str())!=Some("mp4"){return Err("Выберите ролик MP4.".into());}
    crate::authorize(app,&path)?;
    let path_string=path.to_string_lossy().into_owned();let name=path.file_name().unwrap_or_default().to_string_lossy().into_owned();
    match load_record(&path) {
        Ok(record)=>{
            let reason=verify_source_content(&record.sources).err();
            let pause=engine::content_pause(&record.settings,&record.clip);
            Ok(SubtitleDocument {path:path_string.clone(),name,duration:record.clip.duration,pause_at:pause.map(|p|p.0),pause_duration:pause.map_or(0.0,|p|p.1),words:record.words.clone(),settings:record.settings.clone(),preview_path:path_string,editable:reason.is_none(),reason,revision:revision(&record)})
        },
        Err(reason)=>{
            let ff=crate::resource(app,"ffmpeg")?;let media=engine::probe(&ff,&path,&crate::cache(app)?)?;
            Ok(SubtitleDocument {path:path_string.clone(),name,duration:media.duration,pause_at:None,pause_duration:0.0,words:vec![],settings:Settings {output_dir:path.parent().unwrap().to_string_lossy().into_owned(),..Settings::default()},preview_path:path_string,editable:false,reason:Some(reason),revision:String::new()})
        }
    }
}
#[tauri::command]
pub async fn subtitle_document(app:AppHandle,path:String)->Result<SubtitleDocument,String>{tauri::async_runtime::spawn_blocking(move||{let _access=EDIT_ACCESS.lock().map_err(|_|"Редактор временно недоступен.".to_string())?;document(&app,&path)}).await.map_err(|_|"Не открыть редактор субтитров.".to_string())?}

pub(crate) fn validate_words(words:&[SubtitleCue],duration:f64)->Result<(),String> {
    if !duration.is_finite() || duration<=0. || duration>180. || words.len()>5000 {return Err("Некорректная длительность или слишком много слов.".into());}
    let mut previous=0.;
    for word in words {
        if !word.start.is_finite() || !word.end.is_finite() || word.start<0. || word.end>duration+0.000001 || word.end-word.start<0.01 || word.start+0.000001<previous {return Err("Время слов должно идти по порядку, без пересечений и за пределы ролика.".into());}
        if word.text.trim()!=word.text || word.text.is_empty() || word.text.chars().count()>120 || word.text.split_whitespace().count()!=1 || word.text.chars().any(char::is_control) || !word.text.chars().any(char::is_alphanumeric) || word.text.contains(['<','>']) {return Err("В каждой строке должно быть одно слово без переносов и разметки.".into());}
        previous=word.end;
    } Ok(())
}
fn srt(words:&[SubtitleCue])->String {
    let time=|seconds:f64|{let ms=(seconds*1000.).round() as u64;format!("{:02}:{:02}:{:02},{:03}",ms/3600000,ms/60000%60,ms/1000%60,ms%1000)};
    words.iter().enumerate().map(|(i,w)|format!("{}\n{} --> {}\n{}\n\n",i+1,time(w.start),time(w.end),w.text)).collect()
}
// Editing is local and serialized with rendering; no account or publication state is involved.
struct EditGuard {app:AppHandle,path:PathBuf,original:String}
impl EditGuard {
    fn original_hash(&self)->&str {&self.original}
    fn verify(&self)->Result<(),String>{if hash(&self.path)?==self.original{Ok(())}else{Err("Ролик изменился во время редактирования.".into())}}
    fn finish(self)->Result<(),String>{Ok(())}
}
impl Drop for EditGuard {fn drop(&mut self){self.app.state::<crate::AppState>().busy.store(false,std::sync::atomic::Ordering::SeqCst);}}
fn acquire_edit(app:&AppHandle,path:&Path)->Result<EditGuard,String>{
    let state=app.state::<crate::AppState>();
    if state.busy.compare_exchange(false,true,std::sync::atomic::Ordering::SeqCst,std::sync::atomic::Ordering::SeqCst).is_err(){return Err("Дождитесь завершения текущей операции.".into());}
    match hash(path){Ok(original)=>Ok(EditGuard{app:app.clone(),path:path.to_path_buf(),original}),Err(error)=>{state.busy.store(false,std::sync::atomic::Ordering::SeqCst);Err(error)}}
}
struct EditTemporary {output:PathBuf,words:PathBuf}
impl Drop for EditTemporary {fn drop(&mut self){let _=fs::remove_file(&self.output);let _=fs::remove_file(&self.words);remove_record(&self.output);}}
fn save(app:&AppHandle,request:SubtitleEditRequest)->Result<SubtitleDocument,String> {
    let path=regular(Path::new(&request.path))?;
    let mut record=load_record(&path)?;
    if revision(&record)!=request.revision{return Err("Ролик уже изменён. Откройте редактор заново.".into());}
    validate_words(&request.words,record.clip.duration)?;
    let mut settings=record.settings.clone();settings.subtitle_font=request.subtitle_font;settings.subtitle_color=request.subtitle_color.to_ascii_uppercase();settings.subtitle_size=request.subtitle_size;settings.subtitle_offset=request.subtitle_offset;settings.subtitles_enabled=true;
    if let Some(value)=request.subtitle_words_per_line { settings.subtitle_words_per_line=value; }
    if let Some(value)=request.subtitle_animation { settings.subtitle_animation=value; }
    if let Some(value)=request.subtitle_highlight_enabled { settings.subtitle_highlight_enabled=value; }
    if let Some(value)=request.subtitle_highlight_color { settings.subtitle_highlight_color=value.to_ascii_uppercase(); }
    engine::validate_settings(&settings)?;
    let guard=acquire_edit(app,&path)?;
    if guard.original_hash()!=record.output_hash{return Err("Ролик изменился перед сохранением. Откройте редактор заново.".into());}
    verify_source_content(&record.sources)?;
    if hash(&path)?!=record.output_hash{return Err("Ролик изменился перед сохранением.".into());}
    let control=RenderControl::new();*app.state::<crate::AppState>().control.lock().unwrap_or_else(|e|e.into_inner())=Some(control.clone());
    struct ClearControl<'a>(&'a AppHandle);impl Drop for ClearControl<'_>{fn drop(&mut self){*self.0.state::<crate::AppState>().control.lock().unwrap_or_else(|e|e.into_inner())=None;}}
    let _clear=ClearControl(app);
    let temporary=EditTemporary {output:path.parent().unwrap().join(format!(".graffin-caption-edit-{}.mp4",token())),words:crate::cache(app)?.join(format!("graffin-edited-words-{}.srt",token()))};
    fs::write(&temporary.words,srt(&request.words)).map_err(|_|"Не подготовить исправленные слова.")?;
    let mut clip=record.clip.clone();clip.output_path=temporary.output.to_string_lossy().into_owned();
    let banner=if record.banner_path.is_empty(){crate::resource(app,"banner.png")?}else{PathBuf::from(&record.banner_path)};
    engine::render_clip_inner(&crate::resource(app,"ffmpeg")?,&banner,&settings,&clip,&control,Some(&temporary.words),|_|{})?;
    if control.is_cancelled(){return Err("Сохранение отменено. Исходный ролик сохранён.".into());}
    guard.verify()?;verify_source_content(&record.sources)?;
    let new_hash=hash(&temporary.output)?;
    record.settings=settings;record.words=request.words;record.output_hash=new_hash;
    // Keep the original video until a complete replacement and its new recipe
    // both exist. A recoverable backup lets an interrupted pair commit roll back.
    let backup=path.parent().unwrap().join(format!(".graffin-caption-backup-{}.mp4",token()));
    let backup_result=(|| {
        fs::hard_link(&path,&backup).or_else(|_|fs::copy(&path,&backup).map(|_|())).map_err(|_|"Не сохранить исходный ролик перед заменой.")?;
        crate::platform::sync_file(&backup).map_err(|_|"Не сохранить резервную копию ролика.")?;
        if let Some(parent)=backup.parent(){if let Ok(dir)=File::open(parent){let _=dir.sync_all();}}
        Ok::<_,String>(())
    })();
    if let Err(error)=backup_result{let _=fs::remove_file(&backup);return Err(error);}
    let old_record=load_record(&path)?;
    if hash(&backup)?!=old_record.output_hash || guard.verify().is_err(){let _=fs::remove_file(&backup);return Err("Ролик изменился перед заменой. Исходный файл сохранён.".into());}
    if control.is_cancelled(){let _=fs::remove_file(&backup);return Err("Сохранение отменено. Исходный ролик сохранён.".into());}
    let transaction=EditTransaction {old_record:old_record.clone(),new_record:record.clone(),backup_name:backup.file_name().unwrap().to_string_lossy().into(),temporary_name:temporary.output.file_name().unwrap().to_string_lossy().into()};
    let journal=transaction_path(&path)?;
    if let Err(error)=atomic_json(&journal,&transaction){let _=fs::remove_file(&backup);return Err(error);}
    let commit=(|| {
        fs::rename(&temporary.output,&path).map_err(|_|"Не заменить готовый ролик.")?;
        if let Some(parent)=path.parent(){if let Ok(dir)=File::open(parent){let _=dir.sync_all();}}
        persist(&record_path(&path,true)?,&record)
    })();
    if commit.is_err() && backup.exists(){
        if fs::rename(&backup,&path).is_ok(){let _=persist(&record_path(&path,true)?,&old_record);}
    }
    // Recovery examines the actual current video hash. It finalizes a completed
    // commit or restores the old recipe after a rollback, including app crashes.
    let recovered=recover_transaction(&path);
    if let Err(error)=commit{return Err(error);}
    recovered?;
    guard.finish()?;
    document(app,path.to_str().ok_or("Не открыть обновлённый ролик.")?)
}
#[tauri::command]
pub async fn subtitle_save(app:AppHandle,request:SubtitleEditRequest)->Result<SubtitleDocument,String>{tauri::async_runtime::spawn_blocking(move||{let _access=EDIT_ACCESS.lock().map_err(|_|"Редактор временно недоступен.".to_string())?;save(&app,request)}).await.map_err(|_|"Не сохранить исправленные субтитры.".to_string())?}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn caption_style_request_preserves_old_client_and_accepts_new_options() {
        let old=r##"{"path":"/tmp/a.mp4","revision":"r1","words":[],"subtitleFont":"manrope","subtitleColor":"#FFFFFF","subtitleSize":56,"subtitleOffset":0}"##;
        let request:SubtitleEditRequest=serde_json::from_str(old).unwrap();
        assert!(request.subtitle_words_per_line.is_none());assert!(request.subtitle_animation.is_none());assert!(request.subtitle_highlight_enabled.is_none());assert!(request.subtitle_highlight_color.is_none());
        let mut value:serde_json::Value=serde_json::from_str(old).unwrap();
        value["subtitleWordsPerLine"]=3.into();value["subtitleAnimation"]="slide".into();value["subtitleHighlightEnabled"]=true.into();value["subtitleHighlightColor"]="#A6FF52".into();
        let request:SubtitleEditRequest=serde_json::from_value(value).unwrap();
        assert_eq!(request.subtitle_words_per_line,Some(3));assert_eq!(request.subtitle_animation.as_deref(),Some("slide"));assert_eq!(request.subtitle_highlight_enabled,Some(true));assert_eq!(request.subtitle_highlight_color.as_deref(),Some("#A6FF52"));
    }

    #[test]
    fn render_snapshots_music_and_interruption_and_exposes_content_pause() {
        let dir=std::env::temp_dir().join(format!("graffin-editor-assets-{}",token()));fs::create_dir(&dir).unwrap();let source=dir.join("source.mp4");let music=dir.join("music.wav");let banner=dir.join("pause.png");
        for path in [&source,&music,&banner] {fs::write(path,b"asset").unwrap();}
        let clip=ClipPlan{index:1,top_path:source.to_string_lossy().into(),bottom_path:source.to_string_lossy().into(),top_start:0.0,bottom_start:0.0,duration:5.0,output_path:dir.join("output.mp4").to_string_lossy().into()};
        let settings=Settings{music_path:music.to_string_lossy().into(),interruption_enabled:true,interruption_path:banner.to_string_lossy().into(),interruption_timing:"fixed".into(),interruption_start:2.0,interruption_duration:1.0,output_dir:dir.to_string_lossy().into(),..Settings::default()};
        let identities=render_source_identities(&clip,&settings,None).unwrap();assert_eq!(identities.len(),3);verify_source_identities(&identities).unwrap();
        let pause=engine::content_pause(&settings,&clip).unwrap();
        let document=SubtitleDocument{path:clip.output_path.clone(),name:"output.mp4".into(),duration:clip.duration,pause_at:Some(pause.0),pause_duration:pause.1,words:vec![],settings:settings.clone(),preview_path:clip.output_path.clone(),editable:true,reason:None,revision:"test".into()};
        let value=serde_json::to_value(document).unwrap();assert_eq!(value["pauseAt"],2.0);assert_eq!(value["pauseDuration"],1.0);assert_eq!(engine::content_pause(&settings,&clip),Some(pause));
        fs::write(&music,b"changed music").unwrap();assert!(verify_source_identities(&identities).is_err());fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn full_composition_recipe_accepts_all_five_assets_and_checks_changes() {
        let dir=std::env::temp_dir().join(format!("graffin-editor-five-{}",token()));fs::create_dir(&dir).unwrap();
        let top=dir.join("top.mp4");let bottom=dir.join("bottom.mp4");let banner=dir.join("banner.png");let music=dir.join("music.wav");let interruption=dir.join("interruption.png");
        for path in [&top,&bottom,&banner,&music,&interruption] {fs::write(path,b"unchanged asset").unwrap();}
        let output=dir.join("output.mp4");let partial=dir.join("partial.mp4");fs::write(&partial,b"rendered video").unwrap();
        let clip=ClipPlan{index:1,top_path:top.to_string_lossy().into(),bottom_path:bottom.to_string_lossy().into(),top_start:0.0,bottom_start:0.0,duration:5.0,output_path:output.to_string_lossy().into()};
        let settings=Settings{music_path:music.to_string_lossy().into(),interruption_enabled:true,interruption_path:interruption.to_string_lossy().into(),output_dir:dir.to_string_lossy().into(),..Settings::default()};
        let identities=render_source_identities(&clip,&settings,Some(&banner)).unwrap();assert_eq!(identities.len(),5);
        record_render(&partial,&output,Some(&banner),&settings,&clip,"",identities).unwrap();fs::rename(&partial,&output).unwrap();
        let loaded=load_record(&output).unwrap();assert_eq!(loaded.sources.len(),5);verify_source_content(&loaded.sources).unwrap();
        fs::write(&interruption,b"changed banner").unwrap();assert!(verify_source_content(&loaded.sources).is_err());fs::remove_dir_all(dir).unwrap();
    }

    #[test] fn edited_words_reject_overlap_and_injection_lines(){
        let valid=vec![SubtitleCue {start:0.1,end:0.4,text:"Интернет".into()},SubtitleCue {start:0.6,end:1.,text:"свободный!".into()}];assert!(validate_words(&valid,2.).is_ok());
        for words in [vec![SubtitleCue{start:0.,end:1.,text:"два слова".into()}],vec![SubtitleCue{start:f64::NAN,end:1.,text:"слово".into()}],vec![SubtitleCue{start:0.,end:3.,text:"слово".into()}],vec![SubtitleCue{start:0.,end:1.,text:"слово\n1".into()}],vec![SubtitleCue{start:0.,end:1.,text:"первое".into()},SubtitleCue{start:0.5,end:1.5,text:"второе".into()}]]{assert!(validate_words(&words,2.).is_err());}
        assert!(validate_words(&[],2.).is_ok());assert_eq!(engine::single_word_cues(engine::subtitle_cues(&srt(&valid),2.)),valid);
    }
    #[test] fn interrupted_pair_commit_recovers_new_recipe_without_losing_video(){
        let dir=std::env::temp_dir().join(format!("graffin-editor-recovery-{}",token()));fs::create_dir(&dir).unwrap();let source=dir.join("source.mp4");let output=dir.join("clip.mp4");let partial=dir.join("partial.mp4");fs::write(&source,b"source movie").unwrap();fs::write(&partial,b"old rendered movie").unwrap();
        let clip=ClipPlan{index:1,top_path:source.to_string_lossy().into(),bottom_path:source.to_string_lossy().into(),top_start:0.,bottom_start:0.,duration:5.,output_path:output.to_string_lossy().into()};let settings=Settings{output_dir:dir.to_string_lossy().into(),..Settings::default()};
        record_render(&partial,&output,None,&settings,&clip,"1\n00:00:00,100 --> 00:00:00,500\nСтарое\n",source_identities(&clip).unwrap()).unwrap();fs::rename(partial,&output).unwrap();let old=load_record(&output).unwrap();let backup=dir.join(".graffin-caption-backup-test.mp4");fs::hard_link(&output,&backup).unwrap();let temp=dir.join(".graffin-caption-edit-test.mp4");fs::write(&temp,b"new rendered movie").unwrap();let mut new=old.clone();new.words[0].text="Новое".into();new.output_hash=hash(&temp).unwrap();let journal=transaction_path(&output).unwrap();atomic_json(&journal,&EditTransaction{old_record:old,new_record:new,backup_name:backup.file_name().unwrap().to_string_lossy().into(),temporary_name:temp.file_name().unwrap().to_string_lossy().into()}).unwrap();
        fs::rename(&temp,&output).unwrap(); // Simulate process death before recipe commit.
        let recovered=load_record(&output).unwrap();assert_eq!(recovered.words[0].text,"Новое");assert!(!backup.exists());assert!(!journal.exists());assert_eq!(fs::read(&output).unwrap(),b"new rendered movie");fs::remove_dir_all(dir).unwrap();
    }

    #[test] fn recipe_identity_and_stale_revision_are_detected(){
        let dir=std::env::temp_dir().join(format!("graffin-editor-test-{}",token()));fs::create_dir(&dir).unwrap();let source=dir.join("source.mp4");let output=dir.join("clip.mp4");let partial=dir.join("render.partial");fs::write(&source,b"source movie").unwrap();fs::write(&partial,b"rendered movie").unwrap();
        let clip=ClipPlan{index:1,top_path:source.to_string_lossy().into(),bottom_path:source.to_string_lossy().into(),top_start:0.,bottom_start:0.,duration:5.,output_path:output.to_string_lossy().into()};let settings=Settings{output_dir:dir.to_string_lossy().into(),..Settings::default()};
        record_render(&partial,&output,None,&settings,&clip,"1\n00:00:00,100 --> 00:00:00,500\nСлово\n",source_identities(&clip).unwrap()).unwrap();fs::rename(partial,&output).unwrap();let mut saved=load_record(&output).unwrap();assert_eq!(saved.words[0].text,"Слово");let original=revision(&saved);saved.words[0].text="Новое".into();assert_ne!(original,revision(&saved));fs::write(&output,b"external modification").unwrap();assert!(load_record(&output).is_err());fs::remove_dir_all(dir).unwrap();
    }
}
