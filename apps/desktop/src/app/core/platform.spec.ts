import { detectMac, keyLabel, shortcutLabel } from './platform';

describe('platform', () => {
  it('recognises a Mac from any of the navigator fields', () => {
    expect(detectMac({ platform: 'MacIntel' })).toBeTrue();
    expect(detectMac({ userAgentData: { platform: 'macOS' } })).toBeTrue();
    expect(detectMac({ userAgent: 'Mozilla/5.0 (Macintosh; Intel Mac OS X 14_6) AppleWebKit/605.1.15' })).toBeTrue();
    expect(detectMac({ platform: 'Win32', userAgent: 'Mozilla/5.0 (Windows NT 10.0; Win64; x64)' })).toBeFalse();
    expect(detectMac(undefined)).toBeFalse();
  });

  it('keeps Windows shortcuts as they are', () => {
    expect(shortcutLabel('Ctrl+Shift+N', false)).toBe('Ctrl+Shift+N');
    expect(keyLabel('Ctrl', false)).toBe('Ctrl');
  });

  it('writes Mac shortcuts with the modifier symbols in Apple order', () => {
    expect(shortcutLabel('Ctrl+N', true)).toBe('⌘N');
    expect(shortcutLabel('Ctrl+Shift+N', true)).toBe('⇧⌘N');
    expect(shortcutLabel('Ctrl+,', true)).toBe('⌘,');
    expect(shortcutLabel('Ctrl+B', true)).toBe('⌘B');
    expect(shortcutLabel('Alt ↑', true)).toBe('⌥ ↑');
    expect(shortcutLabel('Space', true)).toBe('Space');
    expect(shortcutLabel('Del', true)).toBe('Del');
    expect(keyLabel('Ctrl', true)).toBe('⌘');
    expect(keyLabel('Shift', true)).toBe('⇧');
    expect(keyLabel('N', true)).toBe('N');
  });
});
