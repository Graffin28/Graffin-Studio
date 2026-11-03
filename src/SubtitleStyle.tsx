import { subtitleFonts, subtitlePalette } from './studioTypes';
import { useState } from 'react';
import { open } from '@tauri-apps/plugin-dialog';
import { Loader2, Plus, Play, Pause } from 'lucide-react';
import { native } from './bridge';
import { useFontLibrary } from './FontLibrary';
import CaptionPreview from './CaptionPreview';
import type { Settings } from './types';

export interface SubtitleStyleValue { subtitleFont: string; subtitleColor: string; subtitleSize: number; subtitleOffset: number; subtitleWordsPerLine: number; subtitleAnimation: Settings['subtitleAnimation']; subtitleHighlightEnabled: boolean; subtitleHighlightColor: string }
interface Props { value: SubtitleStyleValue; onChange: (value: SubtitleStyleValue) => void; disabled?: boolean; compact?: boolean }
export default function SubtitleStyle({ value, onChange, disabled, compact }: Props) {
  const { fonts, addFont, fontError } = useFontLibrary(value.subtitleFont);
  const [pending, setPending] = useState(false), [error, setError] = useState('');
  const [samplePlaying, setSamplePlaying] = useState(true);
  const change = (patch: Partial<SubtitleStyleValue>) => onChange({ ...value, ...patch });
  const upload = async () => {
    try {
      const path = await open({ title: 'Добавить шрифт в библиотеку', multiple: false, filters: [{ name: 'Шрифт', extensions: ['ttf', 'otf'] }] });
      if (typeof path !== 'string') return;
      setPending(true); setError('');
      const entry = await addFont(path); change({ subtitleFont: entry.id });
    } catch (error) { setError(String(error)); } finally { setPending(false); }
  };
  return <div className={`subtitle-style-controls ${compact ? 'compact' : ''}`}>
    <div className="caption-preview-card"><div className="caption-preview-heading"><span>Предпросмотр стиля</span><button type="button" className="icon-button" aria-label={samplePlaying ? 'Остановить пример субтитров' : 'Проиграть пример субтитров'} aria-pressed={samplePlaying} onClick={() => setSamplePlaying(current => !current)}>{samplePlaying ? <Pause size={14} /> : <Play size={14} />}</button></div><div className="caption-style-sample" aria-label="Образец выбранного стиля"><CaptionPreview value={value} playing={samplePlaying} /></div></div>
    <label className="field-label">Слов в строке<select aria-label="Слов в строке субтитров" value={value.subtitleWordsPerLine || 1} disabled={disabled} onChange={event => change({subtitleWordsPerLine:Number(event.target.value)})}>{[1,2,3,4,5].map(count => <option key={count} value={count}>{count === 1 ? 'По одному слову' : `До ${count} слов`}</option>)}</select></label>
    <label className="field-label">Анимация<select aria-label="Анимация субтитров" value={value.subtitleAnimation || 'pop'} disabled={disabled} onChange={event => change({subtitleAnimation:event.target.value as Settings['subtitleAnimation']})}><option value="none">Без анимации</option><option value="fade">Плавное появление</option><option value="pop">Пружина</option><option value="slide">Выезд снизу</option></select></label>
    <label className="check-option"><input type="checkbox" aria-label="Подсвечивать произносимое слово" checked={value.subtitleHighlightEnabled || false} disabled={disabled} onChange={event => change({subtitleHighlightEnabled:event.target.checked})} />Подсвечивать произносимое слово</label>
    {value.subtitleHighlightEnabled && <label className="caption-highlight-color">Цвет подсветки<input aria-label="Цвет произносимого слова" type="color" value={value.subtitleHighlightColor || '#A6FF52'} disabled={disabled} onChange={event => change({subtitleHighlightColor:event.target.value.toUpperCase()})} /></label>}
    <p className="setting-help">Количество слов — максимум в строке. Длинные паузы и конец фразы начинают новую строку.</p>
    <div className="caption-font-grid" role="group" aria-label="Шрифт субтитров">{subtitleFonts.map(font => <button type="button" key={font.id} aria-pressed={value.subtitleFont === font.id} className={value.subtitleFont === font.id ? 'selected' : ''} disabled={disabled} onClick={() => change({ subtitleFont: font.id })} style={{ fontFamily: `'${font.family}', sans-serif` }}><span>{font.name}</span><small>{font.feel}</small></button>)}</div>
    <div className="custom-font-library"><label className="field-label">Мои шрифты<select aria-label="Мои шрифты" value={value.subtitleFont.startsWith('custom-') ? value.subtitleFont : ''} disabled={disabled || pending} onChange={event => event.target.value && change({ subtitleFont: event.target.value })}><option value="">Выберите свой шрифт</option>{value.subtitleFont.startsWith('custom-') && !fonts.some(font => font.id === value.subtitleFont) && <option value={value.subtitleFont}>Сохранённый шрифт</option>}{fonts.map(font => <option key={font.id} value={font.id}>{font.name}</option>)}</select></label><button type="button" className="secondary-button" disabled={disabled || pending || !native} onClick={upload}>{pending ? <Loader2 size={15} className="spin" /> : <Plus size={15} />}{pending ? 'Загружаем…' : 'Добавить TTF / OTF'}</button><p className="setting-help">Шрифт сохраняется в приложении и используется при экспорте.</p>{(error || fontError) && <p className="inline-setting-error" role="alert">{error || fontError}</p>}</div>
    <div className="caption-color-line"><span>Цвет</span><div className="caption-colors" role="group" aria-label="Цвет субтитров">{subtitlePalette.map(color => <button type="button" key={color} className={value.subtitleColor.toUpperCase() === color ? 'selected' : ''} aria-label={`Цвет ${color}`} aria-pressed={value.subtitleColor.toUpperCase() === color} style={{ '--swatch': color } as React.CSSProperties} disabled={disabled} onClick={() => change({ subtitleColor: color })} />)}<label className="caption-custom-color" title="Свой цвет"><input type="color" aria-label="Свой цвет субтитров" value={value.subtitleColor} disabled={disabled} onChange={event => change({ subtitleColor: event.target.value.toUpperCase() })} /><span>+</span></label></div></div>

  </div>;
}
