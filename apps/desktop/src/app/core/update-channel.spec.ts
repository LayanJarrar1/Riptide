import { effectiveChannel, isPrerelease } from './update-channel';

describe('update channel resolution', () => {
  it('recognises pre-release versions', () => {
    expect(isPrerelease('0.1.0-alpha.4')).toBeTrue();
    expect(isPrerelease('1.0.0-rc.1+build.5')).toBeTrue();
    expect(isPrerelease('1.2.3')).toBeFalse();
    expect(isPrerelease('1.2.3+meta-with-dash')).toBeFalse();
    expect(isPrerelease('')).toBeFalse();
    expect(isPrerelease(null)).toBeFalse();
    expect(isPrerelease(undefined)).toBeFalse();
  });

  it('forces the beta feed for a pre-release build whatever was saved', () => {
    expect(effectiveChannel('stable', '0.1.0-alpha.4')).toBe('beta');
    expect(effectiveChannel('beta', '0.1.0-alpha.4')).toBe('beta');
  });

  it('lets a stable build keep its saved channel', () => {
    expect(effectiveChannel('stable', '1.0.0')).toBe('stable');
    expect(effectiveChannel('beta', '1.0.0')).toBe('beta');
    expect(effectiveChannel('stable', null)).toBe('stable');
  });
});
