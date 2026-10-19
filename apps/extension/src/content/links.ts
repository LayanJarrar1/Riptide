/**
 * The "Download all links / selected links / all images" checklist (content script, top frame).
 *
 * Collects candidates from the page, shows them in an overlay inside the content script's closed
 * shadow root (so the page cannot style or read it) and sends the chosen URLs to the background
 * in one `handoff-many` message. Keyboard accessible (listbox with roving focus, focus trap,
 * Escape), RTL-aware, and virtualised: lists longer than `VIRTUALISE_ABOVE` render only the rows
 * in view. Nothing from the page is executed; only URL strings leave this file.
 */
import { dirOf } from '../i18n';
import type { Lang, MessageKey } from '../i18n';
import {
  LINK_GROUPS,
  MAX_BATCH,
  VIRTUALISE_ABOVE,
  buildItems,
  countByGroup,
  filterItems,
  largestFromSrcset,
  visibleRange,
} from '../links';
import type { LinkGroup, LinkItem, RawLink } from '../links';
import type { HandoffManyReply, Message, PanelMode } from '../messages';

/** What the checklist needs from the content script it lives in. */
export interface LinksContext {
  root(): ShadowRoot;
  lang(): Lang;
  tr(key: MessageKey, params?: Record<string, string | number>): string;
  send<T>(message: Message): Promise<T | undefined>;
  /** Hide the hover badge while the overlay is open. */
  hideBadge(): void;
}

/** Controls handed back to the content script. */
export interface LinksUi {
  show(mode: PanelMode): void;
  close(): void;
  isOpen(): boolean;
}

const ROW_HEIGHT = 46;

const CSS = `
.rl-scrim { position: fixed; inset: 0; background: rgb(0 0 0 / 45%); display: flex; align-items: center; justify-content: center; }
.rl-panel { width: min(640px, calc(100vw - 32px)); height: min(80vh, 720px); display: flex; flex-direction: column;
  background: #10161b; color: #e9eef2; border: 1px solid rgb(255 255 255 / 10%); border-radius: 14px;
  box-shadow: 0 20px 60px rgb(0 0 0 / 55%); overflow: hidden; font-size: 13px; }
.rl-panel button, .rl-panel input { font: inherit; }
.rl-panel :focus-visible { outline: 2px solid #00d5e8; outline-offset: 2px; }
.rl-head { display: flex; align-items: center; gap: 10px; padding: 14px 16px; border-bottom: 1px solid rgb(255 255 255 / 8%); }
.rl-head h2 { flex: 1; margin: 0; font-size: 15px; font-weight: 600; }
.rl-x { border: 0; background: transparent; color: #a9b5bf; font-size: 18px; cursor: pointer; padding: 2px 8px; border-radius: 6px; }
.rl-x:hover { background: rgb(255 255 255 / 7%); color: #e9eef2; }
.rl-tools { display: flex; align-items: center; gap: 8px; padding: 10px 16px 6px; flex-wrap: wrap; }
.rl-search { flex: 1; min-width: 160px; padding: 7px 10px; border-radius: 8px; border: 1px solid rgb(255 255 255 / 14%); background: #161e25; color: #e9eef2; }
.rl-chips { display: flex; gap: 6px; padding: 4px 16px 10px; flex-wrap: wrap; border-bottom: 1px solid rgb(255 255 255 / 8%); }
.rl-chip { border: 1px solid rgb(255 255 255 / 14%); background: transparent; color: #a9b5bf; padding: 3px 10px; border-radius: 999px; cursor: pointer; }
.rl-chip:hover { background: rgb(255 255 255 / 6%); }
.rl-chip[aria-pressed="true"] { background: rgb(25 198 181 / 16%); border-color: #19c6b5; color: #5ae0d1; }
.rl-chip[hidden] { display: none; }
.rl-sel { display: flex; align-items: center; gap: 8px; padding: 8px 16px; color: #a9b5bf; }
.rl-sel .rl-count { flex: 1; }
.rl-link { border: 0; background: transparent; color: #5ae0d1; cursor: pointer; padding: 2px 4px; border-radius: 4px; }
.rl-link:hover { text-decoration: underline; }
.rl-list { position: relative; flex: 1; overflow: auto; padding: 0 8px; outline: none; }
.rl-sizer { position: relative; width: 100%; }
.rl-row { position: absolute; inset-inline: 0; height: ${ROW_HEIGHT}px; display: flex; align-items: center; gap: 10px; padding: 0 8px;
  border-radius: 8px; cursor: pointer; }
.rl-row:hover { background: rgb(255 255 255 / 4%); }
.rl-row[data-active] { background: rgb(255 255 255 / 7%); }
.rl-row[data-active]:focus-visible { outline-offset: -2px; }
.rl-box { flex: none; width: 16px; height: 16px; border-radius: 4px; border: 1.5px solid #6d7b87; display: grid; place-items: center; font-size: 11px; line-height: 1; color: #06201d; }
.rl-row[aria-selected="true"] .rl-box { background: #19c6b5; border-color: #19c6b5; }
.rl-row[aria-selected="true"] .rl-box::after { content: "\\2713"; }
.rl-text { flex: 1; min-width: 0; }
.rl-name, .rl-url { display: block; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
.rl-url { color: #6d7b87; font-size: 11px; direction: ltr; unicode-bidi: isolate; text-align: start; }
.rl-ext { flex: none; padding: 2px 7px; border-radius: 999px; background: rgb(25 198 181 / 12%); color: #5ae0d1; font-size: 11px; font-weight: 600; text-transform: uppercase; }
.rl-empty { padding: 32px; text-align: center; color: #6d7b87; }
.rl-foot { display: flex; align-items: center; gap: 10px; padding: 12px 16px; border-top: 1px solid rgb(255 255 255 / 8%); }
.rl-status { flex: 1; color: #a9b5bf; }
.rl-btn { padding: 8px 14px; border-radius: 8px; border: 1px solid rgb(255 255 255 / 14%); background: #161e25; color: #e9eef2; font-weight: 600; cursor: pointer; }
.rl-btn:hover { background: #1d272f; }
.rl-btn.rl-primary { background: #19c6b5; border-color: #19c6b5; color: #06201d; }
.rl-btn.rl-primary:hover { background: #5ae0d1; }
.rl-btn:disabled { opacity: 0.5; cursor: default; }
@media (prefers-reduced-motion: no-preference) { .rl-panel { animation: rl-in 140ms ease-out; } }
@keyframes rl-in { from { transform: translateY(6px); opacity: 0; } to { transform: none; opacity: 1; } }
`;

type Attrs = Record<string, string>;

function el<K extends keyof HTMLElementTagNameMap>(
  tag: K,
  attrs: Attrs = {},
  children: (Node | string)[] = [],
): HTMLElementTagNameMap[K] {
  const node = document.createElement(tag);
  for (const [k, v] of Object.entries(attrs)) node.setAttribute(k, v);
  for (const c of children) node.append(c);
  return node;
}

/** Read candidates from the live document. Page strings only become `RawLink` data. */
function collect(mode: PanelMode): RawLink[] {
  const out: RawLink[] = [];
  if (mode === 'images') {
    for (const img of document.querySelectorAll<HTMLImageElement>('img')) {
      const label = img.alt || img.title;
      const srcset = img.getAttribute('srcset');
      const best = srcset ? largestFromSrcset(srcset, document.baseURI) : null;
      if (best) out.push({ url: best, text: label, source: 'image' });
      out.push({ url: img.currentSrc || img.src, text: label, source: 'image' });
    }
    for (const s of document.querySelectorAll<HTMLSourceElement>('picture source[srcset]')) {
      const best = largestFromSrcset(s.getAttribute('srcset') ?? '', document.baseURI);
      if (best) out.push({ url: best, source: 'image' });
    }
    for (const i of document.querySelectorAll<HTMLInputElement>('input[type="image"][src]')) {
      out.push({ url: i.src, text: i.alt, source: 'image' });
    }
    // Links that point straight at a picture are usually the full-size version.
    for (const a of document.querySelectorAll<HTMLAnchorElement>('a[href]')) {
      if (/\.(jpe?g|png|gif|webp|avif|bmp|tiff?|heic|svg)(?:$|[?#])/i.test(a.pathname + a.search)) {
        out.push({ url: a.href, text: a.title || a.textContent || '', source: 'image' });
      }
    }
    return out;
  }
  const ranges: Range[] = [];
  if (mode === 'selection') {
    const sel = window.getSelection();
    for (let i = 0; i < (sel?.rangeCount ?? 0); i++) {
      const r = sel?.getRangeAt(i);
      if (r && !r.collapsed) ranges.push(r);
    }
  }
  for (const a of document.querySelectorAll<HTMLAnchorElement>('a[href]')) {
    if (mode === 'selection' && !ranges.some((r) => r.intersectsNode(a))) continue;
    out.push({ url: a.href, text: a.textContent || a.getAttribute('download') || a.title, source: 'link' });
  }
  return out;
}

/** Wire up the checklist. */
export function initLinks(ctx: LinksContext): LinksUi {
  let styled = false;
  let scrim: HTMLDivElement | null = null;
  let restoreFocus: Element | null = null;
  let disposeKeys: (() => void) | null = null;

  function close(): void {
    disposeKeys?.();
    disposeKeys = null;
    scrim?.remove();
    scrim = null;
    if (restoreFocus instanceof HTMLElement) restoreFocus.focus({ preventScroll: true });
    restoreFocus = null;
  }

  function show(mode: PanelMode): void {
    close();
    ctx.hideBadge();
    const root = ctx.root();
    if (!styled) {
      const style = document.createElement('style');
      style.textContent = CSS;
      root.appendChild(style);
      styled = true;
    }
    restoreFocus = document.activeElement;
    const { tr } = ctx;
    const items = buildItems(collect(mode));
    const counts = countByGroup(items);

    // ---- state ----
    const selected = new Set<string>(items.map((i) => i.url));
    const groups = new Set<LinkGroup>();
    let text = '';
    let shown: LinkItem[] = items;
    let active = 0;
    let sending = false;

    // ---- elements ----
    const titleKey: MessageKey =
      mode === 'images' ? 'panel.title.images' : mode === 'selection' ? 'panel.title.selection' : 'panel.title.all';
    const titleId = 'rl-title';
    const title = el('h2', { id: titleId }, [tr(titleKey)]);
    const closeBtn = el('button', { class: 'rl-x', type: 'button', 'aria-label': tr('panel.close') }, ['×']);
    const search = el('input', {
      class: 'rl-search',
      type: 'search',
      placeholder: tr('panel.filter'),
      'aria-label': tr('panel.filter'),
    });
    const chips = new Map<LinkGroup, HTMLButtonElement>();
    const chipBar = el('div', { class: 'rl-chips', role: 'group', 'aria-label': tr('panel.types') });
    for (const g of LINK_GROUPS) {
      const chip = el('button', { class: 'rl-chip', type: 'button', 'aria-pressed': 'false' }, [
        `${tr(`panel.type.${g}`)} ${counts[g]}`,
      ]);
      if (counts[g] === 0) chip.hidden = true;
      chip.addEventListener('click', () => {
        if (groups.has(g)) groups.delete(g);
        else groups.add(g);
        chip.setAttribute('aria-pressed', String(groups.has(g)));
        refilter();
      });
      chips.set(g, chip);
      chipBar.append(chip);
    }
    const count = el('span', { class: 'rl-count', 'aria-live': 'polite' });
    const allBtn = el('button', { class: 'rl-link', type: 'button' }, [tr('panel.select_all')]);
    const noneBtn = el('button', { class: 'rl-link', type: 'button' }, [tr('panel.select_none')]);
    const list = el('div', {
      class: 'rl-list',
      role: 'listbox',
      'aria-multiselectable': 'true',
      'aria-labelledby': titleId,
      tabindex: '0',
    });
    const sizer = el('div', { class: 'rl-sizer' });
    list.append(sizer);
    const empty = el('div', { class: 'rl-empty' }, [tr('panel.empty')]);
    const status = el('span', { class: 'rl-status', role: 'status' });
    const cancelBtn = el('button', { class: 'rl-btn', type: 'button' }, [tr('panel.cancel')]);
    const goBtn = el('button', { class: 'rl-btn rl-primary', type: 'button' });

    const panel = el('div', { class: 'rl-panel', role: 'dialog', 'aria-modal': 'true', 'aria-labelledby': titleId }, [
      el('div', { class: 'rl-head' }, [title, closeBtn]),
      el('div', { class: 'rl-tools' }, [search]),
      chipBar,
      el('div', { class: 'rl-sel' }, [count, allBtn, noneBtn]),
      list,
      el('div', { class: 'rl-foot' }, [status, cancelBtn, goBtn]),
    ]);
    panel.setAttribute('dir', dirOf(ctx.lang()));
    const layer = el('div', { class: 'rl-scrim' }, [panel]);
    scrim = layer;

    // ---- rendering ----
    const rowPool = new Map<number, HTMLDivElement>();

    function selectedShown(): string[] {
      return shown.filter((i) => selected.has(i.url)).map((i) => i.url);
    }

    function updateCounts(): void {
      const n = selectedShown().length;
      count.textContent = tr('panel.count', { n, total: shown.length });
      goBtn.textContent = tr('panel.download', { n: Math.min(n, MAX_BATCH) });
      goBtn.disabled = n === 0 || sending;
    }

    function makeRow(index: number, item: LinkItem): HTMLDivElement {
      const row = el('div', { class: 'rl-row', role: 'option', id: `rl-row-${index}` }, [
        el('span', { class: 'rl-box', 'aria-hidden': 'true' }),
        el('span', { class: 'rl-text' }, [
          el('span', { class: 'rl-name' }, [item.name]),
          el('span', { class: 'rl-url' }, [item.url]),
        ]),
      ]);
      if (item.ext) row.append(el('span', { class: 'rl-ext' }, [item.ext]));
      row.style.top = `${index * ROW_HEIGHT}px`;
      row.title = item.url;
      row.setAttribute('aria-selected', String(selected.has(item.url)));
      if (index === active) row.setAttribute('data-active', '');
      row.addEventListener('click', () => {
        active = index;
        toggle(item.url);
        list.focus({ preventScroll: true });
      });
      return row;
    }

    function render(): void {
      sizer.style.height = `${shown.length * ROW_HEIGHT}px`;
      const range =
        shown.length > VIRTUALISE_ABOVE
          ? visibleRange(shown.length, list.scrollTop, list.clientHeight || 400, ROW_HEIGHT)
          : { start: 0, end: shown.length };
      for (const [i, row] of rowPool) {
        if (i < range.start || i >= range.end) {
          row.remove();
          rowPool.delete(i);
        }
      }
      for (let i = range.start; i < range.end; i++) {
        const item = shown[i];
        if (!item) continue;
        const existing = rowPool.get(i);
        if (existing) {
          existing.setAttribute('aria-selected', String(selected.has(item.url)));
          if (i === active) existing.setAttribute('data-active', '');
          else existing.removeAttribute('data-active');
          continue;
        }
        const row = makeRow(i, item);
        rowPool.set(i, row);
        sizer.append(row);
      }
      const activeRow = rowPool.get(active);
      if (activeRow) list.setAttribute('aria-activedescendant', activeRow.id);
      else list.removeAttribute('aria-activedescendant');
      empty.remove();
      if (shown.length === 0) list.append(empty);
      updateCounts();
    }

    function resetRows(): void {
      for (const row of rowPool.values()) row.remove();
      rowPool.clear();
    }

    function refilter(): void {
      shown = filterItems(items, { groups, text });
      active = 0;
      list.scrollTop = 0;
      resetRows();
      render();
    }

    function toggle(url: string): void {
      if (selected.has(url)) selected.delete(url);
      else selected.add(url);
      render();
    }

    function moveActive(to: number): void {
      if (shown.length === 0) return;
      active = Math.max(0, Math.min(shown.length - 1, to));
      const top = active * ROW_HEIGHT;
      if (top < list.scrollTop) list.scrollTop = top;
      else if (top + ROW_HEIGHT > list.scrollTop + list.clientHeight) list.scrollTop = top + ROW_HEIGHT - list.clientHeight;
      render();
    }

    function setAll(on: boolean): void {
      for (const i of shown) {
        if (on) selected.add(i.url);
        else selected.delete(i.url);
      }
      render();
    }

    // ---- events ----
    search.addEventListener('input', () => {
      text = search.value;
      refilter();
    });
    allBtn.addEventListener('click', () => setAll(true));
    noneBtn.addEventListener('click', () => setAll(false));
    closeBtn.addEventListener('click', close);
    cancelBtn.addEventListener('click', close);
    list.addEventListener('scroll', () => {
      if (shown.length > VIRTUALISE_ABOVE) render();
    });
    list.addEventListener('keydown', (e) => {
      switch (e.key) {
        case 'ArrowDown':
          moveActive(active + 1);
          break;
        case 'ArrowUp':
          moveActive(active - 1);
          break;
        case 'PageDown':
          moveActive(active + Math.max(1, Math.floor(list.clientHeight / ROW_HEIGHT) - 1));
          break;
        case 'PageUp':
          moveActive(active - Math.max(1, Math.floor(list.clientHeight / ROW_HEIGHT) - 1));
          break;
        case 'Home':
          moveActive(0);
          break;
        case 'End':
          moveActive(shown.length - 1);
          break;
        case ' ':
        case 'Enter': {
          const item = shown[active];
          if (item) toggle(item.url);
          break;
        }
        case 'a':
        case 'A':
          if (!(e.ctrlKey || e.metaKey)) return;
          setAll(true);
          break;
        default:
          return;
      }
      e.preventDefault();
    });
    goBtn.addEventListener('click', () => {
      void (async () => {
        const urls = selectedShown().slice(0, MAX_BATCH);
        if (urls.length === 0 || sending) return;
        sending = true;
        goBtn.disabled = true;
        cancelBtn.disabled = true;
        status.textContent = tr('panel.sending', { n: urls.length });
        const result = await ctx.send<HandoffManyReply>({ kind: 'handoff-many', urls, referrer: location.href });
        sending = false;
        const ok = result?.ok ?? 0;
        const failed = result?.failed ?? urls.length;
        status.textContent = failed === 0 ? tr('panel.done', { n: ok }) : tr('panel.partial', { ok, failed });
        cancelBtn.disabled = false;
        cancelBtn.textContent = tr('panel.close');
        updateCounts();
        if (failed === 0) setTimeout(() => scrim === layer && close(), 1_600);
      })();
    });

    // Keys: Escape closes; Tab stays inside the panel (the page behind is inert for the user).
    const onKey = (e: KeyboardEvent): void => {
      if (e.key === 'Escape') {
        e.preventDefault();
        e.stopPropagation();
        close();
        return;
      }
      if (e.key !== 'Tab') return;
      const focusable = [...panel.querySelectorAll<HTMLElement>('button, input, [tabindex="0"]')].filter(
        (n) => !(n instanceof HTMLButtonElement && n.disabled) && !n.hidden,
      );
      const first = focusable[0];
      const last = focusable[focusable.length - 1];
      if (!first || !last) return;
      const current = root.activeElement;
      if (e.shiftKey && (current === first || current === null)) {
        e.preventDefault();
        last.focus();
      } else if (!e.shiftKey && current === last) {
        e.preventDefault();
        first.focus();
      }
    };
    layer.addEventListener('keydown', onKey);
    layer.addEventListener('click', (e) => {
      if (e.target === layer) close();
    });
    // Keep page shortcuts from firing while typing in the overlay.
    for (const type of ['keydown', 'keyup', 'keypress'] as const) layer.addEventListener(type, (e) => e.stopPropagation());
    disposeKeys = () => layer.removeEventListener('keydown', onKey);

    root.appendChild(layer);
    refilter();
    if (items.length > 0) search.focus();
    else closeBtn.focus();
  }

  return { show, close, isOpen: () => scrim !== null };
}
