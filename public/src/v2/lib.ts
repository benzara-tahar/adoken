/**
 * Shared by Astro components (build time) and the page scripts (runtime), so the server-rendered
 * frame and the animated one are the same markup. No DOM access here.
 */
import type { app } from './copy';

const escapes: Record<string, string> = { '&': '&amp;', '<': '&lt;', '>': '&gt;', '"': '&quot;' };
export const esc = (text: string) => text.replace(/[&<>"]/g, (ch) => escapes[ch]);

/** Turns the ⟨k|text⟩ highlight markup from copy.ts into escaped HTML spans (`t-k`, `t-s`, …). */
export function hl(src: string) {
  let out = '';
  let last = 0;
  for (const match of src.matchAll(/⟨(\w)\|([^⟩]*)⟩/g)) {
    out += esc(src.slice(last, match.index)) + `<span class="t-${match[1]}">${esc(match[2])}</span>`;
    last = match.index + match[0].length;
  }
  return out + esc(src.slice(last));
}

/** Avatar gradient for a hue (people are fictional; the color is just a stable accent). */
export const avatarStyle = (hue: number) =>
  `background:linear-gradient(135deg,hsl(${hue} 80% 70%),hsl(${(hue + 40) % 360} 70% 50%))`;

const svg = (body: string) =>
  `<svg viewBox="0 0 16 16" fill="none" stroke="currentColor" stroke-width="1.4" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">${body}</svg>`;

/** 16px line icons for the app mocks. */
export const icons: Record<string, string> = {
  pr: svg(
    '<circle cx="4" cy="3.5" r="1.6"/><circle cx="4" cy="12.5" r="1.6"/><circle cx="12" cy="12.5" r="1.6"/><path d="M4 5.1v5.8M12 10.9V6.5a2 2 0 0 0-2-2H7.5M9 3 7.5 4.5 9 6"/>',
  ),
  wi: svg('<rect x="2.5" y="2.5" width="11" height="11" rx="2.5"/><path d="m5.5 8 1.8 1.8L10.8 6"/>'),
  pipe: svg('<circle cx="8" cy="8" r="5.5"/><path d="M7 5.8v4.4L10.2 8z"/>'),
  person: svg('<circle cx="8" cy="5.5" r="2.5"/><path d="M3 13.5c.8-2.4 2.7-3.6 5-3.6s4.2 1.2 5 3.6"/>'),
  cmd: svg('<path d="m4 5 3 3-3 3M8.5 11h4"/>'),
  link: svg(
    '<path d="M7 9a2.8 2.8 0 0 0 4 0l2-2a2.8 2.8 0 0 0-4-4l-.7.7M9 7a2.8 2.8 0 0 0-4 0l-2 2a2.8 2.8 0 0 0 4 4l.7-.7"/>',
  ),
  search: svg('<circle cx="7" cy="7" r="4.5"/><path d="m10.5 10.5 3 3"/>'),
  sun: svg(
    '<circle cx="8" cy="8" r="2.8"/><path d="M8 1.5v1.3M8 13.2v1.3M1.5 8h1.3M13.2 8h1.3M3.4 3.4l.9.9M11.7 11.7l.9.9M3.4 12.6l.9-.9M11.7 4.3l.9-.9"/>',
  ),
  inbox: svg('<path d="M2.5 9.5 4 3.5h8l1.5 6v3h-11z"/><path d="M2.5 9.5h3.2l.8 1.5h3l.8-1.5h3.2"/>'),
  item: svg('<rect x="2.5" y="2.5" width="11" height="11" rx="2.5"/><path d="M5.5 6h5M5.5 8.5h5M5.5 11h3"/>'),
  check: svg('<path d="m3.5 8.5 3 3 6-7"/>'),
  x: svg('<path d="m4.5 4.5 7 7M11.5 4.5l-7 7"/>'),
  bell: svg('<path d="M4 11V7a4 4 0 0 1 8 0v4l1 1.5H3z"/><path d="M6.5 14h3"/>'),
  moon: svg('<path d="M12.8 10.2A5.5 5.5 0 0 1 5.8 3.2a5.5 5.5 0 1 0 7 7z"/>'),
  lock: svg('<rect x="3" y="7" width="10" height="7" rx="2"/><path d="M5.5 7V5a2.5 2.5 0 0 1 5 0v2"/>'),
  plane: svg('<path d="M14 8c0-.6-.5-1-1.2-1H9.6L6.7 2.5H5.3L6.8 7H4L3 5.5H2l.6 2.5L2 10.5h1L4 9h2.8l-1.5 4.5h1.4L9.6 9h3.2c.7 0 1.2-.4 1.2-1z"/>'),
  wifi: svg('<path d="M2 6.2a8.5 8.5 0 0 1 12 0M4.2 8.6a5.3 5.3 0 0 1 7.6 0M6.4 11a2.2 2.2 0 0 1 3.2 0"/><circle cx="8" cy="13" r=".4"/>'),
  skip: svg('<path d="M4 4v8M12 4 6.5 8 12 12z"/>'),
  spark: svg('<path d="M8 2v3M8 11v3M2 8h3M11 8h3M4 4l1.8 1.8M10.2 10.2 12 12M4 12l1.8-1.8M10.2 5.8 12 4"/>'),
};

type Corpus = (typeof app)['palette']['corpus'];
type CorpusItem = Corpus[number];
export interface Hit {
  item: CorpusItem;
  marks: number[];
  score: number;
}

const typeWeight: Record<string, number> = { pr: 40, wi: 30, pipe: 20, person: 10, cmd: 5 };

/**
 * A pocket version of the palette matcher: contiguous match (word-start bonus) beats a scattered
 * subsequence; type breaks ties. Returns matched character indexes for highlighting.
 */
export function rank(corpus: Corpus, query: string, limit = 6): Hit[] {
  const q = query.toLowerCase();
  const hits: Hit[] = [];
  for (const item of corpus) {
    const title = item.title.toLowerCase();
    const at = title.indexOf(q);
    let marks: number[] = [];
    let score = typeWeight[item.type];
    if (at >= 0) {
      marks = Array.from({ length: q.length }, (_, i) => at + i);
      score += 1000 - at + (at === 0 || title[at - 1] === ' ' ? 300 : 0);
    } else {
      let from = 0;
      for (const ch of q) {
        const found = title.indexOf(ch, from);
        if (found < 0) break;
        marks.push(found);
        from = found + 1;
      }
      if (marks.length < q.length) continue;
      score += 500 - (marks[marks.length - 1] - marks[0]) * 4 + (marks[0] === 0 ? 100 : 0);
    }
    hits.push({ item, marks, score });
  }
  return hits.sort((a, b) => b.score - a.score).slice(0, limit);
}

/** Palette rows as HTML. `link` renders the single "pasted URL" row instead. */
export function paletteRows(hits: Hit[], selected = 0, link?: string) {
  return hits
    .map(({ item, marks }, index) => {
      const set = new Set(marks);
      const title = [...item.title]
        .map((ch, i) => (set.has(i) ? `<mark>${esc(ch)}</mark>` : esc(ch)))
        .join('')
        .replaceAll('</mark><mark>', '');
      const tone = item.type === 'pipe' ? (item.title.includes('failed') ? 'bad' : 'ok') : item.type;
      return `<li class="pr-row${index === selected ? ' is-sel' : ''}" data-tone="${tone}">
        <span class="pr-ico">${icons[link ? 'link' : item.type]}</span>
        <span class="pr-main"><span class="pr-title">${link ? `<b>${esc(link)}</b> ` : ''}${title}</span><span class="pr-meta">${esc(item.meta)}</span></span>
        <span class="pr-id">${esc(item.id)}</span><span class="pr-enter" aria-hidden="true">↵</span></li>`;
    })
    .join('');
}

type Detail = (typeof app)['details'][keyof (typeof app)['details']];

/** The PR detail pane the palette opens into. */
export function detailHtml(id: number, pr: Detail) {
  const [add, del] = pr.stats.split(' ');
  const { thread } = pr;
  const lines = pr.lines
    .map(([kind, code], i) => {
      const line = `<div class="dl dl-${kind === '+' ? 'add' : kind === '-' ? 'del' : 'ctx'}" style="--i:${i}"><span class="dl-n">${41 + i}</span><span class="dl-k">${kind.trim()}</span><code>${hl(code)}</code></div>`;
      if (i !== thread.at) return line;
      return `${line}<div class="dt" style="--i:${i + 1}"><span class="avatar" style="${avatarStyle(thread.hue)}">${thread.who}</span><div><b>${esc(thread.author)}</b><span>just now</span><p>${esc(thread.text)}</p></div></div>`;
    })
    .join('');
  const files = pr.files
    .map(
      (file, i) =>
        `<li class="${i === 0 ? 'is-on' : ''}"><span class="df-check${i > 0 && i < 3 ? ' is-viewed' : ''}">${icons.check}</span>${esc(file.split('/').pop()!)}</li>`,
    )
    .join('');
  return `<header class="dh">
      <div class="dh-top"><span class="dh-id">!${id}</span><span class="dh-state">Active</span></div>
      <b class="dh-title">${esc(pr.title)}</b>
      <div class="dh-meta"><span class="avatar" style="${avatarStyle(pr.hue)}">${pr.who}</span>${esc(pr.author)}<span class="dh-branch">${esc(pr.branch)} → ${esc(pr.target)}</span><span class="dh-add">${add}</span><span class="dh-del">${del}</span></div>
    </header>
    <div class="db">
      <ul class="df">${files}</ul>
      <div class="dd"><div class="dd-file">${esc(pr.files[0])}</div>${lines}</div>
    </div>`;
}
