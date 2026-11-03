import { useEffect, useRef, useState } from 'react';
import type { CSSProperties } from 'react';
import { open } from '@tauri-apps/plugin-dialog';
import { AlertCircle, ArrowUpRight, Captions, CheckCircle2, ChevronDown, ChevronUp, Clock3, Film, FolderOpen, ImagePlus, Link2, Loader2, Monitor, Play, Plus, Settings2, Square, Volume2, X } from 'lucide-react';
import { api, assetUrl, native } from './bridge';
import { defaultSettings, emptyJob } from './types';
import type { BannerAsset, Bootstrap, Job, Media, MediaToolsStatus, Settings } from './types';
import PresetPanel from './PresetPanel';
import SubtitleStyle from './SubtitleStyle';
import CaptionPreview from './CaptionPreview';
import SubtitleEditor from './SubtitleEditor';
import { subtitleFamily } from './studioTypes';
import type { StudioPreset } from './studioTypes';
import StudioTimeline from './StudioTimeline';
import FolderLocations from './FolderLocations';
import MomentsPanel from './MomentsPanel';
import { emptyMoments } from './momentTypes';
import type { MomentSnapshot, MomentSelection } from './momentTypes';
import MusicPanel from './MusicPanel';
import InterruptionPanel from './InterruptionPanel';
import './studio22.css';
import './studio23.css';

const message = (error: unknown) => error instanceof Error ? error.message : String(error);
const time = (s: number) => `${Math.floor(s / 60)}:${String(Math.floor(s % 60)).padStart(2, '0')}`;
const clamp = (n: number, min: number, max: number) => Math.min(max, Math.max(min, n));
const compositionGeometry = (settings: Settings) => {
  const width = settings.resolution, height = width * 16 / 9;
  const even = (value: number) => Math.round(value / 2) * 2;
  if (settings.layoutMode === 'single') return { width, height, banner: even(settings.bannerHeight * width / 1080), footer: even(settings.bannerBottom * width / 1080), top: even(height * settings.singleScale), bottom: 0 };
  const banner = even(settings.bannerHeight * width / 1080);
  const footer = even(settings.bannerBottom * width / 1080);
  const available = height - banner - footer;
  const top = even(available * settings.topSplit);
  return { width, height, banner, footer, top, bottom: available - top };
};

function Numeric({ label, value, min, max, suffix, onChange, disabled }: { label: string; value: number; min: number; max: number; suffix?: string; onChange: (n: number) => void; disabled?: boolean }) {
  const [draft, setDraft] = useState(String(value));
  const [editing, setEditing] = useState(false);
  useEffect(() => { if (!editing) setDraft(String(value)); }, [value, editing]);
  const commit = () => { const n = Number(draft); const next = draft.trim() && Number.isFinite(n) ? clamp(Math.round(n), min, max) : value; setDraft(String(next)); setEditing(false); onChange(next); };
  return <label className="numeric-label"><span>{label}</span><div className="numeric-input"><input aria-label={label} type="number" min={min} max={max} step={1} value={draft} disabled={disabled} onFocus={() => setEditing(true)} onChange={e => setDraft(e.target.value)} onBlur={commit} onKeyDown={e => { if (e.key === 'Enter') e.currentTarget.blur(); }} />{suffix && <span>{suffix}</span>}</div></label>;
}

function Range({ label, value, min = 0, max = 1, step = .01, display, onChange, disabled }: { label: string; value: number; min?: number; max?: number; step?: number; display: string; onChange: (n: number) => void; disabled?: boolean }) {
  return <label className="range-label"><span className="range-heading"><span>{label}</span><output>{display}</output></span><input aria-label={label} aria-valuetext={display} type="range" min={min} max={max} step={step} value={value} disabled={disabled} onChange={e => onChange(Number(e.target.value))} /></label>;
}

function SourceBank({ kind, items, selected, onSelect, onAdd, onLink, onFolder, onRemove, pending, disabled, sourceDir }: { kind: 'top' | 'bottom'; sourceDir?: string; items: Media[]; selected: string; onSelect: (id: string) => void; onAdd: () => void; onLink: () => void; onFolder: () => void; onRemove: (id: string) => void; pending: boolean; disabled: boolean }) {
  const upper = kind === 'top';
  return <section className="source-bank" aria-label={upper ? 'Верхние видео' : 'Нижние видео'}>
    <div className="bank-heading"><span className={`position-icon ${upper ? 'upper' : 'lower'}`}><i /><i /></span><div><h2>{upper ? 'Основное видео' : 'Фоновое видео'}</h2></div><span className="file-count">{items.length}</span></div>
    {items.length ? <div className="source-list">{items.map(item => <div key={item.id} className={`source-item ${selected === item.id ? 'selected' : ''}`}>
      <button className="source-select" onClick={() => onSelect(item.id)} title={item.path} aria-label={`Показать ${item.name}`}><img src={assetUrl(item.thumbnail)} alt="" /><span><strong>{item.name}</strong><small>{time(item.duration)} <span>·</span> {item.width}×{item.height}{item.hasAudio && <Volume2 size={11} />}</small></span></button>
      <button className="icon-button remove-file" disabled={disabled} onClick={() => onRemove(item.id)} title="Убрать из списка" aria-label={`Убрать ${item.name}`}><X size={14} /></button>
    </div>)}</div> : <div className="source-empty"><Film size={30} strokeWidth={1.2} /><p>{upper ? 'История, стрим, разговор' : 'Фон для второго экрана'}</p></div>}
    <div className="source-actions"><button className="add-source" disabled={disabled || pending} onClick={onAdd}>{pending ? <Loader2 size={16} className="spin" /> : <Plus size={16} />}{pending ? 'Загружаем…' : 'Из файлов'}</button><button className="source-link" aria-label={upper ? 'Основные видео из папки' : 'Фоновые видео из папки'} title="Добавить видео из папки" disabled={disabled || pending} onClick={onFolder}><FolderOpen size={16} /></button><button className="source-link" aria-label={upper ? 'Основное видео по ссылке' : 'Фоновое видео по ссылке'} title="Добавить по ссылке" disabled={disabled || pending} onClick={onLink}><Link2 size={17} /></button></div>{sourceDir && <p className="source-folder-path" title={sourceDir}>Последняя папка: {sourceDir.split(/[\\/]/).filter(Boolean).pop()}</p>}
  </section>;
}

function Composition({ top, bottom, settings, banner, bannerKind, onSplit, onBannerBottom, disabled, captionPlaying }: { top?: Media; bottom?: Media; settings: Settings; banner: string; bannerKind: 'video' | 'image'; onSplit: (n: number) => void; onBannerBottom: (n: number) => void; disabled: boolean; captionPlaying: boolean }) {
  const frame = useRef<HTMLDivElement>(null);
  const geometry = compositionGeometry(settings);
  const bannerRatio = geometry.banner / geometry.height;
  const footerRatio = geometry.footer / geometry.height;
  const topPercent = geometry.top / geometry.height * 100;
  const startDrag = (e: React.PointerEvent) => {
    if (disabled) return;
    e.currentTarget.setPointerCapture(e.pointerId);
    const update = (clientY: number) => {
      const rect = frame.current!.getBoundingClientRect();
      onSplit(clamp((clientY - rect.top) / (rect.height * (1 - bannerRatio - footerRatio)), .3, .7));
    };
    update(e.clientY);
    const target = e.currentTarget;
    const move = (event: Event) => update((event as PointerEvent).clientY);
    const end = () => { target.removeEventListener('pointermove', move); target.removeEventListener('pointerup', end); target.removeEventListener('pointercancel', end); };
    target.addEventListener('pointermove', move); target.addEventListener('pointerup', end); target.addEventListener('pointercancel', end);
  };
  const startBannerDrag = (e: React.PointerEvent) => {
    if (disabled) return;
    e.preventDefault();
    e.currentTarget.setPointerCapture(e.pointerId);
    const startY = e.clientY, startBottom = settings.bannerBottom;
    const height = frame.current!.getBoundingClientRect().height;
    const target = e.currentTarget;
    const move = (event: Event) => onBannerBottom(Math.round(clamp(startBottom + (startY - (event as PointerEvent).clientY) * 1920 / height, 0, 600) / 2) * 2);
    const end = () => { target.removeEventListener('pointermove', move); target.removeEventListener('pointerup', end); target.removeEventListener('pointercancel', end); };
    target.addEventListener('pointermove', move); target.addEventListener('pointerup', end); target.addEventListener('pointercancel', end);
  };
  const content = (item: Media | undefined, upper: boolean) => item ? <img className={settings.fitMode === 'contain' ? 'media-fit contain' : 'media-fit'} src={assetUrl(item.thumbnail)} style={{ objectPosition: `${(upper ? settings.topFocus : settings.bottomFocus) * 100}% 50%` }} alt={upper ? 'Кадр верхнего видео' : 'Кадр нижнего видео'} /> : <div className={`preview-placeholder ${upper ? 'preview-upper' : 'preview-lower'}`}><span className="placeholder-shape"><Film size={upper ? 29 : 25} strokeWidth={1.25} /></span><strong>{upper ? 'Основное видео' : 'Фоновое видео'}</strong></div>;
  return <div className={`composition-frame ${settings.layoutMode === 'single' ? 'single-composition' : ''}`} ref={frame} style={{ '--top-size': `${topPercent}%`, '--banner-size': `${bannerRatio * 100}%`, '--footer-size': `${footerRatio * 100}%`, '--single-y': `${(100 - topPercent) / 2}%` } as CSSProperties}>
    <div className="preview-top">{content(top, true)}<span className="preview-corner">{settings.layoutMode === 'single' ? 'Основное видео' : 'Верх'}</span></div>
    {settings.layoutMode === 'split' && <div className="preview-bottom">{content(bottom, false)}<span className="preview-corner">Низ</span></div>}
    {bannerRatio > 0 && <div className="preview-banner">{settings.bannerEnabled ? bannerKind === 'video' ? <video src={banner} autoPlay loop muted playsInline aria-label={`Баннер ${settings.bannerName}`} /> : <img src={banner} alt={`Баннер ${settings.bannerName}`} /> : <span>Зона баннера</span>}</div>}
    {footerRatio > 0 && <div className="preview-footer"><span>Место под описание</span></div>}
    {settings.subtitlesEnabled && <div className="preview-subtitles" aria-label="Предпросмотр субтитров" style={{ top: `${(settings.layoutMode === 'single' ? 72 : topPercent) + settings.subtitleOffset / 1920 * 100}%`, fontSize: `calc(var(--frame-width) * ${settings.subtitleSize / 1080})`, fontFamily: `'${subtitleFamily(settings.subtitleFont)}', sans-serif`, color: settings.subtitleColor }}><CaptionPreview value={settings} playing={captionPlaying} /></div>}
    {settings.layoutMode === 'split' && <button className="split-handle" aria-label="Граница между верхним и нижним видео" title="Перетащите, чтобы изменить высоту видео" onPointerDown={startDrag} disabled={disabled} style={{ top: `${topPercent}%` }}><i /><i /></button>}
    {bannerRatio > 0 && <button className="banner-position-handle" role="slider" aria-label="Отступ баннера снизу" aria-valuemin={0} aria-valuemax={Math.round(600 * settings.resolution / 1080 / 2) * 2} aria-valuenow={geometry.footer} aria-valuetext={`${geometry.footer} пикселей`} title="Перетащите, чтобы поднять баннер. Стрелки вверх и вниз меняют отступ." onPointerDown={startBannerDrag} disabled={disabled} style={{ bottom: `${footerRatio * 100}%` }} onKeyDown={e => { if (e.key === 'ArrowUp' || e.key === 'ArrowDown') { e.preventDefault(); onBannerBottom(clamp(settings.bannerBottom + (e.key === 'ArrowUp' ? 1 : -1) * (e.shiftKey ? 20 : 2), 0, 600)); } }}><ChevronUp size={11} /><ChevronDown size={11} /></button>}
  </div>;
}

export default function App() {
  const [inspectorTab, setInspectorTab] = useState('frame');
  const [musicPending, setMusicPending] = useState(false);
  const [captionPlaying, setCaptionPlaying] = useState(true);
  const [moments, setMoments] = useState<MomentSnapshot>(emptyMoments), [momentSelection, setMomentSelection] = useState<MomentSelection[]>([]), [momentsOpen, setMomentsOpen] = useState(false);
  const [analysisKey, setAnalysisKey] = useState('');
  const initializedAnalysis = useRef('');
  const momentsTrigger = useRef<HTMLElement | null>(null);
  const openMoments = () => { momentsTrigger.current = document.activeElement as HTMLElement | null; setMomentsOpen(true); };
  const [boot, setBoot] = useState<Bootstrap | null>(null);
  const [settings, setSettings] = useState<Settings>(defaultSettings);
  const [top, setTop] = useState<Media[]>([]), [bottom, setBottom] = useState<Media[]>([]);
  const [selectedTop, setSelectedTop] = useState(''), [selectedBottom, setSelectedBottom] = useState('');
  const [job, setJob] = useState<Job>(emptyJob);
  const [error, setError] = useState('');
  const [loadingSource, setLoadingSource] = useState<'top' | 'bottom' | null>(null);
  const [previewing, setPreviewing] = useState(false), [previewPath, setPreviewPath] = useState('');
  const [starting, setStarting] = useState(false), [advanced, setAdvanced] = useState(false);
  const [saveState, setSaveState] = useState('');
  const [editingPath, setEditingPath] = useState('');
  const [linkKind, setLinkKind] = useState<'top' | 'bottom' | null>(null);
  const [linkUrl, setLinkUrl] = useState(''), [linkError, setLinkError] = useState('');
  const [bannerPending, setBannerPending] = useState(false);
  const [tools, setTools] = useState<MediaToolsStatus | null>(null), [toolsPending, setToolsPending] = useState(false);
  const linkDialog = useRef<HTMLElement>(null);
  const montageBusy = starting || previewing || job.status === 'running' || job.status === 'cancelling';
  const momentsWorking = moments.status === 'analyzing' || moments.status === 'cancelling';
  const busy = montageBusy || momentsWorking || bannerPending || musicPending || toolsPending || loadingSource !== null;
  const sourceKey = JSON.stringify([top.map(item => [item.id,item.path,item.duration]),settings.duration,settings.subtitleLanguage]);
  const change = <K extends keyof Settings>(key: K, value: Settings[K]) => setSettings(s => ({ ...s, [key]: value }));

  useEffect(() => {
    let alive = true;
    api.bootstrap().then(data => { if (alive) { setBoot(data); setSettings({ ...defaultSettings, ...data.restoredSettings, bannerLibrary: data.restoredSettings?.bannerLibrary || [], outputDir: data.restoredSettings?.outputDir || data.defaultOutput, importDir: data.restoredSettings?.importDir || data.defaultImportDir }); } }).catch(e => alive && setError(message(e)));
    api.mediaToolsStatus().then(result => alive && setTools(result)).catch(() => {});
    return () => { alive = false; };
  }, []);
  useEffect(() => {
    if (!boot) return;
    const timer = setTimeout(() => { api.saveSettings(settings).then(() => setSaveState('Настройки сохранены')).catch(e => setSaveState(`Не удалось сохранить: ${message(e)}`)); }, 700);
    return () => clearTimeout(timer);
  }, [settings, boot]);
  useEffect(() => {
    if (!boot) return;
    let alive = true;
    const poll = () => api.getJob().then(result => { if (alive) setJob(result); }).catch(e => { if (alive) setError(message(e)); });
    poll(); const timer = setInterval(poll, 1200);
    return () => { alive = false; clearInterval(timer); };
  }, [boot]);
  useEffect(() => {
    if (!moments.id || !momentsWorking) return;
    let alive = true, pending = false;
    const poll = async () => { if (pending) return; pending = true; try { const next = await api.moments(); if (alive && next.id === moments.id) setMoments(next); } catch (error) { if (alive) setError(message(error)); } finally { pending = false; } };
    void poll(); const timer = window.setInterval(poll, 900);
    return () => { alive = false; clearInterval(timer); };
  }, [moments.id,momentsWorking]);
  useEffect(() => {
    if (moments.status === 'ready' && initializedAnalysis.current !== moments.id) {
      initializedAnalysis.current = moments.id;
      setMomentSelection(moments.candidates.slice(0,settings.count).map(item => ({candidateId:item.id,start:item.start,end:item.end})));
    }
  }, [moments.status,moments.id,moments.candidates,settings.count]);
  useEffect(() => {
    if (analysisKey && sourceKey !== analysisKey) {
      if (momentsWorking) void api.cancelMoments(moments.id).catch(() => {});
      setMoments(emptyMoments); setMomentSelection([]); setMomentsOpen(false); initializedAnalysis.current = ''; setAnalysisKey('');
    }
  }, [sourceKey]);
  useEffect(() => {
    if (!previewPath) return;
    const close = (event: KeyboardEvent) => { if (event.key === 'Escape') setPreviewPath(''); };
    window.addEventListener('keydown', close);
    return () => window.removeEventListener('keydown', close);
  }, [previewPath]);
  useEffect(() => {
    if (!linkKind) return;
    const previous = document.activeElement as HTMLElement | null;
    const onKey = (event: KeyboardEvent) => {
      if (event.key === 'Escape' && !loadingSource) { event.preventDefault(); setLinkKind(null); }
      if (event.key === 'Tab') {
        const controls = Array.from(linkDialog.current?.querySelectorAll<HTMLElement>('button:not(:disabled), input:not(:disabled)') || []);
        const first = controls[0], last = controls[controls.length - 1];
        if (event.shiftKey && document.activeElement === first) { event.preventDefault(); last?.focus(); }
        if (!event.shiftKey && document.activeElement === last) { event.preventDefault(); first?.focus(); }
      }
    };
    document.addEventListener('keydown', onKey);
    return () => { document.removeEventListener('keydown', onKey); previous?.focus(); };
  }, [linkKind, loadingSource]);

  const capacity = top.reduce((sum, file) => sum + Math.floor((file.duration + .000001) / settings.duration), 0);
  const layout = compositionGeometry(settings);
  const topHeight = layout.top, bottomHeight = layout.bottom;
  const haveSources = top.length > 0 && (settings.layoutMode === 'single' || bottom.length > 0);
  const subtitlesReady = !!tools?.subtitlesReady && !!tools?.modelReady;
  const canExport = haveSources && (!settings.interruptionEnabled || !!settings.interruptionPath) && (!settings.musicPath || settings.musicStart < settings.musicDuration) && (settings.cutMode === 'smart' || settings.count <= capacity) && !!settings.outputDir && !!boot?.ffmpegReady && ((!settings.subtitlesEnabled && settings.cutMode !== 'smart') || subtitlesReady);
  const exportHint = !haveSources ? '' : settings.interruptionEnabled && !settings.interruptionPath ? 'Выберите баннер для паузы во вкладке «Кадр»' : (settings.subtitlesEnabled || settings.cutMode === 'smart') && !subtitlesReady ? 'Подготовьте речевой модуль во вкладке «Нарезка»' : settings.cutMode === 'fixed' && settings.count > capacity ? 'Уменьшите количество роликов или добавьте исходники' : !settings.outputDir ? 'Выберите папку во вкладке «Нарезка»' : '';
  const choose = async (kind: 'top' | 'bottom') => {
    setError('');
    if (!native) { setError('Откройте приложение для выбора файлов.'); return; }
    try {
      const result = await open({ title: kind === 'top' ? 'Основные видео — верхний блок' : 'Залипательные видео — нижний блок', multiple: true, directory: false, filters: [{ name: 'Видео', extensions: ['mp4', 'mov', 'm4v', 'mkv', 'webm', 'avi', 'ts', 'mts'] }] });
      if (!result) return;
      setLoadingSource(kind);
      const paths = Array.isArray(result) ? result : [result];
      const existing = kind === 'top' ? top : bottom;
      const unique = paths.filter(path => !existing.some(item => item.path === path));
      if (!unique.length) return;
      const files = await api.inspectMedia(unique);
      if (kind === 'top') { setTop(old => [...old, ...files]); setSelectedTop(old => old || files[0]?.id || ''); }
      else { setBottom(old => [...old, ...files]); setSelectedBottom(old => old || files[0]?.id || ''); }
    } catch (e) { setError(message(e)); } finally { setLoadingSource(null); }
  };
  const chooseSourceFolder = async (kind: 'top' | 'bottom') => {
    if (!native || busy) return;
    try {
      const path = await open({title: kind === 'top' ? 'Папка основных видео' : 'Папка фоновых видео',directory:true,multiple:false,defaultPath:(kind === 'top' ? settings.topSourceDir : settings.bottomSourceDir) || undefined});
      if (typeof path !== 'string') return;
      setLoadingSource(kind); setError('');
      const files = await api.inspectMediaFolder(path);
      if (kind === 'top') { setTop(files); setSelectedTop(files[0]?.id || ''); change('topSourceDir',path); }
      else { setBottom(files); setSelectedBottom(files[0]?.id || ''); change('bottomSourceDir',path); }
      if (!files.length) setError('В этой папке нет поддерживаемых видео. Выберите другую папку или добавьте отдельные файлы.');
    } catch (error) { setError(message(error)); } finally { setLoadingSource(null); }
  };
  const importLink = async () => {
    if (!linkKind || busy || !/^https?:\/\//i.test(linkUrl.trim())) return;
    const kind = linkKind;
    setLinkError(''); setLoadingSource(kind);
    try {
      const file = await api.importMediaUrl(linkUrl.trim(), settings.importDir);
      if (kind === 'top') { setTop(old => old.some(item => item.path === file.path) ? old : [...old, file]); setSelectedTop(file.id); }
      else { setBottom(old => old.some(item => item.path === file.path) ? old : [...old, file]); setSelectedBottom(file.id); }
      setLinkKind(null); setLinkUrl('');
      api.mediaToolsStatus().then(setTools).catch(() => {});
    } catch (e) { setLinkError(message(e)); } finally { setLoadingSource(null); }
  };
  const addBanner = async (interruption = false) => {
    if (!native || busy) return;
    try {
      const path = await open({ title: 'Добавить баннер', multiple: false, filters: [{ name: 'Баннер', extensions: ['png', 'jpg', 'jpeg', 'webp', 'gif', 'mp4', 'mov', 'webm'] }] });
      if (typeof path !== 'string') return;
      setBannerPending(true); setError('');
      const asset = await api.importBanner(path);
      setSettings(current => ({ ...current, ...(interruption ? { interruptionEnabled: true, interruptionPath: asset.path } : { bannerEnabled: true, bannerPath: asset.path, bannerName: asset.name }), bannerLibrary: [...current.bannerLibrary.filter(item => item.path !== asset.path), asset] }));
    } catch (e) { setError(message(e)); } finally { setBannerPending(false); }
  };
  const selectBanner = (path: string) => {
    const asset = settings.bannerLibrary.find(item => item.path === path);
    setSettings(current => ({ ...current, bannerPath: path, bannerName: asset?.name || 'Barsik VPN' }));
  };
  const renameBanner = (name: string) => {
    setSettings(current => ({ ...current, bannerName: name, bannerLibrary: current.bannerLibrary.map(item => item.path === current.bannerPath ? { ...item, name } : item) }));
  };
  const prepareTools = async () => {
    if (busy || !native) return;
    setToolsPending(true); setError('');
    try { setTools(await api.prepareMediaTools()); } catch (e) { setError(message(e)); } finally { setToolsPending(false); }
  };
  const chooseOutput = async () => {
    setError('');
    if (!native) { setError('Откройте приложение для выбора папки.'); return; }
    try { const path = await open({ title: 'Куда сохранить готовые ролики', directory: true, multiple: false }); if (typeof path === 'string') change('outputDir', path); } catch (e) { setError(message(e)); }
  };
  const request = () => ({ top, bottom, settings, bannerPath: settings.bannerPath || boot?.bannerPath || '' });
  const beginAnalysis = async () => {
    if (!top.length || busy || !subtitlesReady) return;
    if (!momentsOpen) momentsTrigger.current = document.activeElement as HTMLElement | null;
    setError(''); setStarting(true); setAnalysisKey(sourceKey); setMomentSelection([]); initializedAnalysis.current = '';
    try { const next = await api.analyzeMoments({sources:top,targetDuration:settings.duration,language:settings.subtitleLanguage,limit:60}); setMoments(next); setMomentsOpen(true); }
    catch (error) { setError(message(error)); } finally { setStarting(false); }
  };
  const exportSelected = async () => {
    if (!canExport || busy || !momentSelection.length || moments.status !== 'ready' || analysisKey !== sourceKey) return;
    setStarting(true); setError('');
    try {
      await api.saveSettings(settings);
      setJob(await api.startSelected({analysisId:moments.id,selection:momentSelection,bottom,settings,bannerPath:settings.bannerPath || boot?.bannerPath || ''}));
      setMomentsOpen(false); setMoments(emptyMoments); setMomentSelection([]); setAnalysisKey(''); initializedAnalysis.current = '';
    } catch (error) { setError(message(error)); } finally { setStarting(false); }
  };
  const start = async () => {
    if (settings.cutMode === 'smart') { if (moments.id && analysisKey === sourceKey && moments.status === 'ready') openMoments(); else await beginAnalysis(); return; }
    if (!canExport || busy) return;
    setStarting(true); setError('');
    try { await api.saveSettings(settings); setJob(await api.startBatch(request())); } catch (e) { setError(message(e)); } finally { setStarting(false); }
  };
  const renderPreview = async () => {
    if (!haveSources || busy || (settings.subtitlesEnabled && !subtitlesReady)) return;
    setPreviewing(true); setError('');
    try {
      const req = request();
      req.top = [...top].sort((a, b) => Number(b.id === selectedTop) - Number(a.id === selectedTop));
      req.bottom = [...bottom].sort((a, b) => Number(b.id === selectedBottom) - Number(a.id === selectedBottom));
      req.settings = { ...req.settings, count: 1, order: 'sequential' };
      setPreviewPath(await api.renderPreview(req));
    } catch (e) { setError(message(e)); } finally { setPreviewing(false); }
  };
  const cancel = async () => { try { setJob(await api.cancelBatch()); } catch (e) { setError(message(e)); } };
  const applyPreset = async (preset: StudioPreset) => {
    setSettings(current => ({ ...current, ...preset.settings, count: current.count, duration: current.duration, cutMode: current.cutMode, order: current.order, outputDir: current.outputDir, importDir: current.importDir, topSourceDir: current.topSourceDir, bottomSourceDir: current.bottomSourceDir, bannerLibrary: [...current.bannerLibrary.filter(item => !preset.settings.bannerLibrary.some(saved => saved.path === item.path)), ...preset.settings.bannerLibrary] }));
  };
  const chooseSubtitleVideo = async () => {
    try { const path = await open({ title: 'Редактировать субтитры готового ролика', multiple: false, filters: [{ name: 'Готовое видео', extensions: ['mp4'] }], defaultPath: settings.outputDir || undefined }); if (typeof path === 'string') setEditingPath(path); } catch (error) { setError(message(error)); }
  };
  const openOutput = async () => { try { await api.openFolder(job.outputs[0] || settings.outputDir); } catch (e) { setError(message(e)); } };
  const selectedBanner: BannerAsset | undefined = settings.bannerLibrary.find(item => item.path === settings.bannerPath);
  const bannerKind = selectedBanner?.kind || (/\.(mp4|mov|webm)$/i.test(settings.bannerPath) ? 'video' : 'image');
  const banner = assetUrl(selectedBanner?.path || settings.bannerPath || boot?.bannerPreview || '/banner.png');
  const activeTop = top.find(item => item.id === selectedTop) || top[0];
  const activeBottom = bottom.find(item => item.id === selectedBottom) || bottom[0];
  const jobActive = job.status !== 'idle';

  return <div className="app-shell montage-only">
    <div className="studio-body"><header className="app-header"><div className="studio-brand-heading"><img src="/graffin-mark.png" alt="" /><div className="workspace-title"><span className="workspace-kicker">Graffin Studio</span><h1>Монтаж</h1></div></div><div className="header-session-meta"><span className={`save-pill ${saveState.startsWith('Не удалось') ? 'has-error' : ''}`} title={saveState}>{saveState.startsWith('Не удалось') ? 'Ошибка сохранения' : saveState ? 'Сохранено' : 'Готово к работе'}</span><button className="header-output" onClick={chooseOutput} disabled={busy} title={settings.outputDir || 'Выбрать папку экспорта'}><FolderOpen size={15} aria-hidden="true" /><span>{settings.outputDir.split(/[\\/]/).filter(Boolean).pop() || 'Папка экспорта'}</span></button></div></header>
    {!native && <div className="demo-notice"><Monitor size={16} /><span>Предпросмотр интерфейса. Откройте приложение для работы с файлами.</span></div>}
    {error && <div className="error-bar" role="alert"><AlertCircle size={17} /><span>{error}</span><button className="icon-button" aria-label="Закрыть сообщение" onClick={() => setError('')}><X size={16} /></button></div>}
    {!boot && !error && <div className="loading-app"><Loader2 size={18} className="spin" />Подготавливаем студию…</div>}
    <main className="workspace">
      <aside className="sources-pane"><div className="pane-title"><h2>Медиатека</h2><span>{top.length + bottom.length} файлов</span></div>
        <SourceBank kind="top" sourceDir={settings.topSourceDir} items={top} selected={selectedTop} onSelect={setSelectedTop} onAdd={() => choose('top')} onFolder={() => chooseSourceFolder('top')} onLink={() => { setLinkKind('top'); setLinkError(''); }} onRemove={id => setTop(files => files.filter(file => file.id !== id))} pending={loadingSource === 'top'} disabled={busy} />
        {settings.layoutMode === 'split' && <SourceBank kind="bottom" sourceDir={settings.bottomSourceDir} items={bottom} selected={selectedBottom} onSelect={setSelectedBottom} onAdd={() => choose('bottom')} onFolder={() => chooseSourceFolder('bottom')} onLink={() => { setLinkKind('bottom'); setLinkError(''); }} onRemove={id => setBottom(files => files.filter(file => file.id !== id))} pending={loadingSource === 'bottom'} disabled={busy} />}
        {(moments.id || settings.cutMode === 'smart') && <button className="moments-open-button" disabled={montageBusy || !top.length || (!moments.id && !subtitlesReady)} onClick={() => moments.id ? openMoments() : void beginAnalysis()}>{momentsWorking ? <Loader2 className="spin" size={15} /> : <Film size={15} />}{momentsWorking ? 'Ищем моменты…' : moments.status === 'ready' ? `Выбрано ${momentSelection.length} моментов` : 'Найти моменты'}</button>}
        <FolderLocations settings={settings} disabled={busy} onChange={patch => setSettings(current => ({...current,...patch}))} onError={setError} /><div className="source-footer">{boot && !boot.ffmpegReady && <span className="engine-unavailable"><AlertCircle size={15} />Не найден модуль обработки видео</span>}</div>
      </aside>

      <section className="preview-pane" aria-label="Композиция видео"><div className="preview-toolbar"><div><h2>Предпросмотр</h2><span>9:16</span></div><div className="monitor-tools">{settings.subtitlesEnabled && <button className="icon-button" aria-label={captionPlaying ? 'Остановить анимацию в кадре' : 'Проиграть анимацию в кадре'} aria-pressed={captionPlaying} onClick={() => setCaptionPlaying(value => !value)}><Captions size={15} /></button>}<span className="resolution-badge">{settings.resolution} × {Math.round(settings.resolution * 16 / 9)}</span></div></div>
        <div className="monitor-area"><div className="preview-stage"><Composition top={activeTop} bottom={activeBottom} settings={settings} captionPlaying={captionPlaying} banner={banner} bannerKind={bannerKind} disabled={busy} onSplit={value => change('topSplit', Math.round(value * 100) / 100)} onBannerBottom={value => change('bannerBottom', value)} />
          <div className="dimension-rail" hidden={settings.layoutMode === 'single'} aria-hidden="true" style={{ gridTemplateRows: [topHeight, bottomHeight, layout.banner, layout.footer].filter(height => height > 0).map(height => `${height}fr`).join(' ') }}><span>{topHeight}</span><span>{bottomHeight}</span>{layout.banner > 0 && <span>{layout.banner}</span>}{layout.footer > 0 && <span>{layout.footer}</span>}</div>
        </div>
        <div className="preview-controls"><span>{time(settings.duration)} <span className="control-divider" /> 30 fps</span><button className="secondary-button" disabled={!haveSources || busy || !boot?.ffmpegReady || (settings.subtitlesEnabled && !subtitlesReady)} onClick={renderPreview}>{previewing ? <Loader2 size={16} className="spin" /> : <Play size={14} fill="currentColor" />}{previewing ? 'Готовим…' : 'Просмотр 5 сек.'}</button></div>
        <p className="preview-caption">{settings.interruptionEnabled ? `Баннер-пауза добавит ${settings.interruptionDuration} сек. к ролику` : settings.layoutMode === 'single' ? 'Основное видео без второго экрана' : 'Перетащите границу, чтобы изменить композицию'}</p></div>
        <StudioTimeline settings={settings} top={activeTop} bottom={activeBottom} onInspect={setInspectorTab} />
      </section>

      <aside className="settings-pane"><div className="pane-title"><h2>Настройки</h2><Settings2 size={18} /></div><PresetPanel settings={settings} disabled={busy} onApply={applyPreset} onError={setError} />
<div className="inspector-tabs" role="tablist" aria-label="Настройки ролика">{[{id:'frame',label:'Кадр'},{id:'audio',label:'Звук'},{id:'text',label:'Текст'},{id:'cuts',label:'Нарезка'}].map(tab => <button key={tab.id} role="tab" id={`tab-${tab.id}`} aria-controls={`panel-${tab.id}`} aria-selected={inspectorTab === tab.id} tabIndex={inspectorTab === tab.id ? 0 : -1} onClick={() => setInspectorTab(tab.id)} onKeyDown={event => { if (['ArrowLeft','ArrowRight','Home','End'].includes(event.key)) { event.preventDefault(); const ids=['frame','audio','text','cuts']; const index=event.key==='Home'?0:event.key==='End'?3:(ids.indexOf(inspectorTab)+(event.key==='ArrowRight'?1:3))%4; setInspectorTab(ids[index]); document.getElementById(`tab-${ids[index]}`)?.focus(); } }}>{tab.label}</button>)}</div>
<div className="inspector-tab-panel" role="tabpanel" id="panel-frame" aria-labelledby="tab-frame" hidden={inspectorTab !== 'frame'}><section className="settings-section layout-settings"><h3>Вид ролика</h3><div className="layout-choices" role="group" aria-label="Вид ролика"><button className={settings.layoutMode === 'split' ? 'selected' : ''} aria-pressed={settings.layoutMode === 'split'} disabled={busy} onClick={() => change('layoutMode', 'split')}><span className="layout-mini split"><i /><i /></span><span>С фоном<small>Два видео</small></span></button><button className={settings.layoutMode === 'single' ? 'selected' : ''} aria-pressed={settings.layoutMode === 'single'} disabled={busy} onClick={() => change('layoutMode', 'single')}><span className="layout-mini single"><i /></span><span>Без фона<small>Одно видео</small></span></button></div></section><section className="settings-section composition-settings"><h3>Кадрирование</h3><div className="segmented-control" aria-label="Способ заполнения кадра"><button className={settings.fitMode === 'crop' ? 'active' : ''} disabled={busy} onClick={() => change('fitMode', 'crop')}>Заполнить</button><button className={settings.fitMode === 'contain' ? 'active' : ''} disabled={busy} onClick={() => change('fitMode', 'contain')}>Вписать целиком</button></div>
          {settings.layoutMode === 'split' ? <Range label="Доля основного видео" value={settings.topSplit} min={.3} max={.7} display={`${Math.round(settings.topSplit * 100)}%`} disabled={busy} onChange={n => change('topSplit', n)} /> : <Range label="Высота основного видео" value={settings.singleScale} min={.5} max={1} display={`${Math.round(settings.singleScale * 100)}%`} disabled={busy} onChange={n => change('singleScale', n)} />}
          <button className="disclosure-button" aria-expanded={advanced} onClick={() => setAdvanced(!advanced)}>Положение кадра{advanced ? <ChevronUp size={14} /> : <ChevronDown size={14} />}</button>
          {advanced && <div className="advanced-settings"><Range label="Кадр сверху: левее / правее" value={settings.topFocus} display={`${Math.round(settings.topFocus * 100)}%`} disabled={busy || settings.fitMode === 'contain'} onChange={n => change('topFocus', n)} />{settings.layoutMode === 'split' && <Range label="Кадр снизу: левее / правее" value={settings.bottomFocus} display={`${Math.round(settings.bottomFocus * 100)}%`} disabled={busy || settings.fitMode === 'contain'} onChange={n => change('bottomFocus', n)} />}</div>}
        </section><section className="settings-section banner-settings"><div className="inspector-section-title"><h3>Баннер</h3><button className="icon-button" aria-label="Добавить свой баннер" title="Добавить свой баннер" disabled={!native || busy} onClick={() => addBanner()}>{bannerPending ? <Loader2 size={18} className="spin" /> : <ImagePlus size={18} />}</button></div>
          <div className="banner-selector"><select aria-label="Выбрать баннер" value={settings.bannerPath} disabled={busy} onChange={e => selectBanner(e.target.value)}><option value="">Barsik VPN · встроенный</option>{settings.bannerLibrary.map(item => <option value={item.path} key={item.id}>{item.name || 'Без названия'}</option>)}</select></div>
          <div className="section-heading banner-name-row"><input className="banner-name" aria-label="Название баннера" maxLength={80} value={settings.bannerName} disabled={busy} placeholder="Название баннера" onChange={e => renameBanner(e.target.value)} /><button className={`switch ${settings.bannerEnabled ? 'on' : ''}`} role="switch" aria-checked={settings.bannerEnabled} aria-label={`Показывать баннер ${settings.bannerName}`} disabled={busy} onClick={() => change('bannerEnabled', !settings.bannerEnabled)}><i /></button></div>
          {settings.bannerEnabled && <div className="animated-expansion"><Range label="Высота" value={settings.bannerHeight} min={0} max={360} step={2} display={`${layout.banner} px`} disabled={busy} onChange={n => change('bannerHeight', n)} /><Range label="Отступ снизу" value={settings.bannerBottom} min={0} max={600} step={2} display={`${layout.footer} px`} disabled={busy} onChange={n => change('bannerBottom', n)} /><span className="loop-indicator">∞ Зациклен</span></div>}
        </section><InterruptionPanel settings={settings} disabled={busy} onChange={patch => setSettings(current => ({...current,...patch}))} onImport={() => addBanner(true)} /></div>
<div className="inspector-tab-panel" role="tabpanel" id="panel-audio" aria-labelledby="tab-audio" hidden={inspectorTab !== 'audio'}><MusicPanel settings={settings} disabled={busy} onChange={patch => setSettings(current => ({...current,...patch}))} onError={setError} onPending={setMusicPending} /><section className="settings-section"><h3>Звук исходников</h3><Range label="Звук основного видео" value={settings.topVolume} max={1.5} display={`${Math.round(settings.topVolume * 100)}%`} disabled={busy} onChange={n => change('topVolume', n)} />{settings.layoutMode === 'split' && <Range label="Звук нижнего видео" value={settings.bottomVolume} max={1.5} display={settings.bottomVolume ? `${Math.round(settings.bottomVolume * 100)}%` : 'Выключен'} disabled={busy} onChange={n => change('bottomVolume', n)} />}</section></div>
<div className="inspector-tab-panel" role="tabpanel" id="panel-text" aria-labelledby="tab-text" hidden={inspectorTab !== 'text'}><section className="settings-section subtitles-settings"><div className="section-heading"><h3><Captions size={18} />Субтитры</h3><button className={`switch ${settings.subtitlesEnabled ? 'on' : ''}`} role="switch" aria-checked={settings.subtitlesEnabled} aria-label="Автоматические субтитры" disabled={busy} onClick={() => change('subtitlesEnabled', !settings.subtitlesEnabled)}><i /></button></div>
          {settings.subtitlesEnabled && <div className="animated-expansion"><label className="field-label">Язык<select aria-label="Язык субтитров" value={settings.subtitleLanguage} disabled={busy} onChange={e => change('subtitleLanguage', e.target.value)}><option value="ru">Русский</option><option value="en">Английский</option><option value="auto">Определить</option></select></label><SubtitleStyle value={settings} disabled={busy} onChange={style => setSettings(current => ({ ...current, ...style }))} /><Range label="Размер текста" value={settings.subtitleSize} min={28} max={96} step={2} display={`${settings.subtitleSize} px`} disabled={busy} onChange={n => change('subtitleSize', n)} /><Range label="Смещение от стыка" value={settings.subtitleOffset} min={-600} max={600} step={2} display={`${settings.subtitleOffset} px`} disabled={busy} onChange={n => change('subtitleOffset', n)} />{!subtitlesReady && <div className="subtitle-setup"><p>{toolsPending ? 'Загружаем распознавание речи…' : 'Для первого запуска нужен речевой модуль.'}</p><button className="secondary-button" disabled={busy || !native} onClick={prepareTools}>{toolsPending ? <Loader2 size={16} className="spin" /> : <Plus size={16} />}{toolsPending ? 'Устанавливаем…' : 'Установить модуль'}</button></div>}</div>}
          <button className="subtitle-editor-open" disabled={!native || busy} onClick={chooseSubtitleVideo}><Captions size={16} />Редактировать слова в видео<ArrowUpRight size={13} /></button>
        </section></div>
<div className="inspector-tab-panel" role="tabpanel" id="panel-cuts" aria-labelledby="tab-cuts" hidden={inspectorTab !== 'cuts'}><section className="settings-section cut-settings"><h3>Нарезка</h3><div className="cut-mode-control" role="group" aria-label="Способ нарезки"><button className={settings.cutMode === 'smart' ? 'active' : ''} aria-pressed={settings.cutMode === 'smart'} disabled={busy} onClick={() => change('cutMode', 'smart')}>По смыслу</button><button className={settings.cutMode === 'fixed' ? 'active' : ''} aria-pressed={settings.cutMode === 'fixed'} disabled={busy} onClick={() => change('cutMode', 'fixed')}>По времени</button></div>{settings.cutMode === 'smart' && <p className="cut-mode-hint">Выбираем выразительные фразы и плотную речь, исключаем долгие паузы и похожие моменты. Длительность — ориентир.</p>}{settings.cutMode === 'smart' && !subtitlesReady && <div className="subtitle-setup"><p>Для анализа речи нужен речевой модуль.</p><button className="secondary-button" disabled={busy || !native} onClick={prepareTools}>{toolsPending ? <Loader2 size={16} className="spin" /> : <Plus size={16} />}{toolsPending ? 'Устанавливаем…' : 'Подготовить анализ речи'}</button></div>}<div className="number-pair"><Numeric label="Количество" value={settings.count} min={1} max={200} suffix="шт." disabled={busy} onChange={n => change('count', n)} /><Numeric label="Длительность" value={settings.duration} min={5} max={180} suffix="сек." disabled={busy} onChange={n => change('duration', n)} /></div>
          {top.length > 0 && <div className={`capacity-note ${settings.cutMode !== 'smart' && settings.count > capacity ? 'capacity-warning' : ''}`}><Clock3 size={13} /><span>{settings.cutMode === 'smart' ? 'Перед сборкой можно просмотреть и выбрать до 60 найденных моментов' : `Доступно ${capacity} без повторов`}</span></div>}
          <label className="field-label">Порядок фрагментов<select value={settings.order} disabled={busy} onChange={e => change('order', e.target.value as Settings['order'])}><option value="random">Перемешивать</option><option value="sequential">По порядку</option></select></label>
          <label className="field-label">Качество экспорта<select value={settings.resolution} disabled={busy} onChange={e => change('resolution', Number(e.target.value))}><option value={1080}>1080 × 1920 · Full HD</option><option value={720}>720 × 1280 · быстрее</option></select></label>
        </section><section className="settings-section output-settings"><h3>Сохранение</h3><button className="folder-picker" disabled={busy} onClick={chooseOutput} title={settings.outputDir || 'Выбрать папку'}><FolderOpen size={16} /><span>{settings.outputDir ? settings.outputDir.split(/[\\/]/).filter(Boolean).slice(-2).join(' / ') : 'Выбрать папку'}</span><ChevronDown size={13} /></button></section></div>

      </aside>
    </main>

    <footer className="export-bar"><div className="export-status">{jobActive ? <><span className={`job-indicator ${job.status}`} >{job.status === 'completed' ? <CheckCircle2 size={19} /> : job.status === 'failed' ? <AlertCircle size={19} /> : job.status === 'cancelled' ? <Square size={17} /> : <Loader2 size={19} className="spin" />}</span><div><strong>{job.status === 'completed' ? `Готово ${job.completed} из ${job.total}` : job.status === 'cancelled' ? `Остановлено · готово ${job.completed} из ${job.total}` : job.status === 'failed' ? 'Сборка остановлена с ошибкой' : job.status === 'cancelling' ? 'Останавливаем сборку…' : job.current === 0 ? settings.cutMode === 'smart' ? 'Ищем законченные моменты' : 'Подготавливаем исходники' : `Собираем ролик ${job.current} из ${job.total}`}</strong><small>{job.error || job.message || 'Готовые ролики сохраняются по одному'}</small></div></> : <><span className="export-idle"><Film size={20} /></span><div><strong>{haveSources ? `${settings.count} роликов ${settings.cutMode === 'smart' ? 'около' : 'по'} ${settings.duration} секунд` : settings.layoutMode === 'single' ? 'Добавьте основное видео, чтобы начать' : 'Добавьте основное и фоновое видео'}</strong>{exportHint && <small>{exportHint}</small>}</div></>}</div>
      {(job.status === 'running' || job.status === 'cancelling') && <div className="batch-progress"><div><span>Готово {job.completed} из {job.total}</span><strong>{Math.round(job.progress * 100)}%</strong></div><progress max={1} value={job.progress} aria-label="Прогресс сборки" /></div>}
      <div className="export-actions">{job.outputs.length > 0 && <button className="secondary-button" disabled={busy} onClick={() => setEditingPath(job.outputs[0])}><Captions size={16} />Субтитры</button>}{job.outputs.length > 0 && <button className="secondary-button" onClick={openOutput}><FolderOpen size={15} />Готовые ролики<ArrowUpRight size={13} /></button>}{job.status === 'running' || job.status === 'cancelling' ? <button className="stop-button" onClick={cancel} disabled={job.status === 'cancelling'}><Square size={12} fill="currentColor" />Остановить</button> : <button className="primary-button" onClick={start} disabled={(settings.cutMode === 'smart' ? !top.length || !subtitlesReady || !boot?.ffmpegReady : !canExport) || busy}>{starting ? <Loader2 size={17} className="spin" /> : <Play size={15} fill="currentColor" />}{starting ? 'Запускаем…' : settings.cutMode === 'smart' ? moments.status === 'ready' && analysisKey === sourceKey ? 'Выбрать моменты' : 'Найти моменты' : `Собрать ${settings.count} ${settings.count === 1 ? 'ролик' : settings.count > 1 && settings.count < 5 ? 'ролика' : 'роликов'}`}</button>}</div>
    </footer><div className="bottom-meta"><span className={saveState.startsWith('Не удалось') ? 'save-error' : ''}>{saveState}</span></div></div>
    {linkKind && <div className="modal-scrim" onClick={() => !loadingSource && setLinkKind(null)}><section ref={linkDialog} className="import-modal" role="dialog" aria-modal="true" aria-labelledby="link-dialog-title" onClick={e => e.stopPropagation()}><div className="modal-heading"><Link2 size={22} /><h2 id="link-dialog-title">{linkKind === 'top' ? 'Основное видео' : 'Фоновое видео'} по ссылке</h2><button className="icon-button" aria-label="Закрыть импорт" disabled={!!loadingSource} onClick={() => setLinkKind(null)}><X size={20} /></button></div><form onSubmit={e => { e.preventDefault(); void importLink(); }}><label className="field-label">Ссылка на видео<input aria-label="Ссылка на видео" autoFocus type="url" required value={linkUrl} disabled={!!loadingSource} placeholder="https://…" onChange={e => setLinkUrl(e.target.value)} /></label><p className="import-help">Видео появится в исходниках и сохранится в выбранной папке.</p><div className="import-folder-notice"><FolderOpen size={15} aria-hidden="true" /><span>{settings.importDir || 'Выберите папку для загрузок в медиатеке'}</span><button type="button" className="text-action" disabled={!!loadingSource || !settings.importDir} onClick={() => void api.openFolder(settings.importDir).catch(e => setLinkError(message(e)))}>Открыть папку</button></div>{linkError && <div className="import-error" role="alert"><AlertCircle size={17} /><span>{linkError}</span></div>}<div className="modal-actions"><button type="button" className="secondary-button" disabled={!!loadingSource} onClick={() => setLinkKind(null)}>Отмена</button><button className="primary-button" type="submit" disabled={!!loadingSource || !/^https?:\/\//i.test(linkUrl.trim()) || !native}>{loadingSource ? <Loader2 size={17} className="spin" /> : <Link2 size={17} />}{loadingSource ? 'Загружаем видео…' : 'Загрузить'}</button></div></form></section></div>}
    {momentsOpen && <MomentsPanel returnFocus={momentsTrigger.current} snapshot={moments} selection={momentSelection} onSelection={next => { setMomentSelection(next); setError(''); }} onClose={() => setMomentsOpen(false)} onCancel={() => void api.cancelMoments(moments.id).then(setMoments).catch(error => setError(message(error)))} onAnalyze={beginAnalysis} onExport={exportSelected} exporting={starting} exportReason={!haveSources ? 'Добавьте фоновое видео или выберите режим без фона в настройках кадра.' : exportHint} error={error} />}
    {editingPath && <SubtitleEditor path={editingPath} onClose={() => setEditingPath('')} onSaved={() => setSaveState('Видео обновлено')} />}
    {previewPath && <div className="modal-scrim" onClick={() => setPreviewPath('')}><section className="preview-modal" role="dialog" aria-modal="true" aria-label="Проверка готового видео" onClick={e => e.stopPropagation()}><div><h2>Проверка композиции</h2><button className="icon-button" aria-label="Закрыть просмотр" onClick={() => setPreviewPath('')}><X size={20} /></button></div><video key={previewPath} src={assetUrl(previewPath)} controls autoPlay playsInline /><p>5 секунд основного видео с кадрированием и звуком. Баннер-пауза добавляется к этой длительности.</p></section></div>}
  </div>;
}
