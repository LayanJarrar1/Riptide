import { MemoryPreferenceStore, PREFERENCE_STORE } from '../../core/preferences';
import { NARROW_QUERY, NAV_MAIN_KEY, NAV_RAIL_KEY, NarrowQuery, NavRailService, prefersRail } from '../../core/nav-rail.service';
import { TestBed } from '@angular/core/testing';
import { provideRouter, Router } from '@angular/router';

import { chooseCategory, chooseMode, durationText, platformOf, reasonLines } from './research-modes';
import { clampSide, LAST_CATEGORY_KEY, ResearchLayoutService, SIDE_KEY } from './research-layout.service';

describe('mode and category coupling', () => {
  it('choosing Video Search sets Videos, with a note only when it changed a chosen category', () => {
    expect(chooseMode('video-search', null)).toEqual({ mode: 'video-search', category: 'videos', note: null });
    expect(chooseMode('video-search', 'software')).toEqual({ mode: 'video-search', category: 'videos', note: 'research.note.videoSets' });
    expect(chooseMode('video-search', 'videos')).toEqual({ mode: 'video-search', category: 'videos', note: null });
  });

  it('leaves every other mode alone', () => {
    expect(chooseMode('deep-research', 'images')).toEqual({ mode: 'deep-research', category: 'images', note: null });
    expect(chooseMode('source-finder', null)).toEqual({ mode: 'source-finder', category: null, note: null });
  });

  it('choosing another category while Video Search is on goes back to Quick Search with a note', () => {
    expect(chooseCategory('documents', 'video-search')).toEqual({ mode: 'quick-search', category: 'documents', note: 'research.note.videoOff' });
    expect(chooseCategory('videos', 'video-search')).toEqual({ mode: 'video-search', category: 'videos', note: null });
    expect(chooseCategory('documents', 'deep-research')).toEqual({ mode: 'deep-research', category: 'documents', note: null });
  });
});

describe('reasons and facts of a result', () => {
  it('lists the most telling reasons first and at most three', () => {
    const lines = reasonLines([
      { kind: 'loose_title' },
      { kind: 'missing', constraint: 'season' },
      { kind: 'matched', constraint: 'episode', value: '22' },
      { kind: 'title_all_words' },
      { kind: 'conflict', constraint: 'year', wanted: '2019', found: '2020' },
    ]);
    expect(lines.map((l) => l.key)).toEqual(['research.reason.conflict.year', 'research.reason.matched.episode', 'research.reason.missing.season']);
    expect(lines[1]).toEqual({ key: 'research.reason.matched.episode', params: { value: '22' }, tone: 'good' });
    expect(reasonLines([])).toEqual([]);
  });

  it('formats a running time only when there is one', () => {
    expect(durationText(null)).toBeNull();
    expect(durationText(0)).toBeNull();
    expect(durationText(65)).toBe('1:05');
    expect(durationText(3725)).toBe('1:02:05');
  });

  it('names a platform only from the file extension', () => {
    expect(platformOf('app-setup.EXE', 'https://e.org/x')).toBe('windows');
    expect(platformOf(null, 'https://e.org/app.dmg?x=1')).toBe('macos');
    expect(platformOf('app.AppImage', 'https://e.org/x')).toBe('linux');
    expect(platformOf('app.apk', 'https://e.org/x')).toBe('android');
    expect(platformOf('app.zip', 'https://e.org/x')).toBeNull();
  });
});

describe('research layout preferences', () => {
  function layout(initial: Record<string, string> = {}): { service: ResearchLayoutService; store: MemoryPreferenceStore } {
    const store = new MemoryPreferenceStore(initial);
    TestBed.configureTestingModule({ providers: [{ provide: PREFERENCE_STORE, useValue: store }] });
    return { service: TestBed.inject(ResearchLayoutService), store };
  }

  it('keeps the sidebar width between 220 and 420 and remembers it', () => {
    expect([clampSide(100), clampSide(300), clampSide(900), clampSide(Number.NaN)]).toEqual([220, 300, 420, 264]);
    const { service, store } = layout();
    expect(service.sideWidth()).toBe(264);
    service.setSideWidth(500);
    expect(service.sideWidth()).toBe(420);
    expect(JSON.parse(store.get(SIDE_KEY) ?? '{}')).toEqual({ open: true, width: 420 });
  });

  it('does not write storage while a drag is in progress', () => {
    const { service, store } = layout();
    service.setSideWidth(300, false);
    expect(store.get(SIDE_KEY)).toBeNull();
    service.setSideWidth(300);
    expect(JSON.parse(store.get(SIDE_KEY) ?? '{}').width).toBe(300);
  });

  it('reads a stored layout back and ignores garbage', () => {
    expect(layout({ [SIDE_KEY]: JSON.stringify({ open: false, width: 999 }) }).service.sideOpen()).toBe(false);
    TestBed.resetTestingModule();
    const { service } = layout({ [SIDE_KEY]: 'not json', [LAST_CATEGORY_KEY]: 'rm -rf' });
    expect(service.sideOpen()).toBe(true);
    expect(service.sideWidth()).toBe(264);
    expect(service.lastCategory()).toBeNull();
  });

  it('remembers the last category as a suggestion', () => {
    const { service, store } = layout();
    service.rememberCategory('images');
    expect(store.get(LAST_CATEGORY_KEY)).toBe('images');
    TestBed.resetTestingModule();
    expect(layout({ [LAST_CATEGORY_KEY]: 'images' }).service.lastCategory()).toBe('images');
  });
});

describe('navigation rail', () => {
  const never: NarrowQuery = { matches: false, addEventListener: (): void => undefined };

  function make(store: MemoryPreferenceStore, narrow: NarrowQuery = never): NavRailService {
    TestBed.resetTestingModule();
    TestBed.configureTestingModule({
      providers: [
        provideRouter([{ path: '**', children: [] }]),
        { provide: PREFERENCE_STORE, useValue: store },
        { provide: NARROW_QUERY, useValue: narrow },
      ],
    });
    return TestBed.inject(NavRailService);
  }

  async function at(url: string): Promise<void> {
    await TestBed.inject(Router).navigateByUrl(url);
  }

  it('applies on Research and Files only', () => {
    expect(['/research', '/research?x=1', '/files', '/files/a'].every(prefersRail)).toBe(true);
    expect(['/', '/settings', '/researchers', '/#/research'].some(prefersRail)).toBe(false);
  });

  it('starts compact on Research and Files, expanded on Downloads and Settings', async () => {
    const rail = make(new MemoryPreferenceStore());
    for (const [url, expanded] of [['/', true], ['/settings', true], ['/research', false], ['/files', false]] as const) {
      await at(url);
      expect(rail.expanded()).withContext(url).toBe(expanded);
      expect(rail.rail()).withContext(url).toBe(!expanded);
    }
  });

  it('remembers the choice per page group, without losing the old stored value', async () => {
    const store = new MemoryPreferenceStore({ [NAV_RAIL_KEY]: 'expanded' });
    let rail = make(store);
    await at('/research');
    expect(rail.expanded()).toBe(true); // the value stored by the previous version still applies
    rail.toggle();
    expect(store.get(NAV_RAIL_KEY)).toBe('rail');
    await at('/settings');
    expect(rail.expanded()).toBe(true); // the other group is untouched
    rail.toggle();
    expect(store.get(NAV_MAIN_KEY)).toBe('rail');
    expect(store.get(NAV_RAIL_KEY)).toBe('rail');
    rail = make(store);
    await at('/');
    expect(rail.expanded()).toBe(false);
    await at('/files');
    expect(rail.expanded()).toBe(false);
    rail.toggle();
    expect(store.get(NAV_RAIL_KEY)).toBe('expanded');
    expect(store.get(NAV_MAIN_KEY)).toBe('rail');
  });

  it('under 900px it is always a rail; the toggle opens a temporary overlay that is not remembered', async () => {
    const store = new MemoryPreferenceStore();
    const rail = make(store, { matches: true, addEventListener: (): void => undefined });
    await at('/');
    expect(rail.rail()).toBe(true);
    expect(rail.overlay()).toBe(false);
    rail.toggle();
    expect(rail.overlay()).toBe(true);
    expect(rail.expanded()).toBe(true);
    expect(store.get(NAV_MAIN_KEY)).toBeNull();
    rail.closeOverlay();
    expect(rail.overlay()).toBe(false);
    rail.toggle();
    await at('/files');
    expect(rail.overlay()).toBe(false); // navigating closes it
  });
});
