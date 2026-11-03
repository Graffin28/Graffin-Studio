import type { Media, Settings } from './types';
export interface MomentCandidate { id: string; sourceId: string; sourceIndex: number; sourceName: string; start: number; end: number; text: string; score: number; reasons: string[] }
export interface MomentSelection { candidateId: string; start: number; end: number }
export interface MomentSnapshot { id: string; status: 'idle' | 'analyzing' | 'ready' | 'cancelling' | 'cancelled' | 'failed'; progress: number; message: string; error: string | null; sources: Media[]; candidates: MomentCandidate[] }
export interface MomentAnalyzeRequest { sources: Media[]; targetDuration: number; language: string; limit?: number }
export interface SelectedBatchRequest { analysisId: string; selection: MomentSelection[]; bottom: Media[]; settings: Settings; bannerPath: string }
export const emptyMoments: MomentSnapshot = { id: '', status: 'idle', progress: 0, message: '', error: null, sources: [], candidates: [] };
