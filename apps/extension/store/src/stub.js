/* global window */
// Screenshot-only stand-in for the extension APIs, injected by store/render.mjs before the real
// built popup/options/content bundles run. webextension-polyfill returns globalThis.browser as-is
// when it already has runtime.id, so a promise-based object is enough. Values come from
// window.__RT (set per shot): the language the pages should use.
(() => {
  const cfg = window.__RT || {};
  const ar = cfg.lang === 'ar';
  const listeners = [];
  window.__rtMessageListeners = listeners;
  const event = () => ({ addListener() {}, removeListener() {}, hasListener: () => false });
  const settings = { language: cfg.lang || 'en' };
  const now = Date.now();
  const status = {
    status: 'connected',
    version: '0.1.0',
    captured: 4,
    paused: false,
    rulesEnabled: true,
    listAvailable: true,
    recent: [
      { download_id: 4, url: 'https://releases.example.org/ubuntu-24.04.1-desktop-amd64.iso', filename: 'ubuntu-24.04.1-desktop-amd64.iso', at: now - 60e3, state: { state: 'downloading' } },
      { download_id: 3, url: 'https://cdn.example.com/lecture-07.mp4', filename: ar ? 'محاضرة-07-الإشارات.mp4' : 'lecture-07-signals.mp4', at: now - 9 * 60e3, state: { state: 'verifying' } },
      { download_id: 2, url: 'https://files.example.net/dataset-2026-q3.zip', filename: 'dataset-2026-q3.zip', at: now - 31 * 60e3, state: { state: 'completed' } },
      { download_id: 1, url: 'https://example.org/report.pdf', filename: ar ? 'التقرير-السنوي.pdf' : 'annual-report.pdf', at: now - 74 * 60e3, state: { state: 'completed' } },
    ],
  };
  const storageArea = {
    get: async () => ({ settings }),
    set: async () => undefined,
    remove: async () => undefined,
  };
  const api = {
    runtime: {
      id: 'dcoeaodjhaibfchhcbfdkekpcacofnhm',
      getManifest: () => ({ version: '0.1.0' }),
      getURL: (p) => p,
      sendMessage: async (m) =>
        m && m.kind === 'handoff' ? { ok: true, status: 'connected', download_id: 5 } : status,
      openOptionsPage: async () => undefined,
      onMessage: { addListener: (fn) => listeners.push(fn), removeListener() {}, hasListener: () => false },
    },
    storage: { local: storageArea, session: storageArea, onChanged: event() },
    i18n: { getUILanguage: () => cfg.lang || 'en', getMessage: () => '' },
    permissions: { contains: async () => true, request: async () => true },
  };
  window.browser = api;
  window.chrome = { runtime: { id: api.runtime.id } };
})();
