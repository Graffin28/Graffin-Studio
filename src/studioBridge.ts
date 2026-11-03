import { invoke } from '@tauri-apps/api/core';
import { native } from './bridge';
import type { PresetSaveRequest, StudioPreset, SubtitleDocument, SubtitleEditRequest } from './studioTypes';

const PRESET_KEY = 'graffin-studio-presets-v2';
const demoPresets = (): StudioPreset[] => { try { const value: unknown = JSON.parse(localStorage.getItem(PRESET_KEY) || '[]'); return Array.isArray(value) ? value.filter((entry): entry is StudioPreset => !!entry && typeof entry === 'object' && typeof entry.id === 'string' && typeof entry.name === 'string' && !!entry.settings && typeof entry.settings === 'object') : []; } catch { return []; } };
const unavailable = <T,>(): Promise<T> => Promise.reject(new Error('Откройте приложение для редактирования видео.'));
export const studioApi = {
  presets: (): Promise<StudioPreset[]> => native ? invoke('studio_presets') : Promise.resolve(demoPresets()),
  savePreset: (request: PresetSaveRequest): Promise<StudioPreset[]> => {
    if (native) return invoke('studio_save_preset', { request });
    const entry = { ...request, id: request.id || crypto.randomUUID(), updatedAt: Date.now() };
    const entries = [...demoPresets().filter(item => item.id !== entry.id), entry].sort((a, b) => b.updatedAt - a.updatedAt);
    localStorage.setItem(PRESET_KEY, JSON.stringify(entries)); return Promise.resolve(entries);
  },
  deletePreset: (id: string): Promise<StudioPreset[]> => {
    if (native) return invoke('studio_delete_preset', { id });
    const entries = demoPresets().filter(item => item.id !== id);
    localStorage.setItem(PRESET_KEY, JSON.stringify(entries)); return Promise.resolve(entries);
  },
  subtitleDocument: (path: string): Promise<SubtitleDocument> => native ? invoke('subtitle_document', { path }) : unavailable(),
  saveSubtitles: (request: SubtitleEditRequest): Promise<SubtitleDocument> => native ? invoke('subtitle_save', { request }) : unavailable(),
};
