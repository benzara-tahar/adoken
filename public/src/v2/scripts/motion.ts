/**
 * Tiny runtime for the page's looping mocks. A scene only advances while its element is on screen
 * (and the element gets `.is-paused` otherwise, which freezes its CSS animations too). Under
 * prefers-reduced-motion nothing loops: the server-rendered markup is already a finished frame.
 */
export const reducedMotion = matchMedia('(prefers-reduced-motion: reduce)').matches;

export class Scene {
  visible = false;
  #waiting: (() => void)[] = [];

  constructor(readonly root: HTMLElement) {
    root.classList.add('is-paused');
    new IntersectionObserver(
      ([entry]) => {
        this.visible = entry.isIntersecting;
        root.classList.toggle('is-paused', !this.visible);
        if (this.visible) for (const wake of this.#waiting.splice(0)) wake();
      },
      { threshold: 0.2 },
    ).observe(root);
  }

  /** Waits `ms`, then (if the scene scrolled away meanwhile) until it is visible again. */
  async sleep(ms: number) {
    const timer = Promise.withResolvers<void>();
    setTimeout(timer.resolve, ms);
    await timer.promise;
    if (this.visible) return;
    const shown = Promise.withResolvers<void>();
    this.#waiting.push(shown.resolve);
    await shown.promise;
  }
}

/** Runs `step` forever while `root` is on screen. No-op under reduced motion. */
export function loop(root: HTMLElement | null, step: (scene: Scene) => Promise<void>) {
  if (!root || reducedMotion) return;
  const scene = new Scene(root);
  void (async () => {
    await scene.sleep(0);
    for (;;) await step(scene);
  })();
}

/** Calls `run` once, the first time `el` is mostly on screen. */
export function onceVisible(el: Element, run: () => void, threshold = 0.35) {
  const io = new IntersectionObserver(
    ([entry]) => {
      if (!entry.isIntersecting) return;
      io.disconnect();
      run();
    },
    { threshold },
  );
  io.observe(el);
}
