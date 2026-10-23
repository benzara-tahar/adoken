/**
 * Shared plumbing for the /v1/ mock animations. Every demo is a looping async "scene" that only runs
 * while it is on screen and the tab is visible, and never runs at all under prefers-reduced-motion
 * (the static markup is the designed fallback).
 */
export const motionOK = matchMedia('(prefers-reduced-motion: no-preference)').matches;

class Stopped extends Error {}

export type Wait = (ms: number) => Promise<void>;

/**
 * Loops `scene` while `el` is at least `threshold` visible. Leaving the viewport (or hiding the tab)
 * aborts the pending wait; the next run starts from the top, so scenes must set up their own state.
 * Returns a function that stops the loop for good (e.g. when the user takes over a demo).
 */
export function playWhileVisible(el: Element, scene: (wait: Wait) => Promise<void>, threshold = 0.3) {
  if (!motionOK) return () => {};
  let controller: AbortController | null = null;
  let onScreen = false;
  let done = false;

  const stop = () => {
    controller?.abort();
    controller = null;
  };
  const start = () => {
    if (controller || done || !onScreen || document.hidden) return;
    const current = new AbortController();
    controller = current;
    const wait: Wait = (ms) => {
      const { promise, resolve, reject } = Promise.withResolvers<void>();
      if (current.signal.aborted) {
        reject(new Stopped());
        return promise;
      }
      const timer = setTimeout(resolve, ms);
      current.signal.addEventListener('abort', () => (clearTimeout(timer), reject(new Stopped())), { once: true });
      return promise;
    };
    void (async () => {
      try {
        while (!current.signal.aborted) await scene(wait);
      } catch (error) {
        if (!(error instanceof Stopped)) throw error;
      }
    })();
  };

  new IntersectionObserver(
    ([entry]) => {
      onScreen = entry.isIntersecting;
      if (onScreen) start();
      else stop();
    },
    { threshold },
  ).observe(el);
  document.addEventListener('visibilitychange', () => (document.hidden ? stop() : start()));

  return () => {
    done = true;
    stop();
  };
}

/** Types `text` into `target` (an input's value or an element's text) one character at a time. */
export async function typeInto(target: HTMLInputElement | HTMLElement, text: string, wait: Wait, speed = 70, onChar?: (value: string) => void) {
  for (let i = 1; i <= text.length; i++) {
    const value = text.slice(0, i);
    if (target instanceof HTMLInputElement) target.value = value;
    else target.textContent = value;
    onChar?.(value);
    await wait(speed + Math.round(Math.random() * speed * 0.6));
  }
}

/** Presses an on-screen key/button: adds `.is-pressed` briefly. */
export async function press(el: Element | null, wait: Wait, ms = 180) {
  el?.classList.add('is-pressed');
  await wait(ms);
  el?.classList.remove('is-pressed');
}
