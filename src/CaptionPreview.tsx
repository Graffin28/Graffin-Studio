import { useEffect, useLayoutEffect, useRef, useState } from 'react';
import { subtitleFamily } from './studioTypes';
import type { Settings } from './types';

export type CaptionLook = Pick<Settings, 'subtitleFont' | 'subtitleColor' | 'subtitleWordsPerLine' | 'subtitleAnimation' | 'subtitleHighlightEnabled' | 'subtitleHighlightColor'>;
const sample = ['Вот', 'это', 'действительно', 'интересный', 'момент', 'в', 'нашей', 'истории'];
export default function CaptionPreview({ value, playing = true }: { value: CaptionLook; playing?: boolean }) {
  const root = useRef<HTMLSpanElement>(null);
  const [index, setIndex] = useState(0), [visible, setVisible] = useState(false), [reduced, setReduced] = useState(false), [fit, setFit] = useState(1);
  const count = Math.min(5, Math.max(1, value.subtitleWordsPerLine || 1));
  const start = Math.floor(index / count) * count;
  const words = sample.slice(start, start + count);
  const text = words.join(' ');
  useEffect(() => {
    const media = matchMedia('(prefers-reduced-motion: reduce)');
    const update = () => setReduced(media.matches);
    update(); media.addEventListener('change', update);
    const observer = new IntersectionObserver(entries => setVisible(entries[0]?.isIntersecting ?? false));
    if (root.current) observer.observe(root.current);
    return () => { media.removeEventListener('change', update); observer.disconnect(); };
  }, []);
  useEffect(() => {
    if (!playing || !visible || reduced) return;
    let timer = 0;
    const sync = () => { clearInterval(timer); if (!document.hidden) timer = window.setInterval(() => setIndex(current => (current + 1) % sample.length), 620); };
    sync(); document.addEventListener('visibilitychange', sync);
    return () => { clearInterval(timer); document.removeEventListener('visibilitychange', sync); };
  }, [playing, visible, reduced]);
  useEffect(() => setIndex(0), [count]);
  useLayoutEffect(() => {
    const element = root.current; if (!element) return;
    const canvas = document.createElement('canvas'), context = canvas.getContext('2d');
    let alive = true;
    const measure = () => {
      if (!alive || !context) return;
      const style = getComputedStyle(element), width = element.getBoundingClientRect().width;
      if (!width) return;
      context.font = `800 ${style.fontSize} ${style.fontFamily}`;
      const overshoot = value.subtitleAnimation === 'pop' ? 1.08 : 1;
      const next = Math.min(1, width * .94 / Math.max(1, context.measureText(text).width * overshoot));
      setFit(current => Math.abs(current - next) > .005 ? next : current);
    };
    const observer = new ResizeObserver(measure); observer.observe(element); measure(); void document.fonts.ready.then(measure);
    return () => { alive = false; observer.disconnect(); };
  }, [text, value.subtitleFont, value.subtitleAnimation]);
  return <span className="caption-preview" ref={root} style={{ fontFamily: `'${subtitleFamily(value.subtitleFont)}', sans-serif`, color: value.subtitleColor }}><span key={`${count}-${start}-${value.subtitleAnimation}`} className={`caption-phrase caption-anim-${value.subtitleAnimation || 'pop'}`} style={{ fontSize: `${fit}em` }}>{words.map((word, offset) => <span key={offset} style={{ color: value.subtitleHighlightEnabled && start + offset === index ? value.subtitleHighlightColor : value.subtitleColor }}>{offset > 0 ? ' ' : ''}{word}</span>)}</span></span>;
}
