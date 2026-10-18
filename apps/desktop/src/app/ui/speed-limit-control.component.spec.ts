import { isValidProxyUrl } from '../features/settings/settings-page.component';
import { toLimitBps, unitFor } from './speed-limit-control.component';

describe('speed limit helpers', () => {
  it('converts amounts to bytes per second; blanks and non-positive are unlimited', () => {
    expect(toLimitBps(500, 'kbps')).toBe(500 * 1024);
    expect(toLimitBps(2.5, 'mbps')).toBe(2.5 * 1024 * 1024);
    expect(toLimitBps(0, 'mbps')).toBeNull();
    expect(toLimitBps(-1, 'kbps')).toBeNull();
    expect(toLimitBps(Number.NaN, 'kbps')).toBeNull();
  });

  it('picks the unit a limit reads best in', () => {
    expect(unitFor(null)).toBe('kbps');
    expect(unitFor(300 * 1024)).toBe('kbps');
    expect(unitFor(5 * 1024 * 1024)).toBe('mbps');
  });
});

describe('isValidProxyUrl', () => {
  it('accepts http, https and socks5 proxies', () => {
    for (const ok of ['http://127.0.0.1:8080', 'https://proxy.example', 'socks5://user:pw@10.0.0.1:1080', 'socks5h://h:1/']) {
      expect(isValidProxyUrl(ok)).withContext(ok).toBeTrue();
    }
  });

  it('refuses anything else', () => {
    for (const bad of ['', 'proxy:8080', 'ftp://proxy', 'socks4://p:1', 'http://p:1/path', 'http://p:1/?q=1']) {
      expect(isValidProxyUrl(bad)).withContext(bad).toBeFalse();
    }
  });
});
