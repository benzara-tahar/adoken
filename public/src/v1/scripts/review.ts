/** Move 02 demo loop: step through changes (J), reply (C), approve (A) into a combo, Complete pinned. */
import { playWhileVisible, press, typeInto } from './motion';

const root = document.querySelector<HTMLElement>('[data-review]');

if (root) {
  const changed = [...root.querySelectorAll('.drow.add, .drow.del')];
  const reply = root.querySelector<HTMLElement>('[data-reply]')!;
  const fullReply = reply.textContent!.trim();
  const keycast = root.querySelector<HTMLElement>('.keycast')!;
  const key = keycast.querySelector('[data-keycast]')!;
  const label = keycast.querySelector('[data-keycast-label]')!;

  const cast = (k: string, text: string) => {
    key.textContent = k;
    label.textContent = text;
    keycast.classList.remove('is-on');
    void keycast.offsetWidth;
    keycast.classList.add('is-on');
  };

  playWhileVisible(root, async (wait) => {
    root.classList.remove('is-approved', 'is-burst', 'is-pinned');
    keycast.classList.remove('is-on');
    reply.textContent = '';
    await wait(900);

    for (const row of changed) {
      changed.forEach((r) => r.classList.toggle('is-cursor', r === row));
      cast('J', 'Next change');
      await wait(300);
    }
    await wait(300);

    cast('C', 'Reply in thread');
    await wait(350);
    await typeInto(reply, fullReply, wait, 34);
    cast('⌘⏎', 'Post');
    await wait(700);
    changed.forEach((r) => r.classList.remove('is-cursor'));

    cast('A', 'Approve');
    await press(root.querySelector('[data-approve]'), wait, 200);
    root.classList.add('is-approved', 'is-burst');
    await wait(1600);

    cast('⌘⏎', 'Complete');
    await press(root.querySelector('[data-complete]'), wait, 200);
    root.classList.add('is-pinned');
    await wait(3000);
  });
}
