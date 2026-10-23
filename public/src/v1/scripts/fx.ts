/**
 * Page-wide effects for /v1/: scroll reveals, count-up scores and the next-round countdown.
 * Content is fully visible without JS or with reduced motion; this only adds entrances.
 */
import { motionOK, playWhileVisible } from './motion';

document.documentElement.classList.add('js');

const countUp = (el: HTMLElement) => {
  const target = Number(el.dataset.countTo);
  const decimals = (el.dataset.countTo!.split('.')[1] ?? '').length;
  const start = performance.now();
  const duration = 1200;
  const frame = (now: number) => {
    const t = Math.min(1, (now - start) / duration);
    el.textContent = (target * (1 - (1 - t) ** 3)).toFixed(decimals);
    if (t < 1) requestAnimationFrame(frame);
  };
  requestAnimationFrame(frame);
};

const reveal = new IntersectionObserver(
  (entries) => {
    for (const entry of entries) {
      if (!entry.isIntersecting) continue;
      entry.target.classList.add('is-in');
      if (motionOK) entry.target.querySelectorAll<HTMLElement>('[data-count-to]').forEach(countUp);
      reveal.unobserve(entry.target);
    }
  },
  { threshold: 0.15, rootMargin: '0px 0px -6% 0px' },
);
document.querySelectorAll('[data-reveal]').forEach((el) => reveal.observe(el));

const countdown = document.querySelector<HTMLElement>('[data-countdown]');
if (countdown) {
  playWhileVisible(countdown, async (wait) => {
    for (let n = 9; n >= 0; n--) {
      countdown.textContent = String(n);
      countdown.classList.remove('is-tick');
      void countdown.offsetWidth;
      countdown.classList.add('is-tick');
      await wait(1000);
    }
  });
}
