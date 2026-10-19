/**
 * Video errors from the app. The engine tags the refusals it understands (`[rt:login] ...`,
 * `[rt:refused] ...`); the popover shows a short translated message, with yt-dlp's own line
 * behind a "Details" button and a "Try again" button.
 */

/** The refusal kinds the app tags. */
export type MediaRefusal = 'login' | 'refused';

/** A video error split into its kind and plain-text details. */
export interface ParsedMediaError {
  /** The tagged kind, `null` for any other message. */
  kind: MediaRefusal | null;
  /** The app's message without the tag: plain text, raw line cut to 300 characters, hint kept. */
  details: string;
}

const TAG = /\[rt:(login|refused)\]\s*/;
const MAX_RAW = 300;

function plain(text: string): string {
  return Array.from(text, (ch) => (ch.charCodeAt(0) < 32 ? ' ' : ch))
    .join('')
    .replace(/\s+/g, ' ')
    .trim();
}

/** Splits the app's error message into the refusal kind and the details. */
export function parseMediaError(message: string | undefined): ParsedMediaError {
  const text = message ?? '';
  const match = TAG.exec(text);
  const kind = match ? (match[1] as MediaRefusal) : null;
  const body = match ? text.slice(match.index + match[0].length) : text;
  // the app appends the JavaScript-runtime hint after " | "; only the raw line is cut
  const [raw = '', ...hint] = body.split(' | ');
  const details = [plain(raw).slice(0, MAX_RAW), ...hint.map(plain)].filter((s) => s.length > 0).join(' | ');
  return { kind, details };
}
