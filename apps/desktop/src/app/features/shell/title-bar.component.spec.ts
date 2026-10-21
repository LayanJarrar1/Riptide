import { ComponentFixture, TestBed } from '@angular/core/testing';
import { provideRouter } from '@angular/router';

import { MemoryPreferenceStore, NAVIGATOR_LANGUAGES, PREFERENCE_STORE } from '../../core/preferences';
import { NATIVE_WINDOW, NativeWindow } from '../../core/window.service';
import { DownloadEngine } from '../../data/download-engine';
import { MOCK_ENGINE_AUTOSTART, MockEngineService } from '../../data/mock-engine.service';
import { I18nService } from '../../i18n/i18n.service';
import { TAURI_AVAILABLE, TAURI_INVOKE } from '../../tauri/tauri-bridge';
import { SettingsService } from '../settings/settings.service';
import { TitleBarComponent } from './title-bar.component';
import { UiState } from './ui-state.service';

/** A native window that records what it was asked and keeps its own maximized flag. */
class FakeWindow implements NativeWindow {
  readonly calls: string[] = [];
  maximized = false;

  minimize(): Promise<void> {
    this.calls.push('minimize');
    return Promise.resolve();
  }

  maximize(): Promise<void> {
    this.calls.push('maximize');
    this.maximized = true;
    return Promise.resolve();
  }

  unmaximize(): Promise<void> {
    this.calls.push('unmaximize');
    this.maximized = false;
    return Promise.resolve();
  }

  close(): Promise<void> {
    this.calls.push('close');
    return Promise.resolve();
  }

  isMaximized(): Promise<boolean> {
    return Promise.resolve(this.maximized);
  }
}

describe('TitleBarComponent', () => {
  let fixture: ComponentFixture<TitleBarComponent>;
  let native: FakeWindow;
  let ui: UiState;
  let i18n: I18nService;
  let invoked: { cmd: string; args?: Record<string, unknown> }[];

  const el = (): HTMLElement => fixture.nativeElement as HTMLElement;
  const tops = (): HTMLButtonElement[] => Array.from(el().querySelectorAll<HTMLButtonElement>('.top-button'));
  const top = (name: string): HTMLButtonElement => {
    const b = tops().find((t) => t.textContent?.replace(/\s+/g, '') === name);
    if (b === undefined) {
      throw new Error(`no top item ${name}`);
    }
    return b;
  };
  const menu = (): HTMLElement | null => el().querySelector<HTMLElement>('[role="menu"]');
  const rows = (): HTMLButtonElement[] => Array.from(el().querySelectorAll<HTMLButtonElement>('[role="menu"] .item'));
  const row = (label: string): HTMLButtonElement => {
    const r = rows().find((b) => b.querySelector('.label')?.textContent?.trim() === label);
    if (r === undefined) {
      throw new Error(`no menu row ${label}`);
    }
    return r;
  };

  /** Lets the lazy menu chunk load and renders. */
  async function settle(): Promise<void> {
    fixture.detectChanges();
    await fixture.whenStable();
    fixture.detectChanges();
    await new Promise((r) => setTimeout(r, 0));
    fixture.detectChanges();
  }

  function key(target: Element, k: string, init: KeyboardEventInit = {}): void {
    target.dispatchEvent(new KeyboardEvent('keydown', { key: k, bubbles: true, cancelable: true, ...init }));
  }

  async function create(tauri: boolean, lang = 'en-US'): Promise<void> {
    invoked = [];
    native = new FakeWindow();
    spyOn(console, 'warn');
    await TestBed.configureTestingModule({
      imports: [TitleBarComponent],
      providers: [
        provideRouter([]),
        // the simulation even where the bridge is "available": the spec drives its list
        { provide: DownloadEngine, useExisting: MockEngineService },
        { provide: MOCK_ENGINE_AUTOSTART, useValue: false },
        { provide: NAVIGATOR_LANGUAGES, useValue: [lang] },
        { provide: PREFERENCE_STORE, useValue: new MemoryPreferenceStore() },
        { provide: TAURI_AVAILABLE, useValue: tauri },
        {
          provide: TAURI_INVOKE,
          useValue: <T>(cmd: string, args?: Record<string, unknown>): Promise<T> => {
            if (cmd === 'get_engine_settings' || cmd === 'autostart_enabled') {
              return Promise.reject(new Error('no engine in this spec'));
            }
            invoked.push({ cmd, args });
            return Promise.resolve(undefined as unknown as T);
          },
        },
        { provide: NATIVE_WINDOW, useValue: () => (tauri ? Promise.resolve(native) : null) },
      ],
    }).compileComponents();
    ui = TestBed.inject(UiState);
    i18n = TestBed.inject(I18nService);
    await i18n.ready();
    fixture = TestBed.createComponent(TitleBarComponent);
    document.body.appendChild(fixture.nativeElement);
    await settle();
  }

  afterEach(() => {
    fixture.nativeElement.remove();
    document.documentElement.lang = 'en';
    document.documentElement.dir = 'ltr';
  });

  describe('in a plain browser', () => {
    beforeEach(async () => {
      await create(false);
    });

    it('draws the mark and a menu bar with five menus (no Window menu) and no window controls', () => {
      expect(el().querySelector('img[src="brand-mark.svg"]')).not.toBeNull();
      expect(el().querySelector('[role="menubar"]')?.getAttribute('aria-label')).toBe('Application menu');
      expect(tops().map((t) => t.textContent?.replace(/\s+/g, ''))).toEqual(['File', 'Downloads', 'Tools', 'View', 'Help']);
      expect(el().querySelector('.controls')).toBeNull();
      expect(el().querySelectorAll('[data-tauri-drag-region]').length).toBeGreaterThan(2);
    });

    it('has one tab stop (roving tabindex) and the menu button semantics', () => {
      expect(tops().map((t) => t.tabIndex)).toEqual([0, -1, -1, -1, -1]);
      expect(tops().every((t) => t.getAttribute('aria-haspopup') === 'menu' && t.getAttribute('role') === 'menuitem')).toBeTrue();
      expect(top('File').getAttribute('aria-expanded')).toBe('false');
      expect(tops().map((t) => t.getAttribute('aria-keyshortcuts'))).toEqual(['Alt+F', 'Alt+D', 'Alt+T', 'Alt+V', 'Alt+H']);
    });

    it('underlines the mnemonic letter only while the keyboard drives the bar', async () => {
      expect(top('File').querySelector('.mn')?.textContent).toBe('F');
      expect(getComputedStyle(top('File').querySelector('.mn') as Element).textDecorationLine).toBe('none');
      key(document.body, 'F10');
      fixture.detectChanges();
      expect(getComputedStyle(top('File').querySelector('.mn') as Element).textDecorationLine).toBe('underline');
      key(top('File'), 'Escape');
      await settle();
    });

    it('opens a menu on click and shows its rows with shortcut hints', async () => {
      top('File').click();
      await settle();
      expect(top('File').getAttribute('aria-expanded')).toBe('true');
      expect(menu()?.id).toBe('menu-file');
      expect(menu()?.getAttribute('aria-labelledby')).toBe('menu-button-file');
      expect(rows().map((r) => r.querySelector('.label')?.textContent?.trim())).toEqual([
        'Add download',
        'Batch download',
        'Site grabber',
        'Import link list…',
        'Open torrent file…',
        'Open magnet link from clipboard',
        'Open file',
        'Show in folder',
      ]);
      expect(row('Add download').querySelector('kbd')?.textContent).toBe('Ctrl+N');
      // nothing is selected, so the file commands are present but not available
      expect(row('Open file').getAttribute('aria-disabled')).toBe('true');
    });

    it('a click on the open menu closes it, and Escape closes it and returns focus', async () => {
      top('File').click();
      await settle();
      top('File').click();
      await settle();
      expect(menu()).toBeNull();

      document.body.focus();
      const outside = document.createElement('button');
      document.body.appendChild(outside);
      outside.focus();
      top('Tools').click();
      await settle();
      expect(menu()).not.toBeNull();
      key(menu() as HTMLElement, 'Escape');
      await settle();
      expect(menu()).toBeNull();
      expect(document.activeElement).toBe(outside);
      outside.remove();
    });

    it('switches menus on hover while one is open, but not before', async () => {
      top('File').parentElement?.dispatchEvent(new PointerEvent('pointerenter'));
      top('Tools').parentElement?.dispatchEvent(new PointerEvent('pointerenter'));
      await settle();
      expect(menu()).toBeNull();
      top('File').click();
      await settle();
      top('Tools').parentElement?.dispatchEvent(new PointerEvent('pointerenter'));
      await settle();
      expect(menu()?.id).toBe('menu-tools');
      expect(top('File').getAttribute('aria-expanded')).toBe('false');
    });

    it('closes on a click outside the bar', async () => {
      top('View').click();
      await settle();
      expect(menu()).not.toBeNull();
      document.body.dispatchEvent(new PointerEvent('pointerdown', { bubbles: true }));
      await settle();
      expect(menu()).toBeNull();
    });

    it('Arrow keys walk the bar; Down opens a menu with focus on its first row', async () => {
      top('File').focus();
      key(top('File'), 'ArrowRight');
      expect(document.activeElement).toBe(top('Downloads'));
      fixture.detectChanges();
      expect(top('Downloads').tabIndex).toBe(0);
      expect(top('File').tabIndex).toBe(-1);
      key(top('Downloads'), 'ArrowLeft');
      key(top('File'), 'ArrowLeft'); // wraps to the last
      expect(document.activeElement).toBe(top('Help'));
      key(top('Help'), 'Home');
      expect(document.activeElement).toBe(top('File'));
      key(top('File'), 'End');
      expect(document.activeElement).toBe(top('Help'));

      key(top('Tools'), 'ArrowDown');
      await settle();
      expect(menu()?.id).toBe('menu-tools');
      expect(document.activeElement).toBe(rows()[0]);
    });

    it('moves inside a menu with Up / Down / Home / End, skipping unavailable rows, and wraps', async () => {
      top('File').focus();
      key(top('File'), 'Enter');
      await settle();
      expect(document.activeElement).toBe(row('Add download'));
      key(row('Add download'), 'ArrowDown');
      expect(document.activeElement).toBe(row('Batch download'));
      key(row('Batch download'), 'End');
      // Open file / Show in folder are unavailable: the last reachable row is the magnet one
      expect(document.activeElement).toBe(row('Open magnet link from clipboard'));
      key(row('Open magnet link from clipboard'), 'ArrowDown');
      expect(document.activeElement).toBe(row('Add download'));
      key(row('Add download'), 'ArrowUp');
      expect(document.activeElement).toBe(row('Open magnet link from clipboard'));
      key(row('Open magnet link from clipboard'), 'Home');
      expect(document.activeElement).toBe(row('Add download'));
    });

    it('Left / Right inside a menu open the neighbour', async () => {
      top('File').focus();
      key(top('File'), 'ArrowDown');
      await settle();
      key(row('Add download'), 'ArrowRight');
      await settle();
      expect(menu()?.id).toBe('menu-downloads');
      key(rows().find((r) => !r.getAttribute('aria-disabled')) as HTMLElement, 'ArrowLeft');
      await settle();
      expect(menu()?.id).toBe('menu-file');
    });

    it('Up on a top item opens the menu on its last row', async () => {
      top('Help').focus();
      key(top('Help'), 'ArrowUp');
      await settle();
      expect(document.activeElement).toBe(rows()[rows().length - 1]);
    });

    it('choosing a row runs the command and closes the menu; an unavailable row does nothing', async () => {
      top('File').click();
      await settle();
      row('Open file').click();
      await settle();
      expect(menu()).not.toBeNull();
      row('Add download').click();
      await settle();
      expect(ui.addOpen()).toBeTrue();
      expect(menu()).toBeNull();
    });

    it('Alt + the underlined letter opens that menu; F10 and a lone Alt focus the bar', async () => {
      key(document.body, 'v', { altKey: true });
      await settle();
      expect(menu()?.id).toBe('menu-view');
      expect(el().querySelector('.bar')?.classList).toContain('is-keyboard');
      key(menu() as HTMLElement, 'Escape');
      await settle();

      // the bar remembers where it was last: View
      key(document.body, 'F10');
      expect(document.activeElement).toBe(top('View'));
      top('View').blur();

      document.dispatchEvent(new KeyboardEvent('keydown', { key: 'Alt', bubbles: true }));
      document.dispatchEvent(new KeyboardEvent('keyup', { key: 'Alt', bubbles: true }));
      expect(document.activeElement).toBe(top('View'));
    });

    it('Alt is not a bar shortcut when another key was pressed in between', () => {
      document.dispatchEvent(new KeyboardEvent('keydown', { key: 'Alt', bubbles: true }));
      document.dispatchEvent(new KeyboardEvent('keydown', { key: 'ArrowUp', altKey: true, bubbles: true }));
      document.dispatchEvent(new KeyboardEvent('keyup', { key: 'Alt', bubbles: true }));
      expect(el().contains(document.activeElement)).toBeFalse();
    });

    it('View shows the current theme, language and density as checked radios and switches live', async () => {
      top('View').click();
      await settle();
      const checked = (): string[] => rows().filter((r) => r.getAttribute('aria-checked') === 'true').map((r) => r.querySelector('.label')?.textContent?.trim() ?? '');
      expect(checked()).toEqual(['Light', 'English', 'Comfortable', 'Sidebar']);
      expect(row('Dark').getAttribute('aria-label')).toBe('Appearance: Dark');
      row('Compact').click();
      await settle();
      top('View').click();
      await settle();
      expect(checked()).toContain('Compact');
    });

    it('View > Language switches the app language live and the bar follows (Arabic labels, mirrored)', async () => {
      top('View').click();
      await settle();
      row('العربية').click();
      for (let i = 0; i < 100 && i18n.lang() !== 'ar'; i++) {
        await new Promise((r) => setTimeout(r, 10));
      }
      await settle();
      expect(i18n.lang()).toBe('ar');
      expect(tops().map((t) => t.textContent?.replace(/\s+/g, ''))).toEqual(['ملف', 'التنزيلات', 'أدوات', 'عرض', 'مساعدة']);
      expect(el().querySelector('.start')?.getAttribute('dir')).toBe('rtl');
    });
  });

  describe('in Arabic', () => {
    beforeEach(async () => {
      await create(true, 'ar-SA');
    });

    it('mirrors the bar but keeps the window controls at the physical right', () => {
      expect(el().querySelector('.start')?.getAttribute('dir')).toBe('rtl');
      expect(el().querySelector('.bar')).not.toBeNull();
      const bar = getComputedStyle(el().querySelector('.bar') as HTMLElement);
      expect(bar.direction).toBe('ltr');
      const controls = (el().querySelector('.controls') as HTMLElement).getBoundingClientRect();
      const start = (el().querySelector('.start') as HTMLElement).getBoundingClientRect();
      const drag = (el().querySelector('.drag') as HTMLElement).getBoundingClientRect();
      expect(controls.left).toBeGreaterThan(start.left);
      expect(start.left).toBeGreaterThan(drag.left - 1);
      expect(tops().map((t) => t.textContent?.replace(/\s+/g, ''))).toEqual(['ملف', 'التنزيلات', 'أدوات', 'عرض', 'نافذة', 'مساعدة']);
    });

    it('does not split an Arabic label with a span (it would cut the joining of its letters)', () => {
      expect(tops().every((t) => t.querySelector('.mn') === null)).toBeTrue();
      expect(tops().map((t) => t.getAttribute('aria-keyshortcuts'))).toEqual(['Alt+م', 'Alt+ت', 'Alt+د', 'Alt+ع', 'Alt+ن', 'Alt+س']);
    });

    it('arrow keys are mirrored: Left goes to the next menu', () => {
      top('ملف').focus();
      key(top('ملف'), 'ArrowLeft');
      expect(document.activeElement).toBe(top('التنزيلات'));
      key(top('التنزيلات'), 'ArrowRight');
      expect(document.activeElement).toBe(top('ملف'));
    });

    it('Alt + the Arabic mnemonic opens the menu', async () => {
      key(document.body, 'ع', { altKey: true });
      await settle();
      expect(menu()?.id).toBe('menu-view');
      expect(rows().map((r) => r.querySelector('.label')?.textContent?.trim())).toContain('الشريط الجانبي');
    });
  });

  describe('inside the desktop app', () => {
    beforeEach(async () => {
      await create(true);
    });

    it('draws minimize, maximize and close as named buttons at the end of the bar', () => {
      const buttons = Array.from(el().querySelectorAll<HTMLButtonElement>('.controls .ctl'));
      expect(buttons.map((b) => b.getAttribute('aria-label'))).toEqual(['Minimize', 'Maximize', 'Close']);
      expect(tops().map((t) => t.textContent?.replace(/\s+/g, ''))).toContain('Window');
      expect(el().querySelector('.controls')?.compareDocumentPosition(el().querySelector('.drag') as Node)).toBe(Node.DOCUMENT_POSITION_PRECEDING);
      expect(buttons.every((b) => b.getBoundingClientRect().width >= 28 && b.getBoundingClientRect().height >= 28)).toBeTrue();
    });

    it('minimize, maximize and close reach the native window; the maximize button turns into restore', async () => {
      const [min, max, close] = Array.from(el().querySelectorAll<HTMLButtonElement>('.controls .ctl'));
      min.click();
      await settle();
      expect(native.calls).toEqual(['minimize']);

      max.click();
      await settle();
      expect(native.calls).toEqual(['minimize', 'maximize']);
      expect(el().querySelectorAll<HTMLButtonElement>('.controls .ctl')[1].getAttribute('aria-label')).toBe('Restore');
      expect(el().querySelector('.bar')?.classList).toContain('is-maximized');

      el().querySelectorAll<HTMLButtonElement>('.controls .ctl')[1].click();
      await settle();
      expect(native.calls).toEqual(['minimize', 'maximize', 'unmaximize']);
      expect(el().querySelectorAll<HTMLButtonElement>('.controls .ctl')[1].getAttribute('aria-label')).toBe('Maximize');

      close.click();
      await settle();
      expect(native.calls[native.calls.length - 1]).toBe('close');
    });

    it('dims while another window has focus and comes back on focus', async () => {
      window.dispatchEvent(new Event('blur'));
      await settle();
      expect(el().querySelector('.bar')?.classList).toContain('is-inactive');
      window.dispatchEvent(new Event('focus'));
      await settle();
      expect(el().querySelector('.bar')?.classList).not.toContain('is-inactive');
    });

    it('the Window menu does the same through the menu, and Exit quits through the engine command', async () => {
      top('Window').click();
      await settle();
      row('Minimize').click();
      await settle();
      expect(native.calls).toEqual(['minimize']);

      // with downloads running and the warning on, Exit asks first (the exit dialog takes the request)
      top('File').click();
      await settle();
      row('Exit').click();
      await settle();
      expect(ui.exitRequest()).toBeGreaterThan(0);
      expect(invoked.map((c) => c.cmd)).not.toContain('quit_app');

      TestBed.inject(SettingsService).patch({ confirmOnExit: false });
      top('File').click();
      await settle();
      row('Exit').click();
      await settle();
      expect(invoked.map((c) => c.cmd)).toContain('quit_app');
    });

    it('Help opens only the two fixed addresses, through the opener command', async () => {
      top('Help').click();
      await settle();
      row('Documentation').click();
      await settle();
      top('Help').click();
      await settle();
      row('GitHub').click();
      await settle();
      const opened = invoked.filter((c) => c.cmd === 'plugin:opener|open_url').map((c) => c.args?.['url']);
      expect(opened).toEqual(['https://layanjarrar1.github.io/Riptide/', 'https://github.com/LayanJarrar1/Riptide']);
    });

    it('Tools > Update video support asks the Rust side', async () => {
      top('Tools').click();
      await settle();
      row('Update video support').click();
      await settle();
      expect(invoked.map((c) => c.cmd)).toContain('update_media_tools');
    });
  });
});
