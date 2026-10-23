import { AttentionMeter, ScrollDepth, dominantSection, type Capture, type Properties } from './engagement';

// Fixed public sections, independent of copy and animated mock content. No product component needs editing.
const SECTIONS: Record<string, Record<string, string>> = {
  v2: { '#search': 'search', '#review': 'features', '#speed': 'speed', '#themes': 'themes', '#horizon': 'roadmap', '#launch': 'launch' },
  v1: { '#top': 'hero', '#moves': 'features', '#bosses': 'pain_points', '#scores': 'speed', '#themes': 'themes', '#fight': 'activity', '#dojo': 'principles', '#stages': 'roadmap', '#join': 'launch' },
  v3: { 'section.hero': 'hero', '#chapters': 'contents', '#desk': 'workspace', '#ch-search': 'search', '#ch-review': 'review', '#ch-calm': 'notifications', '#ch-nudges': 'nudges', '#ch-ken': 'ken', '#ch-ink': 'themes', '#race': 'speed', 'section.roadmap': 'roadmap', '#colophon': 'launch' },
};
const OS = new Set(['macOS', 'Windows', 'Linux']);
const PLACEMENTS = new Set(['v1-hero', 'v1-footer', 'v2-hero', 'v2-footer', 'v3-hero', 'v3-footer']);

export function siteDesign(pathname: string): string {
  return pathname === '/' ? 'v2' : /^\/v[13]\/?$/.test(pathname) ? pathname.slice(1, 3) : 'notice';
}

/** Inject capture for browser verification; production supplies the existing cookieless SDK. */
export function instrument(capture: Capture, doc: Document = document, win: Window = window): () => void {
  const design = siteDesign(win.location.pathname);
  const sections = Object.entries(SECTIONS[design] ?? {}).flatMap(([selector, section]) => {
    const element = doc.querySelector<HTMLElement>(selector);
    return element ? [{ element, section }] : [];
  });
  const now = () => win.performance.now();
  const meter = new AttentionMeter(capture, now());
  const depth = new ScrollDepth(capture);
  const foreground = () => doc.visibilityState === 'visible' && doc.hasFocus();
  const listeners: (() => void)[] = [];
  const listen = (target: EventTarget, type: string, callback: EventListener, options?: AddEventListenerOptions) => {
    target.addEventListener(type, callback, options);
    listeners.push(() => target.removeEventListener(type, callback, options));
  };
  let frame = 0;
  const measure = () => {
    frame = 0;
    meter.setSection(dominantSection(sections.map(({ element, section }) => {
      const rect = element.getBoundingClientRect();
      return { section, top: rect.top, bottom: rect.bottom, left: rect.left, right: rect.right };
    }), win.innerWidth, win.innerHeight), now());
    if (sections.length && foreground()) depth.update(win.scrollY + win.innerHeight, doc.documentElement.scrollHeight);
  };
  const schedule = () => { if (!frame) frame = win.requestAnimationFrame(measure); };
  const refreshForeground = () => {
    meter.setForeground(foreground(), now());
    if (foreground()) schedule();
  };
  refreshForeground();
  schedule();
  let lastActivity = -1000;
  const activity = (event: Event) => {
    if (!event.isTrusted || !foreground()) return;
    const time = now();
    if (time - lastActivity >= 1000) { meter.activity(time); lastActivity = time; }
  };
  for (const type of ['pointermove', 'pointerdown', 'keydown', 'input', 'wheel', 'touchstart']) {
    listen(doc, type, activity, { passive: true });
  }
  listen(win, 'scroll', (event) => { activity(event); schedule(); }, { passive: true });
  listen(win, 'resize', schedule, { passive: true });
  listen(win, 'focus', refreshForeground);
  listen(win, 'blur', refreshForeground);
  listen(doc, 'visibilitychange', refreshForeground);
  listen(win, 'pagehide', () => meter.setForeground(false, now()));
  // Returning from the back/forward cache must restart foreground accounting without the hidden interval.
  listen(win, 'pageshow', refreshForeground);
  const timer = win.setInterval(() => meter.flush(now()), 15_000);

  // Allowlist the rendered public theme buttons once. Later field values or animation ticks are never read.
  const interactions = new Map<Element, Properties>();
  for (const button of doc.querySelectorAll<HTMLElement>('#themes button[data-pick], #themes button[data-name], #ch-ink button[data-pick]')) {
    const target = (button.dataset.pick ?? button.dataset.name ?? '').normalize('NFKD').toLowerCase()
      .replace(/[\u0300-\u036f]/g, '').replace(/[^a-z0-9]+/g, '-').replace(/^-|-$/g, '').slice(0, 64);
    if (target) interactions.set(button, { interaction: 'theme_selected', target, section: 'themes' });
  }
  for (const button of doc.querySelectorAll('#speed button[data-replay], #race button.race-replay')) {
    interactions.set(button, { interaction: 'speed_replayed', target: 'speed', section: 'speed' });
  }
  const designNav = 'nav[aria-label="Design variants"], nav[aria-label="Design editions"]';
  for (const link of doc.querySelectorAll<HTMLAnchorElement>(`a[href^="#"], ${designNav.split(', ').map(nav => `${nav} a`).join(', ')}`)) {
    const href = link.getAttribute('href') ?? '';
    if (link.closest(designNav)) {
      const target = href === '/' ? 'v2' : /^\/v[13]\/?$/.test(href) ? href.slice(1, 3) : '';
      if (target) interactions.set(link, { interaction: 'design_switched', target, section: 'navigation' });
    } else {
      const element = href.length > 1 && /^#[a-z0-9_-]+$/i.test(href) ? doc.querySelector(href) : null;
      const section = sections.find(({ element: root }) => element && (root === element || root.contains(element)));
      if (section) interactions.set(link, { interaction: 'section_navigated', target: section.section, section: 'navigation' });
    }
  }

  const formsStarted = new Set<HTMLFormElement>();
  const started = (form: HTMLFormElement) => {
    const placement = form.dataset.placement;
    if (!placement || !PLACEMENTS.has(placement) || formsStarted.has(form)) return;
    formsStarted.add(form);
    capture('launch_list_started', { placement });
  };
  listen(doc, 'focusin', (event) => {
    if (!event.isTrusted || !(event.target instanceof Element)) return;
    const form = event.target.closest<HTMLFormElement>('form[data-track="launch_list_submitted"]');
    if (form) started(form);
  });
  listen(doc, 'change', (event) => {
    if (!event.isTrusted || !(event.target instanceof HTMLInputElement)) return;
    const input = event.target;
    const form = input.closest<HTMLFormElement>('form[data-track="launch_list_submitted"]');
    if (!form || !PLACEMENTS.has(form.dataset.placement ?? '') || input.name !== 'metadata__os'
      || input.type !== 'radio' || !OS.has(input.value)) return;
    started(form);
    capture('launch_list_os_selected', { os: input.value, placement: form.dataset.placement! });
  });
  listen(doc, 'submit', (event) => {
    if (!event.isTrusted || !(event.target instanceof HTMLFormElement)) return;
    const form = event.target;
    if (form.dataset.track !== 'launch_list_submitted' || !PLACEMENTS.has(form.dataset.placement ?? '')) return;
    const selected = form.querySelector<HTMLInputElement>('input[type="radio"][name="metadata__os"]:checked');
    const properties: Properties = { placement: form.dataset.placement! };
    if (selected && OS.has(selected.value)) properties.os = selected.value;
    started(form);
    meter.flush(now(), true);
    capture('launch_list_submitted', properties, true);
  });
  listen(doc, 'click', (event) => {
    if (!event.isTrusted || !(event.target instanceof Element)) return;
    const control = event.target.closest('button, a');
    if (!control) return;
    const interaction = interactions.get(control);
    if (interaction) capture('site_interaction', interaction, control.tagName === 'A');
    if (control instanceof HTMLElement && control.dataset.track === 'follow_x_clicked') {
      // Placement is authored markup, never visitor input; keep its bounded label only.
      const placement = control.dataset.placement;
      if (placement && /^[a-z0-9-]{1,40}$/.test(placement)) capture('follow_x_clicked', { placement }, true);
    }
  });
  let searchUsed = false;
  listen(doc, 'input', (event) => {
    if (searchUsed || !event.isTrusted || !(event.target instanceof Element)
      || !event.target.matches('[data-palette] input[data-input]')) return;
    searchUsed = true;
    capture('site_interaction', { interaction: 'search_demo_used', target: 'search', section: 'features' });
  });

  return () => {
    meter.setForeground(false, now());
    win.clearInterval(timer);
    if (frame) win.cancelAnimationFrame(frame);
    for (const remove of listeners) remove();
  };
}
