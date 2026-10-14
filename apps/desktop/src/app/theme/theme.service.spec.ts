import { TestBed } from '@angular/core/testing';

import {
  DARK_SCHEME_QUERY,
  DarkSchemeQuery,
  MemoryPreferenceStore,
  PREFERENCE_STORE,
} from '../core/preferences';
import { THEME_STORAGE_KEY, ThemeService } from './theme.service';

class FakeDarkQuery implements DarkSchemeQuery {
  private listeners: ((event: { matches: boolean }) => void)[] = [];

  constructor(public matches: boolean) {}

  addEventListener(_type: 'change', listener: (event: { matches: boolean }) => void): void {
    this.listeners.push(listener);
  }

  /** Simulates the OS switching its colour scheme. */
  flip(matches: boolean): void {
    this.matches = matches;
    for (const listener of this.listeners) {
      listener({ matches });
    }
  }
}

function setup(systemDark: boolean, saved: Record<string, string> = {}) {
  const store = new MemoryPreferenceStore(saved);
  const query = new FakeDarkQuery(systemDark);
  TestBed.configureTestingModule({
    providers: [
      { provide: PREFERENCE_STORE, useValue: store },
      { provide: DARK_SCHEME_QUERY, useValue: query },
    ],
  });
  return { service: TestBed.inject(ThemeService), store, query };
}

describe('ThemeService', () => {
  afterEach(() => {
    delete document.documentElement.dataset['theme'];
  });

  it('starts on "system" and resolves it from the OS preference', () => {
    const dark = setup(true);
    expect(dark.service.theme()).toBe('system');
    expect(dark.service.resolved()).toBe('dark');

    TestBed.resetTestingModule();
    const light = setup(false);
    expect(light.service.resolved()).toBe('light');
  });

  it('restores a saved theme', () => {
    const { service } = setup(true, { [THEME_STORAGE_KEY]: 'light' });
    expect(service.theme()).toBe('light');
    expect(service.resolved()).toBe('light');
  });

  it('ignores an invalid saved value', () => {
    const { service } = setup(false, { [THEME_STORAGE_KEY]: 'neon' });
    expect(service.theme()).toBe('system');
  });

  it('setTheme persists and applies data-theme on <html>', () => {
    const { service, store } = setup(false);
    TestBed.tick();
    expect(document.documentElement.dataset['theme']).toBe('light');

    service.setTheme('dark');
    TestBed.tick();
    expect(service.resolved()).toBe('dark');
    expect(document.documentElement.dataset['theme']).toBe('dark');
    expect(store.get(THEME_STORAGE_KEY)).toBe('dark');
  });

  it('follows the OS while on "system" and ignores it otherwise', () => {
    const { service, query } = setup(false);
    expect(service.resolved()).toBe('light');
    query.flip(true);
    expect(service.resolved()).toBe('dark');

    service.setTheme('light');
    query.flip(false);
    query.flip(true);
    expect(service.resolved()).toBe('light');
  });
});
