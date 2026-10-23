/**
 * Tiny scene runner for the /v3/ mocks. A scene is an async function that awaits `stage.wait()`
 * between beats; the runner holds at the next beat while the mock is off screen or the tab is
 * hidden, and CSS loops inside it pause too (`[data-paused]` in page.css).
 * Callers only start scenes when `motionOK`: the markup's default state is the static fallback.
 */

export const motionOK = matchMedia('(prefers-reduced-motion: no-preference)').matches;

export interface Stage {
  /** Resolves after `ms` of on-screen time. */
  wait(ms: number): Promise<void>;
  /** Types `text` into `node` one character at a time. */
  type(node: Element, text: string, msPerChar?: number): Promise<void>;
}

interface Options {
  /** Restart the scene when it ends (default true). */
  loop?: boolean;
  /** Share of the element that must be visible to play (default 0.25). */
  threshold?: number;
}

/** Plays `scene` while `el` is visible. Returns a function that restarts it from the top. */
export function play(el: HTMLElement, scene: (stage: Stage) => Promise<void>, options: Options = {}): () => void {
  const { loop = true, threshold = 0.25 } = options;
  let intersecting = false;
  let generation = 0;
  let waiters: (() => void)[] = [];

  const update = () => {
    const visible = intersecting && !document.hidden;
    el.toggleAttribute('data-paused', !visible);
    if (visible) {
      for (const wake of waiters) wake();
      waiters = [];
    }
    return visible;
  };

  const onScreen = async () => {
    if (update()) return;
    const { promise, resolve } = Promise.withResolvers<void>();
    waiters.push(resolve);
    await promise;
  };

  const run = async () => {
    const gen = ++generation;
    const wait = async (ms: number) => {
      const { promise, resolve } = Promise.withResolvers<void>();
      setTimeout(resolve, ms);
      await promise;
      await onScreen();
      if (gen !== generation) throw new Error('superseded');
    };
    const stage: Stage = {
      wait,
      async type(node, text, msPerChar = 70) {
        node.textContent = '';
        for (const ch of text) {
          node.textContent += ch;
          await wait(msPerChar * (0.6 + Math.random() * 0.8));
        }
      },
    };
    try {
      await onScreen();
      do await scene(stage);
      while (loop && gen === generation);
    } catch {
      // superseded by a restart: the newer run owns the element now
    }
  };

  let started = false;
  new IntersectionObserver(
    ([entry]) => {
      intersecting = entry.isIntersecting;
      update();
      if (intersecting && !started) {
        started = true;
        run();
      }
    },
    { threshold },
  ).observe(el);
  document.addEventListener('visibilitychange', update);

  return run;
}

/** Sets exactly the given scene flags (`is-*` classes) on `el`. */
export function scene(el: Element, ...flags: string[]) {
  for (const c of [...el.classList]) if (c.startsWith('is-')) el.classList.remove(c);
  for (const f of flags) el.classList.add(`is-${f}`);
}
