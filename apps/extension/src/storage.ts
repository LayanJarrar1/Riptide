/** `storage.local` (settings) and `storage.session` (per-browser-session state) helpers. */
import browser from 'webextension-polyfill';

import { normaliseSettings } from './settings';
import type { Settings } from './settings';

const SETTINGS_KEY = 'settings';
const SESSION_PREFIX = 'session:';

/** Load and normalise the settings. */
export async function loadSettings(): Promise<Settings> {
  const got = await browser.storage.local.get(SETTINGS_KEY);
  return normaliseSettings(got[SETTINGS_KEY]);
}

/** Persist the settings. */
export async function saveSettings(settings: Settings): Promise<void> {
  await browser.storage.local.set({ [SETTINGS_KEY]: settings });
}

/** Call `cb` whenever the settings change (from any page of the extension). */
export function onSettingsChanged(cb: (settings: Settings) => void): void {
  browser.storage.onChanged.addListener((changes, area) => {
    const change = changes[SETTINGS_KEY];
    if (area === 'local' && change) cb(normaliseSettings(change.newValue));
  });
}

/**
 * Session storage survives service-worker restarts and is cleared when the browser closes;
 * older browsers without it fall back to `storage.local` under a prefix.
 */
function sessionArea(): { area: typeof browser.storage.local; prefix: string } {
  if ('session' in browser.storage && browser.storage.session) {
    return { area: browser.storage.session, prefix: '' };
  }
  return { area: browser.storage.local, prefix: SESSION_PREFIX };
}

/** Read a session value. */
export async function getSession<T>(key: string, fallback: T): Promise<T> {
  const { area, prefix } = sessionArea();
  const k = prefix + key;
  try {
    const got = await area.get(k);
    const v = got[k];
    return v === undefined ? fallback : (v as T);
  } catch {
    return fallback;
  }
}

/** Write a session value. */
export async function setSession(key: string, value: unknown): Promise<void> {
  const { area, prefix } = sessionArea();
  try {
    await area.set({ [prefix + key]: value });
  } catch {
    /* storage unavailable: state is best-effort */
  }
}
