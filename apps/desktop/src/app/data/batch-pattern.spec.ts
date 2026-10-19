import type { BatchError } from '../tauri/bridge/BatchError';
import { exportUrlList, expandPattern, importUrlList, isHttpUrl, MAX_EXPANSION } from './batch-pattern';

// The same cases as the Rust tests in crates/riptide-core/src/batch.rs: this port and the engine
// must agree.
describe('batch pattern expander', () => {
  const ok = (p: string): readonly string[] => {
    const r = expandPattern(p);
    expect(r.error).toBeNull();
    return r.urls;
  };

  it('keeps the width of a zero-padded range', () => {
    const urls = ok('https://h/file[001-120].zip');
    expect(urls.length).toBe(120);
    expect(urls[0]).toBe('https://h/file001.zip');
    expect(urls[9]).toBe('https://h/file010.zip');
    expect(urls[119]).toBe('https://h/file120.zip');
  });

  it('handles unpadded, descending, letter and stepped ranges', () => {
    expect(ok('http://h/[8-10]')).toEqual(['http://h/8', 'http://h/9', 'http://h/10']);
    expect(ok('http://h/[3-1]')).toEqual(['http://h/3', 'http://h/2', 'http://h/1']);
    expect(ok('http://h/img_[a-d].jpg').length).toBe(4);
    expect(ok('http://h/[C-A]')).toEqual(['http://h/C', 'http://h/B', 'http://h/A']);
    const stepped = ok('http://h/p[0-100:5]');
    expect(stepped.length).toBe(21);
    expect(stepped[1]).toBe('http://h/p5');
    expect(stepped[20]).toBe('http://h/p100');
    expect(ok('http://h/[a-z:5]').join(' ')).toBe('http://h/a http://h/f http://h/k http://h/p http://h/u http://h/z');
  });

  it('is a cartesian product with the rightmost group fastest', () => {
    expect(ok('http://h/a[1-2]b[x-y]')).toEqual(['http://h/a1bx', 'http://h/a1by', 'http://h/a2bx', 'http://h/a2by']);
    expect(ok('http://h[1-3].example/v[01-10]/f[a-c]').length).toBe(90);
  });

  it('escapes a literal bracket', () => {
    expect(ok('http://h/a\\[1\\]-[1-2]')).toEqual(['http://h/a[1]-1', 'http://h/a[1]-2']);
    expect(ok('http://h/x]y')).toEqual(['http://h/x]y']);
    expect(ok('http://h/a\\\\b')).toEqual(['http://h/a\\b']);
    expect(ok('http://h/a\\b[1-1]')).toEqual(['http://h/a\\b1']);
  });

  it('trims the pattern and keeps non-ASCII text', () => {
    expect(ok('  http://h/a.zip \n')).toEqual(['http://h/a.zip']);
    expect(ok('http://h/ملف[1-2].zip')).toEqual(['http://h/ملف1.zip', 'http://h/ملف2.zip']);
  });

  it('reports typed errors', () => {
    const code = (p: string): string | undefined => expandPattern(p).error?.code;
    expect(code('  ')).toBe('empty');
    expect(code('ftp://h/[1-2]')).toBe('not_http');
    expect(code('http:///[1-2]')).toBe('not_http');
    expect(code('htt[p-q]://h/')).toBe('not_http');
    expect(code('http://h/[1-2')).toBe('unclosed');
    for (const p of ['http://h/[1-]', 'http://h/[a-5]', 'http://h/[a-Z]', 'http://h/[ab-cd]', 'http://h/[1-5:x]', 'http://h/[1-[2]', 'http://h/[]', 'http://h/[5]']) {
      expect(code(p)).withContext(p).toBe('invalid_range');
    }
    expect(code('http://h/[1-5:0]')).toBe('zero_step');
    expect(code('http://h/[1-1234567890123456789]')).toBe('number_too_large');
    expect(code('http://h/a b')).toBe('invalid_char');
    expect(expandPattern('http://h/[1-2').error).toEqual({ code: 'unclosed', at: 9 });
  });

  it('enforces the cap before generating', () => {
    expect(ok('http://h/[1-10000]').length).toBe(MAX_EXPANSION);
    const tooMany: BatchError = { code: 'too_many', limit: MAX_EXPANSION };
    expect(expandPattern('http://h/[1-10001]').error).toEqual(tooMany);
    expect(expandPattern('http://h/[1-100][1-101]').error).toEqual(tooMany);
    expect(expandPattern('http://h/[0-999999999999999999][a-z][a-z]').error).toEqual(tooMany);
    expect(expandPattern('http://h/[1-100][1-100]').urls.length).toBe(10_000);
  });
});

describe('url list import and export', () => {
  it('filters, de-duplicates and keeps order', () => {
    const text =
      '\uFEFF# my list\n\nhttps://a/1.zip\r\n  http://b/2.zip  \nftp://c/3\nnot a url\nhttps://a/1.zip\nhttp://b/2.zip\n#https://d/4\nHTTPS://E/5\n';
    const list = importUrlList(text);
    expect(list.urls).toEqual(['https://a/1.zip', 'http://b/2.zip', 'HTTPS://E/5']);
    expect(list.ignored).toBe(2);
    expect(list.duplicates).toBe(2);
  });

  it('round-trips through export', () => {
    const urls = ['http://a/1', 'https://b/2'];
    expect(exportUrlList(urls)).toBe('http://a/1\nhttps://b/2\n');
    expect(importUrlList(exportUrlList(urls)).urls).toEqual(urls);
    expect(exportUrlList([])).toBe('');
  });

  it('recognizes http urls', () => {
    expect(isHttpUrl('http://h')).toBeTrue();
    expect(isHttpUrl('HTTPS://h/a?b#c')).toBeTrue();
    expect(isHttpUrl('http://')).toBeFalse();
    expect(isHttpUrl('http:///x')).toBeFalse();
    expect(isHttpUrl('httpx://h')).toBeFalse();
    expect(isHttpUrl('http://h/a b')).toBeFalse();
  });
});
