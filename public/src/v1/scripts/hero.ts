/**
 * Hero cabinet loop (~10.5 s): insert coin → boot → PRESS ⌘P → typo-forgiving palette → review →
 * approve combo → K.O. The static markup is the palette state; this only runs when motion is OK.
 */
import { playWhileVisible, press, typeInto } from './motion';

const cab = document.querySelector<HTMLElement>('[data-cabinet]');

if (cab) {
  const query = cab.querySelector<HTMLElement>('[data-query]')!;
  const reply = cab.querySelector<HTMLElement>('[data-reply]')!;
  const credit = cab.querySelector<HTMLElement>('[data-credit]')!;
  const approve = cab.querySelector('[data-approve]');
  const button = (key: string) => cab.querySelector(`.deck-btn[data-key="${key}"]`);
  const fullQuery = query.textContent ?? '';
  const fullReply = reply.textContent?.trim() ?? '';
  const creditText = credit.textContent ?? '';
  const flags = ['is-results', 'is-forgiven', 'is-approved', 'is-burst', 'is-hit', 'is-loading', 'is-coin'];
  const stage = (name: string) => (cab.dataset.stage = name);

  playWhileVisible(
    cab,
    async (wait) => {
      // Attract screen.
      cab.classList.remove(...flags);
      credit.textContent = creditText.replace(/\d+$/, '00');
      query.textContent = '';
      reply.textContent = '';
      stage('coin');
      await wait(650);
      cab.classList.add('is-coin');
      await wait(450);
      credit.textContent = creditText;
      await wait(250);

      // Boot.
      stage('boot');
      cab.classList.add('is-loading');
      await wait(1500);
      stage('start');
      await wait(700);
      button('cmd')?.classList.add('is-pressed');
      await wait(120);
      await press(button('p'), wait, 200);
      button('cmd')?.classList.remove('is-pressed');

      // ⌘P: type the typo, results land, badge slams.
      stage('palette');
      await wait(260);
      await typeInto(query, fullQuery, wait, 95, (value) => {
        if (value.length === 3) cab.classList.add('is-results');
      });
      await wait(120);
      cab.classList.add('is-forgiven');
      await wait(1250);
      cab.classList.add('is-hit');
      await press(button('enter'), wait, 220);

      // Review: reply, approve, combo.
      stage('review');
      cab.classList.remove('is-hit');
      await wait(500);
      await typeInto(reply, fullReply, wait, 22);
      await wait(350);
      button('a')?.classList.add('is-pressed');
      await press(approve, wait, 200);
      button('a')?.classList.remove('is-pressed');
      cab.classList.add('is-approved', 'is-burst');
      await wait(1300);

      // Your own PR merges meanwhile: K.O.
      stage('ko');
      cab.classList.remove('is-burst');
      await wait(30);
      cab.classList.add('is-burst');
      await wait(1900);
    },
    0.2,
  );
}
