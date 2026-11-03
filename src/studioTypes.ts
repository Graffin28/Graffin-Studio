import type { Settings } from './types';

export const subtitleFonts = [
  { id: 'manrope', name: 'Manrope', family: 'Manrope ExtraBold', feel: 'Чистый' },
  { id: 'montserrat', name: 'Montserrat', family: 'Montserrat ExtraBold', feel: 'Плотный' },
  { id: 'rubik', name: 'Rubik', family: 'Rubik Bold', feel: 'Мягкий' },
  { id: 'oswald', name: 'Oswald', family: 'Oswald Bold', feel: 'Узкий' },
  { id: 'russo', name: 'Russo One', family: 'Russo One', feel: 'Акцентный' },
] as const;
export const subtitlePalette = ['#FFFFFF', '#FFE45C', '#A7F0D1', '#83C8FF', '#FF9CB7', '#C9B6FF'];
export const subtitleFamily = (id: string) => id.startsWith('custom-') ? `Graffin-${id}` : subtitleFonts.find(font => font.id === id)?.family || subtitleFonts[0].family;
export interface SubtitleWord { start: number; end: number; text: string }
export interface SubtitleDocument {
  path: string; name: string; duration: number; words: SubtitleWord[]; settings: Settings;
  previewPath: string; pauseAt?: number | null; pauseDuration?: number; editable: boolean; reason: string | null; revision: string;
}
export interface SubtitleEditRequest {
  path: string; revision: string; words: SubtitleWord[];
  subtitleFont: string; subtitleColor: string; subtitleSize: number; subtitleOffset: number;
  subtitleWordsPerLine?: number; subtitleAnimation?: Settings['subtitleAnimation']; subtitleHighlightEnabled?: boolean; subtitleHighlightColor?: string;
}
export interface StudioPreset { id: string; name: string; settings: Settings; updatedAt: number }
export interface PresetSaveRequest { id?: string | null; name: string; settings: Settings }
