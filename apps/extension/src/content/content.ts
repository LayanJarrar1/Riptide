/**
 * Content script (top frame only). Three small jobs, all isolated in a shadow root so the page
 * cannot style us and we cannot shift its layout:
 *
 * 1. Alt tracking: an Alt-click on a link tells the background to let the browser keep that
 *    download ("hold Alt to bypass").
 * 2. Hover badge: hovering a direct file link shows a small "Download with Riptide" button.
 * 3. Link panel: on "Download all / selected links" the background asks us to collect the
 *    candidates and show a checklist.
 *
 * Nothing from the page is executed; only `href` strings are sent to the background.
 */
import browser from 'webextension-polyfill';

import { dirOf, resolveLang, t } from '../i18n';
import type { Lang, MessageKey } from '../i18n';
import { isMessage } from '../messages';
import type { HandoffManyReply, HandoffReply, Message } from '../messages';
import { extensionOf, hostOf, isDirectFileLink } from '../rules';
import { DEFAULT_SETTINGS, normaliseSettings } from '../settings';
import type { Settings } from '../settings';

let settings: Settings = DEFAULT_SETTINGS;
let lang: Lang = 'en';

function applySettings(raw: unknown): void {
  settings = normaliseSettings(raw);
  lang = resolveLang(settings.language, browser.i18n.getUILanguage());
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
.rt-badge svg { width: 12px; height: 12px; flex: none; }
.rt-scrim { position: fixed; inset: 0; background: rgb(0 0 0 / 45%); display: flex; align-items: center; justify-content: center; }
.rt-panel { width: min(560px, calc(100vw - 32px)); max-height: min(80vh, 720px); display: flex; flex-direction: column;
  background: #10161b; border: 1px solid rgb(255 255 255 / 10%); border-radius: 14px; box-shadow: 0 20px 60px rgb(0 0 0 / 55%); overflow: hidden; }
.rt-head { display: flex; align-items: center; gap: 10px; padding: 14px 16px; border-bottom: 1px solid rgb(255 255 255 / 8%); }
.rt-head h2 { flex: 1; margin: 0; font-size: 15px; font-weight: 600; }
.rt-close { border: 0; background: transparent; color: #a9b5bf; font: inherit; font-size: 18px; cursor: pointer; padding: 2px 6px; border-radius: 6px; }
.rt-close:hover { background: rgb(255 255 255 / 7%); color: #e9eef2; }
.rt-tools { display: flex; align-items: center; gap: 12px; padding: 10px 16px; border-bottom: 1px solid rgb(255 255 255 / 8%); flex-wrap: wrap; }
.rt-tools input[type="search"] { flex: 1; min-width: 140px; padding: 7px 10px; border-radius: 8px; border: 1px solid rgb(255 255 255 / 14%);
  background: #161e25; color: #e9eef2; font: inherit; }
.rt-tools label { display: inline-flex; align-items: center; gap: 6px; color: #a9b5bf; cursor: pointer; }
.rt-list { flex: 1; overflow: auto; padding: 6px 8px; }
.rt-item { display: flex; align-items: center; gap: 10px; padding: 7px 8px; border-radius: 8px; cursor: pointer; }
.rt-item:hover { background: rgb(255 255 255 / 4%); }
.rt-item input { flex: none; accent-color: #19c6b5; }
.rt-item .rt-name { flex: 1; min-width: 0; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
.rt-item .rt-url { display: block; color: #6d7b87; font-size: 11px; overflow: hidden; text-overflow: ellipsis; direction: ltr; unicode-bidi: isolate; text-align: start; }
.rt-item .rt-ext { flex: none; padding: 2px 7px; border-radius: 999px; background: rgb(25 198 181 / 12%); color: #5ae0d1; font-size: 11px; font-weight: 600; text-transform: uppercase; }
.rt-empty { padding: 32px; text-align: center; color: #6d7b87; }
.rt-foot { display: flex; align-items: center; gap: 10px; padding: 12px 16px; border-top: 1px solid rgb(255 255 255 / 8%); }
.rt-foot .rt-status { flex: 1; color: #a9b5bf; }
.rt-btn { padding: 8px 14px; border-radius: 8px; border: 1px solid rgb(255 255 255 / 14%); background: #161e25; color: #e9eef2; font: inherit; font-weight: 600; cursor: pointer; }
.rt-btn:hover { background: #1d272f; }
.rt-btn.rt-primary { background: #19c6b5; border-color: #19c6b5; color: #06201d; }
.rt-btn.rt-primary:hover { background: #5ae0d1; }
.rt-btn:disabled { opacity: 0.5; cursor: default; }
@media (prefers-reduced-motion: no-preference) { .rt-panel { animation: rt-in 140ms ease-out; } }
@keyframes rt-in { from { transform: translateY(6px); opacity: 0; } to { transform: none; opacity: 1; } }
`;

const RIPTIDE_ICON =
  '<svg viewBox="0 0 100 100" aria-hidden="true"><rect width="100" height="100" rx="22" fill="#06201d"/><g fill="#19c6b5"><rect x="30" y="22" width="12" height="56"/><path d="M30 22 H47 A15.5 15.5 0 0 1 47 53 H30 V43 H47 A5.5 5.5 0 0 0 47 32 H30 Z"/><path d="M38 56 L50 48 L72 72 L72 78 L60 78 Z"/></g></svg>';

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
// 3. Link panel
// ---------------------------------------------------------------------------------------------

interface LinkCandidate {
  url: string;
  text: string;
  ext: string;
  direct: boolean;
}

function collectLinks(selectionOnly: boolean): LinkCandidate[] {
  const ranges: Range[] = [];
  if (selectionOnly) {
    const sel = window.getSelection();
    for (let i = 0; i < (sel?.rangeCount ?? 0); i++) {
      const r = sel?.getRangeAt(i);
      if (r && !r.collapsed) ranges.push(r);
    }
  }
  const seen = new Map<string, LinkCandidate>();
  for (const a of document.querySelectorAll<HTMLAnchorElement>('a[href]')) {
    const url = a.href;
    if (hostOf(url) === null || seen.has(url)) continue;
    if (selectionOnly && !ranges.some((r) => r.intersectsNode(a))) continue;
    const text = (a.textContent ?? '').replace(/\s+/g, ' ').trim() || a.getAttribute('download') || '';
    seen.set(url, {
      url,
      text,
      ext: extensionOf(url),
      direct: isDirectFileLink(url, settings.rules) || a.hasAttribute('download'),
    });
  }
  return [...seen.values()];
}

let panelScrim: HTMLDivElement | null = null;

function closePanel(): void {
  panelScrim?.remove();
  panelScrim = null;
}

function showPanel(selectionOnly: boolean): void {
  closePanel();
  hideBadge();
  const links = collectLinks(selectionOnly);
  const anyDirect = links.some((l) => l.direct);

  const title = el('h2', { textContent: tr(selectionOnly ? 'panel.title.selection' : 'panel.title.all') });
  const close = el('button', { className: 'rt-close', type: 'button', textContent: '×', title: tr('panel.close') });
  const filter = el('input', { type: 'search', placeholder: tr('panel.filter') });
  const directOnly = el('input', { type: 'checkbox' });
  directOnly.checked = anyDirect;
  const selectAll = el('input', { type: 'checkbox' });
  selectAll.checked = true;
  const list = el('div', { className: 'rt-list' });
  const status = el('span', { className: 'rt-status' });
  const cancel = el('button', { className: 'rt-btn', type: 'button', textContent: tr('panel.cancel') });
  const go = el('button', { className: 'rt-btn rt-primary', type: 'button' });

  const checks = new Map<string, HTMLInputElement>();
  const rows: { link: LinkCandidate; row: HTMLLabelElement; box: HTMLInputElement }[] = [];
  for (const link of links) {
    const box = el('input', { type: 'checkbox' });
    box.checked = true;
    const name = el('span', { className: 'rt-name' }, [
      link.text || link.url,
      el('span', { className: 'rt-url', textContent: link.url }),
    ]);
    const row = el('label', { className: 'rt-item' }, [box, name]);
    if (link.ext) row.append(el('span', { className: 'rt-ext', textContent: link.ext }));
    box.addEventListener('change', updateCount);
    checks.set(link.url, box);
    rows.push({ link, row, box });
    list.append(row);
  }
  if (links.length === 0) list.append(el('div', { className: 'rt-empty', textContent: tr('panel.empty') }));

  function visible(): typeof rows {
    const q = filter.value.trim().toLowerCase();
    return rows.filter(({ link }) => {
      if (directOnly.checked && !link.direct) return false;
      return !q || link.url.toLowerCase().includes(q) || link.text.toLowerCase().includes(q);
    });
  }

  function applyFilter(): void {
    const shown = new Set(visible());
    for (const r of rows) r.row.style.display = shown.has(r) ? '' : 'none';
    updateCount();
  }

  function selected(): string[] {
    return visible()
      .filter((r) => r.box.checked)
      .map((r) => r.link.url);
  }

  function updateCount(): void {
    const n = selected().length;
    go.textContent = tr('panel.download', { n });
    go.disabled = n === 0;
  }

  filter.addEventListener('input', applyFilter);
  directOnly.addEventListener('change', applyFilter);
  selectAll.addEventListener('change', () => {
    for (const r of visible()) r.box.checked = selectAll.checked;
    updateCount();
  });
  close.addEventListener('click', closePanel);
  cancel.addEventListener('click', closePanel);
  go.addEventListener('click', () => {
    void (async () => {
      const urls = selected();
      go.disabled = true;
      cancel.disabled = true;
      const result = await send<HandoffManyReply>({ kind: 'handoff-many', urls, referrer: location.href });
      const ok = result?.ok ?? 0;
      const failed = result?.failed ?? urls.length;
      status.textContent = failed === 0 ? tr('panel.done', { n: ok }) : tr('panel.partial', { ok, failed });
      cancel.disabled = false;
      cancel.textContent = tr('panel.close');
      if (failed === 0) setTimeout(closePanel, 1_600);
    })();
  });

  const panel = el('div', { className: 'rt-panel' }, [
    el('div', { className: 'rt-head' }, [title, close]),
    el('div', { className: 'rt-tools' }, [
      filter,
      el('label', {}, [directOnly, tr('panel.direct_only')]),
      el('label', {}, [selectAll, tr('panel.select_all')]),
    ]),
    list,
    el('div', { className: 'rt-foot' }, [status, cancel, go]),
  ]);
  panel.dir = dirOf(lang);
  panel.addEventListener('click', (e) => e.stopPropagation());
  panelScrim = el('div', { className: 'rt-scrim' }, [panel]);
  panelScrim.addEventListener('click', closePanel);
  root().appendChild(panelScrim);
  applyFilter();
  filter.focus();
}

window.addEventListener('keydown', (e) => {
  if (e.key === 'Escape' && panelScrim) closePanel();
});

browser.runtime.onMessage.addListener((message: unknown) => {
  if (isMessage(message) && message.kind === 'show-panel') showPanel(message.selectionOnly);
  return undefined;
});
