import { useEffect, useRef, useState } from 'react';
import { AlertCircle, Check, ChevronLeft, ChevronRight, Loader2, Play, RefreshCw, Search, Square, X } from 'lucide-react';
import { assetUrl } from './bridge';
import type { MomentSnapshot, MomentSelection } from './momentTypes';

const stamp = (value: number) => `${Math.floor(value / 60)}:${(value % 60).toFixed(1).padStart(4, '0')}`;
interface Props { snapshot: MomentSnapshot; selection: MomentSelection[]; onSelection: (value: MomentSelection[]) => void; onClose: () => void; onCancel: () => void; onAnalyze: () => void; onExport: () => void; exporting: boolean; exportReason: string; error: string; returnFocus: HTMLElement | null }
export default function MomentsPanel({ snapshot, selection, onSelection, onClose, onCancel, onAnalyze, onExport, exporting, exportReason, error, returnFocus }: Props) {
  const dialog = useRef<HTMLDialogElement>(null), video = useRef<HTMLVideoElement>(null);
  const [active, setActive] = useState(''), [query, setQuery] = useState(''), [page, setPage] = useState(0);
  const analyzing = snapshot.status === 'analyzing' || snapshot.status === 'cancelling';
  useEffect(() => { const node = dialog.current, prior = returnFocus || document.activeElement as HTMLElement | null; node?.showModal(); return () => { node?.close(); prior?.focus(); }; }, []);
  useEffect(() => { if (!snapshot.candidates.some(item => item.id === active)) setActive(snapshot.candidates[0]?.id || ''); }, [snapshot.id, snapshot.candidates, active]);
  const candidate = snapshot.candidates.find(item => item.id === active);
  const source = snapshot.sources.find(item => item.id === candidate?.sourceId) || snapshot.sources[candidate?.sourceIndex ?? -1];
  const interval = selection.find(item => item.candidateId === active) || (candidate ? {candidateId:active,start:candidate.start,end:candidate.end} : null);
  useEffect(() => {
    const player = video.current;
    if (!player || !interval) return;
    player.pause();
    if (player.readyState >= 1) player.currentTime = interval.start;
  }, [candidate?.id, interval?.start, source?.path]);
  const filtered = snapshot.candidates.filter(item => `${item.text} ${item.sourceName}`.toLocaleLowerCase().includes(query.toLocaleLowerCase()));
  const pages = Math.max(1, Math.ceil(filtered.length / 15)), currentPage = Math.min(page, pages - 1);
  const rows = filtered.slice(currentPage * 15, (currentPage + 1) * 15);
  const toggle = (id: string) => { const item = snapshot.candidates.find(item => item.id === id); if (!item) return; onSelection(selection.some(value => value.candidateId === id) ? selection.filter(value => value.candidateId !== id) : [...selection,{candidateId:id,start:item.start,end:item.end}]); };
  const edit = (patch: Partial<MomentSelection>) => { if (!interval) return; const next = {...interval,...patch}; onSelection(selection.some(item => item.candidateId === active) ? selection.map(item => item.candidateId === active ? next : item) : [...selection,next]); };
  let validation = '';
  const intervals = selection.map(item => ({...item,candidate:snapshot.candidates.find(candidate => candidate.id === item.candidateId)}));
  for (const item of intervals) {
    const media = snapshot.sources.find(source => source.id === item.candidate?.sourceId);
    if (!item.candidate || !media || !Number.isFinite(item.start) || !Number.isFinite(item.end) || item.start < 0 || item.end > media.duration + .001 || item.end - item.start < 5 - .001 || item.end - item.start > 180 + .001) { validation = 'Длительность выбранного момента должна быть от 5 до 180 секунд, в пределах исходника.'; break; }
  }
  if (!validation) for (let i = 0; i < intervals.length; i++) for (let j = i + 1; j < intervals.length; j++) if (intervals[i].candidate?.sourceId === intervals[j].candidate?.sourceId && intervals[i].start < intervals[j].end - .001 && intervals[j].start < intervals[i].end - .001) validation = 'Выбранные моменты пересекаются. Измените границы или снимите один из них.';
  const jump = () => { if (video.current && interval) { video.current.currentTime = interval.start; void video.current.play().catch(() => {}); } };
  const blocking = validation || snapshot.error || exportReason;
  const problem = validation || error || snapshot.error || exportReason;
  return <dialog ref={dialog} className="moments-dialog" aria-labelledby="moments-title" onCancel={event => { event.preventDefault(); if (!exporting) onClose(); }} onKeyDown={event => {
    if (event.key !== 'Tab') return;
    const controls = Array.from(event.currentTarget.querySelectorAll<HTMLElement>('button:not(:disabled), input:not(:disabled), select:not(:disabled), textarea:not(:disabled), video[controls], a[href], [tabindex="0"]')).filter(node => node.getClientRects().length > 0);
    const first = controls[0], last = controls[controls.length - 1];
    if (!first) { event.preventDefault(); return; }
    if (event.shiftKey && document.activeElement === first) { event.preventDefault(); last.focus(); }
    else if (!event.shiftKey && document.activeElement === last) { event.preventDefault(); first.focus(); }
  }}>
    <header className="moments-heading"><div><h2 id="moments-title">Выберите моменты</h2><p>{analyzing ? 'Анализируем исходники локально' : `${snapshot.candidates.length} предложений · выбрано ${selection.length}`}</p></div><button type="button" className="icon-button" aria-label="Закрыть выбор моментов" disabled={exporting} onClick={onClose}><X size={21} /></button></header>
    {analyzing ? <div className="moments-analysis" role="status"><Loader2 className="spin" size={30} /><h3>Ищем законченные фрагменты</h3><p>{snapshot.message || 'Подготавливаем анализ речи…'}</p><progress max={1} value={snapshot.progress} aria-label="Прогресс поиска моментов" /><span>{Math.round(snapshot.progress * 100)}%</span></div> : snapshot.status !== 'ready' ? <div className="moments-analysis"><Search size={30} /><h3>{snapshot.status === 'cancelled' ? 'Поиск остановлен' : 'Моменты пока не найдены'}</h3><p>{snapshot.error || snapshot.message || 'Запустите анализ основных видео.'}</p><button className="secondary-button" type="button" onClick={onAnalyze}><RefreshCw size={15} />Повторить поиск</button></div> : <div className="moments-body">
      <section className="moments-list-pane" aria-label="Предложенные моменты"><label className="moments-search"><Search size={16} aria-hidden="true" /><input aria-label="Поиск по найденным моментам" value={query} placeholder="Слово или тема" onChange={event => { setQuery(event.target.value); setPage(0); }} /></label><ul className="moment-list">{rows.map(item => { const checked = selection.some(value => value.candidateId === item.id); return <li key={item.id} className={`moment-card ${active === item.id ? 'active' : ''} ${checked ? 'selected' : ''}`}><input type="checkbox" aria-label={`Выбрать момент: ${item.sourceName}, ${stamp(item.start)}`} checked={checked} disabled={exporting} onChange={() => toggle(item.id)} /><button type="button" className="moment-open" aria-pressed={active === item.id} onClick={() => setActive(item.id)}><span className="moment-meta"><strong>{stamp(item.start)} — {stamp(item.end)}</strong><span>{(item.end - item.start).toFixed(1)} сек.</span></span><span className="moment-copy">{item.text}</span><span className="moment-source">{item.sourceName}</span><span className="moment-reasons">{item.reasons.slice(0,3).map(reason => <span key={reason}>{reason}</span>)}</span></button></li>; })}{!rows.length && <li className="moments-empty">По этому запросу ничего не найдено.</li>}</ul><div className="moments-pagination"><button className="icon-button" aria-label="Предыдущая страница моментов" disabled={!currentPage} onClick={() => setPage(currentPage - 1)}><ChevronLeft size={17} /></button><span>{currentPage + 1} / {pages}</span><button className="icon-button" aria-label="Следующая страница моментов" disabled={currentPage >= pages - 1} onClick={() => setPage(currentPage + 1)}><ChevronRight size={17} /></button></div></section>
      <section className="moment-preview-pane" aria-label="Просмотр выбранного момента">{candidate && source && interval ? <><video ref={video} key={source.path} src={assetUrl(source.path)} controls playsInline preload="metadata" onLoadedMetadata={event => { event.currentTarget.currentTime = interval.start; }} onTimeUpdate={event => { if (event.currentTarget.currentTime >= interval.end && !event.currentTarget.paused) event.currentTarget.pause(); }} /><h3>{source.name}</h3><p className="setting-help">Просмотр исходника. Оформление добавится при сборке. После изменения границ текст субтитров пересчитается по выбранному времени.</p><div className="moment-boundaries"><label>Начало, сек.<input aria-label="Начало выбранного момента" type="number" min={0} max={source.duration} step="0.1" value={interval.start} disabled={exporting} onChange={event => edit({start:Number(event.target.value)})} /></label><label>Конец, сек.<input aria-label="Конец выбранного момента" type="number" min={0} max={source.duration} step="0.1" value={interval.end} disabled={exporting} onChange={event => edit({end:Number(event.target.value)})} /></label></div><button type="button" className="secondary-button" onClick={jump}><Play size={14} />Проиграть этот момент</button><label className="check-option"><input type="checkbox" checked={selection.some(item => item.candidateId === active)} disabled={exporting} onChange={() => toggle(active)} />Включить в сборку</label><p className="moment-full-text">{candidate.text}</p></> : <p className="setting-help">Выберите момент слева, чтобы посмотреть и уточнить границы.</p>}</section>
    </div>}
    {problem && <div className="moments-error" role="alert"><AlertCircle size={16} /><span>{problem}</span></div>}
    <footer className="moments-footer"><span>{selection.length ? `Готовим ${selection.length} ${selection.length === 1 ? 'ролик' : 'роликов'}` : 'Выберите подходящие фрагменты'}</span><div>{snapshot.status === 'ready' && <button type="button" className="secondary-button moments-refresh" disabled={exporting} onClick={onAnalyze}><RefreshCw size={14} />Обновить поиск</button>}{analyzing ? <button type="button" className="secondary-button" disabled={snapshot.status === 'cancelling'} onClick={onCancel}><Square size={13} />Остановить поиск</button> : <><button type="button" className="secondary-button" disabled={exporting} onClick={onClose}>Вернуться к оформлению</button><button type="button" className="primary-button" disabled={exporting || !!blocking || !selection.length || snapshot.status !== 'ready'} onClick={onExport}>{exporting ? <Loader2 className="spin" size={15} /> : <Check size={15} />}Собрать выбранные</button></>}</div></footer>
  </dialog>;
}
