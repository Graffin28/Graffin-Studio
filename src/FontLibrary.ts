import { useEffect, useState } from 'react';
import { api, assetUrl } from './bridge';
import type { FontAsset } from './types';

const loaded = new Map<string, FontFace>();
let cachedFonts: FontAsset[] = [];
const subscribers = new Set<(fonts: FontAsset[]) => void>();
export const customFontFamily = (id: string) => `Graffin-${id}`;
async function loadFonts(entries: FontAsset[]) {
  await Promise.all(entries.map(async entry => {
    if (loaded.has(entry.id)) return;
    const face = new FontFace(customFontFamily(entry.id), `url(${JSON.stringify(assetUrl(entry.path))})`);
    await face.load(); document.fonts.add(face); loaded.set(entry.id, face);
  }));
}
export function useFontLibrary(selectedFont?: string) {
  const [fonts, setFonts] = useState<FontAsset[]>(cachedFonts);
  const [fontError, setFontError] = useState('');
  useEffect(() => {
    let alive = true; subscribers.add(setFonts);
    api.fonts().then(async entries => {
      if (alive) { cachedFonts = entries; setFonts(entries); }
    }).catch(error => alive && setFontError(String(error)));
    return () => { alive = false; subscribers.delete(setFonts); };
  }, []);
  useEffect(() => { const selected = fonts.find(font => font.id === selectedFont); if (selected) loadFonts([selected]).catch(() => setFontError('Не удалось показать этот шрифт. Выберите другой TTF / OTF.')); }, [fonts, selectedFont]);
  const addFont = async (path: string) => {
    const entry = await api.importFont(path);
    cachedFonts = [...cachedFonts.filter(font => font.id !== entry.id), entry];
    for (const subscriber of subscribers) subscriber(cachedFonts);
    setFontError('');
    try { await loadFonts([entry]); } catch { setFontError('Шрифт сохранён, но его предпросмотр недоступен. Попробуйте другой TTF / OTF.'); }
    return entry;
  };
  return { fonts, addFont, fontError };
}
