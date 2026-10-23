/**
 * Tiny build-time TypeScript highlighter for the mock diffs: returns HTML with `.tk-*` spans.
 * Good enough for hand-written snippets; not a parser.
 */
const CLASS_BY_WORD: Record<string, 'k' | 'y'> = {};
for (const word of 'import from export async function const let return await if else new type interface throw for of true false null undefined'.split(' '))
  CLASS_BY_WORD[word] = 'k';
for (const word of 'Request Response Promise string number boolean URL'.split(' ')) CLASS_BY_WORD[word] = 'y';

const TOKEN =
  /(\/\/.*$)|(`(?:[^`\\]|\\.)*`|'(?:[^'\\]|\\.)*'|"(?:[^"\\]|\\.)*")|(\b\d[\d_]*\b)|([A-Za-z_$][\w$]*)|(\s+)|([^\sA-Za-z_$\d]+)/g;

const escape = (s: string) => s.replace(/&/g, '&amp;').replace(/</g, '&lt;').replace(/>/g, '&gt;');
const span = (cls: string, s: string) => `<span class="tk-${cls}">${escape(s)}</span>`;

export function highlight(code: string): string {
  let html = '';
  for (const m of code.matchAll(TOKEN)) {
    const [text, comment, str, num, ident] = m;
    if (comment) html += span('c', text);
    else if (str) html += span('s', text);
    else if (num) html += span('n', text);
    else if (ident && Object.hasOwn(CLASS_BY_WORD, text)) html += span(CLASS_BY_WORD[text], text);
    else if (ident && code.slice(m.index + text.length).trimStart()[0] === '(') html += span('f', text);
    else html += escape(text);
  }
  return html;
}
