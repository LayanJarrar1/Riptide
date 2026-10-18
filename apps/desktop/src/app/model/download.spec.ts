import { isMediaUrl } from './download';

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
