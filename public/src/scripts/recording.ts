import { analyticsUrl, privacyOptOut } from './engagement';

export const RECORDING_CHOICE_KEY = 'adoken.recording-choice.v1';
export const RECORDING_CHOICE_EVENT = 'adoken:recording-choice';
export const REPLAY_STORE = 'adoken_replay';
const CHOICE_LIFETIME = 180 * 24 * 60 * 60 * 1000;
export type RecordingChoice = 'allowed' | 'declined' | 'pending';

/** This preference contains a decision and expiry, never a visitor identifier. */
export function recordingChoice(storage: Pick<Storage, 'getItem'>, now = Date.now()): RecordingChoice {
  try {
    const value = JSON.parse(storage.getItem(RECORDING_CHOICE_KEY) ?? 'null');
    if (!value || !Number.isFinite(value.expiresAt) || value.expiresAt <= now ||
      value.expiresAt > now + CHOICE_LIFETIME || !['allowed', 'declined'].includes(value.choice)) return 'pending';
    return value.choice;
  } catch { return 'pending'; }
}

export function rememberRecordingChoice(storage: Pick<Storage, 'setItem'>, choice: Exclude<RecordingChoice, 'pending'>, now = Date.now()) {
  try {
    storage.setItem(RECORDING_CHOICE_KEY, JSON.stringify({ choice, expiresAt: now + CHOICE_LIFETIME }));
    return true;
  } catch { return false; }
}

export function recordingAllowed(choice: RecordingChoice, pathname: string, signals: Parameters<typeof privacyOptOut>[0]) {
  return choice === 'allowed' && pathname === '/' && !privacyOptOut(signals);
}

/** Replay snapshots have nested URL fields that ordinary event-property filtering cannot cover. */
export function replayUrl(value: string) {
  try {
    const url = new URL(analyticsUrl(new URL(value, 'https://adoken.dev/').href));
    if (!['https:', 'http:'].includes(url.protocol)) return '';
    url.search = '';
    url.hash = '';
    return url.href;
  } catch { return ''; }
}

export function replayAttribute(name: string, value: string) {
  if (name === 'value') return '[masked]';
  if (['href', 'src', 'action', 'formaction', 'poster', 'xlink:href'].includes(name)) return replayUrl(value);
  if (/@|%40/i.test(value)) return '[masked]';
  return value;
}

/** Remove only this site's replay storage when consent expires or is withdrawn. */
export function clearReplayStorage(storage: Storage) {
  try {
    const keys = Array.from({ length: storage.length }, (_, index) => storage.key(index));
    for (const key of keys) if (key?.startsWith(`ph_${REPLAY_STORE}`)) storage.removeItem(key);
  } catch { /* Storage can be blocked by the browser. */ }
}
