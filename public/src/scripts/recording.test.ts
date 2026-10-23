import { describe, expect, test } from 'bun:test';
import { recordingAllowed, recordingChoice, rememberRecordingChoice, replayUrl, replayAttribute, clearReplayStorage, RECORDING_CHOICE_KEY } from './recording';

function memoryStorage() {
  const values = new Map<string, string>();
  return {
    get length() { return values.size; },
    key(index: number) { return [...values.keys()][index] ?? null; },
    getItem(key: string) { return values.get(key) ?? null; },
    setItem(key: string, value: string) { values.set(key, value); },
    removeItem(key: string) { values.delete(key); },
    clear() { values.clear(); },
  };
}

describe('recording consent and replay privacy', () => {
  test('fails closed for missing, malformed, expired or inaccessible preferences', () => {
    const storage = memoryStorage();
    expect(recordingChoice(storage, 1000)).toBe('pending');
    storage.setItem(RECORDING_CHOICE_KEY, 'invalid JSON');
    expect(recordingChoice(storage, 1000)).toBe('pending');
    storage.setItem(RECORDING_CHOICE_KEY, JSON.stringify({ choice: 'allowed', expiresAt: 999 }));
    expect(recordingChoice(storage, 1000)).toBe('pending');
    storage.setItem(RECORDING_CHOICE_KEY, JSON.stringify({ choice: 'allowed', expiresAt: Number.MAX_SAFE_INTEGER }));
    expect(recordingChoice(storage, 1000)).toBe('pending');
    expect(recordingChoice({ getItem() { throw new Error('Blocked'); } })).toBe('pending');
    expect(rememberRecordingChoice({ setItem() { throw new Error('Blocked'); } }, 'allowed')).toBe(false);
  });

  test('records only after affirmative consent on the homepage, with privacy signals taking precedence', () => {
    const storage = memoryStorage();
    rememberRecordingChoice(storage, 'allowed', 1000);
    expect(recordingChoice(storage, 2000)).toBe('allowed');
    expect(recordingAllowed('allowed', '/', {})).toBe(true);
    for (const choice of ['pending', 'declined'] as const) expect(recordingAllowed(choice, '/', {})).toBe(false);
    for (const path of ['/subscribed/', '/confirmed/', '/privacy/', '/v1/', '/v3/']) expect(recordingAllowed('allowed', path, {})).toBe(false);
    expect(recordingAllowed('allowed', '/', { globalPrivacyControl: true })).toBe(false);
    expect(recordingAllowed('allowed', '/', { doNotTrack: '1' })).toBe(false);
    rememberRecordingChoice(storage, 'declined', 2000);
    expect(recordingChoice(storage, 3000)).toBe('declined');
    expect(JSON.parse(storage.getItem(RECORDING_CHOICE_KEY)!)).toEqual({ choice: 'declined', expiresAt: 15_552_002_000 });
  });

  test('removes all replay URL query/hash/credentials and masks input and email-bearing attributes', () => {
    expect(replayUrl('https://name:secret@adoken.dev/?email=private%40example.com&ref=launch#private'))
      .toBe('https://adoken.dev/');
    expect(replayUrl('/?utm_source=reddit')).toBe('https://adoken.dev/');
    expect(replayUrl('https://adoken.dev/private%40example.com')).toBe('https://adoken.dev/');
    expect(replayUrl('javascript:alert(1)')).toBe('');
    expect(replayAttribute('value', 'private@example.com')).toBe('[masked]');
    expect(replayAttribute('data-email', 'private%40example.com')).toBe('[masked]');
    expect(replayAttribute('href', '/?email=private%40example.com#email')).toBe('https://adoken.dev/');
    expect(replayAttribute('class', 'hero theme-nord')).toBe('hero theme-nord');
  });

  test('withdrawal clears only replay storage and retains the choice and unrelated feature state', () => {
    const storage = memoryStorage();
    for (const key of ['ph_adoken_replay', 'ph_adoken_replay_window_id', 'ph_adoken_replay_primary_window_exists', 'ph_adoken_replay_consent', 'theme', RECORDING_CHOICE_KEY]) storage.setItem(key, 'test');
    clearReplayStorage(storage);
    expect(storage.length).toBe(2);
    expect(storage.getItem('theme')).toBe('test');
    expect(storage.getItem(RECORDING_CHOICE_KEY)).toBe('test');
  });
});
