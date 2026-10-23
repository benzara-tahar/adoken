/**
 * The ⌘P demo's matcher: prefix > substring > one-typo (transposition counts as one edit) per word,
 * scopes (! # @ >), and pasted dev.azure.com links. Runs at build time for the static markup and in the
 * browser for the live palette, so both always agree.
 */
export interface Entry {
  readonly kind: string;
  readonly id: string;
  readonly title: string;
  readonly meta: string;
}

export interface Hit {
  entry: Entry;
  /** [start, end) ranges in `entry.title` to highlight. */
  ranges: [number, number][];
  score: number;
}

export interface Result {
  hits: Hit[];
  mode: 'search' | 'link';
  /** The word a typo was forgiven into (for the badge), if the top hit needed one. */
  forgiven: string | null;
}

const KINDS_BY_SCOPE: Record<string, readonly string[]> = {
  '!': ['pr'],
  '#': ['bug', 'task'],
  '@': ['person'],
  '>': ['cmd'],
};

/** Optimal string alignment distance: Levenshtein plus adjacent transpositions (lgoin → login = 1). */
function osa(a: string, b: string): number {
  const d = Array.from({ length: a.length + 1 }, (_, i) => [i, ...Array<number>(b.length).fill(0)]);
  for (let j = 1; j <= b.length; j++) d[0][j] = j;
  for (let i = 1; i <= a.length; i++) {
    for (let j = 1; j <= b.length; j++) {
      const cost = a[i - 1] === b[j - 1] ? 0 : 1;
      d[i][j] = Math.min(d[i - 1][j] + 1, d[i][j - 1] + 1, d[i - 1][j - 1] + cost);
      if (i > 1 && j > 1 && a[i - 1] === b[j - 2] && a[i - 2] === b[j - 1]) d[i][j] = Math.min(d[i][j], d[i - 2][j - 2] + 1);
    }
  }
  return d[a.length][b.length];
}

interface WordMatch {
  score: number;
  start: number;
  end: number;
  typo: string | null;
}

function matchToken(token: string, text: string): WordMatch | null {
  let best: WordMatch | null = null;
  for (const m of text.matchAll(/[\p{L}\d]+/gu)) {
    const word = m[0].toLowerCase();
    const at = m.index;
    let found: WordMatch | null = null;
    if (word.startsWith(token)) found = { score: 3, start: at, end: at + token.length, typo: null };
    else if (token.length >= 3 && word.includes(token)) {
      const start = at + word.indexOf(token);
      found = { score: 2, start, end: start + token.length, typo: null };
    } else if (token.length >= 4 && osa(token, word.slice(0, token.length)) <= 1)
      found = { score: 1, start: at, end: at + Math.min(token.length, word.length), typo: word.slice(0, token.length) };
    else if (token.length >= 4 && Math.abs(word.length - token.length) === 1 && osa(token, word) <= 1)
      found = { score: 1, start: at, end: at + word.length, typo: word };
    if (found && (!best || found.score > best.score)) best = found;
  }
  return best;
}

export function search(index: readonly Entry[], raw: string, limit = 6): Result {
  const query = raw.trim();
  const link = query.match(/dev\.azure\.com\/[^/]+\/[^/]+\/_(?:git\/[^/]+\/pullrequest|workitems\/edit)\/(\d+)/i);
  if (link) {
    const entry = index.find((e) => e.id.slice(1) === link[1]);
    return { hits: entry ? [{ entry, ranges: [], score: 1 }] : [], mode: 'link', forgiven: null };
  }

  const kinds = Object.hasOwn(KINDS_BY_SCOPE, query[0] ?? '') ? KINDS_BY_SCOPE[query[0]] : null;
  const tokens = (kinds ? query.slice(1) : query).toLowerCase().split(/\s+/).filter(Boolean);
  const pool = kinds ? index.filter((e) => kinds.includes(e.kind)) : index;
  if (tokens.length === 0) {
    return { hits: pool.slice(0, limit).map((entry) => ({ entry, ranges: [], score: 0 })), mode: 'search', forgiven: null };
  }

  const hits: (Hit & { typo: string | null })[] = [];
  for (const entry of pool) {
    let score = 0;
    let typo: string | null = null;
    const ranges: [number, number][] = [];
    for (const token of tokens) {
      const inTitle = matchToken(token, entry.title);
      // People match their own work through the meta line (prefix only, half weight).
      const inMeta = inTitle ? null : matchToken(token, entry.meta);
      const idMatch = entry.id.toLowerCase().replace(/^[!#@]/, '').startsWith(token);
      if (inTitle) {
        score += inTitle.score;
        typo ??= inTitle.typo;
        ranges.push([inTitle.start, inTitle.end]);
      } else if (idMatch) score += 3;
      else if (inMeta && inMeta.score === 3) score += 1.5;
      else {
        score = -1;
        break;
      }
    }
    if (score > 0) hits.push({ entry, ranges, score, typo });
  }
  hits.sort((a, b) => b.score - a.score);
  const top = hits.slice(0, limit);
  return { hits: top, mode: 'search', forgiven: top[0]?.typo ?? null };
}

const escapeHtml = (s: string) => s.replace(/&/g, '&amp;').replace(/</g, '&lt;').replace(/>/g, '&gt;').replace(/"/g, '&quot;');

/** `<li>` rows for a result; `labels` maps kind → chip text. */
export function renderHits(result: Result, labels: Readonly<Record<string, string>>, selected = 0): string {
  return result.hits
    .map(({ entry, ranges }, i) => {
      let title = '';
      let cursor = 0;
      for (const [start, end] of [...ranges].sort((a, b) => a[0] - b[0])) {
        if (start < cursor) continue;
        title += `${escapeHtml(entry.title.slice(cursor, start))}<mark>${escapeHtml(entry.title.slice(start, end))}</mark>`;
        cursor = end;
      }
      title += escapeHtml(entry.title.slice(cursor));
      const id = entry.id && entry.kind !== 'repo' && entry.kind !== 'file' ? `<i>${escapeHtml(entry.id)}</i>` : '';
      const meta = entry.meta.split(' · ');
      return `<li class="pal-row${i === selected ? ' is-sel' : ''}" style="--i:${i}"><span class="kind" data-kind="${entry.kind}">${escapeHtml(labels[entry.kind] ?? entry.kind)}</span><span><span class="pal-t">${id}${title}</span><span class="pal-m">${escapeHtml(meta.slice(0, -1).join(' · ') || meta[0])}</span></span><span>${escapeHtml(meta.at(-1) ?? '')}</span></li>`;
    })
    .join('');
}
