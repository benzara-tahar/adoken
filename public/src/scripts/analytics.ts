/**
 * Cookieless page analytics by default, with homepage replay only after explicit consent.
 * Production only; both SDK variants load after paint and idle. DNT / GPC always take precedence.
 *
 * Instrumentation counts allowlisted section, demo, navigation and launch-form interactions.
 * It reads only public markup and the optional work-platform choice, never typed field values.
 */
import { posthog as config } from '../integrations';
import { SITE_VERSION, attribution, privacyOptOut, sanitizeProperties } from './engagement';
import { instrument, siteDesign } from './instrumentation';
import {
  recordingAllowed, recordingChoice, clearReplayStorage, replayUrl, replayAttribute,
  RECORDING_CHOICE_EVENT, REPLAY_STORE,
} from './recording';

async function start() {
  // Development and production previews must never pollute the real project.
  if (location.hostname !== 'adoken.dev' || privacyOptOut(navigator)) return;
  const allowed = () => {
    try { return recordingAllowed(recordingChoice(localStorage), location.pathname, navigator); }
    catch { return false; }
  };
  let replayAuthorized = allowed();
  let client: typeof import('posthog-js/dist/module.slim').default | typeof import('posthog-js/dist/module.full.no-external').default | undefined;
  addEventListener(RECORDING_CHOICE_EVENT, (event) => {
    if ((event as CustomEvent).detail !== 'declined') return;
    replayAuthorized = false;
    if (client && !client.config.cookieless_mode) {
      client.stopSessionRecording();
      client.opt_out_capturing();
      client.set_config({ disable_persistence: true, disable_session_recording: true });
    }
    try { clearReplayStorage(sessionStorage); clearReplayStorage(localStorage); } catch { /* Optional storage. */ }
  });
  if (!replayAuthorized) { try { clearReplayStorage(sessionStorage); clearReplayStorage(localStorage); } catch { /* Optional storage. */ } }
  const common = {
    ...attribution(location.search, document.referrer, location.hostname),
    site_version: SITE_VERSION,
    site_design: siteDesign(location.pathname),
    recording_allowed: replayAuthorized,
    // A page-local random ID for averaging attention increments. Never stored or reused across visits.
    page_visit_id: crypto.randomUUID(),
  };
  // Separate chunks: visitors who decline never download the recorder. The self-contained variant
  // includes rrweb locally, retaining CSP script-src 'self' without a third-party script exception.
  const { default: posthog } = replayAuthorized
    ? await import('posthog-js/dist/module.full.no-external')
    : await import('posthog-js/dist/module.slim');
  if (privacyOptOut(navigator) || (replayAuthorized && !allowed())) return;

  posthog.init(config.token, {
    api_host: config.apiHost,
    ui_host: config.uiHost,
    defaults: '2026-08-30',
    cookieless_mode: replayAuthorized ? undefined : 'always',
    persistence: 'sessionStorage',
    persistence_name: REPLAY_STORE,
    consent_persistence_name: `ph_${REPLAY_STORE}_consent`,
    disable_persistence: !replayAuthorized,
    cross_subdomain_cookie: false,
    save_campaign_params: false,
    save_referrer: false,
    person_profiles: 'never',
    respect_dnt: true,
    autocapture: false,
    rageclick: false,
    enable_heatmaps: false,
    capture_exceptions: false,
    capture_performance: false,
    enable_recording_console_log: false,
    disable_surveys: true,
    disable_conversations: true,
    disable_product_tours: true,
    disable_session_recording: !replayAuthorized,
    // Replay needs the project's remote recording configuration, loaded as JSON through the proxy.
    advanced_disable_flags: !replayAuthorized,
    advanced_disable_feature_flags: true,
    advanced_disable_feature_flags_on_first_load: true,
    // Nothing is fetched as a script at runtime, so CSP `script-src 'self'` holds.
    disable_external_dependency_loading: true,
    disable_capture_url_hashes: true,
    session_recording: {
      maskAllInputs: true,
      blockSelector: 'input[type="hidden"], input[type="file"], iframe, [contenteditable="true"]',
      maskAttributeFn: replayAttribute,
      recordHeaders: false,
      recordBody: false,
      recordCrossOriginIframes: false,
      captureCanvas: { recordCanvas: false },
      captureJsonLd: false,
      strictMinimumDuration: true,
      maskCapturedNetworkRequestFn: data => ({ ...data, name: replayUrl(data.name ?? '') }),
    },
    // Buttondown's redirects may carry email addresses. Keep only safe public campaign labels in URLs.
    before_send: (event) => {
      if (!event) return event;
      if (privacyOptOut(navigator) || (event.event === '$snapshot' && (!replayAuthorized || !allowed()))) return null;
      event.properties = sanitizeProperties({ ...event.properties, ...common });
      return event;
    },
  });
  client = posthog;

  instrument((event, properties, immediate) => {
    posthog.capture(event, properties, immediate ? { transport: 'sendBeacon' } : undefined);
  });
}

/** Resolves at first contentful paint (immediately if the browser doesn't report paint timing). */
function firstPaint() {
  const { promise, resolve } = Promise.withResolvers<void>();
  if (!PerformanceObserver.supportedEntryTypes?.includes('paint')) {
    resolve();
    return promise;
  }
  const observer = new PerformanceObserver((list) => {
    if (list.getEntriesByName('first-contentful-paint').length === 0) return;
    observer.disconnect();
    resolve();
  });
  observer.observe({ type: 'paint', buffered: true });
  return promise;
}

if (import.meta.env.PROD) {
  const loaded = Promise.withResolvers<void>();
  if (document.readyState === 'complete') loaded.resolve();
  else addEventListener('load', () => loaded.resolve(), { once: true });
  void loaded.promise.then(firstPaint).then(() => {
    const load = () => { void start().catch(() => { /* Analytics must never break the website. */ }); };
    if ('requestIdleCallback' in window) requestIdleCallback(load, { timeout: 3000 });
    else setTimeout(load, 200);
  });
}
