import { invoke, convertFileSrc } from '@tauri-apps/api/core';
import type { BannerAsset, Bootstrap, BatchRequest, Job, Media, MediaToolsStatus, Settings, FontAsset, MusicAsset } from './types';
import { emptyJob } from './types';
import type { MomentAnalyzeRequest, MomentSnapshot, SelectedBatchRequest } from './momentTypes';
import { emptyMoments } from './momentTypes';

export const native = typeof window !== 'undefined' && '__TAURI_INTERNALS__' in window;
export const assetUrl = (path: string) => !path ? '' : /^(https?:|data:|blob:|\/banner\.png|\/mascot\.jpg|\/graffin-mark\.png)/.test(path) ? path : native ? convertFileSrc(path) : path;
const demoError = () => Promise.reject(new Error('Откройте приложение для экспорта и выбора файлов.'));
function restoredDemo(): Settings | null { try { return JSON.parse(localStorage.getItem('barsik-demo-settings') || 'null'); } catch { return null; } }
export const api = {
  bootstrap: (): Promise<Bootstrap> => native ? invoke('bootstrap') : Promise.resolve({
    defaultOutput: '', defaultImportDir: '', bannerPath: '/banner.png', bannerPreview: '/banner.png', version: '2.4.0', ffmpegReady: true,
    restoredSettings: restoredDemo(),
  }),
  inspectMedia: (paths: string[]): Promise<Media[]> => native ? invoke('inspect_media', { paths }) : demoError(),
  inspectMediaFolder: (path: string): Promise<Media[]> => native ? invoke('inspect_media_folder', { path }) : demoError(),
  fonts: (): Promise<FontAsset[]> => native ? invoke('font_library') : Promise.resolve([]),
  importFont: (path: string): Promise<FontAsset> => native ? invoke('import_font', { path }) : demoError(),
  music: (): Promise<MusicAsset[]> => native ? invoke('music_library') : Promise.resolve([]),
  importMusic: (path: string): Promise<MusicAsset> => native ? invoke('import_music', { path }) : demoError(),
  importBanner: (path: string): Promise<BannerAsset> => native ? invoke('import_banner', { path }) : demoError(),
  importMediaUrl: (url: string, importDir: string): Promise<Media> => native ? invoke('import_media_url', { url, importDir }) : demoError(),
  mediaToolsStatus: (): Promise<MediaToolsStatus> => native ? invoke('media_tools_status') : Promise.resolve({ downloaderReady: false, subtitlesReady: false, modelReady: false, message: 'Доступно в установленном приложении.' }),
  prepareMediaTools: (): Promise<MediaToolsStatus> => native ? invoke('prepare_media_tools') : demoError(),
  analyzeMoments: (request: MomentAnalyzeRequest): Promise<MomentSnapshot> => native ? invoke('moments_analyze', { request }) : demoError(),
  moments: (): Promise<MomentSnapshot> => native ? invoke('moments_snapshot') : Promise.resolve(emptyMoments),
  cancelMoments: (id: string): Promise<MomentSnapshot> => native ? invoke('moments_cancel', { id }) : demoError(),
  startSelected: (request: SelectedBatchRequest): Promise<Job> => native ? invoke('start_selected_batch', { request }) : demoError(),
  startBatch: (request: BatchRequest): Promise<Job> => native ? invoke('start_batch', { request }) : demoError(),
  getJob: (): Promise<Job> => native ? invoke('get_job') : Promise.resolve(emptyJob),
  cancelBatch: (): Promise<Job> => native ? invoke('cancel_batch') : demoError(),
  renderPreview: (request: BatchRequest): Promise<string> => native ? invoke('render_preview', { request }) : demoError(),
  saveSettings: (settings: Settings): Promise<void> => native ? invoke('save_settings', { settings }) : Promise.resolve(localStorage.setItem('barsik-demo-settings', JSON.stringify(settings))),
  openFolder: (path: string): Promise<void> => native ? invoke('open_folder', { path }) : demoError(),
};
