import { useEffect, useRef, useState } from 'react';
import { AlertCircle, Check, ChevronLeft, ChevronRight, Loader2, Plus, Save, Search, Trash2, X } from 'lucide-react';
import { api, assetUrl } from './bridge';
import { studioApi } from './studioBridge';
import type { SubtitleDocument, SubtitleWord } from './studioTypes';
import SubtitleStyle, { type SubtitleStyleValue } from './SubtitleStyle';

interface Props { path: string; onClose: () => void; onSaved?: () => void }
const format = (seconds: number) => `${Math.floor(seconds / 60)}:${(seconds % 60).toFixed(2).padStart(5, '0')}`;
export default function SubtitleEditor({ path, onClose, onSaved }: Props) {
  const [doc, setDoc] = useState<SubtitleDocument | null>(null), [words, setWords] = useState<SubtitleWord[]>([]);
  const [style, setStyle] = useState<SubtitleStyleValue>({ subtitleFont: 'manrope', subtitleColor: '#FFFFFF', subtitleSize: 56, subtitleOffset: 0, subtitleWordsPerLine: 1, subtitleAnimation: 'pop', subtitleHighlightEnabled: false, subtitleHighlightColor: '#A6FF52' });
  const [selected, setSelected] = useState(0), [clock, setClock] = useState(0), [query, setQuery] = useState(''), [pending, setPending] = useState('load'), [error, setError] = useState(''), [saved, setSaved] = useState(false);
  const video = useRef<HTMLVideoElement>(null), dialog = useRef<HTMLElement>(null), closeButton = useRef<HTMLButtonElement>(null);
  const dirty = !!doc && (JSON.stringify(words) !== JSON.stringify(doc.words) || Object.entries(style).some(([key, value]) => doc.settings[key as keyof SubtitleStyleValue] !== value));
  const saving = pending === 'save', loading = pending === 'load';
  const editable = !!doc?.editable && !pending;
  const invalid = words.some((word, index) => !word.text.trim() || /\s/u.test(word.text.trim()) || word.text.length > 120 || /[<>\x00-\x1F]/u.test(word.text) || !/[\p{L}\p{N}]/u.test(word.text) || !Number.isFinite(word.start) || !Number.isFinite(word.end) || word.start < 0 || word.end - word.start < .01 || word.end > (doc?.duration || 0) + .001 || (index > 0 && word.start < words[index - 1].end - .001));
  useEffect(() => {
    let alive = true;
    studioApi.subtitleDocument(path).then(next => { if (alive) { setDoc(next); setWords(next.words); setStyle({ subtitleFont: next.settings.subtitleFont || 'manrope', subtitleColor: next.settings.subtitleColor || '#FFFFFF', subtitleSize: next.settings.subtitleSize, subtitleOffset: next.settings.subtitleOffset, subtitleWordsPerLine: next.settings.subtitleWordsPerLine || 1, subtitleAnimation: next.settings.subtitleAnimation || 'pop', subtitleHighlightEnabled: next.settings.subtitleHighlightEnabled || false, subtitleHighlightColor: next.settings.subtitleHighlightColor || '#A6FF52' }); } }).catch(cause => alive && setError(cause instanceof Error ? cause.message : String(cause))).finally(() => alive && setPending(''));
    const previous = document.activeElement as HTMLElement | null;
    closeButton.current?.focus();
    return () => { alive = false; previous?.focus(); };
  }, [path]);
  useEffect(() => {
    const key = (event: KeyboardEvent) => {
      if (event.key === 'Escape' && !saving) { event.preventDefault(); if (!dirty) onClose(); else setError('Есть изменения. Сохраните их или нажмите «Закрыть без сохранения».'); }
      if (event.key === 'Tab') { const controls = Array.from(dialog.current?.querySelectorAll<HTMLElement>('button:not(:disabled),input:not(:disabled),select:not(:disabled),video[controls],[tabindex="0"]') || []).filter(el => el.offsetParent !== null); if (!controls.length) return; if (event.shiftKey && document.activeElement === controls[0]) { event.preventDefault(); controls.at(-1)?.focus(); } else if (!event.shiftKey && document.activeElement === controls.at(-1)) { event.preventDefault(); controls[0].focus(); } }
    };
    window.addEventListener('keydown', key); return () => window.removeEventListener('keydown', key);
  }, [saving, dirty, onClose]);
  const outputTime = (contentTime: number) => doc?.pauseAt != null && contentTime >= doc.pauseAt ? contentTime + (doc.pauseDuration || 0) : contentTime;
  const contentTime = (outputTime: number) => doc?.pauseAt != null && outputTime >= doc.pauseAt ? Math.max(doc.pauseAt, outputTime - (doc.pauseDuration || 0)) : outputTime;
  const choose = (index: number) => { setSelected(index); if (video.current && words[index]) { video.current.currentTime = outputTime(words[index].start); setClock(words[index].start); } };
  const addWord = () => {
    if (!editable || !doc || words.length >= 5000) return;
    const next = words.map(word => ({ ...word }));
    let index = Math.max(0, selected);
    if (!next.length) { next.push({ start: 0, end: Math.min(.7, doc.duration), text: 'Слово' }); index = 0; }
    else { const word = next[index]; const middle = (word.start + word.end) / 2; if (word.end - word.start < .04) { setError('Сначала увеличьте длительность выбранного слова, чтобы разделить его.'); return; } const end = word.end; word.end = middle; next.splice(index + 1, 0, { start: middle, end, text: 'Слово' }); index += 1; }
    setWords(next); setSelected(index); setQuery(''); setSaved(false); setError('');
  };
  const updateWord = (patch: Partial<SubtitleWord>) => { setSaved(false); setWords(current => current.map((word, index) => index === selected ? { ...word, ...patch } : word)); };
  const save = async () => {
    if (!doc || !editable || invalid || !dirty) return;
    video.current?.pause(); setPending('save'); setError(''); setSaved(false);
    try { const next = await studioApi.saveSubtitles({ path: doc.path, revision: doc.revision, words: words.map(word => ({ ...word, text: word.text.trim() })), ...style }); setDoc(next); setWords(next.words); setSelected(index => Math.min(index, Math.max(0, next.words.length - 1))); setSaved(true); onSaved?.(); }
    catch (cause) { setError(cause instanceof Error ? cause.message : String(cause)); }
    finally { setPending(''); }
  };
  const close = () => { if (saving) return; if (dirty) setError('Есть изменения. Сохраните их или нажмите «Закрыть без сохранения».'); else onClose(); };
  const current = words[selected];
  const previewUrl = doc ? assetUrl(doc.previewPath) : '';
  const videoUrl = previewUrl && !/^(blob:|data:)/.test(previewUrl) ? `${previewUrl}${previewUrl.includes('?') ? '&' : '?'}revision=${encodeURIComponent(doc?.revision || 'legacy')}` : previewUrl;
  return <div className="editor-scrim"><section ref={dialog} className="subtitle-editor" role="dialog" aria-modal="true" aria-labelledby="subtitle-editor-title"><header className="subtitle-editor-heading"><div><h2 id="subtitle-editor-title">Редактор субтитров</h2><p>{doc?.name || path.split(/[\\/]/).pop()}</p></div><button ref={closeButton} className="icon-button" aria-label="Закрыть редактор субтитров" disabled={saving} onClick={close}><X size={21} /></button></header>
    {loading ? <div className="editor-loading"><Loader2 className="spin" size={26} /><span>Открываем слова и видео…</span></div> : <><div className="subtitle-editor-body"><div className="editor-video-pane">{doc && <video ref={video} key={`${doc.previewPath}-${doc.revision}`} src={videoUrl} controls playsInline preload="metadata" onTimeUpdate={event => setClock(contentTime(event.currentTarget.currentTime))} />}<span className="editor-play-clock">{format(clock)} / {format(doc?.duration || 0)}</span>{!!doc?.pauseDuration && <p className="setting-help">Время слов указано без баннера-паузы. Плеер учитывает вставку.</p>}</div>
    <div className="editor-word-pane"><label className="editor-word-search"><Search size={16} /><input aria-label="Найти слово в субтитрах" value={query} placeholder="Найти слово" onChange={event => setQuery(event.target.value)} /></label>{doc && !doc.editable && <div className="editor-readonly"><AlertCircle size={18} /><p>{doc.reason || 'Этот ролик доступен только для просмотра.'}</p></div>}
    <div className="editor-word-tools"><span>{words.length} слов</span><button className="secondary-button" disabled={!editable || words.length >= 5000} onClick={addWord}><Plus size={14} />Добавить слово</button></div><div className="editor-word-grid" aria-label="Слова субтитров">{words.map((word, index) => !query || word.text.toLocaleLowerCase().includes(query.toLocaleLowerCase()) ? <button key={index} type="button" aria-label={`Слово ${index + 1}: ${word.text}`} aria-pressed={index === selected} disabled={saving} className={`${index === selected ? 'selected' : ''} ${word.start <= clock && word.end > clock ? 'playing' : ''}`} onClick={() => choose(index)}><span>{word.text || '…'}</span><small>{format(word.start)}</small></button> : null)}{!words.length && <p className="editor-empty">{doc?.editable ? 'В этом фрагменте нет распознанных слов.' : 'Слова доступны у роликов, созданных в новой версии.'}</p>}</div>
    {current && <div className="editor-word-fields"><div className="editor-word-navigation"><span>Слово {selected + 1} из {words.length}</span><button className="icon-button" aria-label="Предыдущее слово" disabled={saving || selected === 0} onClick={() => choose(selected - 1)}><ChevronLeft size={17} /></button><button className="icon-button" aria-label="Следующее слово" disabled={saving || selected === words.length - 1} onClick={() => choose(selected + 1)}><ChevronRight size={17} /></button></div><label>Текст<input aria-label="Текст выбранного слова" value={current.text} maxLength={120} disabled={!editable} onChange={event => updateWord({ text: event.target.value })} /></label><div className="editor-timing"><label>Начало, сек.<input aria-label="Начало слова" type="number" min={0} max={doc?.duration} step="0.01" value={Number(current.start.toFixed(3))} disabled={!editable} onChange={event => updateWord({ start: Number(event.target.value) })} /></label><label>Конец, сек.<input aria-label="Конец слова" type="number" min={0} max={doc?.duration} step="0.01" value={Number(current.end.toFixed(3))} disabled={!editable} onChange={event => updateWord({ end: Number(event.target.value) })} /></label><button className="icon-button" aria-label="Удалить выбранное слово" title="Удалить слово" disabled={!editable} onClick={() => { setWords(current => current.filter((_, index) => index !== selected)); setSelected(index => Math.max(0, index - 1)); setSaved(false); }}><Trash2 size={16} /></button></div></div>}</div>
    <aside className="editor-style-pane"><h3>Оформление</h3><SubtitleStyle compact value={style} disabled={!editable} onChange={next => { setStyle(next); setSaved(false); }} /><label className="editor-size-control">Размер <output>{style.subtitleSize} px</output><input aria-label="Размер субтитров в редакторе" type="range" min={28} max={96} step={2} value={style.subtitleSize} disabled={!editable} onChange={event => { setStyle(current => ({ ...current, subtitleSize: Number(event.target.value) })); setSaved(false); }} /></label><label className="editor-size-control">Смещение <output>{style.subtitleOffset} px</output><input aria-label="Смещение субтитров в редакторе" type="range" min={-600} max={600} step={2} value={style.subtitleOffset} disabled={!editable} onChange={event => { setStyle(current => ({ ...current, subtitleOffset: Number(event.target.value) })); setSaved(false); }} /></label><p className="editor-style-help">После сохранения видео пересоберётся с исправленными словами и выбранным стилем.</p></aside></div>
    {(error || invalid) && <div className="editor-error" role="alert"><AlertCircle size={17} /><span>{error || 'В каждом поле должно быть одно слово. Время — в пределах ролика, без наложений.'}</span></div>}
    <footer className="subtitle-editor-footer"><span aria-live="polite">{saving ? <><Loader2 size={15} className="spin" />Пересобираем видео. Оригинал сохранён до завершения.</> : saved ? <><Check size={16} />Видео обновлено</> : dirty ? 'Есть изменения' : doc?.editable ? 'Выберите слово, чтобы исправить его' : 'Просмотр'}</span><div>{saving ? <button className="secondary-button" onClick={() => void api.cancelBatch().catch(cause => setError(String(cause)))}>Остановить</button> : dirty ? <button className="secondary-button" onClick={onClose}>Закрыть без сохранения</button> : <button className="secondary-button" onClick={onClose}>Закрыть</button>}<button className="primary-button" disabled={!editable || !dirty || invalid} onClick={save}>{saving ? <Loader2 className="spin" size={15} /> : <Save size={15} />}Сохранить видео</button></div></footer></>}
    {!loading && !doc && <div className="editor-error" role="alert"><AlertCircle size={18} />{error || 'Не удалось открыть видео.'}</div>}
  </section></div>;
}
