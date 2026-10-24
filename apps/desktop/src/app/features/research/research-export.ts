import { SourceId } from '../../ai/research.models';
import { TurnItem, Turn } from './research-history';
import { summarize } from './research-turn';

/** What the writers need from the app: the translations, a size formatter and the date to print. */
export interface ExportContext {
  readonly t: (key: string) => string;
  readonly bytes: (n: number) => string;
  /** `YYYY-MM-DD`. */
  readonly date: string;
}

function fill(text: string, params: Record<string, string | number>): string {
  return text.replace(/\{(\w+)\}/g, (m, k: string) => (k in params ? String(params[k]) : m));
}

/** Escapes the characters Markdown would read as formatting in text that came from a source. */
export function escapeMd(text: string): string {
  return text.replace(/[\\`*_{}[\]<>()#+!|~]/g, '\\$&');
}

function rightsLine(i: TurnItem, t: ExportContext['t']): string {
  switch (i.rights.kind) {
    case 'public-domain':
      return t('research.rights.public-domain');
    case 'cc':
      return i.rights.label;
    case 'open-source':
      return fill(t('research.rights.open-source'), { label: i.rights.label });
    default:
      return t('research.rights.unknown');
  }
}

function byline(i: TurnItem): string {
  const who = i.authors.slice(0, 3).join(', ') + (i.authors.length > 3 ? ' …' : '');
  return [who, i.year === null ? '' : String(i.year)].filter((p) => p.length > 0).join(' · ');
}

/** The sentence(s) under the prompt: counts, sources, what was downloaded before. Built from the results. */
export function summaryLines(turn: Turn, c: ExportContext): string[] {
  const s = summarize(turn);
  const lines: string[] = [];
  if (s.total === 0) {
    lines.push(c.t('research.none'));
    return lines;
  }
  const names = s.sources.map((x) => fill(c.t('research.sum.source'), { source: c.t(`research.source.${x.id}`), n: x.count }));
  lines.push(fill(c.t('research.sum.found'), { n: s.total, sources: names.join(', ') }));
  if (s.pages > 0) {
    lines.push(fill(c.t('research.sum.pages'), { n: s.pages }));
  }
  if (s.seen > 0) {
    lines.push(fill(c.t('research.sum.seen'), { n: s.seen }));
  }
  return lines;
}

/** One-line headline for a top pick: title and, when the AI gave one, why it matches. */
function pickLine(i: TurnItem, md: boolean): string {
  const title = md ? escapeMd(i.title) : i.title;
  return i.why ? `${title}: ${md ? escapeMd(i.why) : i.why}` : title;
}

/** The answer as plain text (the "copy response" action). */
export function responseText(turn: Turn, c: ExportContext): string {
  const out = [...summaryLines(turn, c)];
  const s = summarize(turn);
  if (s.top.length > 0) {
    out.push('', c.t(s.ranked ? 'research.sum.top' : 'research.sum.first'));
    s.top.forEach((i, n) => out.push(`${n + 1}. ${pickLine(i, false)}`));
  }
  const rest = turn.items;
  if (rest.length > 0) {
    out.push('');
    for (const i of rest) {
      out.push(`- ${i.title} (${c.t(`research.source.${i.source}`)})${i.pageUrl ?? i.downloadUrl ? ` ${i.pageUrl ?? i.downloadUrl}` : ''}`);
    }
  }
  return out.join('\n');
}

/** The whole turn as a Markdown document (the "save research" action). */
export function toMarkdown(turn: Turn, c: ExportContext): string {
  const out: string[] = [];
  out.push(`# ${escapeMd(turn.prompt)}`, '');
  out.push(`_${fill(c.t('research.md.meta'), { date: c.date, category: c.t(`research.cat.${turn.category}`) })}_`, '');
  for (const line of summaryLines(turn, c)) {
    out.push(line, '');
  }
  const s = summarize(turn);
  if (s.top.length > 0) {
    out.push(`## ${c.t(s.ranked ? 'research.sum.top' : 'research.sum.first')}`, '');
    s.top.forEach((i) => out.push(`- ${pickLine(i, true)}`));
    out.push('');
  }
  if (turn.items.length > 0) {
    out.push(`## ${c.t('research.md.results')}`, '');
    turn.items.forEach((i, n) => {
      out.push(`### ${n + 1}. ${escapeMd(i.title)}`, '');
      const facts = [c.t(`research.source.${i.source}`), byline(i), i.size === null ? '' : c.bytes(i.size)].filter((p) => p.length > 0);
      out.push(`- ${escapeMd(facts.join(' · '))}`);
      out.push(`- ${c.t('research.licence')}: ${escapeMd(rightsLine(i, c.t))}`);
      if (i.why) {
        out.push(`- ${c.t('research.why')}: ${escapeMd(i.why)}`);
      }
      if (i.summary) {
        out.push(`- ${escapeMd(i.summary)}`);
      }
      if (i.pageUrl) {
        out.push(`- ${c.t('research.md.page')}: <${i.pageUrl}>`);
      }
      out.push(`- ${c.t('research.md.download')}: <${i.downloadUrl}>`, '');
    });
  }
  return out.join('\n').replace(/\n{3,}/g, '\n\n');
}

// ---------------------------------------------------------------- citations

/** A result is citable when it has an identifier, or authors and a year. */
export function isCitable(i: TurnItem): boolean {
  return i.doi !== null || i.arxiv !== null || (i.authors.length > 0 && i.year !== null);
}

const BIB_ESCAPES: Record<string, string> = {
  '\\': '\\textbackslash{}',
  '{': '\\{',
  '}': '\\}',
  '&': '\\&',
  '%': '\\%',
  $: '\\$',
  '#': '\\#',
  _: '\\_',
  '~': '\\textasciitilde{}',
  '^': '\\textasciicircum{}',
};

/** Escapes text for a BibTeX field (Arabic and other Unicode stay as they are: UTF-8). */
export function escapeBib(text: string): string {
  return text.replace(/[\\{}&%$#_~^]/g, (ch) => BIB_ESCAPES[ch]);
}

function citeKey(i: TurnItem, used: Set<string>): string {
  const fold = (s: string): string =>
    s
      .normalize('NFKD')
      .replace(/\P{ASCII}/gu, '')
      .replace(/[^A-Za-z0-9]/g, '')
      .toLowerCase();
  const first = i.authors[0] ?? '';
  const last = first.includes(',') ? first.split(',')[0] : (first.split(/\s+/).pop() ?? '');
  const word = i.title.split(/\s+/).map(fold).find((w) => w.length > 3) ?? '';
  const base = `${fold(last)}${i.year ?? ''}${word}` || (i.arxiv ? `arxiv${fold(i.arxiv)}` : 'ref');
  let key = base;
  for (let n = 0; used.has(key); n++) {
    key = `${base}${String.fromCharCode(97 + (n % 26))}${n >= 26 ? n : ''}`;
  }
  used.add(key);
  return key;
}

/** BibTeX for the citable results; `skipped` counts the ones that had too little to cite. */
export function toBibtex(items: readonly TurnItem[]): { text: string; count: number; skipped: number } {
  const used = new Set<string>();
  const entries: string[] = [];
  for (const i of items) {
    if (!isCitable(i)) {
      continue;
    }
    const fields: [string, string][] = [['title', `{${escapeBib(i.title)}}`]];
    if (i.authors.length > 0) {
      fields.push(['author', i.authors.map((a) => escapeBib(a.replace(/\s+and\s+/gi, ' & '))).join(' and ')]);
    }
    if (i.year !== null) {
      fields.push(['year', String(i.year)]);
    }
    if (i.doi) {
      fields.push(['doi', i.doi]);
    }
    if (i.arxiv) {
      fields.push(['eprint', i.arxiv], ['archivePrefix', 'arXiv']);
    }
    // identifiers and addresses are machine-readable: no TeX escapes, only the braces that would unbalance the field
    fields.push(['url', (i.pageUrl ?? i.downloadUrl).replace(/\{/g, '%7B').replace(/\}/g, '%7D')]);
    const body = fields.map(([k, v]) => `  ${k} = {${v}}`).join(',\n');
    entries.push(`@misc{${citeKey(i, used)},\n${body}\n}`);
  }
  return { text: entries.join('\n\n') + (entries.length > 0 ? '\n' : ''), count: entries.length, skipped: items.length - entries.length };
}

const RIS_TYPE: Partial<Record<SourceId, string>> = {
  arxiv: 'UNPB',
  crossref: 'JOUR',
  'open-alex': 'JOUR',
  'europe-pmc': 'JOUR',
  gutenberg: 'BOOK',
  'open-library': 'BOOK',
  'libri-vox': 'SOUND',
  jamendo: 'SOUND',
  github: 'COMP',
  'data-gov': 'DATA',
  blender: 'VIDEO',
};

/** RIS for the citable results. */
export function toRis(items: readonly TurnItem[]): { text: string; count: number; skipped: number } {
  const entries: string[] = [];
  for (const i of items) {
    if (!isCitable(i)) {
      continue;
    }
    const line = (tag: string, value: string): string => `${tag}  - ${value.replace(/[\r\n]+/g, ' ')}`;
    const rows = [line('TY', RIS_TYPE[i.source] ?? 'GEN')];
    for (const a of i.authors) {
      rows.push(line('AU', a));
    }
    rows.push(line('TI', i.title));
    if (i.year !== null) {
      rows.push(line('PY', String(i.year)));
    }
    if (i.doi) {
      rows.push(line('DO', i.doi));
    }
    if (i.arxiv) {
      rows.push(line('N1', `arXiv:${i.arxiv}`));
    }
    if (i.summary) {
      rows.push(line('AB', i.summary));
    }
    rows.push(line('UR', i.pageUrl ?? i.downloadUrl), 'ER  - ');
    entries.push(rows.join('\n'));
  }
  return { text: entries.join('\n\n') + (entries.length > 0 ? '\n' : ''), count: entries.length, skipped: items.length - entries.length };
}

/** A file name for an export: the prompt, safe for any file system. */
export function exportName(prompt: string, ext: string): string {
  const base = prompt
    .normalize('NFC')
    .replace(/[^\p{L}\p{N}]+/gu, '-')
    .replace(/^-+|-+$/g, '')
    .slice(0, 40);
  return `riptide-research-${base || 'results'}.${ext}`;
}
