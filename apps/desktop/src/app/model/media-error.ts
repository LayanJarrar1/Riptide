/**
 * Video-download errors from the engine. The engine tags the refusals it understands
 * (`[rt:login] ...`, `[rt:refused] ...`, see `riptide_media::Error`), so the UI can show a short,
 * translated message and keep yt-dlp's own line behind a "Details" affordance.
 */

/** The refusal kinds the engine tags. */
export type MediaRefusal = 'login' | 'refused';

/** A video error split into its friendly kind and the plain-text details. */
export interface ParsedMediaError {
  /** The tagged refusal kind, `null` for any other error. */
  readonly kind: MediaRefusal | null;
  /** yt-dlp's first error line, plain text, at most 300 characters (the JS-runtime hint follows). */
  readonly details: string;
}

const TAG = /\[rt:(login|refused)\]\s*/;
const MAX_RAW = 300;

/** Strips control characters so the details are safe plain text. */
function plain(text: string): string {
  return Array.from(text, (ch) => (ch.charCodeAt(0) < 32 ? ' ' : ch))
    .join('')
    .replace(/\s+/g, ' ')
    .trim();
}

/** Splits an engine error (a string or an `Error`) into the refusal kind and the details. */
export function parseMediaError(error: unknown): ParsedMediaError {
  const text = String(error instanceof Error ? error.message : error);
  const match = TAG.exec(text);
  const kind = match === null ? null : (match[1] as MediaRefusal);
  const body = match === null ? text : text.slice(match.index + match[0].length);
  // the engine appends the JavaScript-runtime hint after " | "; keep it whole, cut only the raw line
  const [raw = '', ...hint] = body.split(' | ');
  const details = [plain(raw).slice(0, MAX_RAW), ...hint.map(plain)].filter((s) => s.length > 0).join(' | ');
  return { kind, details };
}
