mod platform;
mod engine;
mod download_runtime;
mod render_history;
mod moments;
mod media_tools;
mod banner_library;
mod music_library;
mod smart_cuts;
mod subtitle_editor;
mod studio_presets;
mod font_library;

use engine::{BatchRequest,Media,Settings,RenderControl};
use serde::Serialize;
use std::{fs,path::{Path,PathBuf},sync::{Mutex,atomic::{AtomicBool,Ordering}},time::{SystemTime,UNIX_EPOCH}};
use tauri::{Manager,AppHandle};

#[derive(Clone,Serialize)]
#[serde(rename_all="camelCase")]
struct Job {
    id:String,status:String,total:usize,completed:usize,current:usize,progress:f64,
    message:String,outputs:Vec<String>,error:Option<String>,
}
impl Default for Job {
    fn default()->Self { Self {id:String::new(),status:"idle".into(),total:0,completed:0,current:0,progress:0.,message:"Готов к работе".into(),outputs:vec![],error:None} }
}
#[derive(Default)]
struct AppState {job:Mutex<Job>,control:Mutex<Option<RenderControl>>,busy:AtomicBool,closing:AtomicBool,allow_exit:AtomicBool}
#[derive(Serialize)]
#[serde(rename_all="camelCase")]
struct Bootstrap {default_output:String,default_import_dir:String,banner_path:String,banner_preview:String,version:String,ffmpeg_ready:bool,restored_settings:Option<Settings>}

fn stamp()->String {SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_millis().to_string()}
fn resource(app:&AppHandle,name:&str)->Result<PathBuf,String> {
    let name=platform::resource_name(name);
    let p=app.path().resource_dir().map_err(|e|e.to_string())?.join("resources").join(name);
    if p.is_file(){return Ok(p)}
    if cfg!(debug_assertions){let p=Path::new(env!("CARGO_MANIFEST_DIR")).join("resources").join(name);if p.is_file(){return Ok(p)}}
    Err(format!("Не найден встроенный ресурс: {name}. Перенесите приложение целиком."))
}
fn cache(app:&AppHandle)->Result<PathBuf,String>{let p=app.path().app_cache_dir().map_err(|e|e.to_string())?;fs::create_dir_all(&p).map_err(|e|e.to_string())?;Ok(p)}
fn settings_path(app:&AppHandle)->Result<PathBuf,String>{let p=app.path().app_config_dir().map_err(|e|e.to_string())?;fs::create_dir_all(&p).map_err(|e|e.to_string())?;Ok(p.join("settings.json"))}
fn authorize(app:&AppHandle,path:&Path)->Result<(),String>{app.asset_protocol_scope().allow_file(path).map_err(|e|e.to_string())}
fn valid_media_path(path:&str)->Result<PathBuf,String>{
    let p=Path::new(path).canonicalize().map_err(|_|format!("Файл недоступен: {path}"))?;
    if !p.is_file(){return Err("Выберите видеофайл.".into())}
    let ext=p.extension().and_then(|s|s.to_str()).unwrap_or("").to_ascii_lowercase();
    if !["mp4","mov","mkv","webm","m4v","avi","mpg","mpeg","mts","ts"].contains(&ext.as_str()){return Err(format!("Неподдерживаемый формат файла: {}",p.display()))}
    Ok(p)
}
fn inspect(app:&AppHandle,paths:Vec<String>)->Result<Vec<Media>,String>{
    if paths.is_empty()||paths.len()>200{return Err("Выберите от 1 до 200 файлов.".into())}
    let ff=resource(app,"ffmpeg")?;let folder=cache(app)?;
    paths.into_iter().map(|path|{
        let state=app.state::<AppState>();
        if state.closing.load(Ordering::SeqCst)||state.control.lock().unwrap_or_else(|e|e.into_inner()).as_ref().is_some_and(|c|c.is_cancelled()){return Err("Операция отменена.".into())}
        let p=valid_media_path(&path)?;let m=engine::probe(&ff,&p,&folder)?;
        authorize(app,&p)?;if !m.thumbnail.is_empty(){authorize(app,Path::new(&m.thumbnail))?;}
        Ok(m)
    }).collect()
}

#[tauri::command]
fn bootstrap(app:AppHandle)->Result<Bootstrap,String>{
    let b=resource(&app,"banner.png")?;authorize(&app,&b)?;
    let restored=settings_path(&app).ok().and_then(|p|fs::read(p).ok()).and_then(|x|serde_json::from_slice::<Settings>(&x).ok());
    if let Some(settings) = &restored { banner_library::authorize_saved(&app, settings); }
    let out=app.path().download_dir().or_else(|_|app.path().document_dir()).map_err(|e|e.to_string())?.join("Graffin Studio");
    let imports=out.join("Исходники по ссылкам");
    Ok(Bootstrap{default_output:out.to_string_lossy().into(),default_import_dir:imports.to_string_lossy().into(),banner_path:b.to_string_lossy().into(),banner_preview:b.to_string_lossy().into(),version:env!("CARGO_PKG_VERSION").into(),ffmpeg_ready:resource(&app,"ffmpeg").is_ok(),restored_settings:restored})
}
#[tauri::command]
async fn inspect_media(app:AppHandle,paths:Vec<String>)->Result<Vec<Media>,String>{tauri::async_runtime::spawn_blocking(move||inspect(&app,paths)).await.map_err(|e|e.to_string())?}

fn folder_media_paths(raw:&str)->Result<Vec<String>,String>{
    let folder=Path::new(raw);
    if !folder.is_absolute(){return Err("Выберите папку с полным путём.".into());}
    let folder=folder.canonicalize().map_err(|_|"Папка исходников недоступна.")?;
    if !folder.is_dir(){return Err("Выберите папку с видео.".into());}
    let mut result=Vec::new();
    let mut inspected=0;
    for entry in fs::read_dir(&folder).map_err(|_|"Не прочитать папку исходников.")?{
        inspected+=1;if inspected>20_000{return Err("В папке слишком много файлов. Выберите отдельную папку с видео.".into());}
        let entry=entry.map_err(|_|"Не прочитать файл в папке исходников.")?;
        let path=entry.path();
        let metadata=fs::symlink_metadata(&path).map_err(|_|"Не прочитать файл в папке исходников.")?;
        if !metadata.is_file()||metadata.file_type().is_symlink(){continue;}
        let name=entry.file_name();let name=name.to_string_lossy().to_ascii_lowercase();
        if name.starts_with('.')||name.contains(".partial")||name.contains(".part.")||name.ends_with(".part"){continue;}
        let ext=path.extension().and_then(|s|s.to_str()).unwrap_or("").to_ascii_lowercase();
        if !["mp4","mov","mkv","webm","m4v","avi","mpg","mpeg","mts","ts"].contains(&ext.as_str()){continue;}
        result.push(path.to_string_lossy().into_owned());
        if result.len()>200{return Err("В папке больше 200 видео. Разделите исходники на несколько папок.".into());}
    }
    result.sort();
    if result.is_empty(){return Err("В этой папке нет поддерживаемых видеофайлов.".into());}
    Ok(result)
}
#[tauri::command]
async fn inspect_media_folder(app:AppHandle,path:String)->Result<Vec<Media>,String>{
    tauri::async_runtime::spawn_blocking(move||inspect(&app,folder_media_paths(&path)?)).await.map_err(|_|"Проверка папки прервалась.".to_string())?
}

static SETTINGS_WRITE: Mutex<()> = Mutex::new(());
#[tauri::command]
fn save_settings(app:AppHandle,settings:Settings)->Result<(),String>{
    use std::io::Write;
    let _guard=SETTINGS_WRITE.lock().map_err(|_|"Настройки временно заняты.")?;
    let p=settings_path(&app)?;
    let tmp=p.with_extension(format!("{}.{}.tmp",std::process::id(),stamp()));
    let bytes=serde_json::to_vec_pretty(&settings).map_err(|e|e.to_string())?;
    if bytes.len()>2_000_000{return Err("Слишком много данных в настройках.".into());}
    let result=(||{
        let mut file=fs::OpenOptions::new().write(true).create_new(true).open(&tmp).map_err(|e|e.to_string())?;
        file.write_all(&bytes).map_err(|e|e.to_string())?;file.sync_all().map_err(|e|e.to_string())?;
        fs::rename(&tmp,&p).map_err(|e|e.to_string())
    })();
    if result.is_err(){let _=fs::remove_file(&tmp);}result
}
#[tauri::command]
fn get_job(state:tauri::State<AppState>)->Job{state.job.lock().unwrap_or_else(|e|e.into_inner()).clone()}
#[tauri::command]
fn cancel_batch(state:tauri::State<AppState>)->Job{
    if let Some(c)=state.control.lock().unwrap_or_else(|e|e.into_inner()).as_ref(){c.cancel();}
    let mut j=state.job.lock().unwrap_or_else(|e|e.into_inner());
    if j.status=="running"{j.status="cancelling".into();j.message="Останавливаю экспорт…".into();}
    j.clone()
}
fn fresh_request(app:&AppHandle,mut req:BatchRequest)->Result<BatchRequest,String>{
    req.top=inspect(app,req.top.iter().map(|m|m.path.clone()).collect())?;
    req.bottom=if req.settings.layout_mode=="single" {req.top.first().cloned().into_iter().collect()} else {inspect(app,req.bottom.iter().map(|m|m.path.clone()).collect())?};
    req.banner_path=if req.settings.banner_path.trim().is_empty(){resource(app,"banner.png")?.to_string_lossy().into()}else{Path::new(&req.settings.banner_path).canonicalize().map_err(|_|"Выбранный баннер недоступен. Загрузите его ещё раз.".to_string())?.to_string_lossy().into()};
    if !Path::new(&req.settings.output_dir).is_absolute(){return Err("Выберите папку для готовых роликов.".into())}
    Ok(req)
}

#[tauri::command]
async fn start_batch(app:AppHandle,request:BatchRequest)->Result<Job,String>{
    engine::validate_settings(&request.settings)?;
    let state=app.state::<AppState>();
    if state.busy.swap(true,Ordering::SeqCst){return Err("Дождитесь завершения текущей операции.".into())}
    let control=RenderControl::new();*state.control.lock().unwrap_or_else(|e|e.into_inner())=Some(control.clone());
    let job=Job{id:stamp(),status:"running".into(),total:request.settings.count as usize,completed:0,current:0,progress:0.,message:"Подготавливаю исходники…".into(),outputs:vec![],error:None};
    *state.job.lock().unwrap_or_else(|e|e.into_inner())=job.clone();
    // Return immediately so full-source speech analysis remains visible and
    // cancellable through the same native job controls as rendering.
    std::thread::spawn(move||{
        let state=app.state::<AppState>();
        let result=std::panic::catch_unwind(std::panic::AssertUnwindSafe(||->Result<(),String>{
            let mut req=fresh_request(&app,request)?;
            if control.is_cancelled()||state.closing.load(Ordering::SeqCst){return Err("Операция отменена".into());}
            let ff=resource(&app,"ffmpeg")?;let original=req.settings.clone();
            let root=PathBuf::from(&req.settings.output_dir);fs::create_dir_all(&root).map_err(|e|format!("Не удалось создать папку: {e}"))?;
            let folder=root.join(format!("Graffin_{}",stamp()));fs::create_dir(&folder).map_err(|e|e.to_string())?;
            req.settings.output_dir=folder.to_string_lossy().into();
            let smart=req.settings.cut_mode=="smart";
            let clips=if smart {
                let excluded=render_history::exclusions(&app,&req.top,&control)?;
                smart_cuts::plan_smart(&ff,&req,&excluded,&cache(&app)?,&control,|message,fraction|{
                    let mut j=state.job.lock().unwrap_or_else(|e|e.into_inner());
                    if j.status=="running"{j.message=format!("Ищу законченные моменты. {message}");j.progress=fraction*0.15;}
                })
            }else{engine::plan(&req)};
            let clips=match clips{Ok(clips)=>clips,Err(error)=>{let _=fs::remove_dir(&folder);return Err(error);}};
            let manifest=serde_json::json!({"version":env!("CARGO_PKG_VERSION"),"settings":req.settings,"clips":clips});
            fs::write(folder.join("manifest.json"),serde_json::to_vec_pretty(&manifest).map_err(|e|e.to_string())?).map_err(|e|e.to_string())?;
            let _=save_settings(app.clone(),original);
            let total=clips.len();
            {let mut j=state.job.lock().unwrap_or_else(|e|e.into_inner());j.total=total;j.current=1;}
            for (i,clip) in clips.iter().enumerate(){
                if control.is_cancelled(){break;}
                engine::render_clip(&ff,Path::new(&req.banner_path),&req.settings,clip,&control,|fraction|{
                    let mut j=state.job.lock().unwrap_or_else(|e|e.into_inner());j.current=i+1;
                    let rendered=(i as f64+fraction.clamp(0.,1.))/total as f64;
                    j.progress=if smart{0.15+rendered*0.85}else{rendered};
                    if j.status=="running"{j.message=format!("Ролик {} из {}",i+1,total);}
                })?;
                {let mut j=state.job.lock().unwrap_or_else(|e|e.into_inner());j.completed=i+1;j.outputs.push(clip.output_path.clone());}
                render_history::record(&app,clip,&control)?;
            }
            Ok(())
        })).unwrap_or_else(|_|Err("Обработка прервалась. Готовые ролики сохранены; повторите оставшуюся партию".into()));
        let mut j=state.job.lock().unwrap_or_else(|e|e.into_inner());
        if control.is_cancelled(){j.status="cancelled".into();j.message=format!("Остановлено. Сохранено роликов: {}",j.completed);}
        else if let Err(error)=result{j.status="failed".into();j.message="Не удалось завершить сборку".into();j.error=Some(error);}
        else{j.status="completed".into();j.progress=1.;j.message=format!("Готово: {} роликов",j.completed);}
        drop(j);*state.control.lock().unwrap_or_else(|e|e.into_inner())=None;state.busy.store(false,Ordering::SeqCst);
    });
    Ok(job)
}

#[tauri::command]
async fn render_preview(app:AppHandle,request:BatchRequest)->Result<String,String>{
    let state=app.state::<AppState>();if state.busy.swap(true,Ordering::SeqCst){return Err("Дождитесь завершения текущей операции.".into())}
    let control=RenderControl::new();*state.control.lock().unwrap_or_else(|e|e.into_inner())=Some(control.clone());
    let app2=app.clone();
    let result=tauri::async_runtime::spawn_blocking(move||->Result<String,String>{
        let mut req=fresh_request(&app2,request)?;req.settings.count=1;req.settings.duration=5.;
        let folder=cache(&app2)?.join(format!("preview_{}",stamp()));fs::create_dir(&folder).map_err(|e|e.to_string())?;
        req.settings.output_dir=folder.to_string_lossy().into();
        let mut clips=engine::plan(&req)?;let clip=clips.get_mut(0).ok_or("Нет фрагментов для предпросмотра")?;
        let dest=folder.join("preview.mp4");clip.output_path=dest.to_string_lossy().into();
        engine::render_clip(&resource(&app2,"ffmpeg")?,Path::new(&req.banner_path),&req.settings,clip,&control,|_|{})?;
        authorize(&app2,&dest)?;Ok(dest.to_string_lossy().into())
    }).await.map_err(|e|e.to_string()).and_then(|r|r);
    *state.control.lock().unwrap_or_else(|e|e.into_inner())=None;state.busy.store(false,Ordering::SeqCst);result
}

#[tauri::command]
fn open_folder(path:String)->Result<(),String>{
    let p=PathBuf::from(path);if !p.is_absolute(){return Err("Нужен полный путь к папке.".into())}
    let folder=if p.is_file(){p.parent().ok_or("Папка не найдена")?.to_path_buf()}else{p};
    fs::create_dir_all(&folder).map_err(|e|e.to_string())?;
    platform::open_folder(&folder)
}

fn shutdown(app:AppHandle){
    let s=app.state::<AppState>();if s.closing.swap(true,Ordering::SeqCst){return}
    download_runtime::shutdown();
    media_tools::shutdown();
    if let Some(c)=s.control.lock().unwrap_or_else(|e|e.into_inner()).as_ref(){c.cancel()}
    if let Some(w)=app.get_webview_window("main"){let _=w.hide();}
    if !s.busy.load(Ordering::SeqCst){s.allow_exit.store(true,Ordering::SeqCst);app.exit(0);return}
    std::thread::spawn(move||{for _ in 0..400{if !app.state::<AppState>().busy.load(Ordering::SeqCst){break}std::thread::sleep(std::time::Duration::from_millis(100));}app.state::<AppState>().allow_exit.store(true,Ordering::SeqCst);app.exit(0);});
}

fn native_exit_cleanup(app:&AppHandle){
    let state=app.state::<AppState>();
    if state.allow_exit.load(Ordering::SeqCst){return}
    // macOS Dock Quit and system termination can arrive directly as Exit.
    // Kill the worker child now and allow a short bounded ledger cleanup.
    state.closing.store(true,Ordering::SeqCst);download_runtime::shutdown();media_tools::shutdown();
    if let Some(control)=state.control.lock().unwrap_or_else(|e|e.into_inner()).as_ref(){control.cancel()}
    for _ in 0..300{if !state.busy.load(Ordering::SeqCst){break}std::thread::sleep(std::time::Duration::from_millis(10));}
}

#[cfg(target_os="macos")]
fn install_menu(app:&AppHandle)->tauri::Result<()> {
    use tauri::menu::{Menu,MenuItem};
    let menu=Menu::default(app)?;
    // The native predefined macOS Quit calls NSApplication.terminate directly,
    // bypassing Tauri's cancellable ExitRequested. Use an ordinary menu command
    // so Cmd+Q follows the same cleanup path as the window close button.
    if let Some(submenu)=menu.items()?.first().and_then(|item|item.as_submenu()) {
        let count=submenu.items()?.len();
        if count>0 {submenu.remove_at(count-1)?;}
        let quit=MenuItem::with_id(app,"barsik-quit","Завершить Graffin Studio",true,Some("CmdOrCtrl+Q"))?;
        submenu.append(&quit)?;
    }
    app.set_menu(menu)?;
    Ok(())
}

pub fn run(){
    tauri::Builder::default().plugin(tauri_plugin_dialog::init()).manage(AppState::default())
        .manage(moments::MomentsState::default())
        .setup(|app| { #[cfg(target_os="macos")] install_menu(app.handle())?; let _=font_library::configure(app.handle()); media_tools::configure(app.handle())?; Ok(()) })
        .on_menu_event(|app,event|{if event.id().as_ref()=="barsik-quit" {shutdown(app.clone());}})
        .invoke_handler(tauri::generate_handler![bootstrap,inspect_media,inspect_media_folder,save_settings,get_job,cancel_batch,start_batch,render_preview,open_folder,
            subtitle_editor::subtitle_document,subtitle_editor::subtitle_save,font_library::font_library,font_library::import_font,
            studio_presets::studio_presets,studio_presets::studio_save_preset,studio_presets::studio_delete_preset,
            music_library::music_library,music_library::import_music,banner_library::import_banner,media_tools::media_tools_status,media_tools::prepare_media_tools,media_tools::import_media_url,
            moments::moments_analyze,moments::moments_snapshot,moments::moments_cancel,moments::start_selected_batch])
        .on_window_event(|window,event|{if let tauri::WindowEvent::CloseRequested{api,..}=event {api.prevent_close();shutdown(window.app_handle().clone());}})
        .build(tauri::generate_context!()).expect("Could not start Barsik Studio")
        .run(|app,event|{match event {
            tauri::RunEvent::ExitRequested{api,..}=>{if !app.state::<AppState>().allow_exit.load(Ordering::SeqCst){api.prevent_exit();shutdown(app.clone());}},
            tauri::RunEvent::Exit=>native_exit_cleanup(app),
            _=>{}
        }});
}

#[cfg(test)]
mod folder_tests{
    use super::*;
    fn directory()->PathBuf{let p=std::env::temp_dir().join(format!("graffin-folder-{}-{}-{:x}",std::process::id(),stamp(),rand::random::<u64>()));fs::create_dir_all(&p).unwrap();p}
    #[test]fn scan_is_flat_sorted_and_skips_partial_hidden_and_links(){
        let p=directory();for name in ["b.MP4","a.mov",".hidden.mp4","unfinished.partial.mp4","video.mp4.part","notes.txt"]{fs::write(p.join(name),b"fixture").unwrap();}
        fs::create_dir(p.join("nested")).unwrap();fs::write(p.join("nested/c.mp4"),b"fixture").unwrap();
        #[cfg(unix)]std::os::unix::fs::symlink(p.join("a.mov"),p.join("link.mp4")).unwrap();
        let files=folder_media_paths(p.to_str().unwrap()).unwrap();assert_eq!(files.len(),2);assert!(files[0].ends_with("a.mov"));assert!(files[1].ends_with("b.MP4"));fs::remove_dir_all(p).unwrap();
    }
    #[test]fn scan_rejects_relative_empty_or_oversized_folders(){
        assert!(folder_media_paths("relative").is_err());let p=directory();assert!(folder_media_paths(p.to_str().unwrap()).is_err());
        for i in 0..201{fs::write(p.join(format!("{i}.mp4")),b"fixture").unwrap();}assert!(folder_media_paths(p.to_str().unwrap()).is_err());fs::remove_dir_all(p).unwrap();
    }
}
