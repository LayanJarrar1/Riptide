/**
 * Background script (service worker on Chromium, event page on Firefox).
 *
 * Captures browser downloads that match the rules, hands them to the Riptide app through the
 * Native Messaging host, and degrades to the browser's own download on any failure. Also owns
 * the context menus, the toolbar badge, notifications and the internal message bus.
 *
 * The browser can only ask: nothing that comes from a page is executed or used as a path. The only things
 * sent to the host are URLs, a suggested filename, the referrer, the user agent and the cookies
 * of that URL, inside the `add` schema.
 */
import browser from 'webextension-polyfill';
import type { Downloads, Menus, Notifications, Runtime, Tabs, WebRequest } from 'webextension-polyfill';

import { BypassTracker } from './bypass';
import { HostClient } from './host';
import { resolveLang, t } from './i18n';
import type { Lang, MessageKey } from './i18n';
import { isMessage } from './messages';
import type {
  HandoffManyReply,
  HandoffReply,
  MediaProbeReply,
  MediaQueryReply,
  Message,
  PanelMode,
  RecentItem,
  StatusReply,
} from './messages';
import { MAX_BATCH, mapLimit, mayCarryCookies, sanitiseBatch } from './links';
import { MEDIA_HOSTS, classifyMediaResponse, cookieOriginOf, isMediaKey, withSite } from './media';
import type { DownloadId, DownloadRequest, RequestBody } from './protocol';
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
/** Parallel `add` requests while sending a checklist (the host has no batch message yet). */
const BATCH_CONCURRENCY = 4;
const COLOR_CONNECTED = '#19C6B5';
const COLOR_OFF = '#6D7B87';
const COLOR_BADGE_TEXT = '#06201D';
const COLOR_VIDEO = '#00D5E8';
const MAX_SEEN_PER_TAB = 50;
const PROBE_TIMEOUT_MS = 90_000;

const MENU_LINK = 'riptide-link';
const MENU_ALL = 'riptide-all';
const MENU_SELECTION = 'riptide-selection';
const MENU_IMAGES = 'riptide-images';
const MENU_VIDEO_PAGE = 'riptide-video-page';
const MENU_VIDEO_LINK = 'riptide-video-link';

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
const bypass = new BypassTracker();

/** What each tab has shown us: videos in the DOM (content script) and media on the wire (Firefox). */
interface TabMedia {
  dom: number;
  seen: Set<string>;
}
const mediaTabs = new Map<number, TabMedia>();

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
  return bypass.was(url, Date.now());
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

async function recordSent(c: Candidate, downloadId: DownloadId, quiet = false): Promise<void> {
  const captured = (await getSession('captured', 0)) + 1;
  const recent = await getSession<RecentItem[]>('recent', []);
  recent.unshift({ download_id: downloadId, url: c.url, filename: displayName(c), at: Date.now(), state: null });
  await setSession('captured', captured);
  await setSession('recent', recent.slice(0, MAX_RECENT));
  await updateBadge();
  if (settings.notify && !quiet) {
    await notify(NOTIFY_SENT, tr('notify.sent.title'), tr('notify.sent.body', { name: displayName(c) }));
  }
}

/** Options for one hand-off. */
interface HandoffOptions {
  /** Do not show the per-download notification (batches notify once). */
  quiet?: boolean;
  /** Attach the browser's cookies for the URL. Default true. */
  cookies?: boolean;
}

/** Send `add` for a candidate. Never throws; the caller decides what to do on failure. */
async function handoff(c: Candidate, opts: HandoffOptions = {}): Promise<HandoffReply> {
  const request: DownloadRequest = { url: c.url, user_agent: navigator.userAgent };
  if (c.filename) request.filename = c.filename;
  if (c.referrer && hostOf(c.referrer) !== null) request.referrer = c.referrer;
  const cookies = opts.cookies === false ? null : await cookieHeaderFor(c.url);
  if (cookies) request.secrets = { cookies };

  try {
    const res = await host.request({ type: 'add', request });
    if (res.type === 'added') {
      await recordSent(c, res.download_id, opts.quiet);
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

/**
 * Send a checklist. The host has no batch message, so this is one `add` per URL with a small
 * concurrency cap. Cookies go only with URLs on the page's own origin (`referrer` is the page).
 * Stops early when the app turns out to be down.
 */
async function handoffMany(rawUrls: string[], referrer: string | undefined): Promise<HandoffManyReply> {
  const urls = sanitiseBatch(rawUrls.slice(0, MAX_BATCH));
  const page = referrer && hostOf(referrer) !== null ? referrer : undefined;
  let ok = 0;
  let down: HandoffReply | null = null;
  let lastError: HandoffReply | null = null;
  const results = await mapLimit(
    urls,
    BATCH_CONCURRENCY,
    (url) => handoff({ url, referrer: page }, { quiet: true, cookies: mayCarryCookies(url, page) }),
    () => down !== null,
  );
  for (const r of results) {
    if (!r) continue;
    if (r.ok) ok += 1;
    else {
      lastError = r;
      if (r.status !== 'connected') down = r;
    }
  }
  if (ok > 0 && settings.notify) {
    await notify(NOTIFY_SENT, tr('notify.sent.title'), tr('notify.batch.body', { n: ok }));
  }
  const failure = down ?? lastError;
  if (failure && ok < urls.length) await notifyUnavailable(failure, { url: urls[0] ?? '' });
  return { ok, failed: Math.max(0, rawUrls.length - ok), status: host.status };
}

// ---------------------------------------------------------------------------------------------
// Video pages: detection badge, quality menu, media add
// ---------------------------------------------------------------------------------------------

function tabMedia(tabId: number): TabMedia {
  let m = mediaTabs.get(tabId);
  if (!m) {
    m = { dom: 0, seen: new Set() };
    mediaTabs.set(tabId, m);
  }
  return m;
}

/** Badge counter for a tab: videos found there; falls back to the global badge at zero. */
async function updateTabBadge(tabId: number): Promise<void> {
  const m = mediaTabs.get(tabId);
  const count = m ? Math.max(m.dom, m.seen.size) : 0;
  try {
    if (count > 0) {
      await browser.action.setBadgeText({ tabId, text: String(Math.min(count, 99)) });
      await browser.action.setBadgeBackgroundColor({ tabId, color: COLOR_VIDEO });
      if (typeof browser.action.setBadgeTextColor === 'function') {
        await browser.action.setBadgeTextColor({ tabId, color: COLOR_BADGE_TEXT });
      }
    } else {
      const captured = await getSession('captured', 0);
      await browser.action.setBadgeText({ tabId, text: captured > 0 ? String(captured) : '' });
      await browser.action.setBadgeBackgroundColor({
        tabId,
        color: host.status === 'connected' ? COLOR_CONNECTED : COLOR_OFF,
      });
    }
  } catch {
    /* the tab is gone or badge APIs are unavailable */
  }
}

/** The network showed a media response on a tab (Firefox `webRequest`; observation only). */
function onMediaResponse(details: WebRequest.OnResponseStartedDetailsType): void {
  if (details.tabId < 0 || details.statusCode < 200 || details.statusCode >= 300) return;
  let contentType: string | undefined;
  for (const h of details.responseHeaders ?? []) {
    if (h.name.toLowerCase() === 'content-type') contentType = h.value;
  }
  if (!classifyMediaResponse(details.url, contentType)) return;
  const m = tabMedia(details.tabId);
  if (m.seen.size >= MAX_SEEN_PER_TAB) return;
  const before = m.seen.size;
  m.seen.add(details.url.split('#')[0] ?? details.url);
  if (m.seen.size === before) return;
  void updateTabBadge(details.tabId);
  const hint: Message = { kind: 'media-hint' };
  void browser.tabs.sendMessage(details.tabId, hint).catch(() => undefined);
}

if (IS_FIREFOX && browser.webRequest?.onResponseStarted) {
  browser.webRequest.onResponseStarted.addListener(
    onMediaResponse,
    { urls: ['<all_urls>'], types: ['media', 'xmlhttprequest', 'other'] },
    ['responseHeaders'],
  );
}

browser.tabs.onRemoved.addListener((tabId) => {
  mediaTabs.delete(tabId);
});
browser.tabs.onUpdated.addListener((tabId, change) => {
  if (change.status === 'loading' && change.url !== undefined) mediaTabs.delete(tabId); // new page
});

function answerText(res: { type: string; message?: string }): string {
  return res.message ?? `unexpected answer: ${res.type}`;
}

/** Ask the app what a video page offers. Cookies of that page's origin only. */
async function probeMedia(url: string): Promise<MediaProbeReply> {
  const origin = cookieOriginOf(url);
  if (origin === null) return { ok: false, status: host.status, error: 'not http(s)' };
  const cookies = await cookieHeaderFor(url);
  const body: RequestBody = { type: 'probe_media', url };
  if (cookies) {
    body.cookies = cookies;
    body.cookie_origin = origin;
  }
  try {
    const res = await host.request(body, PROBE_TIMEOUT_MS);
    if (res.type === 'media_probed') return { ok: true, status: host.status, info: res.info };
    if (res.type === 'error') {
      const status = await host.ping();
      return { ok: false, status, error: answerText(res) };
    }
    return { ok: false, status: host.status, error: answerText(res) };
  } catch (e) {
    return { ok: false, status: host.status === 'connected' ? 'app_not_running' : host.status, error: errorText(e) };
  }
}

/** Add a video page with a quality key; cookies only for the page's own origin. */
async function addMedia(url: string, quality: string, title: string | undefined): Promise<HandoffReply> {
  const origin = cookieOriginOf(url);
  if (origin === null || !isMediaKey(quality)) return { ok: false, status: host.status, error: 'invalid request' };
  const request: DownloadRequest = {
    url,
    kind: 'media',
    media_format: quality,
    user_agent: navigator.userAgent,
  };
  const cookies = await cookieHeaderFor(url);
  if (cookies) {
    request.secrets = { cookies };
    request.cookie_origin = origin;
  }
  const c: Candidate = { url, filename: title };
  try {
    const res = await host.request({ type: 'add', request });
    if (res.type === 'added') {
      await recordSent(c, res.download_id);
      return { ok: true, status: host.status, download_id: res.download_id };
    }
    if (res.type === 'error') {
      const status = await host.ping();
      return { ok: false, status, error: res.message };
    }
    return { ok: false, status: host.status, error: answerText(res) };
  } catch (e) {
    return { ok: false, status: host.status === 'connected' ? 'app_not_running' : host.status, error: errorText(e) };
  }
}

async function neverShowVideoOn(site: string): Promise<void> {
  const s = normaliseSite(site);
  if (!s) return;
  const next: Settings = { ...settings, videoNeverSites: withSite(settings.videoNeverSites, s) };
  settings = next;
  await saveSettings(next);
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
    browser.contextMenus.create({ id: MENU_IMAGES, title: browser.i18n.getMessage('menuImages'), contexts: ['page'] });
    browser.contextMenus.create({ id: MENU_ALL, title: browser.i18n.getMessage('menuAll'), contexts: ['page'] });
    const patterns = MEDIA_HOSTS.flatMap((h) => [`*://${h}/*`, `*://*.${h}/*`]);
    browser.contextMenus.create({
      id: MENU_VIDEO_PAGE,
      title: browser.i18n.getMessage('menuVideo'),
      contexts: ['page', 'video'],
      documentUrlPatterns: patterns,
    });
    browser.contextMenus.create({
      id: MENU_VIDEO_LINK,
      title: browser.i18n.getMessage('menuVideo'),
      contexts: ['link'],
      targetUrlPatterns: patterns,
    });
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
  if (info.menuItemId === MENU_VIDEO_PAGE || info.menuItemId === MENU_VIDEO_LINK) {
    const url = info.menuItemId === MENU_VIDEO_LINK ? info.linkUrl : (info.pageUrl ?? tab?.url);
    if (!url || hostOf(url) === null || tab?.id === undefined) return;
    const message: Message = { kind: 'show-video', url };
    try {
      await browser.tabs.sendMessage(tab.id, message);
    } catch {
      await notify(NOTIFY_RELOAD, tr('notify.reload.title'), tr('notify.reload.body'));
    }
    return;
  }
  if (info.menuItemId === MENU_ALL || info.menuItemId === MENU_SELECTION || info.menuItemId === MENU_IMAGES) {
    if (tab?.id === undefined) return;
    const mode: PanelMode =
      info.menuItemId === MENU_SELECTION ? 'selection' : info.menuItemId === MENU_IMAGES ? 'images' : 'all';
    const message: Message = { kind: 'show-panel', mode };
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
      listAvailable = false; // an older host forwarded only ping/probe/add/refresh
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
      return handoffMany(message.urls, message.referrer);
    case 'alt-bypass': {
      bypass.mark(message.url, Date.now());
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
    case 'media-found': {
      const tabId = sender.tab?.id;
      if (tabId === undefined) return undefined;
      tabMedia(tabId).dom = Math.max(0, Math.min(Math.trunc(message.count), 99));
      await updateTabBadge(tabId);
      return undefined;
    }
    case 'media-query': {
      const tabId = sender.tab?.id;
      return { seen: tabId === undefined ? 0 : (mediaTabs.get(tabId)?.seen.size ?? 0) } satisfies MediaQueryReply;
    }
    case 'media-probe':
      return probeMedia(message.url);
    case 'media-add':
      return addMedia(message.url, message.quality, message.title?.slice(0, 200));
    case 'video-never':
      await neverShowVideoOn(message.site);
      return undefined;
    case 'show-panel':
    case 'show-video':
    case 'media-hint':
      return undefined; // background -> content only
  }
}

browser.runtime.onMessage.addListener((message: unknown, sender: Runtime.MessageSender) => {
  if (!isMessage(message)) return undefined;
  return handleMessage(message, sender);
});
