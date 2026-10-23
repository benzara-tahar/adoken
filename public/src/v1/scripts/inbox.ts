/** Move 03 demo loop: comments fold into one row, a banner lands, S snoozes a row, quiet hours kick in. */
import { playWhileVisible } from './motion';

const root = document.querySelector<HTMLElement>('[data-inbox]');

if (root) {
  const rows = [...root.querySelectorAll<HTMLElement>('.ib-row')];
  const count = root.querySelector<HTMLElement>('[data-count]')!;
  const emails = root.querySelector<HTMLElement>('[data-emails]')!;
  const held = root.querySelector<HTMLElement>('[data-held]')!;
  const options = [...root.querySelectorAll<HTMLElement>('.snooze-i')];
  const menu = root.querySelector<HTMLElement>('[data-menu]')!;
  const finalCount = Number(count.textContent);
  const finalEmails = parseInt(emails.textContent!, 10);
  const emailsUnit = emails.textContent!.replace(/^\d+/, '');
  const finalHeld = Number(held.textContent);
  const target = 3;
  const pick = 2;

  playWhileVisible(root, async (wait) => {
    root.classList.remove('is-quiet', 'is-banner', 'is-menu', 'is-toast');
    rows.forEach((row) => row.classList.remove('is-gone', 'is-cursor'));
    options.forEach((o) => o.classList.remove('is-sel'));
    count.textContent = '1';
    emails.textContent = `0${emailsUnit}`;
    held.textContent = '0';
    await wait(700);

    // Twenty emails' worth of comments fold into one row.
    for (let n = 2; n <= finalCount; n++) {
      count.textContent = String(n);
      emails.textContent = `${Math.round((finalEmails * n) / finalCount)}${emailsUnit}`;
      count.classList.remove('is-tick');
      void count.offsetWidth;
      count.classList.add('is-tick');
      await wait(170);
    }
    await wait(500);

    root.classList.add('is-banner');
    await wait(2200);
    root.classList.remove('is-banner');
    await wait(400);

    // j j j, then S → Tomorrow 09:00.
    for (let i = 0; i <= target; i++) {
      rows.forEach((row, j) => row.classList.toggle('is-cursor', j === i));
      await wait(260);
    }
    menu.style.top = `${rows[target].offsetTop + rows[target].offsetHeight + 4}px`;
    root.classList.add('is-menu');
    await wait(400);
    for (let i = 0; i <= pick; i++) {
      options.forEach((o, j) => o.classList.toggle('is-sel', j === i));
      await wait(260);
    }
    await wait(300);
    root.classList.remove('is-menu');
    rows[target].classList.remove('is-cursor');
    rows[target].classList.add('is-gone');
    root.classList.add('is-toast');
    await wait(1700);
    root.classList.remove('is-toast');
    await wait(300);

    // Quiet hours: everything else waits for the morning summary.
    root.classList.add('is-quiet');
    for (let n = 1; n <= finalHeld; n++) {
      held.textContent = String(n);
      await wait(60);
    }
    await wait(2600);
  });
}
