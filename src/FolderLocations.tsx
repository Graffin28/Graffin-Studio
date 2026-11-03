import { open } from '@tauri-apps/plugin-dialog';
import { FolderOpen, Download, ExternalLink, Film } from 'lucide-react';
import { api, native } from './bridge';
import type { Settings } from './types';

interface Props { settings: Settings; disabled: boolean; onChange: (patch: Partial<Settings>) => void; onError: (message: string) => void }
export default function FolderLocations({ settings, disabled, onChange, onError }: Props) {
  const choose = async (key: 'importDir' | 'outputDir') => {
    try {
      const path = await open({ title: key === 'importDir' ? 'Куда сохранять видео по ссылкам' : 'Куда сохранять готовые ролики', directory: true, multiple: false, defaultPath: settings[key] || undefined });
      if (typeof path === 'string') onChange({[key]:path});
    } catch (error) { onError(String(error)); }
  };
  return <section className="folder-locations" aria-label="Папки проекта"><h3><FolderOpen size={15} aria-hidden="true" />Папки</h3>{([{key:'importDir',name:'Видео по ссылкам',icon:Download},{key:'outputDir',name:'Готовые ролики',icon:Film}] as const).map(item => <div className="folder-location" key={item.key}><div className="folder-location-heading"><span><item.icon size={13} aria-hidden="true" />{item.name}</span><button className="icon-button" type="button" aria-label={`Открыть папку: ${item.name}`} title="Открыть папку в проводнике" disabled={disabled || !native || !settings[item.key]} onClick={() => void api.openFolder(settings[item.key]).catch(error => onError(String(error)))}><ExternalLink size={14} aria-hidden="true" /></button></div><button className="folder-location-path" type="button" aria-label={`Выбрать папку: ${item.name}`} disabled={disabled || !native} title={settings[item.key] || 'Выбрать папку'} onClick={() => choose(item.key)}><span>{settings[item.key] || 'Выбрать папку'}</span><FolderOpen size={14} aria-hidden="true" /></button></div>)}</section>;
}
