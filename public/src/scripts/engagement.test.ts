import { describe, expect, test } from 'bun:test';
import {
  AttentionMeter, ScrollDepth, analyticsUrl, attribution, campaignValue, dominantSection,
  privacyOptOut, sanitizeProperties, type Capture, type Properties,
} from './engagement';

function fixture() {
  const events: { event: string; properties: Properties; immediate?: boolean }[] = [];
  const capture: Capture = (event, properties, immediate) => events.push({ event, properties, immediate });
  const meter = new AttentionMeter(capture, 0);
  meter.setSection('search', 0);
  meter.setForeground(true, 0);
  const seconds = () => events.filter(({ event }) => event === 'section_engaged')
    .reduce((sum, { properties }) => sum + Number(properties.active_seconds), 0);
  return { meter, events, capture, seconds };
}

describe('foreground section attention', () => {
  test('cuts a long idle visit off at 30 seconds and resumes only on activity', () => {
    const f = fixture();
    f.meter.flush(120_000);
    expect(f.seconds()).toBe(30);
    f.meter.activity(125_000);
    f.meter.flush(135_000);
    expect(f.seconds()).toBe(40);
  });

  test('excludes hidden and blurred intervals, including a back/forward-cache return', () => {
    const f = fixture();
    f.meter.setForeground(false, 5_000);
    f.meter.setSection('themes', 6_000);
    expect(f.events.filter(e => e.event === 'section_viewed')).toHaveLength(1);
    f.meter.flush(100_000);
    f.meter.setForeground(true, 120_000);
    f.meter.flush(126_000);
    expect(f.seconds()).toBe(11);
    expect(f.events.filter(e => e.event === 'section_viewed').map(e => e.properties.section)).toEqual(['search', 'themes']);
  });

  test('allocates time to one section and preserves short visits without emitting on every scroll', () => {
    const f = fixture();
    f.meter.setSection('themes', 500);
    f.meter.setSection('search', 900);
    expect(f.events.filter(e => e.event === 'section_engaged')).toHaveLength(0);
    f.meter.flush(2_000);
    const emitted = f.events.filter(e => e.event === 'section_engaged');
    expect(emitted.map(e => [e.properties.section, e.properties.active_seconds])).toEqual([
      ['search', 1.6], ['themes', 0.4],
    ]);
    expect(f.seconds()).toBe(2);
    expect(f.events.filter(e => e.event === 'section_viewed')).toHaveLength(2);
  });

  test('repeated periodic and exit flushes never duplicate time', () => {
    const f = fixture();
    f.meter.flush(15_000);
    f.meter.activity(20_000);
    f.meter.setSection('themes', 25_000);
    f.meter.flush(30_000);
    f.meter.setForeground(false, 35_000);
    f.meter.setForeground(false, 35_000);
    f.meter.flush(35_000, true);
    expect(f.seconds()).toBe(35);
    expect(f.events.filter(e => e.event === 'section_viewed')).toHaveLength(2);
    expect(f.events.at(-1)?.immediate).toBe(true);
  });

  test('does not count gaps outside any tracked section', () => {
    const f = fixture();
    f.meter.setSection(null, 2_000);
    f.meter.setSection('speed', 8_000);
    f.meter.flush(10_000);
    expect(f.seconds()).toBe(4);
  });

  test('chooses visible area, not intersection ratio of the entire section', () => {
    expect(dominantSection([
      { section: 'long', top: -1000, bottom: 900, left: 0, right: 1000 },
      { section: 'short', top: 900, bottom: 1000, left: 0, right: 1000 },
    ], 1000, 1000)).toBe('long');
    expect(dominantSection([
      { section: 'offscreen', top: 1100, bottom: 1400, left: 0, right: 1000 },
    ], 1000, 1000)).toBeNull();
  });
});

describe('depth and privacy', () => {
  test('emits four fixed depth milestones at most once despite scrolling back and resizing', () => {
    const f = fixture();
    const depth = new ScrollDepth(f.capture);
    depth.update(250, 1000);
    depth.update(200, 1000);
    depth.update(950, 1000);
    depth.update(950, 1000);
    depth.update(100, 0);
    expect(f.events.filter(e => e.event === 'scroll_depth_reached').map(e => e.properties.percent))
      .toEqual([25, 50, 75, 90]);
  });

  test('rejects addresses, arbitrary text and overlong campaign values', () => {
    for (const value of ['someone@example.com', 'someone%40example.com', 'person name', 'https://example.com', 'x'.repeat(81)]) {
      expect(campaignValue(value)).toBeUndefined();
    }
    expect(attribution('?ref=w3-demo&utm_source=reddit&utm_campaign=launch', 'https://x.com/a', 'adoken.dev'))
      .toEqual({ ref: 'w3-demo', utm_source: 'reddit', utm_campaign: 'launch', source: 'w3-demo' });
    expect(attribution('?ref=someone%40example.com', 'https://adoken.dev/subscribed/?email=private', 'adoken.dev'))
      .toEqual({ source: 'direct' });
    expect(attribution('', 'https://news.ycombinator.com/item?id=42', 'adoken.dev'))
      .toEqual({ source: 'news.ycombinator.com' });
  });

  test('removes redirect email, credentials and tokens from every URL property', () => {
    const original = {
      $current_url: 'https://adoken.dev/subscribed/?email_address=private%40example.com&ref=w3-demo&token=secret',
      $referrer: 'https://name:secret@buttondown.com/confirm?email=private%2540example.com&code=secret',
      $utm_source: 'private@example.com',
      $initial_utm_campaign: 'person name',
      ref: 'unsafe value',
      placement: 'v2-hero',
      active_seconds: 5,
    };
    expect(sanitizeProperties(original)).toEqual({
      $current_url: 'https://adoken.dev/subscribed/?ref=w3-demo',
      $referrer: 'https://buttondown.com/confirm', placement: 'v2-hero', active_seconds: 5,
    });
    expect(original.$current_url).toContain('private');
    expect(analyticsUrl('https://adoken.dev/private%40example.com#email=private')).toBe('https://adoken.dev/');
    expect(analyticsUrl('https://adoken.dev/?utm_source=reddit#themes')).toBe('https://adoken.dev/?utm_source=reddit#themes');
    expect(analyticsUrl('not a URL')).toBe('');
  });

  test('respects both DNT and GPC before analytics loads', () => {
    expect(privacyOptOut({ doNotTrack: '1' })).toBe(true);
    expect(privacyOptOut({ doNotTrack: 'yes' })).toBe(true);
    expect(privacyOptOut({ globalPrivacyControl: true })).toBe(true);
    expect(privacyOptOut({ doNotTrack: '0' })).toBe(false);
    expect(privacyOptOut({})).toBe(false);
  });
});
