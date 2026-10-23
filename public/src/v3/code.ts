/**
 * Build-time syntax highlighting for the TypeScript in the /v3/ mocks. Tiny on purpose:
 * enough token classes to look like a real editor, zero client JS.
 */
const TOKEN =
  /(\/\/.*$)|('(?:[^'\\]|\\.)*'|`[^`]*`)|\b(\d[\d_]*(?:\.\d+)?)\b|\b(export|import|from|const|let|async|await|function|return|if|throw|new|type|interface|for|of|as|try|catch)\b|\b([A-Z][A-Za-z0-9]*)\b|\b([a-z_$][\w$]*)(?=\()/gm;
const CLASS = ['', 'c', 's', 'n', 'k', 't', 'f'];

const escape = (s: string) => s.replace(/&/g, '&amp;').replace(/</g, '&lt;').replace(/>/g, '&gt;');

export function highlight(src: string): string {
  let out = '';
  let last = 0;
  for (const m of src.matchAll(TOKEN)) {
    out += escape(src.slice(last, m.index));
    const group = m.findIndex((g, i) => i > 0 && g !== undefined);
    out += `<span class="${CLASS[group]}">${escape(m[0])}</span>`;
    last = m.index! + m[0].length;
  }
  return out + escape(src.slice(last));
}

export interface DiffLine {
  /** ' ' context, '+' added, '-' removed. */
  op: ' ' | '+' | '-';
  old?: number;
  new?: number;
  html: string;
}

/** Parses a unified-diff body (first char of each line is the op) with line numbers from `oldStart`/`newStart`. */
export function diff(body: string, oldStart: number, newStart: number): DiffLine[] {
  let o = oldStart;
  let n = newStart;
  return body
    .replace(/^\n|\n$/g, '')
    .split('\n')
    .map((raw) => {
      const op = raw[0] as DiffLine['op'];
      const html = highlight(raw.slice(1));
      if (op === '+') return { op, new: n++, html };
      if (op === '-') return { op, old: o++, html };
      return { op: ' ', old: o++, new: n++, html };
    });
}
