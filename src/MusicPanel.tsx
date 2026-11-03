import { useEffect, useRef, useState } from 'react';
import { open } from '@tauri-apps/plugin-dialog';
import { Loader2, Music2, Plus, X } from 'lucide-react';
import { api, assetUrl, native } from './bridge';
import type { MusicAsset, Settings } from './types';

export function Seconds({ label, value, max = 180, min = 0, onChange, disabled }: { label: string; value: number; min?: number; max?: number; onChange: (value: number) => void; disabled?: boolean }) {
  const [draft, setDraft] = useState(String(value));
  useEffect(() => setDraft(String(value)), [value]);
  const commit = () => {
    const n = Number(draft.replace(',', '.'));
    const next = draft.trim() && Number.isFinite(n) ? Math.round(Math.max(min, Math.min(max, n)) * 100) / 100 : value;
    onChange(next); setDraft(String(next));
  };
  return <label className="numeric-label"><span>{label}</span><div className="numeric-input"><input aria-label={label} type="number" min={min} max={max} step="0.1" value={draft} disabled={disabled} onChange={event => setDraft(event.target.value)} onBlur={commit} onKeyDown={event => { if (event.key === 'Enter') event.currentTarget.blur(); }} /><span>сек.</span></div></label>;
}

interface Props { settings: Settings; disabled: boolean; onChange: (patch: Partial<Settings>) => void; onError: (message: string) => void; onPending: (pending: boolean) => void }
export default function MusicPanel({ settings, disabled, onChange, onError, onPending }: Props) {
  const [entries, setEntries] = useState<MusicAsset[]>([]), [pending, setPending] = useState(false);
  const audio = useRef<HTMLAudioElement>(null);
  useEffect(() => { let alive = true; api.music().then(result => alive && setEntries(result)).catch(error => alive && onError(String(error))); return () => { alive = false; }; }, []);
  useEffect(() => { if (audio.current) { audio.current.volume = Math.min(1, settings.musicVolume); if (audio.current.paused) audio.current.currentTime = settings.musicStart; } }, [settings.musicStart, settings.musicVolume]);
  useEffect(() => { if (disabled) audio.current?.pause(); }, [disabled]);
  const select = (entry?: MusicAsset) => onChange({ musicPath: entry?.path || '', musicName: entry?.name || '', musicDuration: entry?.duration || 0, musicStart: 0 });
  const add = async () => {
    try {
      const path = await open({ title: 'Добавить музыку в библиотеку', multiple: false, filters: [{ name: 'Музыка', extensions: ['mp3', 'wav', 'm4a', 'aac', 'flac', 'ogg', 'opus', 'aiff', 'aif'] }] });
      if (typeof path !== 'string') return;
      setPending(true); onPending(true);
      const entry = await api.importMusic(path);
      setEntries(current => [...current.filter(item => item.id !== entry.id), entry]); select(entry);
    } catch (error) { onError(String(error)); } finally { setPending(false); onPending(false); }
  };
  const selected = entries.find(entry => entry.path === settings.musicPath);
  const trackDuration = selected?.duration || settings.musicDuration;
  return <section className="settings-section music-settings" aria-label="Фоновая музыка">
    <div className="inspector-section-title"><h3><Music2 size={17} />Фоновая музыка</h3><button className="icon-button" aria-label="Добавить свою музыку" title="Добавить MP3, WAV или другой трек" disabled={disabled || !native} onClick={add}>{pending ? <Loader2 size={17} className="spin" /> : <Plus size={17} />}</button></div>
    <select aria-label="Трек из библиотеки" value={settings.musicPath} disabled={disabled} onChange={event => select(entries.find(entry => entry.path === event.target.value))}><option value="">Без музыки</option>{settings.musicPath && !selected && <option value={settings.musicPath}>{settings.musicName || 'Сохранённый трек'}</option>}{entries.map(entry => <option key={entry.id} value={entry.path}>{entry.name}</option>)}</select>
    {!settings.musicPath ? <p className="setting-help">Добавьте трек один раз — он останется в библиотеке для следующих роликов.</p> : <div className="animated-expansion">
      <div className="music-selected"><span>{settings.musicName || selected?.name || 'Моя музыка'}</span><button className="icon-button" title="Убрать музыку из ролика" aria-label="Убрать музыку из ролика" disabled={disabled} onClick={() => select()}><X size={14} /></button></div>
      <audio ref={audio} key={settings.musicPath} aria-label="Прослушать музыку" src={assetUrl(settings.musicPath)} controls preload="metadata" onLoadedMetadata={event => { event.currentTarget.currentTime = settings.musicStart; event.currentTarget.volume = Math.min(1, settings.musicVolume); }} onError={() => onError('Не удалось прослушать трек. Загрузите его заново.')} />
      <button className="text-action" disabled={disabled} onClick={() => onChange({ musicStart: Math.round(Math.min(audio.current?.currentTime || 0, Math.max(0, trackDuration - .1)) * 100) / 100 })}>Начать с текущего момента плеера</button>
      <div className="number-pair"><Seconds label="Начало в треке" value={settings.musicStart} max={Math.max(0, trackDuration - .1)} disabled={disabled} onChange={musicStart => onChange({ musicStart })} /><Seconds label="Включить на секунде" value={settings.musicDelay} max={179} disabled={disabled} onChange={musicDelay => onChange({ musicDelay })} /></div>
      <p className="setting-help">Берём музыку с {settings.musicStart.toFixed(1)} сек. трека и включаем на {settings.musicDelay.toFixed(1)} сек. видео. При повторе возвращаемся к выбранному началу.</p>
      <label className="range-label"><span className="range-heading"><span>Громкость музыки</span><output>{Math.round(settings.musicVolume * 100)}%</output></span><input aria-label="Громкость музыки" type="range" min="0" max="1" step="0.01" value={settings.musicVolume} disabled={disabled} onChange={event => onChange({ musicVolume: Number(event.target.value) })} /></label>
      <label className="check-option"><input type="checkbox" checked={settings.musicLoop} disabled={disabled} onChange={event => onChange({ musicLoop: event.target.checked })} />Повторять выбранный участок</label>
      <div className="number-pair"><Seconds label="Плавное начало" value={settings.musicFadeIn} max={15} disabled={disabled} onChange={musicFadeIn => onChange({ musicFadeIn })} /><Seconds label="Плавное затухание" value={settings.musicFadeOut} max={15} disabled={disabled} onChange={musicFadeOut => onChange({ musicFadeOut })} /></div>
      {settings.interruptionEnabled && <p className="setting-help">Во время баннера-паузы музыка тоже останавливается.</p>}
    </div>}
  </section>;
}
