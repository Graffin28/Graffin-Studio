export interface Media {
  id: string; path: string; name: string; duration: number; width: number; height: number;
  hasAudio: boolean; thumbnail: string;
}
export interface BannerAsset {
  id: string; name: string; path: string; preview: string; kind: 'video' | 'image';
}
export interface FontAsset { id: string; name: string; family: string; path: string }
export interface MusicAsset { id: string; name: string; path: string; duration: number }
export interface MediaToolsStatus {
  downloaderReady: boolean; subtitlesReady: boolean; modelReady: boolean; message: string;
}
export interface Settings {
  count: number; duration: number; resolution: number; topSplit: number; bannerHeight: number; bannerBottom: number;
  layoutMode: 'split' | 'single'; singleScale: number;
  musicPath: string; musicName: string; musicDuration: number; musicStart: number; musicDelay: number; musicVolume: number; musicLoop: boolean; musicFadeIn: number; musicFadeOut: number;
  interruptionEnabled: boolean; interruptionPath: string; interruptionDuration: number; interruptionTiming: 'middle' | 'fixed' | 'random' | 'quiet'; interruptionFade: number; interruptionStart: number; interruptionEnd: number;
  bannerEnabled: boolean; fitMode: 'crop' | 'contain'; topFocus: number; bottomFocus: number;
  topVolume: number; bottomVolume: number; order: 'sequential' | 'random'; outputDir: string; importDir: string; topSourceDir: string; bottomSourceDir: string;
  bannerPath: string; bannerName: string; bannerLibrary: BannerAsset[];
  subtitlesEnabled: boolean; subtitleSize: number; subtitleOffset: number; subtitleLanguage: string;
  subtitleWordsPerLine: number; subtitleAnimation: 'none' | 'fade' | 'pop' | 'slide'; subtitleHighlightEnabled: boolean; subtitleHighlightColor: string;
  subtitleFont: string; subtitleColor: string; cutMode: 'smart' | 'fixed';
}
export interface BatchRequest { top: Media[]; bottom: Media[]; settings: Settings; bannerPath: string }
export interface Job {
  id: string; status: 'idle' | 'running' | 'cancelling' | 'cancelled' | 'completed' | 'failed';
  total: number; completed: number; current: number; progress: number; message: string;
  outputs: string[]; error: string | null;
}
export interface Bootstrap {
  defaultOutput: string; defaultImportDir: string; bannerPath: string; bannerPreview: string; version: string;
  ffmpegReady: boolean; restoredSettings: Settings | null;
}
export const defaultSettings: Settings = {
  count: 10, duration: 30, resolution: 1080, topSplit: .5, bannerHeight: 240, bannerBottom: 240,
  layoutMode: 'split', singleScale: .94,
  musicPath: '', musicName: '', musicDuration: 0, musicStart: 0, musicDelay: 0, musicVolume: .18, musicLoop: true, musicFadeIn: 1, musicFadeOut: 1,
  interruptionEnabled: false, interruptionPath: '', interruptionDuration: 3, interruptionTiming: 'middle', interruptionFade: 0, interruptionStart: 5, interruptionEnd: 20,
  bannerEnabled: true, fitMode: 'crop', topFocus: .5, bottomFocus: .5,
  topVolume: 1, bottomVolume: 0, order: 'random', outputDir: '', importDir: '', topSourceDir: '', bottomSourceDir: '',
  bannerPath: '', bannerName: 'Barsik VPN', bannerLibrary: [],
  subtitlesEnabled: false, subtitleSize: 56, subtitleOffset: 0, subtitleLanguage: 'ru',
  subtitleWordsPerLine: 1, subtitleAnimation: 'pop', subtitleHighlightEnabled: false, subtitleHighlightColor: '#A6FF52',
  subtitleFont: 'manrope', subtitleColor: '#FFFFFF', cutMode: 'smart',
};
export const emptyJob: Job = { id: '', status: 'idle', total: 0, completed: 0, current: 0, progress: 0, message: '', outputs: [], error: null };
