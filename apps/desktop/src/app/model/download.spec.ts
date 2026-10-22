import { activeMillis, averageSpeed, isMediaUrl } from './download';

describe('isMediaUrl', () => {
  it('knows the video sites and their subdomains', () => {
    expect(isMediaUrl('https://www.youtube.com/watch?v=abc')).toBeTrue();
    expect(isMediaUrl('https://youtu.be/abc')).toBeTrue();
    expect(isMediaUrl('  https://m.twitch.tv/x  ')).toBeTrue();
  });

  it('rejects look-alikes, plain files and junk', () => {
    expect(isMediaUrl('https://notyoutube.com/watch')).toBeFalse();
    expect(isMediaUrl('https://youtube.com.evil.example/x')).toBeFalse();
    expect(isMediaUrl('https://example.com/file.zip')).toBeFalse();
    expect(isMediaUrl('not a url')).toBeFalse();
  });
});

describe('active time and average speed', () => {
  const now = 1_000_000;

  it('adds the running span only while downloading', () => {
    expect(activeMillis({ activeMs: 5_000, activeSince: now - 2_000, state: 'downloading' }, now)).toBe(7_000);
    expect(activeMillis({ activeMs: 5_000, activeSince: null, state: 'paused' }, now)).toBe(5_000);
    // a stale span on a row that is no longer downloading is ignored
    expect(activeMillis({ activeMs: 5_000, activeSince: now - 2_000, state: 'completed' }, now)).toBe(5_000);
    // a clock that went backwards never subtracts time
    expect(activeMillis({ activeMs: 5_000, activeSince: now + 9_000, state: 'downloading' }, now)).toBe(5_000);
  });

  it('never invents a time that was not recorded', () => {
    expect(activeMillis({ activeMs: null, activeSince: now - 2_000, state: 'downloading' }, now)).toBeNull();
    expect(activeMillis({ state: 'completed' }, now)).toBeNull();
    expect(averageSpeed(1_000_000, null)).toBeNull();
  });

  it('averages bytes over the active time, not over wall-clock time', () => {
    expect(averageSpeed(10_000_000, 4_000)).toBe(2_500_000);
    // under a second (or nothing downloaded) says nothing
    expect(averageSpeed(10_000_000, 999)).toBeNull();
    expect(averageSpeed(0, 60_000)).toBeNull();
  });
});
