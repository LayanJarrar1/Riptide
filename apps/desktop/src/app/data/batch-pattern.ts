import type { BatchError } from '../tauri/bridge/BatchError';

/**
 * A TypeScript port of `riptide_core::batch` (`crates/riptide-core/src/batch.rs`) for the
 * simulated engine used in the plain browser and in Karma. The real engine never runs this
 * code: inside Tauri the `batch_expand` and `batch_import` commands answer. The two are kept
 * honest by the same corpus of cases in `batch-pattern.spec.ts` and in the Rust tests.
 *
 * Syntax: `[from-to]`, `[from-to:step]` with numbers (a leading zero keeps the width) or single
 * same-case ASCII letters; several groups make a cartesian product, rightmost group fastest;
 * `\[`, `\]` and `\\` are literals. At most {@link MAX_EXPANSION} URLs.
 */

/** The most URLs one pattern may expand to. */
export const MAX_EXPANSION = 10_000;

/** What a pattern stands for, or why it does not stand for anything. */
export interface BatchExpansion {
  readonly urls: readonly string[];
  readonly error: BatchError | null;
}

/** What an imported text contained. */
export interface UrlListResult {
  readonly urls: readonly string[];
  readonly ignored: number;
  readonly duplicates: number;
}

type Range =
  | { readonly kind: 'number'; readonly start: bigint; readonly end: bigint; readonly step: bigint; readonly width: number }
  | { readonly kind: 'letter'; readonly start: number; readonly end: number; readonly step: bigint };

type Part = { readonly kind: 'literal'; readonly text: string } | { readonly kind: 'range'; readonly range: Range };

class PatternError extends Error {
  constructor(readonly error: BatchError) {
    super(error.code);
  }
}

const isDigits = (s: string): boolean => /^[0-9]+$/.test(s);

function parseNumber(s: string, at: number): bigint {
  if (s.length > 18) {
    throw new PatternError({ code: 'number_too_large', at });
  }
  return BigInt(s);
}

function abs(a: bigint): bigint {
  return a < 0n ? -a : a;
}

function parseRange(body: string, at: number): Range {
  const invalid = new PatternError({ code: 'invalid_range', at });
  let span = body;
  let step = 1n;
  const colon = body.indexOf(':');
  if (colon >= 0) {
    const raw = body.slice(colon + 1);
    span = body.slice(0, colon);
    if (!isDigits(raw)) {
      throw invalid;
    }
    step = parseNumber(raw, at);
    if (step === 0n) {
      throw new PatternError({ code: 'zero_step', at });
    }
  }
  const dash = span.indexOf('-');
  if (dash < 0) {
    throw invalid;
  }
  const from = span.slice(0, dash);
  const to = span.slice(dash + 1);
  if (isDigits(from) && isDigits(to)) {
    const padded = (s: string): boolean => s.length > 1 && s.startsWith('0');
    const width = padded(from) || padded(to) ? Math.max(from.length, to.length) : 0;
    return { kind: 'number', start: parseNumber(from, at), end: parseNumber(to, at), step, width };
  }
  const lower = /^[a-z]$/;
  const upper = /^[A-Z]$/;
  if ((lower.test(from) && lower.test(to)) || (upper.test(from) && upper.test(to))) {
    return { kind: 'letter', start: from.charCodeAt(0), end: to.charCodeAt(0), step };
  }
  throw invalid;
}

function countOf(r: Range): bigint {
  const [a, b] = r.kind === 'number' ? [r.start, r.end] : [BigInt(r.start), BigInt(r.end)];
  return abs(a - b) / r.step + 1n;
}

function valueOf(r: Range, k: bigint): string {
  if (r.kind === 'number') {
    const v = r.start <= r.end ? r.start + k * r.step : r.start - k * r.step;
    return v.toString().padStart(r.width, '0');
  }
  const start = BigInt(r.start);
  const v = r.start <= r.end ? start + k * r.step : start - k * r.step;
  return String.fromCharCode(Number(v));
}

function parse(pattern: string): Part[] {
  const parts: Part[] = [];
  let literal = '';
  let i = 0;
  while (i < pattern.length) {
    const c = pattern[i];
    if (c === '\\') {
      const next = pattern[i + 1];
      if (next === '[' || next === ']' || next === '\\') {
        literal += next;
        i += 2;
      } else {
        literal += '\\';
        i += 1;
      }
    } else if (c === '[') {
      const close = pattern.indexOf(']', i + 1);
      if (close < 0) {
        throw new PatternError({ code: 'unclosed', at: i });
      }
      const body = pattern.slice(i + 1, close);
      if (body.includes('[')) {
        throw new PatternError({ code: 'invalid_range', at: i });
      }
      const range = parseRange(body, i);
      if (literal.length > 0) {
        parts.push({ kind: 'literal', text: literal });
        literal = '';
      }
      parts.push({ kind: 'range', range });
      i = close + 1;
    } else {
      literal += c;
      i += 1;
    }
  }
  if (literal.length > 0) {
    parts.push({ kind: 'literal', text: literal });
  }
  return parts;
}

/** Whether `s` looks like an `http(s)` URL with a host and no whitespace or control characters. */
export function isHttpUrl(s: string): boolean {
  const match = /^https?:\/\//i.exec(s);
  if (match === null) {
    return false;
  }
  const rest = s.slice(match[0].length);
  // eslint-disable-next-line no-control-regex
  return rest.length > 0 && !/^[/?#]/.test(rest) && !/[\s\u0000-\u001f\u007f-\u009f]/.test(s);
}

/** Expands a pattern; never throws. */
export function expandPattern(input: string): BatchExpansion {
  try {
    return { urls: expandOrThrow(input.trim()), error: null };
  } catch (e: unknown) {
    if (e instanceof PatternError) {
      return { urls: [], error: e.error };
    }
    throw e;
  }
}

function expandOrThrow(pattern: string): string[] {
  if (pattern.length === 0) {
    throw new PatternError({ code: 'empty' });
  }
  // eslint-disable-next-line no-control-regex
  const bad = /[\s\u0000-\u001f\u007f-\u009f]/.exec(pattern);
  if (bad !== null) {
    throw new PatternError({ code: 'invalid_char', at: bad.index });
  }
  const parts = parse(pattern);
  const counts = parts.map((p) => (p.kind === 'range' ? countOf(p.range) : 1n));
  let total = 1n;
  for (const n of counts) {
    total *= n;
    if (total > BigInt(MAX_EXPANSION)) {
      throw new PatternError({ code: 'too_many', limit: MAX_EXPANSION });
    }
  }
  const urls: string[] = [];
  for (let n = 0n; n < total; n++) {
    let index = n;
    const picks = new Array<bigint>(parts.length).fill(0n);
    for (let i = parts.length - 1; i >= 0; i--) {
      picks[i] = index % counts[i];
      index /= counts[i];
    }
    let url = '';
    parts.forEach((part, i) => {
      url += part.kind === 'literal' ? part.text : valueOf(part.range, picks[i]);
    });
    if (!isHttpUrl(url)) {
      throw new PatternError({ code: 'not_http' });
    }
    urls.push(url);
  }
  return urls;
}

/** One URL per line; blanks and `#` comments skipped; http(s) only; repeats dropped. */
export function importUrlList(text: string): UrlListResult {
  const seen = new Set<string>();
  const urls: string[] = [];
  let ignored = 0;
  let duplicates = 0;
  for (const raw of text.replace(/^\uFEFF/, '').split(/\r\n|\n|\r/)) {
    const line = raw.trim();
    if (line.length === 0 || line.startsWith('#')) {
      continue;
    }
    if (!isHttpUrl(line)) {
      ignored++;
    } else if (seen.has(line)) {
      duplicates++;
    } else {
      seen.add(line);
      urls.push(line);
    }
  }
  return { urls, ignored, duplicates };
}

/** The text form of a list: one URL per line, newline-terminated (empty for an empty list). */
export function exportUrlList(urls: readonly string[]): string {
  return urls.map((u) => `${u}\n`).join('');
}
