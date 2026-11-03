//! Reviewable, locally analysed moments. The UI sends IDs and boundaries only;
//! source paths, transcripts and output names remain authoritative in Rust.
use crate::{engine::{self, BatchRequest, ClipPlan, Media, RenderControl, Settings}, smart_cuts::{self, Candidate, Excluded, Word}, AppState, Job};
use rand::Rng;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{collections::HashSet, fs, path::{Path, PathBuf}, sync::{Arc, Mutex, atomic::Ordering}};
use tauri::{AppHandle, Manager};

const MAX_CANDIDATES: usize = 60;
const MAX_RETAINED_WORDS: usize = 500_000;

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all="camelCase")]
pub struct MomentCandidate {
    pub id: String, pub source_id: String, pub source_index: usize, pub source_name: String,
    pub start: f64, pub end: f64, pub text: String, pub score: f64, pub reasons: Vec<String>,
}
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all="camelCase")]
pub struct MomentSnapshot {
    pub id: String, pub status: String, pub progress: f64, pub message: String,
    pub error: Option<String>, pub sources: Vec<Media>, pub candidates: Vec<MomentCandidate>,
}
impl Default for MomentSnapshot {
    fn default()->Self { Self{id:String::new(),status:"idle".into(),progress:0.0,message:"Добавьте основные видео для поиска моментов".into(),error:None,sources:vec![],candidates:vec![]} }
}
#[derive(Clone)]
struct AnalysedSource { media: Media, hash: String, words: Vec<Word> }
struct Analysis { id: String, language: String, sources: Vec<AnalysedSource>, candidates: Vec<MomentCandidate> }
#[derive(Default)]
struct Inner { snapshot: MomentSnapshot, analysis: Option<Arc<Analysis>>, active: Option<(String, RenderControl)> }
#[derive(Default)]
pub struct MomentsState { inner: Mutex<Inner> }

#[derive(Deserialize)]
#[serde(rename_all="camelCase")]
pub struct AnalyzeRequest {
    pub sources: Vec<Media>, pub target_duration: f64, pub language: String,
    #[serde(default="default_limit")] pub limit: usize,
}
fn default_limit()->usize { MAX_CANDIDATES }
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all="camelCase")]
pub struct MomentSelection { pub candidate_id: String, pub start: f64, pub end: f64 }
#[derive(Deserialize)]
#[serde(rename_all="camelCase")]
pub struct SelectedBatchRequest {
    pub analysis_id: String, pub selection: Vec<MomentSelection>, pub bottom: Vec<Media>,
    pub settings: Settings, #[serde(default)] pub banner_path: String,
}

fn token()->String { format!("{}-{:016x}",crate::stamp(),rand::random::<u64>()) }
fn lock(state:&MomentsState)->std::sync::MutexGuard<'_,Inner> { state.inner.lock().unwrap_or_else(|e|e.into_inner()) }
fn ensure_running(control:&RenderControl)->Result<(),String>{ if control.is_cancelled(){Err("Поиск моментов отменён".into())}else{Ok(())} }
fn overlaps(a:f64,b:f64,c:f64,d:f64)->bool { a<d-0.000_001 && c<b-0.000_001 }

/// Release only after the previous operation has cleared its process handle.
/// No successor can start while `busy` remains true, including after a panic.
struct OperationLease { app: AppHandle, control: RenderControl }
impl Drop for OperationLease {
    fn drop(&mut self) {
        // A panic must not leave an auxiliary encoder/recognizer running after
        // the global operation lock has been released.
        self.control.cancel();
        let state=self.app.state::<AppState>();
        *state.control.lock().unwrap_or_else(|e|e.into_inner())=None;
        state.busy.store(false,Ordering::SeqCst);
    }
}
fn begin_operation(app:&AppHandle,control:&RenderControl)->Result<(),String>{
    let state=app.state::<AppState>();
    if state.closing.load(Ordering::SeqCst){return Err("Приложение завершает работу.".into());}
    state.busy.compare_exchange(false,true,Ordering::SeqCst,Ordering::SeqCst).map_err(|_|"Дождитесь завершения текущей операции.")?;
    *state.control.lock().unwrap_or_else(|e|e.into_inner())=Some(control.clone());
    Ok(())
}
fn validate_analyze(request:&AnalyzeRequest)->Result<(),String>{
    if request.sources.is_empty()||request.sources.len()>40{return Err("Для поиска выберите от 1 до 40 основных видео.".into());}
    if !request.target_duration.is_finite()||!(5.0..=180.0).contains(&request.target_duration){return Err("Длительность поиска: от 5 до 180 секунд.".into());}
    if !["ru","en","auto"].contains(&request.language.as_str()){return Err("Выберите язык распознавания речи.".into());}
    if request.limit==0||request.limit>MAX_CANDIDATES{return Err("Можно показать от 1 до 60 найденных моментов.".into());}
    Ok(())
}
fn reasons(candidate:&Candidate,words:&[Word],target:f64)->Vec<String>{
    let part=words.iter().filter(|word|word.end>candidate.start&&word.start<candidate.end).collect::<Vec<_>>();
    let mut result=vec!["Законченная фраза".into()];
    if part.windows(2).all(|pair|pair[1].start-pair[0].end<0.8){result.push("Без длинных пауз".into());}
    if (candidate.end-candidate.start-target).abs()<=target*0.2{result.push("Близко к нужной длине".into());}
    if candidate.text.contains(['!','?']){result.push("Выразительная реплика".into());}
    result
}
fn analyze_sources(
    id:&str,ffmpeg:&Path,sources:Vec<Media>,request:&AnalyzeRequest,excluded:&[Excluded],cache:&Path,
    control:&RenderControl,mut progress:impl FnMut(&str,f64),
)->Result<Analysis,String>{
    validate_analyze(request)?;
    let mut retained=Vec::new();let mut all=Vec::new();let mut seen=HashSet::new();let mut seen_hashes=HashSet::new();let mut word_count=0usize;
    for(index,media)in sources.iter().enumerate(){
        ensure_running(control)?;
        let canonical=Path::new(&media.path).canonicalize().map_err(|_|"Исходник больше недоступен.")?;
        if !seen.insert(canonical){return Err("Один основной исходник выбран дважды.".into());}
        let words=smart_cuts::transcript_words(ffmpeg,media,&request.language,cache,control,|message,fraction|{
            progress(&format!("{}: {message}",media.name),(index as f64+fraction)/sources.len() as f64);
        })?;
        word_count+=words.len();
        if word_count>MAX_RETAINED_WORDS{return Err("Для одной проверки слишком много речи. Разделите исходники на несколько партий.".into());}
        all.extend(smart_cuts::candidates(&words,index,request.target_duration,media.duration));
        if all.len()>32_768{all.sort_by(|a,b|b.score.total_cmp(&a.score));all.truncate(16_384);}
        let hash=smart_cuts::source_hash(Path::new(&media.path),control)?;
        if !seen_hashes.insert(hash.clone()){return Err("Один и тот же основной исходник выбран под разными именами. Оставьте одну копию.".into());}
        retained.push(AnalysedSource{media:media.clone(),hash,words});
    }
    let mut chosen=smart_cuts::select_candidates(all,excluded,request.limit,true);
    chosen.sort_by(|a,b|b.score.total_cmp(&a.score).then_with(||a.source.cmp(&b.source)).then_with(||a.start.total_cmp(&b.start)));
    let candidates=chosen.into_iter().map(|candidate|{
        let source=&retained[candidate.source];
        let candidate_id=format!("moment-{:x}",Sha256::digest(format!("{id}|{}|{:x}|{:x}",candidate.source,candidate.start.to_bits(),candidate.end.to_bits()).as_bytes()));
        MomentCandidate{id:candidate_id,source_id:source.media.id.clone(),source_index:candidate.source,source_name:source.media.name.clone(),start:candidate.start,end:candidate.end,reasons:reasons(&candidate,&source.words,request.target_duration),text:candidate.text,score:candidate.score}
    }).collect();
    ensure_running(control)?;
    Ok(Analysis{id:id.into(),language:request.language.clone(),sources:retained,candidates})
}

#[tauri::command]
pub fn moments_snapshot(state:tauri::State<MomentsState>)->MomentSnapshot { lock(&state).snapshot.clone() }

fn cancel_matching(inner:&mut Inner,id:&str){
    if inner.snapshot.id==id && ["analyzing","cancelling"].contains(&inner.snapshot.status.as_str()) {
        if let Some((active,control))=&inner.active { if active==id {control.cancel();} }
        inner.snapshot.status="cancelling".into();inner.snapshot.message="Останавливаю поиск моментов…".into();
    }
}
#[tauri::command]
pub fn moments_cancel(id:String,state:tauri::State<MomentsState>)->MomentSnapshot {
    let mut inner=lock(&state);cancel_matching(&mut inner,&id);inner.snapshot.clone()
}

#[tauri::command]
pub fn moments_analyze(app:AppHandle,request:AnalyzeRequest)->Result<MomentSnapshot,String>{
    validate_analyze(&request)?;
    let control=RenderControl::new();begin_operation(&app,&control)?;
    let id=token();
    let snapshot=MomentSnapshot{id:id.clone(),status:"analyzing".into(),message:"Проверяю основные исходники…".into(),..MomentSnapshot::default()};
    {let state=app.state::<MomentsState>();let mut inner=lock(&state);inner.snapshot=snapshot.clone();inner.analysis=None;inner.active=Some((id.clone(),control.clone()));}
    std::thread::spawn(move||{
        let _lease=OperationLease{app:app.clone(),control:control.clone()};
        let result=std::panic::catch_unwind(std::panic::AssertUnwindSafe(||->Result<Analysis,String>{
            let sources=crate::inspect(&app,request.sources.iter().map(|media|media.path.clone()).collect())?;
            let excluded=crate::render_history::exclusions(&app,&sources,&control)?;
            analyze_sources(&id,&crate::resource(&app,"ffmpeg")?,sources,&request,&excluded,&crate::cache(&app)?,&control,|message,fraction|{
                let state=app.state::<MomentsState>();let mut inner=lock(&state);
                if inner.snapshot.id==id && inner.snapshot.status=="analyzing"{inner.snapshot.message=message.into();inner.snapshot.progress=fraction.clamp(0.0,0.995);}
            })
        })).unwrap_or_else(|_|Err("Поиск прервался. Исходники сохранены; повторите анализ.".into()));
        let state=app.state::<MomentsState>();let mut inner=lock(&state);
        if inner.snapshot.id!=id{return;}
        inner.active=None;
        if control.is_cancelled(){inner.snapshot.status="cancelled".into();inner.snapshot.message="Поиск моментов остановлен.".into();}
        else{match result{
            Ok(analysis)=>{inner.snapshot.status="ready".into();inner.snapshot.progress=1.0;inner.snapshot.sources=analysis.sources.iter().map(|source|source.media.clone()).collect();inner.snapshot.candidates=analysis.candidates.clone();inner.snapshot.message=if analysis.candidates.is_empty(){"Для этой длины законченные моменты не найдены. Измените длительность или исходник.".into()}else{format!("Найдено моментов: {}. Выберите подходящие перед сборкой.",analysis.candidates.len())};inner.analysis=Some(Arc::new(analysis));},
            Err(error)=>{inner.snapshot.status="failed".into();inner.snapshot.message="Не удалось найти моменты.".into();inner.snapshot.error=Some(error);}
        }}
    });
    Ok(snapshot)
}

fn validate_selection(analysis:&Analysis,selection:&[MomentSelection],excluded:&[Excluded])->Result<Vec<(usize,f64,f64)>,String>{
    if selection.is_empty()||selection.len()>200{return Err("Выберите от 1 до 200 моментов для сборки.".into());}
    let mut ids=HashSet::new();let mut result:Vec<(usize,f64,f64)>=Vec::new();
    for selected in selection{
        if !ids.insert(&selected.candidate_id){return Err("Один момент выбран дважды.".into());}
        let candidate=analysis.candidates.iter().find(|candidate|candidate.id==selected.candidate_id).ok_or("Список моментов устарел. Повторите поиск.")?;
        let duration=selected.end-selected.start;
        let source=&analysis.sources[candidate.source_index];
        if !selected.start.is_finite()||!selected.end.is_finite()||selected.start<0.0||!(5.0..=180.0).contains(&duration)||selected.end>source.media.duration+0.001{return Err("Начало и конец момента должны быть в исходнике, длительность — от 5 до 180 секунд.".into());}
        if result.iter().any(|(index,start,end)|*index==candidate.source_index&&overlaps(*start,*end,selected.start,selected.end)){return Err("Выбранные моменты одного исходника пересекаются. Измените границы.".into());}
        if excluded.iter().any(|interval|interval.source==candidate.source_index&&overlaps(interval.start,interval.end,selected.start,selected.end)){return Err("Этот фрагмент уже использован в готовом ролике. Выберите другой момент или границы.".into());}
        result.push((candidate.source_index,selected.start,selected.end));
    }
    Ok(result)
}
fn verify_sources(analysis:&Analysis,selection:&[(usize,f64,f64)],control:&RenderControl)->Result<(),String>{
    let mut checked=HashSet::new();
    for(index,_,_)in selection{
        if !checked.insert(*index){continue;}
        let source=&analysis.sources[*index];
        if smart_cuts::source_hash(Path::new(&source.media.path),control)?!=source.hash{return Err("Основной исходник изменился после поиска. Повторите анализ перед сборкой.".into());}
    }
    Ok(())
}
fn plans(analysis:&Analysis,selection:&[(usize,f64,f64)],bottom:&[Media],settings:&Settings,folder:&Path)->Vec<ClipPlan>{
    let mut rng=rand::rng();
    selection.iter().enumerate().map(|(index,(source_index,start,end))|{
        let source=&analysis.sources[*source_index].media;let single=settings.layout_mode=="single";
        let background=if single{source}else{&bottom[if settings.order=="random"{rng.random_range(0..bottom.len())}else{index%bottom.len()}]};
        let maximum=(background.duration-(end-start)).max(0.0);
        let bottom_start=if !single&&settings.order=="random"&&maximum>0.0{rng.random_range(0.0..maximum)}else{0.0};
        ClipPlan{index:(index+1)as u32,top_path:source.path.clone(),top_start:*start,bottom_path:background.path.clone(),bottom_start,duration:end-start,output_path:folder.join(format!("Graffin_{:03}.mp4",index+1)).to_string_lossy().into_owned()}
    }).collect()
}

#[tauri::command]
pub fn start_selected_batch(app:AppHandle,request:SelectedBatchRequest)->Result<Job,String>{
    let analysis={let state=app.state::<MomentsState>();let inner=lock(&state);inner.analysis.as_ref().filter(|analysis|analysis.id==request.analysis_id).cloned().ok_or("Список моментов устарел. Повторите поиск.")?};
    validate_selection(&analysis,&request.selection,&[])?;
    if request.settings.subtitle_language!=analysis.language{return Err("Язык изменён после анализа. Повторите поиск моментов.".into());}
    let original_settings=request.settings.clone();
    let mut settings=request.settings;settings.count=request.selection.len()as u32;settings.cut_mode="smart".into();engine::validate_settings(&settings)?;
    let control=RenderControl::new();begin_operation(&app,&control)?;
    let job=Job{id:token(),status:"running".into(),total:request.selection.len(),completed:0,current:0,progress:0.0,message:"Проверяю выбранные моменты…".into(),outputs:vec![],error:None};
    *app.state::<AppState>().job.lock().unwrap_or_else(|e|e.into_inner())=job.clone();
    std::thread::spawn(move||{
        let _lease=OperationLease{app:app.clone(),control:control.clone()};
        let result=std::panic::catch_unwind(std::panic::AssertUnwindSafe(||->Result<(),String>{
            let sources=analysis.sources.iter().map(|source|source.media.clone()).collect::<Vec<_>>();
            let excluded=crate::render_history::exclusions(&app,&sources,&control)?;
            let selection=validate_selection(&analysis,&request.selection,&excluded)?;
            verify_sources(&analysis,&selection,&control)?;
            let fresh=crate::fresh_request(&app,BatchRequest{top:sources,bottom:request.bottom,settings:settings.clone(),banner_path:request.banner_path})?;
            ensure_running(&control)?;
            let root=PathBuf::from(&settings.output_dir);
            fs::create_dir_all(&root).map_err(|e|format!("Не создать папку экспорта: {e}"))?;
            let folder=root.join(format!("Graffin_{}",token()));fs::create_dir(&folder).map_err(|e|format!("Не создать папку партии: {e}"))?;
            let clips=plans(&analysis,&selection,&fresh.bottom,&settings,&folder);
            let manifest=serde_json::json!({"version":env!("CARGO_PKG_VERSION"),"analysisId":analysis.id,"settings":settings,"selection":request.selection,"clips":clips});
            fs::write(folder.join("manifest.json"),serde_json::to_vec_pretty(&manifest).map_err(|e|e.to_string())?).map_err(|e|e.to_string())?;
            let _=crate::save_settings(app.clone(),original_settings);
            let ffmpeg=crate::resource(&app,"ffmpeg")?;let cache=crate::cache(&app)?;
            for(index,clip)in clips.iter().enumerate(){
                ensure_running(&control)?;
                let source_index=selection[index].0;
                let srt=smart_cuts::subtitles_from_words(&analysis.sources[source_index].words,clip.top_start,clip.duration,&cache,&control)?;
                engine::render_clip_with_subtitles(&ffmpeg,Path::new(&fresh.banner_path),&settings,clip,&control,&srt,|fraction|{
                    let state=app.state::<AppState>();let mut job=state.job.lock().unwrap_or_else(|e|e.into_inner());job.current=index+1;job.progress=(index as f64+fraction.clamp(0.0,1.0))/clips.len()as f64;
                    if job.status=="running"{job.message=format!("Выбранный ролик {} из {}",index+1,clips.len());}
                })?;
                {let state=app.state::<AppState>();let mut job=state.job.lock().unwrap_or_else(|e|e.into_inner());job.completed=index+1;job.outputs.push(clip.output_path.clone());}
                crate::render_history::record(&app,clip,&control)?;
            }
            Ok(())
        })).unwrap_or_else(|_|Err("Сборка прервалась. Уже готовые ролики сохранены.".into()));
        let state=app.state::<AppState>();let mut job=state.job.lock().unwrap_or_else(|e|e.into_inner());
        if control.is_cancelled(){job.status="cancelled".into();job.message=format!("Остановлено. Сохранено роликов: {}",job.completed);}
        else if let Err(error)=result{job.status="failed".into();job.message="Не удалось завершить выбранную сборку.".into();job.error=Some(error);}
        else{job.status="completed".into();job.progress=1.0;job.message=format!("Готово: {} выбранных роликов",job.completed);}
    });
    Ok(job)
}

#[cfg(test)]
mod tests{
    use super::*;
    fn source(path:&Path)->AnalysedSource{AnalysedSource{media:Media{id:"source".into(),path:path.to_string_lossy().into(),name:"Исходник.mp4".into(),duration:60.0,width:320,height:180,has_audio:true,thumbnail:String::new()},hash:String::new(),words:vec![]}}
    fn fixture()->Analysis{Analysis{id:"analysis".into(),language:"ru".into(),sources:vec![source(Path::new("/tmp/source.mp4"))],candidates:vec![MomentCandidate{id:"one".into(),source_id:"source".into(),source_index:0,source_name:"Исходник".into(),start:1.0,end:11.0,text:"Первый момент.".into(),score:1.0,reasons:vec![]},MomentCandidate{id:"two".into(),source_id:"source".into(),source_index:0,source_name:"Исходник".into(),start:20.0,end:30.0,text:"Второй момент.".into(),score:2.0,reasons:vec![]}]}}
    fn selected(id:&str,start:f64,end:f64)->MomentSelection{MomentSelection{candidate_id:id.into(),start,end}}
    fn tempdir()->PathBuf{let path=std::env::temp_dir().join(format!("graffin-moments-{}",token()));fs::create_dir_all(&path).unwrap();path}
    #[test]fn selection_rejects_forged_ids_bad_bounds_overlap_and_history(){
        let a=fixture();assert!(validate_selection(&a,&[selected("forged",1.0,11.0)],&[]).is_err());
        for(start,end)in[(f64::NAN,10.0),(-1.0,10.0),(1.0,f64::INFINITY),(1.0,3.0),(50.0,61.0)]{assert!(validate_selection(&a,&[selected("one",start,end)],&[]).is_err());}
        assert!(validate_selection(&a,&[selected("one",1.0,11.0),selected("one",20.0,30.0)],&[]).is_err());
        assert!(validate_selection(&a,&[selected("one",1.0,11.0),selected("two",10.0,20.0)],&[]).is_err());
        assert!(validate_selection(&a,&[selected("one",1.0,11.0)],&[Excluded{source:0,start:5.0,end:9.0}]).is_err());
        assert!(validate_selection(&a,&[],&[]).is_err());
    }
    #[test]fn explicit_selection_order_and_adjusted_times_reach_render_plan(){
        let a=fixture();let selection=validate_selection(&a,&[selected("two",21.25,31.5),selected("one",0.5,9.0)],&[]).unwrap();
        let settings=Settings{layout_mode:"single".into(),..Settings::default()};let clips=plans(&a,&selection,&[],&settings,Path::new("/tmp/exports"));
        assert_eq!(clips[0].top_start,21.25);assert_eq!(clips[0].duration,10.25);assert_eq!(clips[1].top_start,0.5);assert_eq!(clips[0].bottom_path,clips[0].top_path);assert!(clips[0].output_path.ends_with("Graffin_001.mp4"));
    }
    #[test]fn replaced_content_is_rejected_and_hashing_can_be_cancelled(){
        let dir=tempdir();let path=dir.join("source.mp4");fs::write(&path,b"original").unwrap();let mut a=fixture();a.sources[0].media.path=path.to_string_lossy().into();a.sources[0].hash=smart_cuts::source_hash(&path,&RenderControl::new()).unwrap();
        let selected=vec![(0,1.0,11.0)];assert!(verify_sources(&a,&selected,&RenderControl::new()).is_ok());fs::write(&path,b"replaced").unwrap();assert!(verify_sources(&a,&selected,&RenderControl::new()).is_err());let control=RenderControl::new();control.cancel();assert!(verify_sources(&a,&selected,&control).is_err());fs::remove_dir_all(dir).unwrap();
    }
    #[test]fn cancellation_is_bound_to_the_current_analysis_id(){
        let control=RenderControl::new();
        let mut inner=Inner{snapshot:MomentSnapshot{id:"current".into(),status:"analyzing".into(),..MomentSnapshot::default()},analysis:None,active:Some(("current".into(),control.clone()))};
        cancel_matching(&mut inner,"stale");assert!(!control.is_cancelled());assert_eq!(inner.snapshot.status,"analyzing");
        cancel_matching(&mut inner,"current");assert!(control.is_cancelled());assert_eq!(inner.snapshot.status,"cancelling");
        let successor=RenderControl::new();inner.snapshot.status="ready".into();inner.active=Some(("export".into(),successor.clone()));
        cancel_matching(&mut inner,"current");assert!(!successor.is_cancelled());
    }
    #[test]fn reviewed_words_are_rebased_without_asr_and_request_is_bounded(){
        let dir=tempdir();let control=RenderControl::new();let words=vec![Word{start:10.25,end:10.6,text:"Почему".into()},Word{start:12.0,end:12.5,text:"так?".into()}];
        let file=smart_cuts::subtitles_from_words(&words,10.0,5.0,&dir,&control).unwrap();let cues=smart_cuts::words_from_srt(&fs::read_to_string(file).unwrap(),0.0,5.0);assert_eq!(cues[0].start,0.25);assert_eq!(cues[1].text,"так?");
        let mut request=AnalyzeRequest{sources:vec![source(Path::new("/tmp/source.mp4")).media],target_duration:30.0,language:"ru".into(),limit:60};assert!(validate_analyze(&request).is_ok());request.target_duration=f64::NAN;assert!(validate_analyze(&request).is_err());request.target_duration=30.0;request.limit=61;assert!(validate_analyze(&request).is_err());fs::remove_dir_all(dir).unwrap();
    }
    #[test]
    #[ignore="real offline ASR and selected-range FFmpeg render; set BARSIK_SMART_SOURCE"]
    fn actual_analysis_selected_render_and_exact_cached_captions(){
        let ff=std::env::var_os("BARSIK_FFMPEG").map(PathBuf::from).unwrap_or_else(||PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("resources/ffmpeg"));
        let input=PathBuf::from(std::env::var_os("BARSIK_SMART_SOURCE").expect("BARSIK_SMART_SOURCE"));let dir=tempdir();let control=RenderControl::new();
        let media=engine::probe(&ff,&input,&dir).unwrap();let request=AnalyzeRequest{sources:vec![media.clone()],target_duration:8.0,language:"ru".into(),limit:60};
        let cache=PathBuf::from(env!("CARGO_MANIFEST_DIR")).parent().unwrap().join("qa/graffin24-moments/cache");
        let analysis=analyze_sources("real-test",&ff,vec![media],&request,&[],&cache,&control,|message,_|eprintln!("{message}")).unwrap();assert!(!analysis.candidates.is_empty());let candidate=&analysis.candidates[0];
        let selected=validate_selection(&analysis,&[selected(&candidate.id,candidate.start,candidate.end)],&[]).unwrap();verify_sources(&analysis,&selected,&control).unwrap();
        let settings=Settings{layout_mode:"single".into(),resolution:720,banner_enabled:false,banner_height:0,banner_bottom:0,subtitles_enabled:true,output_dir:dir.to_string_lossy().into(),..Settings::default()};
        let clip=plans(&analysis,&selected,&[],&settings,&dir).remove(0);let srt=smart_cuts::subtitles_from_words(&analysis.sources[0].words,clip.top_start,clip.duration,&cache,&control).unwrap();
        let text=smart_cuts::words_from_srt(&fs::read_to_string(&srt).unwrap(),0.0,clip.duration).iter().map(|word|word.text.as_str()).collect::<Vec<_>>().join(" ");assert_eq!(text,candidate.text);
        engine::render_clip_with_subtitles(&ff,Path::new("/missing/banner"),&settings,&clip,&control,&srt,|_|{}).unwrap();
        assert!((engine::probe(&ff,Path::new(&clip.output_path),&dir).unwrap().duration-clip.duration).abs()<0.08);
        let status=std::process::Command::new(&ff).args(["-v","error","-i"]).arg(&clip.output_path).args(["-f","null","-"]).status().unwrap();assert!(status.success());
        let evidence=serde_json::json!({"actualAsr":true,"selectedStart":clip.top_start,"selectedDuration":clip.duration,"reviewedText":candidate.text,"exactCachedCaptions":true,"secondAsr":false,"outputDecoded":true});fs::create_dir_all(cache.parent().unwrap()).unwrap();fs::write(cache.parent().unwrap().join("acceptance.json"),serde_json::to_vec_pretty(&evidence).unwrap()).unwrap();fs::remove_dir_all(dir).unwrap();
    }
}
