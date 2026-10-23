/**
 * Move 01 demo: a real (tiny) typo-tolerant palette over the fictional index. Autoplay types the
 * scripted queries while visible; any user focus or keypress hands control to the user for good.
 */
import { renderHits, search, type Entry } from '../lib/search';
import { playWhileVisible, typeInto, type Wait } from './motion';

const root = document.querySelector<HTMLElement>('[data-palette]');

if (root) {
  const index = JSON.parse(root.dataset.index!) as Entry[];
  const labels = JSON.parse(root.dataset.labels!) as Record<string, string>;
  const script = JSON.parse(root.dataset.script!) as string[];
  const input = root.querySelector<HTMLInputElement>('[data-input]')!;
  const list = root.querySelector<HTMLElement>('[data-list]')!;
  const meta = root.querySelector<HTMLElement>('[data-meta]')!;
  const badge = root.querySelector<HTMLElement>('[data-badge]')!;
  const indexed = meta.textContent!.split(' · ').at(-1);
  let selected = 0;
  let count = 0;

  const run = (query: string) => {
    const result = search(index, query, 5);
    count = result.hits.length;
    selected = 0;
    list.innerHTML = count
      ? renderHits(result, labels, selected)
      : `<li class="pal-empty">No hits. Even the typo tolerance gave up.</li>`;
    const ms = (1.4 + Math.random() * 1.9).toFixed(1);
    meta.textContent = `${count} hit${count === 1 ? '' : 's'} · ${ms} ms · ${indexed}`;

    const kind = result.mode === 'link' ? 'link' : result.forgiven ? 'typo' : '';
    badge.hidden = !kind;
    if (!kind) return;
    badge.dataset.kind = kind;
    badge.firstChild!.textContent = kind === 'link' ? badge.dataset.link! : badge.dataset.typo!;
    badge.querySelector('small')!.textContent =
      kind === 'link' ? (result.hits[0]?.entry.id ?? '') : `${query.trim().split(/\s+/).at(-1)} → ${result.forgiven}`;
    // Restart the slam animation.
    badge.style.animation = 'none';
    void badge.offsetWidth;
    badge.style.animation = '';
  };

  const select = (next: number) => {
    if (!count) return;
    selected = (next + count) % count;
    list.querySelectorAll('.pal-row').forEach((row, i) => row.classList.toggle('is-sel', i === selected));
  };

  input.addEventListener('input', () => run(input.value));
  input.addEventListener('keydown', (event) => {
    if (event.key === 'ArrowDown' || event.key === 'ArrowUp') {
      event.preventDefault();
      select(selected + (event.key === 'ArrowDown' ? 1 : -1));
    } else if (event.key === 'Enter') {
      const row = list.querySelectorAll('.pal-row')[selected];
      row?.classList.add('is-hit');
      setTimeout(() => row?.classList.remove('is-hit'), 220);
    } else if (event.key === 'Escape') {
      input.value = '';
      run('');
    }
  });
  for (const button of root.querySelectorAll<HTMLButtonElement>('[data-scope]')) {
    button.addEventListener('click', () => {
      stopAutoplay();
      input.value = button.dataset.scope!;
      input.focus();
      run(input.value);
    });
  }

  const scene = async (wait: Wait) => {
    for (const query of script) {
      await wait(1100);
      input.value = '';
      run('');
      await wait(350);
      if (query.startsWith('http')) {
        // Pasted, not typed.
        input.value = query;
        run(query);
        input.scrollLeft = input.scrollWidth;
      } else {
        await typeInto(input, query, wait, 85, run);
      }
      await wait(900);
      if (count > 2) {
        select(1);
        await wait(380);
        select(2);
        await wait(380);
        select(0);
      }
      await wait(900);
    }
  };
  const stopAutoplay = playWhileVisible(root, scene, 0.5);
  input.addEventListener('focus', stopAutoplay, { once: true });
  input.addEventListener('pointerdown', stopAutoplay, { once: true });
}
