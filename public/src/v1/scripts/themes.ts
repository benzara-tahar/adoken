/** Theme select: clicking a tile repaints the preview; while untouched and on screen, the cursor roams. */
import { playWhileVisible } from './motion';

const root = document.querySelector<HTMLElement>('[data-themes]');

if (root) {
  const tiles = [...root.querySelectorAll<HTMLButtonElement>('.tile')];
  const preview = root.querySelector<HTMLElement>('[data-preview]')!;
  const name = root.querySelector<HTMLElement>('[data-theme-name]')!;
  let current = 0;

  const choose = (index: number) => {
    current = index;
    tiles.forEach((tile, i) => tile.setAttribute('aria-pressed', String(i === index)));
    preview.setAttribute('style', tiles[index].getAttribute('style') ?? '');
    name.textContent = tiles[index].dataset.name ?? '';
  };

  const stop = playWhileVisible(root, async (wait) => {
    await wait(1500);
    choose((current + 1) % tiles.length);
  });

  tiles.forEach((tile, i) =>
    tile.addEventListener('click', () => {
      stop();
      choose(i);
    }),
  );
}
