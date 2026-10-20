import { parseMediaError } from './media-error';

describe('parseMediaError', () => {
  it('reads the engine tags and keeps the raw line as details', () => {
    expect(parseMediaError('[rt:refused] [youtube] x: The page needs to be reloaded.')).toEqual({
      kind: 'refused',
      details: '[youtube] x: The page needs to be reloaded.',
    });
    expect(parseMediaError(new Error('[rt:login] [youtube] x: Sign in to confirm your age.')).kind).toBe('login');
  });

  it('finds the tag inside a wrapped command error', () => {
    expect(parseMediaError('Error: engine: [rt:refused] boom').kind).toBe('refused');
  });

  it('leaves untagged errors alone', () => {
    expect(parseMediaError('live streams are not supported')).toEqual({ kind: null, details: 'live streams are not supported' });
  });

  it('truncates the raw line to 300 characters, strips control characters and keeps the hint', () => {
    const parsed = parseMediaError(`[rt:refused] ${'x'.repeat(500)}\u0007\n | needs a JavaScript runtime`);
    const [raw, hint] = parsed.details.split(' | ');
    expect(raw?.length).toBe(300);
    expect(hint).toBe('needs a JavaScript runtime');
    expect(Array.from(parsed.details).every((ch) => ch.charCodeAt(0) >= 32)).toBeTrue();
  });
});
