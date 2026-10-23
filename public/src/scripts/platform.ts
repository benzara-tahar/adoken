/** The few navigator fields the download CTA reads. Nothing here is sent anywhere. */
export interface PlatformHints {
  platform?: string;
  userAgent?: string;
  maxTouchPoints?: number;
  userAgentData?: { platform?: string };
}

/**
 * True when the visitor looks like they are on a Mac. iPadOS in desktop mode also reports "MacIntel",
 * so touch support tells the two apart. Browsers can't tell Apple silicon from Intel, so we don't try.
 */
export function isMac(nav: PlatformHints): boolean {
  const platform = nav.userAgentData?.platform || nav.platform || '';
  if (platform) return /^mac/i.test(platform) && !((nav.maxTouchPoints ?? 0) > 1);
  return /Macintosh/.test(nav.userAgent ?? '') && !((nav.maxTouchPoints ?? 0) > 1);
}
