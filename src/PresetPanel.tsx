import { useEffect, useState } from 'react';
import { Check, ChevronDown, Loader2, Plus, Pencil, Save, Trash2, Undo2 } from 'lucide-react';
import type { Settings } from './types';
import type { StudioPreset } from './studioTypes';
import { studioApi } from './studioBridge';

interface Props { settings: Settings; disabled: boolean; onApply: (preset: StudioPreset) => Promise<void>; onError: (message: string) => void }
export default function PresetPanel({ settings, disabled, onApply, onError }: Props) {
  const [entries, setEntries] = useState<StudioPreset[]>([]), [selected, setSelected] = useState('');
  const [name, setName] = useState(''), [editing, setEditing] = useState(false), [pending, setPending] = useState(''), [notice, setNotice] = useState('');
  const [replaceId, setReplaceId] = useState('');
  const [replaceStyle, setReplaceStyle] = useState(false);
  const [deleted, setDeleted] = useState<StudioPreset | null>(null);
  useEffect(() => { let alive = true; studioApi.presets().then(items => alive && setEntries(items)).catch(error => alive && onError(String(error))); return () => { alive = false; }; }, []);
  const current = entries.find(item => item.id === selected);
  const busy = disabled || !!pending;
  const action = async (key: string, fn: () => Promise<void>) => { setPending(key); setNotice(''); try { await fn(); } catch (error) { onError(error instanceof Error ? error.message : String(error)); } finally { setPending(''); } };
  const save = (replace: boolean) => action('save', async () => {
    const original = entries.find(item => item.id === replaceId);
    const next = await studioApi.savePreset({ id: replace ? replaceId : undefined, name: name.trim(), settings: replace && !replaceStyle && original ? original.settings : settings });
    setEntries(next); const saved = next.find(item => item.name === name.trim()); setSelected(saved?.id || ''); setEditing(false); setNotice('Пресет сохранён');
  });
  const remove = () => current && action('delete', async () => { setEntries(await studioApi.deletePreset(current.id)); setDeleted(current); setSelected(''); setName(''); setNotice('Пресет удалён'); });
  const undo = () => deleted && action('undo', async () => { const next = await studioApi.savePreset({ ...deleted }); setEntries(next); setSelected(deleted.id); setName(deleted.name); setDeleted(null); setNotice('Пресет восстановлен'); });
  return <section className="preset-panel" aria-label="Пресеты оформления"><div className="inspector-section-title"><h3>Пресет оформления</h3><button className="icon-button" type="button" aria-label="Сохранить новый пресет" title="Сохранить новый пресет" disabled={busy} onClick={() => { setName(''); setReplaceId(''); setReplaceStyle(false); setEditing(true); setNotice(''); }}><Plus size={17} /></button></div>
    <div className="preset-select-line"><select aria-label="Пресет оформления" value={selected} disabled={busy || editing} onChange={event => { setSelected(event.target.value); setName(entries.find(item => item.id === event.target.value)?.name || ''); setNotice(''); }}><option value="">Текущее оформление</option>{entries.map(item => <option key={item.id} value={item.id}>{item.name}</option>)}</select><ChevronDown size={14} /></div>
    {current && !editing && <div className="preset-actions"><button className="secondary-button" type="button" disabled={busy} onClick={() => action('apply', async () => { await onApply(current); setNotice('Пресет применён'); })}>{pending === 'apply' ? <Loader2 className="spin" size={14} /> : <Check size={14} />}Применить</button><button className="icon-button" type="button" disabled={busy} aria-label="Переименовать или обновить пресет" title="Изменить пресет" onClick={() => { setName(current.name); setReplaceId(current.id); setReplaceStyle(false); setEditing(true); }}><Pencil size={15} /></button><button className="icon-button" type="button" disabled={busy} aria-label="Удалить пресет" title="Удалить пресет" onClick={remove}><Trash2 size={15} /></button></div>}
    {editing && <form className="preset-save-form" onSubmit={event => { event.preventDefault(); void save(!!replaceId); }}><input aria-label="Название пресета" autoFocus placeholder="Например, юмор с жёлтыми субтитрами" maxLength={60} value={name} disabled={busy} onChange={event => setName(event.target.value)} />{!!replaceId && <label className="preset-replace-option"><input type="checkbox" checked={replaceStyle} disabled={busy} onChange={event => setReplaceStyle(event.target.checked)} />Заменить оформление текущим</label>}<div><button className="secondary-button" type="button" disabled={busy} onClick={() => setEditing(false)}>Отмена</button><button className="primary-button" disabled={busy || !name.trim()}>{pending === 'save' ? <Loader2 className="spin" size={14} /> : <Save size={14} />}{'Сохранить'}</button></div></form>}
    <div className="preset-notice" aria-live="polite">{notice || (!entries.length && !editing ? 'Кадр, субтитры, музыка и баннеры' : '')}{deleted && <button type="button" disabled={busy} onClick={undo}><Undo2 size={12} />Отменить удаление</button>}</div>
  </section>;
}
