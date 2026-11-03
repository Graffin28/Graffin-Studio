import { ImagePlus, Pause } from 'lucide-react';
import type { Settings } from './types';
import { assetUrl, native } from './bridge';
import { Seconds } from './MusicPanel';

interface Props { settings: Settings; disabled: boolean; onChange: (patch: Partial<Settings>) => void; onImport: () => void }
export default function InterruptionPanel({ settings, disabled, onChange, onImport }: Props) {
  const asset = settings.bannerLibrary.find(item => item.path === settings.interruptionPath);
  return <section className="settings-section interruption-settings" aria-label="Баннер с паузой">
    <div className="section-heading"><h3><Pause size={17} />Баннер-пауза</h3><button className={`switch ${settings.interruptionEnabled ? 'on' : ''}`} role="switch" aria-checked={settings.interruptionEnabled} aria-label="Баннер с остановкой видео и звука" disabled={disabled} onClick={() => onChange({ interruptionEnabled: !settings.interruptionEnabled })}><i /></button></div>
    <p className="setting-help">Кадр и звук замирают, появляется баннер. Затем ролик продолжается с того же места.</p>
    {settings.interruptionEnabled && <div className="animated-expansion">
      <div className="asset-select-line"><select aria-label="Баннер для паузы" value={settings.interruptionPath} disabled={disabled} onChange={event => onChange({ interruptionPath: event.target.value })}><option value="">Выберите баннер</option>{settings.interruptionPath && !asset && <option value={settings.interruptionPath}>Сохранённый баннер</option>}{settings.bannerLibrary.map(item => <option key={item.id} value={item.path}>{item.name}</option>)}</select><button className="icon-button" aria-label="Загрузить баннер для паузы" title="Загрузить изображение или видео" disabled={disabled || !native} onClick={onImport}><ImagePlus size={18} /></button></div>
      {asset && <img className="interruption-thumbnail" src={assetUrl(asset.preview)} alt={`Баннер-пауза: ${asset.name}`} />}
      <label className="field-label">Когда показать<select aria-label="Когда показать баннер" value={settings.interruptionTiming} disabled={disabled} onChange={event => onChange({ interruptionTiming: event.target.value as Settings['interruptionTiming'] })}><option value="middle">В середине ролика</option><option value="quiet">В тихом моменте</option><option value="fixed">В указанную секунду</option><option value="random">Случайно в заданном диапазоне</option></select></label>
      {settings.interruptionTiming === 'fixed' && <Seconds label="Секунда появления" value={settings.interruptionStart} max={179} disabled={disabled} onChange={interruptionStart => onChange({ interruptionStart })} />}
      {settings.interruptionTiming === 'random' && <div className="number-pair"><Seconds label="Не раньше" value={settings.interruptionStart} max={Math.max(0, settings.interruptionEnd - .1)} disabled={disabled} onChange={interruptionStart => onChange({ interruptionStart })} /><Seconds label="Не позже" value={settings.interruptionEnd} min={settings.interruptionStart + .1} max={180} disabled={disabled} onChange={interruptionEnd => onChange({ interruptionEnd })} /></div>}
      {settings.interruptionTiming === 'quiet' && <p className="setting-help">Ищем паузу в звуке рядом с серединой. Если тихого момента нет, используем середину ролика.</p>}
      <Seconds label="Плавное появление и исчезновение" value={settings.interruptionFade} min={0} max={2} disabled={disabled} onChange={interruptionFade => onChange({interruptionFade})} />
      <Seconds label="Длительность паузы" value={settings.interruptionDuration} min={.5} max={30} disabled={disabled} onChange={interruptionDuration => onChange({ interruptionDuration })} />
      <p className="setting-help">Пауза добавит {settings.interruptionDuration} сек. к ролику. Баннер без звука; короткое видео повторяется. Момент появления ограничен длиной нарезки.</p>
    </div>}
  </section>;
}
