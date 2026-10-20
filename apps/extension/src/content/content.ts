/**
 * Content script (top frame only). Three small jobs, all isolated in a shadow root so the page
 * cannot style us and we cannot shift its layout:
 *
 * 1. Alt tracking: an Alt-click on a link tells the background to let the browser keep that
 *    download ("hold Alt to bypass").
 * 2. Hover badge: hovering a direct file link shows a small "Download with Riptide" button.
 * 3. Link checklist (`./links.ts`): on "Download all / selected links / images" the background
 *    asks us to collect the candidates and show a checklist.
 *
 * Nothing from the page is executed; only `href` strings are sent to the background.
 */
import browser from 'webextension-polyfill';

import brandIcon from '../../brand/riptide-app-icon.svg';
import { dirOf, resolveLang, t } from '../i18n';
import type { Lang, MessageKey } from '../i18n';
import { isMessage } from '../messages';
import type { HandoffReply, Message } from '../messages';
import { hostOf, isDirectFileLink } from '../rules';
import { DEFAULT_SETTINGS, normaliseSettings } from '../settings';
import type { Settings } from '../settings';
import { initLinks } from './links';
import { initVideo } from './video';
import type { VideoUi } from './video';

let settings: Settings = DEFAULT_SETTINGS;
let lang: Lang = 'en';
let video: VideoUi | null = null;

function applySettings(raw: unknown): void {
  settings = normaliseSettings(raw);
  lang = resolveLang(settings.language, browser.i18n.getUILanguage());
  video?.refresh();
}

void browser.storage.local.get('settings').then((got) => applySettings(got['settings']));
browser.storage.onChanged.addListener((changes, area) => {
  const change = changes['settings'];
  if (area === 'local' && change) applySettings(change.newValue);
});

function tr(key: MessageKey, params?: Record<string, string | number>): string {
  return t(lang, key, params);
}

function send<T>(message: Message): Promise<T | undefined> {
  return browser.runtime.sendMessage(message).then(
    (r) => r as T | undefined,
    () => undefined,
  );
}

function anchorOf(target: EventTarget | null): HTMLAnchorElement | null {
  if (!(target instanceof Element)) return null;
  const a = target.closest('a[href]');
  return a instanceof HTMLAnchorElement ? a : null;
}

// ---------------------------------------------------------------------------------------------
// 1. Alt bypass
// ---------------------------------------------------------------------------------------------

let altDown = false;
window.addEventListener('keydown', (e) => (altDown = altDown || e.key === 'Alt'), true);
window.addEventListener('keyup', (e) => (altDown = e.key === 'Alt' ? false : altDown), true);
window.addEventListener('blur', () => (altDown = false));
window.addEventListener(
  'click',
  (e) => {
    if (!(e.altKey || altDown) || !settings.rules.altBypass) return;
    const a = anchorOf(e.target);
    void send({ kind: 'alt-bypass', url: a?.href ?? '' });
  },
  true,
);

// ---------------------------------------------------------------------------------------------
// Shadow root + styles
// ---------------------------------------------------------------------------------------------

const CSS = `
:host { all: initial; position: fixed; inset: 0 auto auto 0; width: 0; height: 0; z-index: 2147483647;
  font-family: "Inter", "IBM Plex Sans Arabic", system-ui, -apple-system, "Segoe UI", sans-serif; font-size: 13px;
  color: #e9eef2; }
* { box-sizing: border-box; }
.rt-badge { position: fixed; display: none; align-items: center; gap: 6px; padding: 4px 9px; border: 0; border-radius: 999px;
  background: #19c6b5; color: #06201d; font: inherit; font-weight: 600; line-height: 1; cursor: pointer;
  box-shadow: 0 2px 10px rgb(0 0 0 / 35%); white-space: nowrap; }
.rt-badge:hover { background: #5ae0d1; }
.rt-badge[data-state="sent"] { background: #3ddc97; }
.rt-badge[data-state="failed"] { background: #f2665f; color: #fff; }
.rt-badge svg { width: 16px; height: 16px; flex: none; }
.rt-item { display: flex; align-items: center; gap: 10px; padding: 7px 8px; border-radius: 8px; cursor: pointer; }
.rt-item:hover { background: rgb(255 255 255 / 4%); }
.rt-item input { flex: none; accent-color: #19c6b5; }
.rt-item .rt-name { flex: 1; min-width: 0; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
.rt-item .rt-url { display: block; color: #6d7b87; font-size: 11px; overflow: hidden; text-overflow: ellipsis; direction: ltr; unicode-bidi: isolate; text-align: start; }
.rt-item .rt-ext { flex: none; padding: 2px 7px; border-radius: 999px; background: rgb(25 198 181 / 12%); color: #5ae0d1; font-size: 11px; font-weight: 600; text-transform: uppercase; }
`;

// The brand-kit app icon, unchanged (bundled as text by esbuild); only the XML prolog is dropped.
const RIPTIDE_ICON = brandIcon.replace(/^<\?xml[^>]*\?>\s*/, '').replace('<svg ', '<svg aria-hidden="true" ');

let shadow: ShadowRoot | null = null;

function root(): ShadowRoot {
  if (shadow) return shadow;
  const host = document.createElement('riptide-ui');
  host.setAttribute('data-riptide', '');
  shadow = host.attachShadow({ mode: 'closed' });
  const style = document.createElement('style');
  style.textContent = CSS;
  shadow.appendChild(style);
  (document.documentElement ?? document.body).appendChild(host);
  return shadow;
}

function el<K extends keyof HTMLElementTagNameMap>(
  tag: K,
  props: Partial<Record<'className' | 'textContent' | 'title' | 'type' | 'placeholder', string>> = {},
  children: (Node | string)[] = [],
): HTMLElementTagNameMap[K] {
  const node = document.createElement(tag);
  if (props.className) node.className = props.className;
  if (props.textContent !== undefined) node.textContent = props.textContent;
  if (props.title) node.title = props.title;
  if (props.type && node instanceof HTMLInputElement) node.type = props.type;
  if (props.placeholder && node instanceof HTMLInputElement) node.placeholder = props.placeholder;
  for (const c of children) node.append(c);
  return node;
}

// ---------------------------------------------------------------------------------------------
// 2. Hover badge
// ---------------------------------------------------------------------------------------------

let badge: HTMLButtonElement | null = null;
let badgeUrl = '';
let hideTimer: ReturnType<typeof setTimeout> | null = null;
let badgeBusy = false;

function ensureBadge(): HTMLButtonElement {
  if (badge) return badge;
  badge = el('button', { className: 'rt-badge', type: 'button' });
  badge.innerHTML = RIPTIDE_ICON;
  badge.append(el('span', { textContent: tr('content.badge') }));
  badge.addEventListener('mouseenter', cancelHide);
  badge.addEventListener('mouseleave', scheduleHide);
  badge.addEventListener('click', (e) => {
    e.preventDefault();
    e.stopPropagation();
    void badgeClicked();
  });
  root().appendChild(badge);
  return badge;
}

async function badgeClicked(): Promise<void> {
  if (!badge || badgeBusy || !badgeUrl) return;
  badgeBusy = true;
  const label = badge.querySelector('span');
  const result = await send<HandoffReply>({ kind: 'handoff', url: badgeUrl, referrer: location.href });
  const ok = result?.ok === true;
  badge.dataset['state'] = ok ? 'sent' : 'failed';
  if (label) label.textContent = ok ? tr('content.sent') : tr('content.failed');
  setTimeout(() => {
    badgeBusy = false;
    hideBadge();
  }, 1_400);
}

function showBadgeFor(a: HTMLAnchorElement): void {
  const b = ensureBadge();
  if (badgeBusy) return;
  cancelHide();
  badgeUrl = a.href;
  delete b.dataset['state'];
  const label = b.querySelector('span');
  if (label) label.textContent = tr('content.badge');
  b.dir = dirOf(lang);
  const rect = a.getBoundingClientRect();
  b.style.display = 'inline-flex';
  const width = b.offsetWidth;
  const height = b.offsetHeight;
  const rtl = dirOf(lang) === 'rtl';
  let left = rtl ? rect.left : rect.right - width;
  let top = rect.top - height - 4;
  if (top < 4) top = rect.bottom + 4;
  left = Math.max(4, Math.min(left, window.innerWidth - width - 4));
  b.style.left = `${left}px`;
  b.style.top = `${top}px`;
}

function hideBadge(): void {
  if (badge && !badgeBusy) badge.style.display = 'none';
  badgeUrl = '';
}

function scheduleHide(): void {
  cancelHide();
  hideTimer = setTimeout(hideBadge, 250);
}

function cancelHide(): void {
  if (hideTimer) {
    clearTimeout(hideTimer);
    hideTimer = null;
  }
}

document.addEventListener(
  'mouseover',
  (e) => {
    if (!settings.hoverBadge || !settings.rules.enabled) return;
    const a = anchorOf(e.target);
    if (!a) return;
    if (!(isDirectFileLink(a.href, settings.rules) || (a.hasAttribute('download') && hostOf(a.href) !== null))) return;
    showBadgeFor(a);
  },
  true,
);
document.addEventListener(
  'mouseout',
  (e) => {
    if (anchorOf(e.target)) scheduleHide();
  },
  true,
);
window.addEventListener('scroll', hideBadge, { passive: true, capture: true });

// ---------------------------------------------------------------------------------------------
// 3. Link checklist (all links / selected links / images) lives in ./links.ts
// ---------------------------------------------------------------------------------------------

const links = initLinks({ root, lang: () => lang, tr, send, hideBadge });

video = initVideo({
  root,
  settings: () => settings,
  lang: () => lang,
  tr,
  send,
});

browser.runtime.onMessage.addListener((message: unknown) => {
  if (!isMessage(message)) return undefined;
  if (message.kind === 'show-panel') links.show(message.mode);
  else if (message.kind === 'show-video') video?.show(message.url);
  else if (message.kind === 'media-hint') video?.hint();
  return undefined;
});
