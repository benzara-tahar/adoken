/** Small, deterministic measurement helpers. No SDK, storage, DOM, or network access. */
export type Properties = Record<string, string | number>;
export type Capture = (event: string, properties: Properties, immediate?: boolean) => void;
export const SITE_VERSION = 'engagement-2026-09-30';
export const IDLE_MS = 30_000;
export const DEPTHS = [25, 50, 75, 90] as const;

/** Campaign labels only. Reject whole values containing email addresses, URLs, or arbitrary text. */
export function campaignValue(value: string | null): string | undefined {
  return value && /^[a-z0-9][a-z0-9._-]{0,79}$/i.test(value) ? value : undefined;
}

export function attribution(search: string, referrer: string, hostname: string): Properties {
  const params = new URLSearchParams(search);
  const result: Properties = {};
  for (const key of ['ref', 'utm_source', 'utm_medium', 'utm_campaign']) {
    const value = campaignValue(params.get(key));
    if (value) result[key] = value;
  }
  let referringHost = '';
  try {
    const host = new URL(referrer).hostname;
    if (host !== hostname) referringHost = host;
  } catch { /* No referrer. */ }
  result.source = result.ref ?? result.utm_source ?? (campaignValue(referringHost) || 'direct');
  return result;
}

/** Keep public campaign labels, discard all other query parameters, credentials, and unsafe fragments. */
export function analyticsUrl(value: string): string {
  try {
    const url = new URL(value);
    url.username = '';
    url.password = '';
    if (decodeURIComponent(url.pathname).includes('@')) url.pathname = '/';
    for (const [name, value] of [...url.searchParams]) {
      if (!['ref', 'utm_source', 'utm_medium', 'utm_campaign', 'utm_content', 'utm_term'].includes(name)
        || !campaignValue(value)) url.searchParams.delete(name);
    }
    if (!/^#[a-z0-9_-]+$/i.test(url.hash)) url.hash = '';
    return url.href;
  } catch {
    return '';
  }
}

export function sanitizeProperties(properties: Record<string, unknown>): Record<string, unknown> {
  const clean = { ...properties };
  for (const [key, value] of Object.entries(clean)) {
    if (typeof value !== 'string') continue;
    if (/^https?:\/\//i.test(value)) clean[key] = analyticsUrl(value);
    else if (/@|%40/i.test(value)) delete clean[key];
    else if (/^(\$?(initial_)?utm_|ref$|source$)/.test(key) && !campaignValue(value)) delete clean[key];
  }
  return clean;
}

export function privacyOptOut(navigator: { doNotTrack?: string | null; globalPrivacyControl?: boolean }): boolean {
  return navigator.doNotTrack === '1' || navigator.doNotTrack === 'yes' || navigator.globalPrivacyControl === true;
}

export interface VisibleSection {
  section: string;
  top: number;
  bottom: number;
  left: number;
  right: number;
}

export function dominantSection(sections: VisibleSection[], width: number, height: number): string | null {
  let best: string | null = null;
  let area = 0;
  for (const section of sections) {
    const visible = Math.max(0, Math.min(height, section.bottom) - Math.max(0, section.top))
      * Math.max(0, Math.min(width, section.right) - Math.max(0, section.left));
    if (visible > area) { area = visible; best = section.section; }
  }
  return best;
}

/** Time is allocated to exactly one section, cut off at idle, and emitted as non-overlapping increments. */
export class AttentionMeter {
  private section: string | null = null;
  private foreground = false;
  private lastTick: number;
  private lastActivity: number;
  private seconds = new Map<string, number>();
  private seen = new Set<string>();

  constructor(private capture: Capture, now: number) {
    this.lastTick = this.lastActivity = now;
  }

  private account(now: number) {
    if (this.foreground && this.section) {
      const ms = Math.max(0, Math.min(now, this.lastActivity + IDLE_MS) - this.lastTick);
      this.seconds.set(this.section, (this.seconds.get(this.section) ?? 0) + ms / 1000);
    }
    this.lastTick = now;
  }

  private reach() {
    if (this.foreground && this.section && !this.seen.has(this.section)) {
      this.seen.add(this.section);
      this.capture('section_viewed', { section: this.section });
    }
  }

  setSection(section: string | null, now: number) {
    this.account(now);
    if (section !== this.section && this.section) this.flushSection(this.section, false, 1);
    this.section = section;
    this.reach();
  }

  setForeground(foreground: boolean, now: number) {
    this.account(now);
    if (foreground && !this.foreground) this.lastActivity = now;
    this.foreground = foreground;
    this.reach();
    if (!foreground) this.flush(now, true);
  }

  activity(now: number) {
    this.account(now);
    this.lastActivity = now;
    this.reach();
  }

  private flushSection(section: string, immediate: boolean, minimum = 0) {
    const seconds = this.seconds.get(section) ?? 0;
    if (seconds <= minimum) return;
    this.seconds.delete(section);
    this.capture('section_engaged', { section, active_seconds: seconds }, immediate);
  }

  flush(now: number, immediate = false) {
    this.account(now);
    for (const section of this.seconds.keys()) this.flushSection(section, immediate);
  }
}

export class ScrollDepth {
  private seen = new Set<number>();
  constructor(private capture: Capture) {}
  update(bottom: number, documentHeight: number) {
    if (documentHeight <= 0) return;
    const percent = 100 * Math.max(0, bottom) / documentHeight;
    for (const milestone of DEPTHS) {
      if (percent >= milestone && !this.seen.has(milestone)) {
        this.seen.add(milestone);
        this.capture('scroll_depth_reached', { percent: milestone });
      }
    }
  }
}
