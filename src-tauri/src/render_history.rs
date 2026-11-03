//! Completed montage intervals, independent of folders, queues or accounts.
use crate::{engine::{ClipPlan, Media, RenderControl}, smart_cuts::Excluded};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{fs::{self, File, OpenOptions}, io::{Read, Write}, path::{Path, PathBuf}, sync::Mutex};
use tauri::{AppHandle, Manager};

static ACCESS: Mutex<()> = Mutex::new(());
const MAX_FILE_BYTES: u64 = 16_000_000;
#[derive(Clone, Serialize, Deserialize)]
struct Interval { source_hash: String, start: f64, end: f64, output_path: String }
#[derive(Default, Serialize, Deserialize)]
struct History { version: u32, intervals: Vec<Interval> }
#[derive(Deserialize)]
struct Legacy { #[serde(default)] intervals: Vec<LegacyInterval> }
#[derive(Deserialize)]
struct LegacyInterval { source_hash: String, start: f64, end: f64, output_path: String, status: String }

fn path(app:&AppHandle)->Result<PathBuf,String>{
    let dir=app.path().app_config_dir().map_err(|e|e.to_string())?;
    fs::create_dir_all(&dir).map_err(|_|"Не создать папку истории монтажа.")?;
    Ok(dir.join("render-history-v1.json"))
}
fn valid(interval:&Interval)->bool{
    interval.source_hash.len()==64 && interval.source_hash.bytes().all(|b|b.is_ascii_hexdigit())
        && interval.start.is_finite() && interval.end.is_finite() && interval.start>=0.0 && interval.end>interval.start
        && interval.end-interval.start<=180.001 && interval.end<=1_209_600.0
        && interval.output_path.len()<=4096 && Path::new(&interval.output_path).is_absolute()
}
fn bounded_read(path:&Path)->Result<Vec<u8>,String>{
    let meta=fs::symlink_metadata(path).map_err(|_|"Не прочитать историю монтажа.")?;
    if !meta.is_file()||meta.file_type().is_symlink()||meta.len()>MAX_FILE_BYTES{return Err("История монтажа повреждена или слишком велика.".into());}
    let file=File::open(path).map_err(|_|"Не прочитать историю монтажа.")?;
    let mut bytes=Vec::new();file.take(MAX_FILE_BYTES+1).read_to_end(&mut bytes).map_err(|_|"Не прочитать историю монтажа.")?;
    if bytes.len() as u64>MAX_FILE_BYTES{return Err("История монтажа слишком велика.".into());}Ok(bytes)
}
fn load(path:&Path)->Result<History,String>{
    if path.exists(){
        let history:History=serde_json::from_slice(&bounded_read(path)?).map_err(|_|"История монтажа повреждена. Сохранённые файлы оставлены на месте.")?;
        if history.version!=1||history.intervals.len()>50_000||history.intervals.iter().any(|i|!valid(i)){return Err("История монтажа повреждена.".into());}
        return Ok(history);
    }
    // Read only completed source ranges from the former local queue. Account,
    // browser and scheduling data are neither deserialized nor used.
    let legacy=path.with_file_name("queue-v1.json");
    let intervals=if legacy.is_file(){
        let old=bounded_read(&legacy).ok().and_then(|bytes|serde_json::from_slice::<Legacy>(&bytes).ok());
        old.map(|old|old.intervals.into_iter().filter(|i|i.status=="rendered").map(|i|Interval{source_hash:i.source_hash,start:i.start,end:i.end,output_path:i.output_path}).filter(valid).take(50_000).collect()).unwrap_or_default()
    }else{Vec::new()};
    Ok(History{version:1,intervals})
}
fn persist(path:&Path,history:&History)->Result<(),String>{
    let bytes=serde_json::to_vec(history).map_err(|_|"Не сохранить историю монтажа.")?;
    if bytes.len() as u64>MAX_FILE_BYTES{return Err("История монтажа слишком велика. Готовые видео сохранены.".into());}
    let temp=path.with_extension(format!("{}.{}.tmp",std::process::id(),crate::stamp()));
    let result=(||{
        let mut file=OpenOptions::new().write(true).create_new(true).open(&temp).map_err(|_|"Не подготовить историю монтажа.")?;
        file.write_all(&bytes).and_then(|_|file.sync_all()).map_err(|_|"Не сохранить историю монтажа.")?;
        fs::rename(&temp,path).map_err(|_|"Не сохранить историю монтажа.")?;Ok(())
    })();
    if result.is_err(){let _=fs::remove_file(temp);}result
}
fn fingerprint(path:&Path,control:&RenderControl)->Result<String,String>{
    let stamp=|path:&Path|->Result<(u64,u128),String>{let m=fs::symlink_metadata(path).map_err(|_|"Исходник больше недоступен.")?;if !m.is_file()||m.file_type().is_symlink(){return Err("Для истории нужен обычный видеофайл.".into());}Ok((m.len(),m.modified().ok().and_then(|v|v.duration_since(std::time::UNIX_EPOCH).ok()).map_or(0,|v|v.as_nanos())))};
    let before=stamp(path)?;let mut file=File::open(path).map_err(|_|"Не проверить исходник.")?;let mut digest=Sha256::new();let mut buffer=[0u8;262144];
    loop{if control.is_cancelled(){return Err("Операция отменена.".into());}let n=file.read(&mut buffer).map_err(|_|"Не проверить исходник.")?;if n==0{break;}digest.update(&buffer[..n]);}
    if before!=stamp(path)?{return Err("Исходник изменился во время проверки.".into());}Ok(format!("{:x}",digest.finalize()))
}
pub(crate) fn exclusions(app:&AppHandle,media:&[Media],control:&RenderControl)->Result<Vec<Excluded>,String>{
    let intervals={let _guard=ACCESS.lock().map_err(|_|"История монтажа занята.")?;load(&path(app)?)?.intervals};
    let mut result=Vec::new();
    for(source,item)in media.iter().enumerate(){let hash=fingerprint(Path::new(&item.path),control)?;result.extend(intervals.iter().filter(|i|i.source_hash==hash).map(|i|Excluded{source,start:i.start,end:i.end}));}Ok(result)
}
fn rendered_source_hash(clip:&ClipPlan)->Result<String,String>{
    // The renderer already hashes and verifies every original before publishing
    // this recipe. Reuse that original identity, even if the source is moved
    // just after export, rather than hashing a long source again for each clip.
    let output=Path::new(&clip.output_path);let parent=output.parent().ok_or("Не найдена папка готового видео.")?;
    let name=output.file_name().and_then(|s|s.to_str()).ok_or("Не найдено имя готового видео.")?;
    let recipe=parent.join(".graffin-edit").join(format!("{name}.json"));
    let data:serde_json::Value=serde_json::from_slice(&bounded_read(&recipe)?).map_err(|_|"Не прочитать данные готового монтажа.")?;
    let recorded=&data["clip"];
    if recorded["topPath"].as_str()!=Some(clip.top_path.as_str())||recorded["outputPath"].as_str()!=Some(clip.output_path.as_str())
        ||recorded["topStart"].as_f64()!=Some(clip.top_start)||recorded["duration"].as_f64()!=Some(clip.duration){return Err("Данные истории принадлежат другому монтажу.".into());}
    let source=&data["sources"][0];
    if source["path"].as_str()!=Some(clip.top_path.as_str()){return Err("Исходник истории не совпадает с монтажом.".into());}
    let hash=source["sha256"].as_str().ok_or("Не найдена контрольная сумма исходника.")?;
    if hash.len()!=64||!hash.bytes().all(|b|b.is_ascii_hexdigit()){return Err("Контрольная сумма истории повреждена.".into());}
    Ok(hash.into())
}
pub(crate) fn record(app:&AppHandle,clip:&ClipPlan,control:&RenderControl)->Result<(),String>{
    if control.is_cancelled(){return Err("Операция отменена. Готовое видео сохранено.".into());}
    let interval=Interval{source_hash:rendered_source_hash(clip)?,start:clip.top_start,end:clip.top_start+clip.duration,output_path:clip.output_path.clone()};
    if !valid(&interval)||!Path::new(&interval.output_path).is_file(){return Err("Готовое видео сохранено, но запись истории не прошла проверку.".into());}
    let _guard=ACCESS.lock().map_err(|_|"История монтажа занята.")?;let path=path(app)?;let mut history=load(&path)?;
    if history.intervals.iter().any(|i|i.source_hash==interval.source_hash&&(i.start-interval.start).abs()<0.0001&&(i.end-interval.end).abs()<0.0001){return Ok(());}
    if history.intervals.len()>=50_000{return Err("История монтажа заполнена. Готовое видео сохранено.".into());}
    history.intervals.push(interval);persist(&path,&history)
}

#[cfg(test)]mod tests{
    use super::*;
    #[test]fn legacy_migration_reads_only_completed_valid_ranges(){
        let root=std::env::temp_dir().join(format!("graffin-render-history-{}-{:x}",std::process::id(),rand::random::<u64>()));fs::create_dir(&root).unwrap();let hash="a".repeat(64);
        fs::write(root.join("queue-v1.json"),serde_json::to_vec(&serde_json::json!({"config":{"caption":"ignored"},"intervals":[{"source_hash":hash,"start":1.0,"end":7.0,"output_path":root.join("one.mp4").to_string_lossy(),"status":"rendered"},{"source_hash":hash,"start":9.0,"end":15.0,"output_path":root.join("two.mp4").to_string_lossy(),"status":"failed"}]})).unwrap()).unwrap();
        let p=root.join("render-history-v1.json");let history=load(&p).unwrap();assert_eq!(history.intervals.len(),1);persist(&p,&history).unwrap();let saved=fs::read_to_string(&p).unwrap();assert!(!saved.contains("caption"));assert_eq!(load(&p).unwrap().intervals[0].start,1.0);fs::remove_dir_all(root).unwrap();
    }
    #[test]fn record_uses_authoritative_completed_recipe_without_reading_moved_source(){
        let root=std::env::temp_dir().join(format!("graffin-history-recipe-{}-{:x}",std::process::id(),rand::random::<u64>()));fs::create_dir(&root).unwrap();fs::create_dir(root.join(".graffin-edit")).unwrap();
        let clip=ClipPlan{index:1,top_path:"/missing/moved-source.mp4".into(),top_start:3.0,bottom_path:"/missing/background.mp4".into(),bottom_start:0.0,duration:5.0,output_path:root.join("ready.mp4").to_string_lossy().into()};
        let mut recipe=serde_json::json!({"clip":clip,"sources":[{"path":clip.top_path,"sha256":"a".repeat(64)}]});
        let path=root.join(".graffin-edit/ready.mp4.json");fs::write(&path,serde_json::to_vec(&recipe).unwrap()).unwrap();assert_eq!(rendered_source_hash(&clip).unwrap(),"a".repeat(64));
        recipe["clip"]["topStart"]=serde_json::json!(99.0);fs::write(path,serde_json::to_vec(&recipe).unwrap()).unwrap();assert!(rendered_source_hash(&clip).is_err());fs::remove_dir_all(root).unwrap();
    }
    #[test]fn fingerprint_tracks_content_across_renames_and_cancels(){
        let root=std::env::temp_dir().join(format!("graffin-hash-{}-{:x}",std::process::id(),rand::random::<u64>()));fs::create_dir(&root).unwrap();let a=root.join("a.mp4");let b=root.join("b.mp4");fs::write(&a,b"one").unwrap();let hash=fingerprint(&a,&RenderControl::new()).unwrap();fs::rename(a,&b).unwrap();assert_eq!(hash,fingerprint(&b,&RenderControl::new()).unwrap());fs::write(&b,b"two").unwrap();assert_ne!(hash,fingerprint(&b,&RenderControl::new()).unwrap());let control=RenderControl::new();control.cancel();assert!(fingerprint(&b,&control).is_err());fs::remove_dir_all(root).unwrap();
    }
}
