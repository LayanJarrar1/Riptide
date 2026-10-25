/**
 * The floating "Download this video" button and its quality popover (content script, top frame).
 *
 * Everything lives in the content script's closed shadow root, so the page cannot style or read
 * it. The page is only read for `<video>` element sizes and the current `src`; nothing from it is
 * executed. Quality menus come from the app through the background script (`media-probe`), and a
 * choice goes back as `media-add` with a fixed quality key.
 */
import { dirOf, formatBytes } from '../i18n';
import type { Lang, MessageKey } from '../i18n';
import { BEST_ROW, buildMenu, isSubstantialVideo, isVideoPageUrl, shouldPrefetch } from '../media';
import { parseMediaError } from '../media-error';
import type { MenuRow } from '../media';
import type { HandoffReply, MediaProbeReply, Message } from '../messages';
import { hostOf, listMatches } from '../rules';
import type { Settings } from '../settings';

/** What the video UI needs from the content script it lives in. */
export interface VideoContext {
  root(): ShadowRoot;
  settings(): Settings;
  lang(): Lang;
  tr(key: MessageKey, params?: Record<string, string | number>): string;
  send<T>(message: Message): Promise<T | undefined>;
}

/** Controls handed back to the content script. */
export interface VideoUi {
  /** Open the quality popover for `url` (context menu). */
  show(url: string): void;
  /** The network saw media on this tab. */
  hint(): void;
  /** Settings or language changed. */
  refresh(): void;
}

const CSS = `
.rt-v { position: fixed; display: none; align-items: center; gap: 2px; direction: inherit; z-index: 1; }
.rt-v[data-on] { display: flex; }
.rt-vbtn, .rt-vx { border: 0; font: inherit; cursor: pointer; color: #06201d; background: #19c6b5;
  box-shadow: 0 2px 10px rgb(0 0 0 / 40%); }
.rt-vbtn { display: inline-flex; align-items: center; gap: 6px; height: 30px; padding: 0 11px 0 8px; border-radius: 999px 0 0 999px;
  font-weight: 600; line-height: 1; white-space: nowrap; }
.rt-v[dir="rtl"] .rt-vbtn { padding: 0 8px 0 11px; border-radius: 0 999px 999px 0; }
.rt-vx { width: 22px; height: 30px; padding: 0; border-radius: 0 999px 999px 0; background: #14a496; font-size: 14px; line-height: 1; }
.rt-v[dir="rtl"] .rt-vx { border-radius: 999px 0 0 999px; }
.rt-vbtn:hover, .rt-vx:hover { background: #00d5e8; }
.rt-vbtn:focus-visible, .rt-vx:focus-visible, .rt-item:focus-visible, .rt-link:focus-visible, .rt-pclose:focus-visible {
  outline: 2px solid #00d5e8; outline-offset: 2px; }
.rt-vbtn svg { width: 16px; height: 16px; flex: none; }
.rt-v[data-compact] .rt-vlabel { display: none; }
.rt-v[data-compact] .rt-vbtn { padding: 0 8px; }
.rt-v[data-compact] .rt-vx { display: none; }
.rt-v[data-compact]:hover .rt-vlabel, .rt-v[data-compact]:focus-within .rt-vlabel { display: inline; }
.rt-v[data-compact]:hover .rt-vx, .rt-v[data-compact]:focus-within .rt-vx { display: block; }
.rt-v[data-state="sent"] .rt-vbtn { background: #3ddc97; }
.rt-v[data-state="failed"] .rt-vbtn { background: #f2665f; color: #fff; }
.rt-pop { position: fixed; z-index: 2; width: min(320px, calc(100vw - 16px)); max-height: min(70vh, 460px); display: none; flex-direction: column;
  background: #10161b; color: #e9eef2; border: 1px solid rgb(255 255 255 / 10%); border-radius: 12px;
  box-shadow: 0 16px 48px rgb(0 0 0 / 55%); overflow: hidden; font-size: 13px; }
.rt-pop[data-on] { display: flex; }
.rt-phead { display: flex; align-items: center; gap: 8px; padding: 10px 12px; border-bottom: 1px solid rgb(255 255 255 / 8%); }
.rt-ptitle { flex: 1; min-width: 0; font-weight: 600; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
.rt-pclose { border: 0; background: transparent; color: #a9b5bf; font: inherit; font-size: 18px; line-height: 1; cursor: pointer; padding: 2px 6px; border-radius: 6px; }
.rt-pclose:hover { background: rgb(255 255 255 / 7%); color: #e9eef2; }
.rt-pbody { overflow: auto; padding: 6px; }
.rt-pmsg { padding: 14px 10px; color: #c7d1d9; line-height: 1.5; }
.rt-pmsg.rt-err { color: #ffb4af; }
.rt-details { display: block; margin-top: 6px; padding: 6px 8px; border-radius: 6px; background: rgb(255 255 255 / 6%); color: #a9b5bf;
  font-size: 11px; line-height: 1.4; overflow-wrap: anywhere; user-select: text; direction: ltr; text-align: left; }
.rt-details[hidden] { display: none; }
.rt-h { padding: 8px 10px 4px; color: #6d7b87; font-size: 11px; font-weight: 600; text-transform: uppercase; letter-spacing: 0.04em; }
.rt-item { display: flex; align-items: center; justify-content: space-between; gap: 12px; width: 100%; padding: 8px 10px; border: 0;
  border-radius: 8px; background: transparent; color: inherit; font: inherit; text-align: start; cursor: pointer; }
.rt-item:hover, .rt-item:focus-visible { background: rgb(25 198 181 / 14%); }
.rt-size { color: #8fa0ad; font-variant-numeric: tabular-nums; white-space: nowrap; }
.rt-pfoot { display: flex; flex-wrap: wrap; gap: 4px 14px; padding: 8px 12px; border-top: 1px solid rgb(255 255 255 / 8%); }
.rt-link { border: 0; background: transparent; padding: 2px 0; color: #5ae0d1; font: inherit; font-size: 12px; cursor: pointer; text-decoration: underline; }
.rt-link:hover { color: #00d5e8; }
@media (prefers-reduced-motion: no-preference) { .rt-pop[data-on] { animation: rt-pop-in 120ms ease-out; } }
@keyframes rt-pop-in { from { transform: translateY(4px); opacity: 0; } to { transform: none; opacity: 1; } }
`;

const ICON =
  '<svg viewBox="0 0 24 24" aria-hidden="true" fill="none" stroke="currentColor" stroke-width="2.4" stroke-linecap="round" stroke-linejoin="round"><path d="M12 4v11"/><path d="m7 11 5 5 5-5"/><path d="M5 20h14"/></svg>';

type Attrs = Partial<Record<'className' | 'textContent' | 'title' | 'type', string>>;

function h<K extends keyof HTMLElementTagNameMap>(tag: K, attrs: Attrs = {}, children: (Node | string)[] = []): HTMLElementTagNameMap[K] {
  const node = document.createElement(tag);
  if (attrs.className) node.className = attrs.className;
  if (attrs.textContent !== undefined) node.textContent = attrs.textContent;
  if (attrs.title) node.title = attrs.title;
  if (attrs.type && node instanceof HTMLButtonElement) node.type = attrs.type === 'submit' ? 'submit' : 'button';
  for (const c of children) node.append(c);
  return node;
}

/** Install the video UI. Cheap until a video page or a `<video>` shows up. */
export function initVideo(ctx: VideoContext): VideoUi {
  let dismissed = false;
  let hinted = false;
  let reported = -1;
  let busy = false;
  let popUrl = '';
  let wrap: HTMLDivElement | null = null;
  let main: HTMLButtonElement | null = null;
  let label: HTMLSpanElement | null = null;
  let pop: HTMLDivElement | null = null;
  let popTitle: HTMLDivElement | null = null;
  let popBody: HTMLDivElement | null = null;
  let popFoot: HTMLDivElement | null = null;
  let compactTimer: ReturnType<typeof setTimeout> | null = null;
  let lastHref = location.href;
  let prefetched = '';

  const tr = ctx.tr;

  function bigVideos(): HTMLVideoElement[] {
    const found: HTMLVideoElement[] = [];
    for (const v of document.querySelectorAll('video')) {
      const r = v.getBoundingClientRect();
      if (isSubstantialVideo(r.width, r.height)) found.push(v);
    }
    return found;
  }

  function mainVideo(): HTMLVideoElement | null {
    let best: HTMLVideoElement | null = null;
    let bestArea = 0;
    for (const v of bigVideos()) {
      const r = v.getBoundingClientRect();
      const visibleW = Math.min(r.right, innerWidth) - Math.max(r.left, 0);
      const visibleH = Math.min(r.bottom, innerHeight) - Math.max(r.top, 0);
      const area = Math.max(0, visibleW) * Math.max(0, visibleH);
      if (area > bestArea) {
        best = v;
        bestArea = area;
      }
    }
    return best;
  }

  function directSource(): string {
    const v = mainVideo() ?? bigVideos()[0];
    const src = v?.currentSrc || v?.src || '';
    return hostOf(src) === null ? '' : src;
  }

  function allowed(): boolean {
    const s = ctx.settings();
    return s.videoButton && !listMatches(s.videoNeverSites, location.hostname) && !dismissed;
  }

  // --- DOM -------------------------------------------------------------------------------

  function build(): void {
    if (wrap) return;
    const root = ctx.root();
    const style = document.createElement('style');
    style.textContent = CSS;
    root.appendChild(style);

    main = h('button', { className: 'rt-vbtn', type: 'button' });
    main.innerHTML = ICON;
    label = h('span', { className: 'rt-vlabel' });
    main.append(label);
    main.setAttribute('aria-haspopup', 'dialog');
    main.addEventListener('click', (e) => {
      e.preventDefault();
      e.stopPropagation();
      if (pop?.hasAttribute('data-on')) closePop();
      else openPop(location.href);
    });
    const x = h('button', { className: 'rt-vx', type: 'button', textContent: '×' });
    x.dataset['role'] = 'dismiss';
    x.addEventListener('click', (e) => {
      e.preventDefault();
      e.stopPropagation();
      dismissed = true;
      closePop();
      evaluate();
    });
    wrap = h('div', { className: 'rt-v' }, [main, x]);
    wrap.setAttribute('role', 'group');
    wrap.addEventListener('mouseenter', () => wrap?.removeAttribute('data-compact'));
    wrap.addEventListener('mouseleave', scheduleCompact);

    popTitle = h('div', { className: 'rt-ptitle' });
    const close = h('button', { className: 'rt-pclose', type: 'button', textContent: '×' });
    close.addEventListener('click', () => {
      closePop();
      main?.focus();
    });
    popBody = h('div', { className: 'rt-pbody' });
    popFoot = h('div', { className: 'rt-pfoot' });
    pop = h('div', { className: 'rt-pop' }, [h('div', { className: 'rt-phead' }, [popTitle, close]), popBody, popFoot]);
    pop.setAttribute('role', 'dialog');
    pop.addEventListener('click', (e) => e.stopPropagation());
    pop.addEventListener('keydown', onPopKey);

    // keystrokes typed in our UI must not trigger the page's shortcuts (YouTube's `f`, `k`, ...)
    for (const node of [wrap, pop]) {
      for (const type of ['keydown', 'keyup', 'keypress'] as const) {
        node.addEventListener(type, (e) => e.stopPropagation());
      }
    }
    root.append(wrap, pop);
    localise();
  }

  function localise(): void {
    if (!wrap || !main || !label || !pop || !popFoot) return;
    const dir = dirOf(ctx.lang());
    wrap.dir = dir;
    pop.dir = dir;
    label.textContent = tr('video.button');
    main.setAttribute('aria-label', tr('video.button.label'));
    main.title = tr('video.button.label');
    const x = wrap.querySelector<HTMLButtonElement>('[data-role="dismiss"]');
    if (x) {
      x.title = tr('video.dismiss');
      x.setAttribute('aria-label', tr('video.dismiss'));
    }
    pop.setAttribute('aria-label', tr('video.button.label'));
    pop.querySelector('.rt-pclose')?.setAttribute('aria-label', tr('video.close'));
  }

  function scheduleCompact(): void {
    if (compactTimer) clearTimeout(compactTimer);
    compactTimer = setTimeout(() => {
      if (wrap && !wrap.matches(':focus-within') && !pop?.hasAttribute('data-on')) wrap.setAttribute('data-compact', '');
    }, 4_000);
  }

  function place(): void {
    if (!wrap || !wrap.hasAttribute('data-on')) return;
    const rtl = dirOf(ctx.lang()) === 'rtl';
    const w = wrap.offsetWidth;
    const hgt = wrap.offsetHeight;
    const video = mainVideo();
    let left: number;
    let top: number;
    if (video) {
      const r = video.getBoundingClientRect();
      left = rtl ? r.left + 10 : r.right - w - 10;
      top = Math.max(r.top, 0) + 10;
    } else {
      left = rtl ? 16 : innerWidth - w - 16;
      top = innerHeight - hgt - 16;
    }
    left = Math.max(4, Math.min(left, innerWidth - w - 4));
    top = Math.max(4, Math.min(top, innerHeight - hgt - 4));
    wrap.style.left = `${left}px`;
    wrap.style.top = `${top}px`;
    if (pop?.hasAttribute('data-on')) placePop(left, top, w, hgt);
  }

  function placePop(left: number, top: number, w: number, hgt: number): void {
    if (!pop) return;
    const pw = pop.offsetWidth;
    const ph = pop.offsetHeight;
    const rtl = dirOf(ctx.lang()) === 'rtl';
    let x = rtl ? left : left + w - pw;
    let y = top + hgt + 6;
    if (y + ph > innerHeight - 4) y = Math.max(4, top - ph - 6);
    x = Math.max(4, Math.min(x, innerWidth - pw - 4));
    pop.style.left = `${x}px`;
    pop.style.top = `${y}px`;
  }

  // --- popover ---------------------------------------------------------------------------

  function setMessage(text: string, error = false): void {
    if (!popBody) return;
    popBody.replaceChildren(h('div', { className: error ? 'rt-pmsg rt-err' : 'rt-pmsg', textContent: text }));
    popBody.setAttribute('role', error ? 'alert' : 'status');
  }

  function footer(withRetry: boolean): void {
    if (!popFoot) return;
    popFoot.replaceChildren();
    if (withRetry) {
      const retry = h('button', { className: 'rt-link', type: 'button', textContent: tr('video.retry') });
      retry.addEventListener('click', () => openPop(popUrl));
      popFoot.append(retry);
    }
    const never = h('button', { className: 'rt-link', type: 'button', textContent: tr('video.never') });
    never.addEventListener('click', () => {
      void ctx.send({ kind: 'video-never', site: location.hostname });
      closePop();
    });
    popFoot.append(never);
  }

  function errorText(r: { status?: string; error?: string }): string {
    if (r.status === 'host_missing') return tr('video.err.host_missing');
    if (r.status === 'connected' && r.error) {
      const { kind, details } = parseMediaError(r.error);
      if (kind === 'login') return tr('video.err.login');
      if (kind === 'refused') return tr('video.err.refused');
      return details || tr('video.err.unknown'); // the app's own, specific message
    }
    if (r.status === 'app_not_running' || r.status === 'unknown') return tr('video.err.not_running');
    return r.error || tr('video.err.unknown');
  }

  /** yt-dlp's own line for a tagged refusal, shown only after "Details" is pressed. */
  function errorDetails(r: { status?: string; error?: string }): string {
    if (r.status !== 'connected' || !r.error) return '';
    const { kind, details } = parseMediaError(r.error);
    return kind === null ? '' : details;
  }

  function rowLabel(row: MenuRow): string {
    if (row.kind === 'audio') return tr('video.audio', { ext: row.ext.toUpperCase() });
    if (row.kind === 'best') {
      const best = tr('video.best');
      return row.height ? `${best} · ${tr('video.height', { h: row.height })}` : best;
    }
    return row.height ? tr('video.height', { h: row.height }) : row.key;
  }

  function itemButton(row: MenuRow, title: string): HTMLButtonElement {
    const b = h('button', { className: 'rt-item', type: 'button' }, [h('span', { textContent: rowLabel(row) })]);
    b.setAttribute('role', 'menuitem');
    if (row.approxSize) {
      b.append(h('span', { className: 'rt-size', textContent: tr('video.size', { size: formatBytes(ctx.lang(), row.approxSize) }) }));
    }
    b.addEventListener('click', () => void choose(row.key, title));
    return b;
  }

  function renderMenu(reply: MediaProbeReply): void {
    if (!popBody || !popTitle) return;
    const info = reply.info;
    if (!info) return;
    popTitle.textContent = info.title;
    popTitle.title = info.title;
    if (info.is_live) {
      setMessage(tr('video.live'), true);
      footer(false);
      return;
    }
    const menu = buildMenu(info.choices);
    if (menu.video.length === 0 && menu.audio.length === 0) {
      setMessage(info.drm ? tr('video.drm') : tr('video.empty'), true);
      footer(true);
      return;
    }
    popBody.replaceChildren();
    popBody.removeAttribute('role');
    const group = h('div');
    group.setAttribute('role', 'menu');
    if (menu.video.length) {
      group.append(h('div', { className: 'rt-h', textContent: tr('video.heading.video') }));
      for (const row of menu.video) group.append(itemButton(row, info.title));
    }
    if (menu.audio.length) {
      group.append(h('div', { className: 'rt-h', textContent: tr('video.heading.audio') }));
      for (const row of menu.audio) group.append(itemButton(row, info.title));
    }
    popBody.append(group);
    footer(false);
    popBody.querySelector<HTMLButtonElement>('.rt-item')?.focus();
  }

  function renderFailure(reply: { status?: string; error?: string }): void {
    setMessage(errorText(reply), true);
    const details = errorDetails(reply);
    if (details && popBody) {
      const shown = h('code', { className: 'rt-details', textContent: details });
      shown.hidden = true;
      const toggle = h('button', { className: 'rt-link', type: 'button', textContent: tr('video.details') });
      toggle.setAttribute('aria-expanded', 'false');
      toggle.addEventListener('click', () => {
        shown.hidden = !shown.hidden;
        toggle.setAttribute('aria-expanded', String(!shown.hidden));
      });
      popBody.append(h('div', { className: 'rt-pmsg' }, [toggle, shown]));
    }
    footer(true);
    const src = directSource();
    if (src && popBody) {
      const direct = h('button', { className: 'rt-item', type: 'button', textContent: tr('video.direct') });
      direct.addEventListener('click', () => void chooseDirect(src));
      popBody.append(direct);
    }
  }

  function openPop(url: string): void {
    build();
    if (!pop || !popBody || !popTitle || !wrap) return;
    popUrl = url;
    wrap.setAttribute('data-on', '');
    wrap.removeAttribute('data-compact');
    pop.setAttribute('data-on', '');
    popTitle.textContent = tr('video.button');
    popTitle.removeAttribute('title');
    setMessage(tr('video.loading'));
    // "Best" works without the list, so it is offered at once; the list replaces it on arrival
    popBody.append(itemButton(BEST_ROW, document.title.slice(0, 200)));
    footer(false);
    place();
    void (async () => {
      const reply = await ctx.send<MediaProbeReply>({ kind: 'media-probe', url });
      if (popUrl !== url || !pop?.hasAttribute('data-on')) return; // closed or reopened meanwhile
      if (reply?.ok && reply.info) renderMenu(reply);
      else renderFailure(reply ?? { status: 'app_not_running' });
      place();
    })();
  }

  function closePop(): void {
    pop?.removeAttribute('data-on');
    popUrl = '';
    scheduleCompact();
  }

  function flash(state: 'sent' | 'failed'): void {
    if (!wrap || !label) return;
    wrap.dataset['state'] = state;
    label.textContent = tr(state === 'sent' ? 'video.sent' : 'video.failed');
    setTimeout(() => {
      if (wrap) delete wrap.dataset['state'];
      localise();
    }, 2_000);
  }

  async function choose(key: string, title: string): Promise<void> {
    if (busy || !popUrl) return;
    busy = true;
    const url = popUrl;
    const reply = await ctx.send<HandoffReply>({ kind: 'media-add', url, quality: key, title });
    busy = false;
    if (reply?.ok) {
      closePop();
      flash('sent');
      main?.focus();
    } else {
      flash('failed');
      renderFailure(reply ?? { status: 'app_not_running' });
    }
  }

  async function chooseDirect(src: string): Promise<void> {
    if (busy) return;
    busy = true;
    const reply = await ctx.send<HandoffReply>({ kind: 'handoff', url: src, referrer: location.href });
    busy = false;
    if (reply?.ok) {
      closePop();
      flash('sent');
    } else {
      flash('failed');
      renderFailure(reply ?? { status: 'app_not_running' });
    }
  }

  function onPopKey(e: KeyboardEvent): void {
    if (e.key === 'Escape') {
      e.preventDefault();
      closePop();
      main?.focus();
      return;
    }
    if (e.key !== 'ArrowDown' && e.key !== 'ArrowUp' && e.key !== 'Home' && e.key !== 'End') return;
    const items = [...(pop?.querySelectorAll<HTMLButtonElement>('.rt-item') ?? [])];
    if (items.length === 0) return;
    e.preventDefault();
    const active = ctx.root().activeElement;
    const i = items.findIndex((it) => it === active);
    let next = i;
    if (e.key === 'ArrowDown') next = i < 0 ? 0 : (i + 1) % items.length;
    else if (e.key === 'ArrowUp') next = i <= 0 ? items.length - 1 : i - 1;
    else if (e.key === 'Home') next = 0;
    else next = items.length - 1;
    items[next]?.focus();
  }

  // --- detection loop --------------------------------------------------------------------

  function evaluate(): void {
    const videos = bigVideos().length;
    const count = videos > 0 ? videos : isVideoPageUrl(location.href) ? 1 : 0;
    if (count !== reported) {
      reported = count;
      void ctx.send({ kind: 'media-found', count });
    }
    const show = allowed() && (count > 0 || hinted);
    if (show && shouldPrefetch(location.href, prefetched)) {
      // warm the app's probe cache so the popover has its list by the time it is opened;
      // same message the popover sends, only on a page already treated as a video page
      prefetched = location.href;
      void ctx.send({ kind: 'media-probe', url: location.href });
    }
    if (show) {
      build();
      if (wrap && !wrap.hasAttribute('data-on')) {
        wrap.setAttribute('data-on', '');
        scheduleCompact();
      }
      place();
    } else if (wrap && !pop?.hasAttribute('data-on')) {
      wrap.removeAttribute('data-on'); // a popover opened from the context menu stays
    }
  }

  let scanTimer: ReturnType<typeof setTimeout> | null = null;
  function scheduleScan(): void {
    if (scanTimer) return;
    scanTimer = setTimeout(() => {
      scanTimer = null;
      evaluate();
    }, 400);
  }

  let placing = false;
  function schedulePlace(): void {
    if (placing) return;
    placing = true;
    requestAnimationFrame(() => {
      placing = false;
      place();
    });
  }

  new MutationObserver(scheduleScan).observe(document.documentElement, { childList: true, subtree: true });
  window.addEventListener('scroll', schedulePlace, { passive: true, capture: true });
  window.addEventListener('resize', schedulePlace, { passive: true });
  // single-page sites change the URL without a navigation
  setInterval(() => {
    if (location.href !== lastHref) {
      lastHref = location.href;
      dismissed = false;
      hinted = false;
      closePop();
      scheduleScan();
    }
  }, 1_000);
  window.addEventListener('keydown', (e) => {
    if (e.key === 'Escape' && pop?.hasAttribute('data-on')) closePop();
  });
  document.addEventListener('click', () => {
    if (pop?.hasAttribute('data-on')) closePop();
  });

  // the network may already have seen media on this tab before the script ran
  void ctx.send<{ seen: number }>({ kind: 'media-query' }).then((r) => {
    if (r && r.seen > 0) ui.hint();
  });
  scheduleScan();

  const ui: VideoUi = {
    show(url) {
      openPop(url);
    },
    hint() {
      hinted = true;
      scheduleScan();
    },
    refresh() {
      localise();
      evaluate();
    },
  };
  return ui;
}
