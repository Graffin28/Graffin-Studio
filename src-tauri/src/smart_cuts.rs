//! Local speech-boundary clipping. Scores rhetorical cues in the actual
//! transcript; no server or invented semantic/virality score is involved.
use crate::{engine::{self, BatchRequest, ClipPlan, Media, RenderControl}, media_tools};
use rand::{Rng, seq::SliceRandom};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{collections::HashSet, fs::{self, File}, io::{Read, Write}, path::{Path, PathBuf}, time::UNIX_EPOCH};

const WINDOW: f64 = 120.0;
const MAX_SOURCE: f64 = 21_600.0;
const TRANSCRIPT_VERSION: u32 = 1;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Word { pub start: f64, pub end: f64, pub text: String }
#[derive(Clone, Debug)]
pub struct Candidate { pub source: usize, pub start: f64, pub end: f64, pub score: f64, pub text: String }
#[derive(Clone, Copy, Debug)]
pub struct Excluded { pub source: usize, pub start: f64, pub end: f64 }
#[derive(Serialize, Deserialize)]
struct Transcript { version: u32, hash: String, duration: f64, language: String, words: Vec<Word> }

fn cancelled(control: &RenderControl) -> Result<(), String> {
    if control.is_cancelled() { Err("Поиск моментов отменён".into()) } else { Ok(()) }
}
fn stamp(path: &Path) -> Result<(u64,u128), String> {
    let m = fs::symlink_metadata(path).map_err(|_| "Исходник для поиска моментов недоступен")?;
    if !m.is_file() || m.file_type().is_symlink() { return Err("Для поиска моментов нужен обычный видеофайл".into()); }
    Ok((m.len(),m.modified().ok().and_then(|x|x.duration_since(UNIX_EPOCH).ok()).map(|x|x.as_nanos()).unwrap_or(0)))
}
pub(crate) fn source_hash(path: &Path, control: &RenderControl) -> Result<String, String> {
    let before = stamp(path)?; let mut file=File::open(path).map_err(|_|"Не прочитать исходник для поиска моментов")?;
    let mut hash=Sha256::new(); let mut buf=vec![0;1_048_576];
    loop { cancelled(control)?; let read=file.read(&mut buf).map_err(|_|"Ошибка чтения исходника")?; if read==0 {break;} hash.update(&buf[..read]); }
    if stamp(path)?!=before {return Err("Исходник изменился во время анализа. Повторите поиск после копирования".into());}
    Ok(format!("{:x}",hash.finalize()))
}
fn source_directory(cache: &Path, hash: &str) -> PathBuf { cache.join("smart-cuts-v1").join(hash) }
fn parse_time(s: &str) -> Option<f64> {
    let s=s.trim().replace(',',"."); let values=s.split(':').map(str::parse::<f64>).collect::<Result<Vec<_>,_>>().ok()?;
    if values.len()!=3 || values.iter().any(|n|!n.is_finite()||*n<0.0) || values[1]>=60.0 || values[2]>=60.0 {return None;}
    Some(values[0]*3600.0+values[1]*60.0+values[2])
}
pub fn words_from_srt(srt: &str, offset: f64, duration: f64) -> Vec<Word> {
    let normal=srt.trim_start_matches('\u{feff}').replace("\r\n","\n").replace('\r',"\n"); let mut words=vec![];
    for block in normal.split("\n\n") {
        let mut lines=block.lines(); let mut times=None;
        for line in lines.by_ref() { if let Some((a,b))=line.split_once("-->") {times=parse_time(a).zip(parse_time(b)); break;} }
        let Some((from,to))=times else {continue};
        if to<=from || from>=duration || to<=0.0 {continue;}
        let text=lines.collect::<Vec<_>>().join(" "); let tokens=text.split_whitespace().collect::<Vec<_>>();
        if tokens.is_empty() || tokens.len()>500 {continue;}
        let start=from.clamp(0.0,duration);let end=to.clamp(0.0,duration);
        for (i,token) in tokens.iter().enumerate() {
            let a=start+(end-start)*i as f64/tokens.len() as f64;let b=start+(end-start)*(i+1) as f64/tokens.len() as f64;
            if b>a && token.chars().any(char::is_alphanumeric) {words.push(Word{start:offset+a,end:offset+b,text:token.to_string()});}
        }
    }
    words.sort_by(|a,b|a.start.total_cmp(&b.start).then_with(||a.end.total_cmp(&b.end)));
    words
}
fn append_window(words:&mut Vec<Word>,mut next:Vec<Word>,boundary:f64){
    // Recognition includes context around each window. If one and the same
    // word was clipped exactly on both sides of that boundary, retain it once
    // with its complete time span. Ordinary repeated words remain separate.
    if let (Some(last),Some(first))=(words.last_mut(),next.first()){
        let normalized=|text:&str|text.chars().filter(|c|c.is_alphanumeric()).flat_map(char::to_lowercase).collect::<String>();
        if (last.end-boundary).abs()<0.000_001&&(first.start-boundary).abs()<0.000_001&&normalized(&last.text)==normalized(&first.text){
            last.end=first.end.max(last.end);if terminal(&first.text){last.text=first.text.clone();}next.remove(0);
        }
    }
    words.extend(next);
}

fn valid_words(words: &[Word], duration: f64) -> bool {
    words.len()<=250_000 && words.windows(2).all(|w|w[0].start<=w[1].start) && words.iter().all(|w| w.start.is_finite()&&w.end.is_finite()&&w.start>=0.0&&w.end>w.start&&w.end<=duration+0.01&&!w.text.is_empty()&&w.text.len()<4096)
}
pub fn transcript_words(ff: &Path, media: &Media, language: &str, cache: &Path, control: &RenderControl, mut on_status: impl FnMut(&str, f64)) -> Result<Vec<Word>, String> {
    cancelled(control)?;
    if !["ru","en","auto"].contains(&language){return Err("Неизвестный язык поиска моментов".into());}
    if !media.has_audio {return Err(format!("В «{}» нет речи: выберите исходник со звуком или режим по времени",media.name));}
    if !media.duration.is_finite() || media.duration<=0.0 || media.duration>MAX_SOURCE {return Err("Поиск по речи поддерживает исходники до 6 часов. Разделите длинный исходник или выберите нарезку по времени".into());}
    let source=fs::canonicalize(&media.path).map_err(|_|"Исходник поиска недоступен")?;let before=stamp(&source)?;
    let hash=source_hash(&source,control)?;let dir=source_directory(cache,&hash);fs::create_dir_all(&dir).map_err(|_|"Не подготовить кэш поиска моментов")?;
    let destination=dir.join(format!("transcript-{language}.json"));
    if destination.is_file() && destination.metadata().map(|x|x.len()<32_000_000).unwrap_or(false) {
        if let Ok(t)=fs::read(&destination).ok().and_then(|b|serde_json::from_slice::<Transcript>(&b).ok()).ok_or(()) {
            if t.version==TRANSCRIPT_VERSION&&t.hash==hash&&t.language==language&&(t.duration-media.duration).abs()<0.02&&valid_words(&t.words,media.duration) {on_status("Речь найдена в кэше",1.0);return Ok(t.words);}
        }
    }
    let windows=(media.duration/WINDOW).ceil() as usize;let mut words=vec![];
    for index in 0..windows {
        cancelled(control)?;
        let start=index as f64*WINDOW;let length=(media.duration-start).min(WINDOW);
        let message=format!("Распознаю речь: {} из {} частей",index+1,windows);on_status(&message,index as f64/windows as f64);
        // A source-content directory also guards the underlying ASR cache from
        // a replaced source retaining its former path/size/modification time.
        let srt=media_tools::transcribe_clip(ff,&source,start,length,language,&dir,control)?;
        let text=fs::read_to_string(srt).map_err(|_|"Не прочитать распознанную речь")?;
        append_window(&mut words,words_from_srt(&text,start,length),start);
        if words.len()>250_000 {return Err("В исходнике слишком много слов. Разделите его на части".into());}
    }
    cancelled(control)?;
    if stamp(&source)?!=before || source_hash(&source,control)?!=hash {return Err("Исходник изменился во время распознавания. Повторите поиск".into());}
    words.sort_by(|a,b|a.start.total_cmp(&b.start));
    // Window context can occasionally yield overlapping repetitions at a seam.
    words.dedup_by(|b,a|a.text==b.text&&(a.start-b.start).abs()<0.025);
    if !valid_words(&words,media.duration) {return Err("Получены некорректные времена речи. Повторите поиск".into());}
    let temporary=dir.join(format!(".transcript-{:016x}.tmp",rand::random::<u64>()));
    let data=serde_json::to_vec(&Transcript{version:TRANSCRIPT_VERSION,hash,duration:media.duration,language:language.into(),words:words.clone()}).map_err(|_|"Не сохранить поиск моментов")?;
    let saved=(||->Result<(),String>{let mut file=File::create(&temporary).map_err(|e|e.to_string())?;file.write_all(&data).and_then(|_|file.sync_all()).map_err(|e|e.to_string())?;cancelled(control)?;fs::rename(&temporary,&destination).map_err(|e|e.to_string())?;Ok(())})();
    if temporary.exists(){let _=fs::remove_file(temporary);} saved?;
    on_status("Речь распознана",1.0);Ok(words)
}
fn clipped_word_srt(words:&[Word],start:f64,duration:f64)->String{
    let mut result=String::new();let mut index=0;
    let stamp=|value:u64|format!("{:02}:{:02}:{:02},{:03}",value/3_600_000,value/60_000%60,value/1000%60,value%1000);
    let end=start+duration;
    for word in words{
        if !word.start.is_finite()||!word.end.is_finite()||word.end<=start||word.start>=end||word.end<=word.start||word.text.split_whitespace().count()!=1{continue;}
        let from=((word.start-start).clamp(0.0,duration)*1000.0).round() as u64;
        let to=((word.end-start).clamp(0.0,duration)*1000.0).round() as u64;
        if to<=from{continue;}
        index+=1;result.push_str(&format!("{index}\n{} --> {}\n{}\n\n",stamp(from),stamp(to),word.text));
    }
    result
}
/// Burn exactly the words used to select this smart moment. Running a second
/// short-context recognition pass could otherwise disagree with its boundaries.
/// The planner and renderer must share this same persistent subtitle cache.
pub fn clip_subtitles(ff:&Path,input:&Path,source_duration:f64,start:f64,duration:f64,language:&str,cache:&Path,control:&RenderControl)->Result<PathBuf,String>{
    cancelled(control)?;
    if !source_duration.is_finite()||source_duration<=0.0||!start.is_finite()||start<0.0||!duration.is_finite()||duration<=0.0||start+duration>source_duration+0.01{
        return Err("Некорректные границы умного фрагмента для субтитров".into());
    }
    let media=Media{id:String::new(),path:input.to_string_lossy().into_owned(),name:input.file_name().unwrap_or_default().to_string_lossy().into_owned(),duration:source_duration,width:0,height:0,has_audio:true,thumbnail:String::new()};
    let words=transcript_words(ff,&media,language,cache,control,|_,_|{})?;
    subtitles_from_words(&words,start,duration,cache,control)
}

pub(crate) fn subtitles_from_words(words:&[Word],start:f64,duration:f64,cache:&Path,control:&RenderControl)->Result<PathBuf,String>{
    cancelled(control)?;
    if !start.is_finite()||start<0.0||!duration.is_finite()||duration<=0.0||duration>180.0 {return Err("Некорректные границы выбранного момента".into());}
    let text=clipped_word_srt(words,start,duration);
    let key=format!("smart-selected-words-v1|{duration:.6}|{text}");let digest=format!("{:x}",Sha256::digest(key.as_bytes()));
    fs::create_dir_all(cache).map_err(|_|"Не подготовить субтитры выбранного момента")?;
    let destination=cache.join(format!("smart-clip-{digest}.srt"));
    if fs::read_to_string(&destination).ok().as_deref()==Some(text.as_str()){return Ok(destination);}
    let temporary=cache.join(format!(".smart-clip-{:016x}.tmp",rand::random::<u64>()));
    let saved=(||->Result<(),String>{
        let mut file=fs::OpenOptions::new().write(true).create_new(true).open(&temporary).map_err(|_|"Не записать субтитры выбранного момента")?;
        file.write_all(text.as_bytes()).and_then(|_|file.sync_all()).map_err(|_|"Не сохранить субтитры выбранного момента")?;
        cancelled(control)?;fs::rename(&temporary,&destination).map_err(|_|"Не завершить сохранение субтитров момента")?;Ok(())
    })();
    if temporary.exists(){let _=fs::remove_file(temporary);}saved?;Ok(destination)
}

#[derive(Clone)]
struct Sentence { first: usize, last: usize, complete: bool }
fn terminal(text: &str) -> bool {
    text.trim_end_matches(['"','\'','»',')',']']).ends_with(['.','!','?','…'])
}
fn sentences(words: &[Word], source_duration: f64) -> Vec<Sentence> {
    let mut output=vec![];let mut first=0;
    for index in 0..words.len() {
        let pause=words.get(index+1).map(|next|next.start-words[index].end).unwrap_or(source_duration-words[index].end);
        let stop=terminal(&words[index].text)||pause>=0.8;
        if stop { output.push(Sentence{first,last:index,complete:true});first=index+1; }
    }
    // A trailing unfinished phrase is deliberately not a candidate endpoint.
    if first<words.len(){output.push(Sentence{first,last:words.len()-1,complete:false});}
    output
}
fn score_words(words: &[Word], target: f64, duration: f64) -> f64 {
    let tokens=normalized_tokens(&words.iter().map(|w|w.text.as_str()).collect::<Vec<_>>().join(" "));
    if tokens.is_empty() || !duration.is_finite() || duration<=0.0 || !target.is_finite() || target<=0.0 {return f64::NEG_INFINITY;}
    // These are explainable speech cues, not an emotion or virality model.
    // Whole-token matching avoids rewarding e.g. "otherwise" for "why".
    let opening=&tokens[..tokens.len().min(6)];
    let hook=if has_phrase(opening,&["почему","представь","представьте","прикол","знаешь","самое","вдруг","смотри","слушай","погоди","оказалось","зачем","что если","what if","why","imagine","listen"]){1.5}else{0.0};
    let payoff=if has_phrase(&tokens,&["но","оказалось","на самом деле","в итоге","вместо","but","turns out","instead"]){0.65}else{0.0};
    let punctuation=words.iter().filter(|w|w.text.contains(['!','?'])).count().min(2) as f64*0.25;
    let unique=tokens.iter().collect::<HashSet<_>>().len() as f64/tokens.len() as f64;
    let rate=tokens.len() as f64/duration;
    // Normal speech density helps; implausibly fast ASR output should not win.
    let density=(rate/2.5).min(1.0)*0.7-((rate-4.8)/2.0).clamp(0.0,1.5);
    let gaps=words.windows(2).map(|w|(w[1].start-w[0].end).max(0.0)).collect::<Vec<_>>();
    let quiet=gaps.iter().filter(|gap|**gap>=0.75).sum::<f64>()/duration;
    let longest=gaps.iter().copied().fold(0.0,f64::max);
    let pause_penalty=quiet*2.2+((longest-2.0).max(0.0)/duration)*1.2;
    let repeated_pairs=tokens.windows(2).filter(|pair|pair[0]==pair[1]).count() as f64/tokens.len() as f64;
    let trigrams=tokens.windows(3).collect::<Vec<_>>();
    let repeated_phrases=if trigrams.is_empty(){0.0}else{1.0-trigrams.iter().copied().collect::<HashSet<_>>().len() as f64/trigrams.len() as f64};
    let fillers=tokens.iter().filter(|word|matches!(word.as_str(),"ээ"|"эээ"|"эм"|"мм"|"ну"|"вот"|"короче"|"типа"|"uh"|"um"|"erm")).count() as f64/tokens.len() as f64;
    let repetition_penalty=repeated_pairs*3.0+repeated_phrases*2.4+(0.4-unique).max(0.0)*2.0+fillers*1.5;
    let context_penalty=if hook==0.0&&matches!(tokens[0].as_str(),"поэтому"|"также"|"потому"|"because"|"therefore"|"also"){0.45}else{0.0};
    let fit=(1.0-(duration-target).abs()/target).clamp(0.0,1.0);
    let boilerplate=if has_phrase(&tokens,&["в данном видеоролике","в этом видео","на этом канале","спасибо за просмотр","подписывайтесь","ставьте лайк","не забудьте подписаться","welcome to my channel","thanks for watching"]){1.8}else{0.0};
    hook+payoff+punctuation+unique*0.5+density+fit*1.2-boilerplate-pause_penalty-repetition_penalty-context_penalty
}
fn normalized_tokens(text:&str)->Vec<String>{
    text.split(|c:char|!c.is_alphanumeric()).filter(|word|!word.is_empty()).map(|word|word.to_lowercase().replace('ё',"е")).collect()
}
fn has_phrase(tokens:&[String],phrases:&[&str])->bool{
    phrases.iter().any(|phrase|{let cue=phrase.split_whitespace().collect::<Vec<_>>();tokens.windows(cue.len()).any(|part|part.iter().zip(&cue).all(|(a,b)|a==b))})
}
fn topic_signature(text:&str)->HashSet<String>{
    normalized_tokens(text).into_iter().filter(|word|word.chars().count()>2&&!matches!(word.as_str(),"это"|"как"|"что"|"так"|"для"|"при"|"его"|"она"|"они"|"был"|"была"|"были"|"the"|"and"|"that"|"this"|"with"|"for"|"was"|"are"|"you")).collect()
}
fn topic_redundancy(a:&HashSet<String>,b:&HashSet<String>)->f64{
    if a.len()<4||b.len()<4{return 0.0;}
    let common=a.intersection(b).count() as f64;let union=(a.len()+b.len()) as f64-common;
    ((common/union-0.4)/0.6).clamp(0.0,1.0)*2.2
}
pub fn candidates(words: &[Word], source: usize, target: f64, source_duration: f64) -> Vec<Candidate> {
    if !target.is_finite()||target<=0.0||!source_duration.is_finite()||source_duration<=0.0||!valid_words(words,source_duration){return vec![];}
    let sentences=sentences(words,source_duration);let mut output=vec![];
    let minimum=(target*0.65).max(5.0);let maximum=(target*1.5).min(180.0);
    for i in 0..sentences.len() {
        // The first actual word is a boundary; subsequent starts follow a
        // recognized punctuation or a real pause, never a fixed time grid.
        for j in i..sentences.len() {
            let a=&sentences[i];let b=&sentences[j];
            let before=if a.first>0 {(words[a.first].start-words[a.first-1].end).max(0.0)/2.0}else{words[a.first].start};
            let after=words.get(b.last+1).map(|w|(w.start-words[b.last].end).max(0.0)/2.0).unwrap_or(source_duration-words[b.last].end);
            let start=(words[a.first].start-before.min(0.035)).max(0.0);let end=(words[b.last].end+after.min(0.07)).min(source_duration);
            let duration=end-start;if duration>maximum {break;}if duration<minimum||!b.complete {continue;}
            let part=&words[a.first..=b.last];if part.len()<4 {continue;}
            let text=part.iter().map(|w|w.text.as_str()).collect::<Vec<_>>().join(" ");
            output.push(Candidate{source,start,end,score:score_words(part,target,duration),text});
            // Keep long-source analysis bounded without discarding later
            // high-ranked moments. Selection still evaluates topical diversity.
            if output.len() >= 8192 {
                output.sort_by(|a,b|b.score.total_cmp(&a.score).then_with(||a.start.total_cmp(&b.start)));
                output.truncate(4096);
            }
        }
    }
    output
}
fn overlaps(a: f64,b: f64,c: f64,d: f64)->bool{a<d-0.000_001&&c<b-0.000_001}
pub fn select_candidates(mut all: Vec<Candidate>, excluded: &[Excluded], count: usize, chronological: bool) -> Vec<Candidate> {
    if count==0 {return vec![];}
    all.retain(|c| c.start.is_finite()&&c.end.is_finite()&&c.score.is_finite()&&c.start>=0.0&&c.end>c.start&&!excluded.iter().any(|e|e.source==c.source&&overlaps(c.start,c.end,e.start,e.end)));
    all.sort_by(|a,b|b.score.total_cmp(&a.score).then_with(||a.source.cmp(&b.source)).then_with(||a.start.total_cmp(&b.start)));
    // Update each candidate's novelty once per chosen clip. This remains linear
    // in candidate count per selection, rather than comparing every pair.
    let mut ranked=all.iter().map(|candidate|(topic_signature(&candidate.text),0.0_f64,true)).collect::<Vec<_>>();
    let mut selected:Vec<Candidate>=vec![];
    while selected.len()<count {
        let next=(0..all.len()).filter(|i|ranked[*i].2).max_by(|a,b|(all[*a].score-ranked[*a].1).total_cmp(&(all[*b].score-ranked[*b].1)).then_with(||b.cmp(a)));
        let Some(next)=next else{break};
        ranked[next].2=false;let item=&all[next];
        for index in 0..all.len(){
            if !ranked[index].2{continue;}
            if all[index].source==item.source&&overlaps(all[index].start,all[index].end,item.start,item.end){ranked[index].2=false;}
            else{ranked[index].1=ranked[index].1.max(topic_redundancy(&ranked[next].0,&ranked[index].0));}
        }
        selected.push(item.clone());
    }
    if selected.len()<count {
        // Earliest-finish interval scheduling finds the actual maximum count
        // when a high-ranked long utterance would crowd out shorter moments.
        let mut short=all;short.sort_by(|a,b|a.source.cmp(&b.source).then_with(||a.end.total_cmp(&b.end)).then_with(||b.score.total_cmp(&a.score)));
        let mut maximum:Vec<Candidate>=vec![];
        for item in short { if maximum.iter().all(|x|x.source!=item.source||!overlaps(x.start,x.end,item.start,item.end)) {maximum.push(item);if maximum.len()>=count{break;}} }
        if maximum.len()>selected.len(){selected=maximum;}
    }
    if chronological {selected.sort_by(|a,b|a.source.cmp(&b.source).then_with(||a.start.total_cmp(&b.start)));}
    else {selected.shuffle(&mut rand::rng());}
    selected
}
pub fn analyze(ff:&Path, sources:&[Media], target:f64, language:&str, cache:&Path, control:&RenderControl, mut on_status:impl FnMut(&str,f64))->Result<Vec<Candidate>,String>{
    if sources.is_empty(){return Err("Добавьте основное видео для поиска моментов".into());}
    let mut output=vec![];let mut seen=HashSet::new();
    for (index,media) in sources.iter().enumerate(){
        cancelled(control)?;let canonical=fs::canonicalize(&media.path).map_err(|_|"Исходник поиска недоступен")?;
        if !seen.insert(canonical){return Err("Один основной исходник выбран дважды".into());}
        let words=transcript_words(ff,media,language,cache,control,|message,progress|on_status(&format!("{}: {message}",media.name),(index as f64+progress)/sources.len() as f64))?;
        output.extend(candidates(&words,index,target,media.duration));
    }
    cancelled(control)?;Ok(output)
}
pub fn plan_smart(ff:&Path, request:&BatchRequest, excluded:&[Excluded], cache:&Path, control:&RenderControl, on_status:impl FnMut(&str,f64))->Result<Vec<ClipPlan>,String>{
    engine::validate_settings(&request.settings)?;
    let single=request.settings.layout_mode=="single";
    if !single&&request.bottom.is_empty(){return Err("Добавьте фоновое видео или выберите режим без фона".into());}
    let all=analyze(ff,&request.top,request.settings.duration,&request.settings.subtitle_language,cache,control,on_status)?;
    let selected=select_candidates(all,excluded,request.settings.count as usize,request.settings.order=="sequential");
    if selected.len()<request.settings.count as usize {return Err(format!("Найдено законченных моментов без повторов: {}. Уменьшите количество или измените длительность",selected.len()));}
    let mut rng=rand::rng();let mut clips=vec![];
    for (index,candidate) in selected.into_iter().enumerate(){
        let source=&request.top[candidate.source];
        // The renderer's single-video branch ignores the second input. Keep
        // the same placeholder convention as the fixed-time planner.
        let background=if single{source}else{&request.bottom[if request.settings.order=="random"{rng.random_range(0..request.bottom.len())}else{index%request.bottom.len()}]};
        let duration=candidate.end-candidate.start;let max=(background.duration-duration).max(0.0);let bottom_start=if !single&&request.settings.order=="random"&&max>0.0{rng.random_range(0.0..max)}else{0.0};
        let output=Path::new(&request.settings.output_dir).join(format!("Graffin_{:03}.mp4",index+1));if output.exists(){return Err("Файл экспорта уже существует. Выберите новую папку".into());}
        clips.push(ClipPlan{index:(index+1)as u32,top_path:request.top[candidate.source].path.clone(),top_start:candidate.start,bottom_path:background.path.clone(),bottom_start,duration,output_path:output.to_string_lossy().into_owned()});
    }
    Ok(clips)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn words(phrases:&[(&str,f64)])->Vec<Word>{let mut t=0.1;let mut words=vec![];for (phrase,pause) in phrases{for token in phrase.split_whitespace(){words.push(Word{start:t,end:t+0.4,text:token.into()});t+=0.45;}t+=pause;}words}
    #[test]fn srt_words_rebase_and_reject_bad_times(){let s="1\r\n00:00:00,100 --> 00:00:00,900\r\nПочему так?\r\n\r\n2\r\n00:00:02,000 --> 00:00:01,000\r\nнет\r\n";let result=words_from_srt(s,120.0,10.0);assert_eq!(result.len(),2);assert!((result[0].start-120.1).abs()<0.0001);assert_eq!(result[1].text,"так?");}
    #[test]fn window_context_keeps_one_clipped_word_but_preserves_real_repetition(){
        let mut previous=vec![Word{start:119.7,end:120.,text:"проверка".into()}];append_window(&mut previous,vec![Word{start:120.,end:120.3,text:"проверка.".into()},Word{start:120.4,end:120.7,text:"Дальше".into()}],120.);assert_eq!(previous.len(),2);assert_eq!(previous[0].start,119.7);assert_eq!(previous[0].end,120.3);assert_eq!(previous[0].text,"проверка.");
        let mut repeated=vec![Word{start:119.7,end:119.99,text:"да".into()}];append_window(&mut repeated,vec![Word{start:120.01,end:120.3,text:"да".into()}],120.);assert_eq!(repeated.len(),2);
    }
    #[test]fn candidates_never_end_on_unfinished_phrase(){let w=words(&[("Почему этот кот внезапно забрал весь мой домашний интернет прямо перед важной работой?",0.2),("Я думал это шутка но оказалось что",0.0)]);let all=candidates(&w,0,5.0,w.last().unwrap().end);assert!(!all.is_empty());for c in all{assert!(c.text.ends_with('?'));assert!(c.end<=w[13].end+0.071);}}
    #[test]fn candidate_starts_follow_sentence_boundaries(){let w=words(&[("Это обычное утро и ничего не происходит.",0.2),("Представь мой кот внезапно открыл свой бизнес!",0.2),("Но оказалось что клиентами были голуби во дворе.",1.0)]);let all=candidates(&w,0,6.0,w.last().unwrap().end+1.0);assert!(!all.is_empty());let starts=sentences(&w,w.last().unwrap().end+1.0).iter().map(|s|(w[s.first].start-0.035).max(0.0)).collect::<Vec<_>>();for c in all{assert!(starts.iter().any(|s|(s-c.start).abs()<0.0001));assert!((5.0..=9.0).contains(&(c.end-c.start)));}}
    #[test]fn rhetorical_opening_scores_above_neutral_equal_length(){let hook=words(&[("Почему этот кот открыл свой бизнес сегодня?",0.0)]);let neutral=words(&[("Сегодня этот кот просто ходил по квартире.",0.0)]);assert!(score_words(&hook,6.0,6.0)>score_words(&neutral,6.0,6.0));}
    #[test]fn rhetorical_cues_match_words_and_normalize_punctuation(){
        assert!(has_phrase(&normalized_tokens("ПОЧЕМУ, всё оказалось иначе?"),&["почему"]));
        assert!(has_phrase(&normalized_tokens("What, if we try again?"),&["what if"]));
        assert!(!has_phrase(&normalized_tokens("Somewhat iffy otherwise."),&["what if","why"]));
        assert_eq!(normalized_tokens("Ещё ещё!"),vec!["еще","еще"]);
    }
    #[test]fn internal_silence_lowers_rank_at_equal_length_and_word_count(){
        let continuous=words(&[("Представь обычный кот внезапно открыл свой бизнес но оказалось что клиентами стали голуби во дворе!",0.0)]);
        let mut paused=continuous.clone();for word in &mut paused[6..]{word.start+=6.0;word.end+=6.0;}
        assert!(score_words(&continuous,15.0,15.0)>score_words(&paused,15.0,15.0)+0.8);
    }
    #[test]fn repeated_asr_phrases_and_filler_do_not_outrank_substantive_speech(){
        let substance=words(&[("Почему обычный кот открыл бизнес сегодня но оказалось что клиентами стали голуби прямо во дворе?",0.0)]);
        let repeated=words(&[("Почему кот открыл бизнес почему кот открыл бизнес почему кот открыл бизнес почему кот открыл бизнес?",0.0)]);
        let filler=words(&[("Почему ну вот короче типа ну вот короче типа ну вот короче типа ну вот?",0.0)]);
        assert!(score_words(&substance,9.0,9.0)>score_words(&repeated,9.0,9.0)+0.8);
        assert!(score_words(&substance,9.0,9.0)>score_words(&filler,9.0,9.0)+1.0);
    }
    #[test]fn batch_prefers_a_different_story_to_punctuation_variants_of_the_same_phrase(){
        let make=|start,score,text:&str|Candidate{source:0,start,end:start+10.0,score,text:text.into()};
        let selected=select_candidates(vec![
            make(0.0,5.0,"Почему рыжий кот открыл собственный бизнес возле нашего дома?"),
            make(20.0,4.9,"Почему, рыжий кот открыл собственный бизнес возле нашего дома!"),
            make(40.0,4.4,"Вчера космонавты впервые обнаружили удивительный ледяной вулкан далеко за Юпитером."),
        ],&[],2,true);
        assert_eq!(selected.iter().map(|c|c.start).collect::<Vec<_>>(),vec![0.0,40.0]);
    }
    #[test]fn invalid_timestamps_and_nonfinite_scores_are_not_candidates(){
        let w=words(&[("Почему этот кот открыл собственный бизнес сегодня?",0.0)]);
        assert!(candidates(&w,0,f64::NAN,20.0).is_empty());
        assert!(candidates(&w,0,6.0,f64::INFINITY).is_empty());
        let mut invalid=w.clone();invalid[0].start=f64::NAN;assert!(candidates(&invalid,0,6.0,20.0).is_empty());
        let selected=select_candidates(vec![Candidate{source:0,start:0.0,end:10.0,score:f64::NAN,text:String::new()}],&[],1,true);
        assert!(selected.is_empty());
    }
    #[test]fn selection_excludes_used_ranges_and_never_repeats(){let make=|s,a,b,score|Candidate{source:s,start:a,end:b,score,text:String::new()};let all=vec![make(0,0.,10.,10.),make(0,5.,15.,20.),make(0,10.,20.,9.),make(0,20.,30.,8.),make(1,0.,10.,7.)];let selected=select_candidates(all,&[Excluded{source:0,start:5.,end:15.}],10,true);assert_eq!(selected.len(),2);assert!(selected.iter().any(|x|x.source==1));for (i,a) in selected.iter().enumerate(){for b in &selected[i+1..]{assert!(a.source!=b.source||!overlaps(a.start,a.end,b.start,b.end));}}}
    #[test]fn maximum_count_fallback_avoids_long_hook_crowding(){let make=|a,b,score|Candidate{source:0,start:a,end:b,score,text:String::new()};let selected=select_candidates(vec![make(0.,20.,10.),make(0.,10.,3.),make(10.,20.,3.)],&[],2,true);assert_eq!(selected.len(),2);}
    #[test]
    #[ignore="requires BARSIK_FFMPEG and BARSIK_SMART_SOURCE; performs real offline ASR"]
    fn actual_speech_boundaries_and_cached_analysis(){
        let ff=PathBuf::from(std::env::var("BARSIK_FFMPEG").unwrap());let input=PathBuf::from(std::env::var("BARSIK_SMART_SOURCE").unwrap());
        let dir=PathBuf::from(std::env::var("BARSIK_SMART_QA").unwrap_or_else(|_|std::env::temp_dir().join(format!("graffin-smart-smoke-{}",std::process::id())).to_string_lossy().into_owned()));fs::create_dir_all(&dir).unwrap();
        let duration=std::env::var("BARSIK_SMART_SECONDS").ok().and_then(|v|v.parse().ok()).unwrap_or(35.0);
        let media=Media{id:"qa".into(),path:input.to_string_lossy().into_owned(),name:"Проверка распознавания".into(),duration,width:1080,height:608,has_audio:true,thumbnail:String::new()};
        let control=RenderControl::new();let words=transcript_words(&ff,&media,"ru",&dir,&control,|message,_|eprintln!("{message}")).unwrap();assert!(words.len()>15);let all=candidates(&words,0,8.0,duration);let selected=select_candidates(all,&[],3,true);assert!(!selected.is_empty(),"complete speech moments must exist");
        let sentences=sentences(&words,duration);
        for c in &selected{assert!(c.start>=0.0&&c.end<=duration);assert!(sentences.iter().any(|s|s.complete&&(c.end-words[s.last].end).abs()<=0.071));}
        let mut cached=false;let again=transcript_words(&ff,&media,"ru",&dir,&control,|message,_|cached|=message.contains("кэше")).unwrap();assert!(cached);assert_eq!(words.len(),again.len());
        // A deliberately nonexistent encoder proves that this step uses the full
        // cached transcript, rather than attempting short-context ASR again.
        let selected_first=&selected[0];let caption_file=clip_subtitles(Path::new("/missing/ffmpeg-must-not-be-used"),&input,duration,selected_first.start,selected_first.end-selected_first.start,"ru",&dir,&control).unwrap();
        let caption_text=fs::read_to_string(caption_file).unwrap();let exact=words_from_srt(&caption_text,0.,selected_first.end-selected_first.start);assert_eq!(exact.iter().map(|w|w.text.as_str()).collect::<Vec<_>>().join(" "),selected_first.text);
        let settings=engine::Settings{count:1,duration:8.0,layout_mode:"single".into(),output_dir:dir.to_string_lossy().into_owned(),..Default::default()};
        let request=BatchRequest{top:vec![media.clone()],bottom:vec![],settings,banner_path:String::new()};
        let single=plan_smart(Path::new("/missing/ffmpeg-must-not-be-used"),&request,&[],&dir,&control,|_,_|{}).unwrap();
        assert_eq!(single.len(),1);assert_eq!(single[0].top_path,media.path);assert_eq!(single[0].bottom_path,media.path);assert_eq!(single[0].bottom_start,0.0);
        let report=serde_json::json!({"actualOfflineAsr":true,"selectedCaptionsReuseFullTranscript":true,"noSecondRecognition":true,"smartSingleVideoWithoutBackground":true,"analyzedSeconds":duration,"words":words.len(),"selected":selected.iter().map(|c|serde_json::json!({"start":c.start,"end":c.end,"text":c.text,"score":c.score})).collect::<Vec<_>>(),"cacheReused":cached});fs::write(dir.join("acceptance.json"),serde_json::to_vec_pretty(&report).unwrap()).unwrap();
    }
    #[test]fn selected_words_rebase_without_recognition_drift_or_losing_punctuation(){
        let words=vec![Word{start:0.,end:1.,text:"Перед".into()},Word{start:1.1,end:1.8,text:"Под".into()},Word{start:1.8,end:2.5,text:"обычным,".into()},Word{start:2.7,end:3.9,text:"непримечательным!".into()},Word{start:5.,end:6.,text:"После".into()}];
        let text=clipped_word_srt(&words,1.05,3.0);let parsed=words_from_srt(&text,0.,3.0);
        assert_eq!(parsed.iter().map(|w|w.text.as_str()).collect::<Vec<_>>(),vec!["Под","обычным,","непримечательным!"]);assert!((parsed[0].start-0.05).abs()<0.000_001);assert!((parsed[2].end-2.85).abs()<0.000_001);
        let clamped=words_from_srt(&clipped_word_srt(&words,1.5,1.5),0.,1.5);assert_eq!(clamped[0].start,0.);assert_eq!(clamped.last().unwrap().end,1.5);
        assert!(clipped_word_srt(&words,8.,2.).is_empty());assert!(clipped_word_srt(&[],0.,2.).is_empty());
    }
    #[test]fn source_content_cache_identity_and_cancel(){let dir=std::env::temp_dir().join(format!("graffin-smart-{:016x}",rand::random::<u64>()));fs::create_dir(&dir).unwrap();let source=dir.join("video.mp4");fs::write(&source,b"first").unwrap();let control=RenderControl::new();let first=source_hash(&source,&control).unwrap();fs::write(&source,b"other").unwrap();let second=source_hash(&source,&control).unwrap();assert_ne!(first,second);assert_ne!(source_directory(&dir,&first),source_directory(&dir,&second));control.cancel();assert!(source_hash(&source,&control).is_err());fs::remove_dir_all(dir).unwrap();}
}
