/**
 * Background script (service worker on Chromium, event page on Firefox).
 *
 * Captures browser downloads that match the rules, hands them to the Riptide app through the
 * Native Messaging host, and degrades to the browser's own download on any failure. Also owns
 * the context menus, the toolbar badge, notifications and the internal message bus.
 *
 * Hard rule 5: nothing that comes from a page is executed or used as a path. The only things
 * sent to the host are URLs, a suggested filename, the referrer, the user agent and the cookies
 * of that URL, inside the `add` schema.
 */
import browser from 'webextension-polyfill';
import type { Downloads, Menus, Notifications, Runtime, Tabs, WebRequest } from 'webextension-polyfill';

import { HostClient } from './host';
import { resolveLang, t } from './i18n';
import type { Lang, MessageKey } from './i18n';
import { isMessage } from './messages';
import type { HandoffManyReply, HandoffReply, Message, RecentItem, StatusReply } from './messages';
import type { DownloadId, DownloadRequest } from './protocol';
import {
  decide,
  filenameFromDisposition,
  filenameFromUrl,
  hostOf,
  isAttachmentDisposition,
  isDownloadMime,
  mediaType,
  normaliseSite,
} from './rules';
import type { Candidate } from './rules';
import { DEFAULT_SETTINGS } from './settings';
import type { Settings } from './settings';
import type { HostStatus } from './status';
import { getSession, loadSettings, onSettingsChanged, saveSettings, setSession } from './storage';

const IS_FIREFOX = __BROWSER__ === 'firefox';
const MAX_RECENT = 10;
const BYPASS_URL_MS = 4_000;
const BYPASS_ANY_MS = 1_500;
const COLOR_CONNECTED = '#19C6B5';
const COLOR_OFF = '#6D7B87';
const COLOR_BADGE_TEXT = '#06201D';

const MENU_LINK = 'riptide-link';
const MENU_ALL = 'riptide-all';
const MENU_SELECTION = 'riptide-selection';

const NOTIFY_NOT_RUNNING = 'riptide-not-running:';
const NOTIFY_HOST_MISSING = 'riptide-host-missing';
const NOTIFY_FAILED = 'riptide-failed';
const NOTIFY_SENT = 'riptide-sent';
const NOTIFY_RELOAD = 'riptide-reload';

// ---------------------------------------------------------------------------------------------
// State
// ---------------------------------------------------------------------------------------------

let settings: Settings = DEFAULT_SETTINGS;
let settingsLoaded = false;
let paused = false;
let missingNotified = false;
let listAvailable = true;
const bypassUrls = new Map<string, number>();
let lastAltClickAt = 0;

const host = new HostClient();
host.onStatus = () => {
  void updateBadge();
};

const ready: Promise<void> = (async () => {
  settings = await loadSettings();
  settingsLoaded = true;
  paused = await getSession('paused', false);
  missingNotified = await getSession('missingNotified', false);
  listAvailable = await getSession('listAvailable', true);
  await updateBadge();
})();

onSettingsChanged((s) => {
  settings = s;
  void updateBadge();
});

function lang(): Lang {
  return resolveLang(settings.language, browser.i18n.getUILanguage());
}

function tr(key: MessageKey, params?: Record<string, string | number>): string {
  return t(lang(), key, params);
}

function baseName(path: string | undefined): string {
  if (!path) return '';
  return path.split(/[/\\]/).pop() ?? '';
}

function displayName(c: Candidate): string {
  return c.filename || filenameFromUrl(c.url) || c.url;
}

function errorText(e: unknown): string {
  return e instanceof Error ? e.message : String(e);
}

function wasBypassed(url: string): boolean {
  const now = Date.now();
  const at = bypassUrls.get(url);
  if (at !== undefined && now - at < BYPASS_URL_MS) return true;
  return now - lastAltClickAt < BYPASS_ANY_MS;
}

// ---------------------------------------------------------------------------------------------
// Badge
// ---------------------------------------------------------------------------------------------

async function updateBadge(): Promise<void> {
  const captured = await getSession('captured', 0);
  const text = captured > 0 ? String(captured) : '';
  const connected = host.status === 'connected';
  const titleKey: MessageKey = paused ? 'badge.title.paused' : `badge.title.${host.status}`;
  try {
    await browser.action.setBadgeText({ text });
    await browser.action.setBadgeBackgroundColor({ color: connected ? COLOR_CONNECTED : COLOR_OFF });
    await browser.action.setTitle({ title: tr(titleKey) });
    if (typeof browser.action.setBadgeTextColor === 'function') {
      await browser.action.setBadgeTextColor({ color: connected ? COLOR_BADGE_TEXT : '#FFFFFF' });
    }
  } catch {
    /* badge APIs are best-effort */
  }
}

// ---------------------------------------------------------------------------------------------
// Hand-off to the app
// ---------------------------------------------------------------------------------------------

async function cookieHeaderFor(url: string): Promise<string | null> {
  try {
    const cookies = await browser.cookies.getAll({ url });
    if (cookies.length === 0) return null;
    return cookies.map((c) => `${c.name}=${c.value}`).join('; ');
  } catch {
    return null; // no permission on this browser/site: Riptide will fetch without cookies
  }
}

async function recordSent(c: Candidate, downloadId: DownloadId): Promise<void> {
  const captured = (await getSession('captured', 0)) + 1;
  const recent = await getSession<RecentItem[]>('recent', []);
  recent.unshift({ download_id: downloadId, url: c.url, filename: displayName(c), at: Date.now(), state: null });
  await setSession('captured', captured);
  await setSession('recent', recent.slice(0, MAX_RECENT));
  await updateBadge();
  if (settings.notify) {
    await notify(NOTIFY_SENT, tr('notify.sent.title'), tr('notify.sent.body', { name: displayName(c) }));
  }
}

/** Send `add` for a candidate. Never throws; the caller decides what to do on failure. */
async function handoff(c: Candidate): Promise<HandoffReply> {
  const request: DownloadRequest = { url: c.url, user_agent: navigator.userAgent };
  if (c.filename) request.filename = c.filename;
  if (c.referrer && hostOf(c.referrer) !== null) request.referrer = c.referrer;
  const cookies = await cookieHeaderFor(c.url);
  if (cookies) request.secrets = { cookies };

  try {
    const res = await host.request({ type: 'add', request });
    if (res.type === 'added') {
      await recordSent(c, res.download_id);
      return { ok: true, status: host.status, download_id: res.download_id };
    }
    if (res.type === 'error') {
      // The host answered, so it exists; find out whether the app is reachable.
      const status = await host.ping();
      return { ok: false, status, error: res.message };
    }
    return { ok: false, status: host.status, error: `unexpected answer: ${res.type}` };
  } catch (e) {
    return { ok: false, status: host.status === 'connected' ? 'app_not_running' : host.status, error: errorText(e) };
  }
}

async function handoffMany(urls: string[], referrer: string | undefined): Promise<HandoffManyReply> {
  let ok = 0;
  let failed = 0;
  let last: HandoffReply | null = null;
  for (const url of urls) {
    if (hostOf(url) === null) continue;
    last = await handoff({ url, referrer });
    if (last.ok) ok += 1;
    else {
      failed += 1;
      if (last.status !== 'connected') break; // app is down: stop hammering
    }
  }
  if (failed > 0 && last && !last.ok) {
    const first = urls[0] ?? '';
    await notifyUnavailable(last, { url: first });
  }
  return { ok, failed: failed + (urls.length - ok - failed), status: host.status };
}

// ---------------------------------------------------------------------------------------------
// Notifications
// ---------------------------------------------------------------------------------------------

async function notify(id: string, title: string, message: string, buttons?: string[]): Promise<void> {
  const options: Notifications.CreateNotificationOptions & { buttons?: { title: string }[] } = {
    type: 'basic',
    iconUrl: browser.runtime.getURL('icons/icon-128.png'),
    title,
    message,
  };
  if (buttons && !IS_FIREFOX) options.buttons = buttons.map((title) => ({ title }));
  try {
    await browser.notifications.create(id, options);
  } catch {
    /* notifications blocked: nothing else to do */
  }
}

async function notifyUnavailable(result: HandoffReply, c: Candidate): Promise<void> {
  const site = hostOf(c.url) ?? '';
  const name = displayName(c);
  if (result.status === 'host_missing') {
    if (missingNotified) return;
    missingNotified = true;
    await setSession('missingNotified', true);
    await notify(NOTIFY_HOST_MISSING, tr('notify.host_missing.title'), tr('notify.host_missing.body', { name }), [
      tr('notify.button.never'),
    ]);
    return;
  }
  if (result.status === 'connected') {
    await notify(NOTIFY_FAILED, tr('notify.failed.title'), tr('notify.failed.body', { name, error: result.error ?? '' }));
    return;
  }
  await notify(NOTIFY_NOT_RUNNING + site, tr('notify.not_running.title'), tr('notify.not_running.body', { name }), [
    tr('notify.button.open'),
    tr('notify.button.never'),
  ]);
}

async function neverAskFor(site: string): Promise<void> {
  const s = normaliseSite(site);
  if (!s || settings.rules.neverAskSites.includes(s)) return;
  const next: Settings = {
    ...settings,
    rules: { ...settings.rules, neverAskSites: [...settings.rules.neverAskSites, s] },
  };
  settings = next;
  await saveSettings(next);
}

async function openApp(): Promise<HostStatus> {
  // `ping` is the only "open" there is: the host decides whether to launch the app.
  return host.ping();
}

browser.notifications.onClicked.addListener((id) => {
  if (id.startsWith(NOTIFY_NOT_RUNNING)) void openApp();
  void browser.notifications.clear(id);
});

if (browser.notifications.onButtonClicked) {
  browser.notifications.onButtonClicked.addListener((id, index) => {
    void (async () => {
      await ready;
      if (id === NOTIFY_HOST_MISSING) {
        /* only "never ask" would make sense per site, but the host is missing everywhere */
      } else if (id.startsWith(NOTIFY_NOT_RUNNING)) {
        const site = id.slice(NOTIFY_NOT_RUNNING.length);
        if (index === 0) await openApp();
        else if (index === 1 && site) await neverAskFor(site);
      }
      await browser.notifications.clear(id);
    })();
  });
}

// ---------------------------------------------------------------------------------------------
// Capture: browser downloads (Chromium `onDeterminingFilename`, Firefox `onCreated` fallback)
// ---------------------------------------------------------------------------------------------

interface BrowserDownloadItem {
  id: number;
  url: string;
  finalUrl?: string;
  referrer?: string;
  filename?: string;
  mime?: string;
  fileSize?: number;
  totalBytes?: number;
  byExtensionId?: string;
}

interface ChromeDownloadsExtras {
  onDeterminingFilename?: {
    addListener(cb: (item: BrowserDownloadItem, suggest: () => void) => void): void;
  };
}

function sizeOf(item: BrowserDownloadItem): number | undefined {
  const size = item.totalBytes !== undefined && item.totalBytes >= 0 ? item.totalBytes : item.fileSize;
  return size !== undefined && size >= 0 ? size : undefined;
}

async function discardBrowserDownload(id: number): Promise<void> {
  try {
    await browser.downloads.cancel(id);
  } catch {
    /* already finished or gone */
  }
  try {
    await browser.downloads.erase({ id });
  } catch {
    /* nothing to erase */
  }
}

async function onBrowserDownload(item: BrowserDownloadItem): Promise<void> {
  await ready;
  if (item.byExtensionId && item.byExtensionId === browser.runtime.id) return; // our own fallback
  const url = item.finalUrl || item.url;
  const candidate: Candidate = {
    url,
    filename: baseName(item.filename),
    mime: mediaType(item.mime),
    size: sizeOf(item),
    referrer: item.referrer || undefined,
    altHeld: wasBypassed(item.url) || wasBypassed(url),
  };
  const decision = decide(settings.rules, candidate, paused);
  if (!decision.take) return;
  if (host.status === 'host_missing' && missingNotified) return; // leave the browser alone

  try {
    await browser.downloads.pause(item.id);
  } catch {
    return; // finished already (tiny file) or not pausable: nothing to take over
  }
  const result = await handoff(candidate);
  if (result.ok) {
    await discardBrowserDownload(item.id);
    return;
  }
  try {
    await browser.downloads.resume(item.id);
  } catch {
    /* the browser keeps it either way */
  }
  await notifyUnavailable(result, candidate);
}

const chromeDownloads = (globalThis as { chrome?: { downloads?: ChromeDownloadsExtras } }).chrome?.downloads;
if (!IS_FIREFOX && chromeDownloads?.onDeterminingFilename) {
  chromeDownloads.onDeterminingFilename.addListener((item, suggest) => {
    suggest(); // keep the browser's filename; we cancel the item instead of renaming it
    void onBrowserDownload(item);
  });
} else {
  browser.downloads.onCreated.addListener((item: Downloads.DownloadItem) => {
    void onBrowserDownload(item);
  });
}

// ---------------------------------------------------------------------------------------------
// Capture: Firefox response headers (before the download exists)
// ---------------------------------------------------------------------------------------------

function headerMap(headers: WebRequest.HttpHeaders | undefined): Map<string, string> {
  const map = new Map<string, string>();
  for (const h of headers ?? []) {
    if (h.value !== undefined) map.set(h.name.toLowerCase(), h.value);
  }
  return map;
}

async function closeBlankTab(tabId: number): Promise<void> {
  try {
    const tab = await browser.tabs.get(tabId);
    if ((!tab.url || tab.url === 'about:blank') && !tab.pinned) await browser.tabs.remove(tabId);
  } catch {
    /* tab is gone or not ours to close */
  }
}

async function handoffFromHeaders(c: Candidate, tabId: number, isMainFrame: boolean): Promise<void> {
  const result = await handoff(c);
  if (result.ok) {
    if (isMainFrame) await closeBlankTab(tabId);
    return;
  }
  // Give it back to the browser with a fresh request through the downloads API.
  try {
    await browser.downloads.download({ url: c.url });
  } catch {
    /* the user can retry the link */
  }
  await notifyUnavailable(result, c);
}

function onHeadersReceived(details: WebRequest.OnHeadersReceivedDetailsType): WebRequest.BlockingResponse | undefined {
  if (!settingsLoaded) return undefined;
  if (details.method !== 'GET' || (details.statusCode !== 200 && details.statusCode !== 206)) return undefined;
  const h = headerMap(details.responseHeaders);
  const disposition = h.get('content-disposition');
  const attachment = isAttachmentDisposition(disposition);
  const mime = mediaType(h.get('content-type'));
  if (!attachment && !isDownloadMime(mime)) return undefined;
  if (host.status === 'host_missing' && missingNotified) return undefined;

  const length = Number.parseInt(h.get('content-length') ?? '', 10);
  const candidate: Candidate = {
    url: details.url,
    filename: filenameFromDisposition(disposition) || filenameFromUrl(details.url),
    mime,
    size: Number.isFinite(length) && length >= 0 ? length : undefined,
    referrer: details.originUrl ?? details.documentUrl,
    attachment,
    altHeld: wasBypassed(details.url),
  };
  const decision = decide(settings.rules, candidate, paused);
  if (!decision.take) return undefined;

  void handoffFromHeaders(candidate, details.tabId, details.type === 'main_frame');
  return { cancel: true };
}

if (IS_FIREFOX && browser.webRequest?.onHeadersReceived) {
  browser.webRequest.onHeadersReceived.addListener(
    onHeadersReceived,
    { urls: ['<all_urls>'], types: ['main_frame', 'sub_frame'] },
    ['blocking', 'responseHeaders'],
  );
}

// ---------------------------------------------------------------------------------------------
// Context menus
// ---------------------------------------------------------------------------------------------

async function createMenus(): Promise<void> {
  try {
    await browser.contextMenus.removeAll();
    browser.contextMenus.create({
      id: MENU_LINK,
      title: browser.i18n.getMessage('menuLink'),
      contexts: ['link', 'image', 'video', 'audio'],
    });
    browser.contextMenus.create({ id: MENU_ALL, title: browser.i18n.getMessage('menuAll'), contexts: ['page'] });
    browser.contextMenus.create({
      id: MENU_SELECTION,
      title: browser.i18n.getMessage('menuSelection'),
      contexts: ['selection'],
    });
  } catch (e) {
    console.warn('[riptide] context menus:', errorText(e));
  }
}

async function onMenuClicked(info: Menus.OnClickData, tab: Tabs.Tab | undefined): Promise<void> {
  await ready;
  if (info.menuItemId === MENU_LINK) {
    const url = info.linkUrl ?? info.srcUrl;
    if (!url || hostOf(url) === null) return;
    const candidate: Candidate = { url, referrer: info.pageUrl };
    const result = await handoff(candidate);
    if (result.ok) return;
    try {
      await browser.downloads.download({ url });
    } catch {
      /* the user can still click the link */
    }
    await notifyUnavailable(result, candidate);
    return;
  }
  if (info.menuItemId === MENU_ALL || info.menuItemId === MENU_SELECTION) {
    if (tab?.id === undefined) return;
    const message: Message = { kind: 'show-panel', selectionOnly: info.menuItemId === MENU_SELECTION };
    try {
      await browser.tabs.sendMessage(tab.id, message);
    } catch {
      await notify(NOTIFY_RELOAD, tr('notify.reload.title'), tr('notify.reload.body'));
    }
  }
}

browser.contextMenus.onClicked.addListener((info, tab) => {
  void onMenuClicked(info, tab);
});

browser.runtime.onInstalled.addListener(() => {
  void createMenus();
  void ready.then(() => host.ping()).then(() => updateBadge());
});

browser.runtime.onStartup.addListener(() => {
  void createMenus();
  void ready.then(() => host.ping()).then(() => updateBadge());
});

// ---------------------------------------------------------------------------------------------
// Internal message bus (popup, options, content script)
// ---------------------------------------------------------------------------------------------

async function refreshRecent(): Promise<RecentItem[]> {
  const recent = await getSession<RecentItem[]>('recent', []);
  if (recent.length === 0 || host.status !== 'connected') return recent;
  try {
    const res = await host.request({ type: 'list' }, 5_000);
    if (res.type === 'downloads') {
      const byId = new Map(res.downloads.map((d) => [d.id, d]));
      for (const item of recent) {
        const d = item.download_id === null ? undefined : byId.get(item.download_id);
        if (d) {
          item.state = d.state;
          item.filename = d.filename || item.filename;
        }
      }
      listAvailable = true;
      await setSession('recent', recent);
    } else if (res.type === 'error') {
      listAvailable = false; // the host forwards only ping/probe/add/refresh
    }
    await setSession('listAvailable', listAvailable);
  } catch {
    /* status already updated by the client */
  }
  return recent;
}

async function statusReply(): Promise<StatusReply> {
  const recent = await refreshRecent();
  return {
    status: host.status,
    version: host.version,
    captured: await getSession('captured', 0),
    recent,
    paused,
    rulesEnabled: settings.rules.enabled,
    listAvailable,
  };
}

async function handleMessage(message: Message, sender: Runtime.MessageSender): Promise<unknown> {
  await ready;
  switch (message.kind) {
    case 'handoff': {
      if (hostOf(message.url) === null) return { ok: false, status: host.status, error: 'not http(s)' } satisfies HandoffReply;
      const candidate: Candidate = { url: message.url, referrer: message.referrer, filename: message.filename };
      const result = await handoff(candidate);
      if (!result.ok && sender.tab) await notifyUnavailable(result, candidate);
      return result;
    }
    case 'handoff-many':
      return handoffMany(message.urls.slice(0, 500), message.referrer);
    case 'alt-bypass': {
      const now = Date.now();
      lastAltClickAt = now;
      if (message.url) bypassUrls.set(message.url, now);
      for (const [url, at] of bypassUrls) if (now - at > BYPASS_URL_MS) bypassUrls.delete(url);
      return undefined;
    }
    case 'get-status':
      if (host.status === 'unknown') await host.ping(3_000);
      return statusReply();
    case 'refresh':
      await host.ping(3_000);
      return statusReply();
    case 'open-app':
      await openApp();
      return statusReply();
    case 'set-paused':
      paused = message.paused;
      await setSession('paused', paused);
      await updateBadge();
      return statusReply();
    case 'never-ask':
      await neverAskFor(message.site);
      return undefined;
    case 'show-panel':
      return undefined; // background -> content only
  }
}

browser.runtime.onMessage.addListener((message: unknown, sender: Runtime.MessageSender) => {
  if (!isMessage(message)) return undefined;
  return handleMessage(message, sender);
});
