import { CrawlFile, groupRows, hostOf, isHttpLink, matchesFilter, megabytesToBytes, parsePatterns, toUiEvent } from './crawl';

const f = (name: string, category: CrawlFile['category'], host = 'a.example'): CrawlFile => ({ url: `https://${host}/${name}`, name, host, category, size: null });

describe('crawl model', () => {
  it('maps wire events to dialog events', () => {
    expect(toUiEvent({ type: 'page-visited', url: 'https://a.example/', depth: 0, links: 4 })).toEqual({ kind: 'page', url: 'https://a.example/', depth: 0 });
    expect(toUiEvent({ type: 'file-found', url: 'https://cdn.a.example/x.zip', guessed_name: 'x.zip', category: 'compressed', size: 10 })).toEqual({
      kind: 'file',
      file: { url: 'https://cdn.a.example/x.zip', name: 'x.zip', host: 'cdn.a.example', category: 'compressed', size: 10 },
    });
    expect(toUiEvent({ type: 'skipped', url: 'mailto:a@b.c' })).toEqual({ kind: 'skipped', url: 'mailto:a@b.c' });
    expect(toUiEvent({ type: 'done', stats: { pages_visited: 3, files_found: 5, skipped: 2 } })).toEqual({ kind: 'done', counters: { pages: 3, files: 5, skipped: 2 } });
  });

  it('groups by type in sidebar order, leaves empty groups out, and filters by name, host or link', () => {
    const files = [f('a.pdf', 'documents'), f('b.mp3', 'music'), f('c.zip', 'compressed', 'cdn.a.example'), f('d.pdf', 'documents')];
    expect(groupRows(files, '').map((r) => (r.kind === 'group' ? `${r.category}:${r.count}` : r.file.name))).toEqual(['compressed:1', 'c.zip', 'documents:2', 'a.pdf', 'd.pdf', 'music:1', 'b.mp3']);
    expect(groupRows(files, 'CDN').length).toBe(2);
    expect(groupRows(files, '  ').length).toBe(7);
    expect(groupRows(files, 'zzz')).toEqual([]);
    expect(matchesFilter(files[0], 'a.PDF')).toBeTrue();
  });

  it('reads pattern boxes, links and sizes', () => {
    expect(parsePatterns(' *.pdf\r\n\n*.zip, /x/** ,')).toEqual(['*.pdf', '*.zip', '/x/**']);
    expect(isHttpLink('https://a.example/x')).toBeTrue();
    expect(isHttpLink(' HTTP://a.example ')).toBeTrue();
    for (const bad of ['ftp://a.example', 'a.example', 'https://', 'javascript:alert(1)', 'https://a b']) {
      expect(isHttpLink(bad)).withContext(bad).toBeFalse();
    }
    expect(megabytesToBytes('1,5')).toBe(1.5 * 1024 * 1024);
    expect(megabytesToBytes('')).toBeNull();
    expect(megabytesToBytes('-1')).toBeNull();
    expect(megabytesToBytes('abc')).toBeNull();
    expect(hostOf('not a url')).toBe('');
  });
});
