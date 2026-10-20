import { actionEntries, buildMenus, isEnabled, MenuActions, MenuDef, MenuState, splitMnemonic } from './title-bar-menus';

const EN: Record<string, string> = {
  'action.add': 'Add download',
  'action.batch': 'Batch download',
  'action.grabber': 'Site grabber',
  'action.openFile': 'Open file',
  'action.openFolder': 'Show in folder',
  'action.pause': 'Pause',
  'action.cancel': 'Cancel',
  'action.remove': 'Remove',
  'menu.importList': 'Import link list…',
  'menu.openTorrent': 'Open torrent file…',
  'menu.exit': 'Exit',
  'menu.startResume': 'Start / Resume',
  'toolbar.pauseAll': 'Pause all',
  'toolbar.resumeAll': 'Resume all',
  'menu.newQueue': 'New queue…',
  'nav.settings': 'Settings',
  'menu.browserIntegration': 'Browser integration',
  'settings.videoUpdate': 'Update video support',
  'menu.research': 'Research',
  'theme.label': 'Appearance',
  'theme.light': 'Light',
  'theme.dark': 'Dark',
  'theme.system': 'System',
  'lang.label': 'Language',
  'lang.en': 'English',
  'lang.ar': 'العربية',
  'menu.density': 'Display density',
  'menu.densityComfortable': 'Comfortable',
  'menu.densityCompact': 'Compact',
  'menu.sidebar': 'Sidebar',
  'window.minimize': 'Minimize',
  'window.maximize': 'Maximize',
  'window.restore': 'Restore',
  'window.close': 'Close',
  'menu.docs': 'Documentation',
  'menu.github': 'GitHub',
  'shortcuts.title': 'Keyboard shortcuts',
  'settings.checkUpdates': 'Check for updates',
  'menu.about': 'About Riptide',
};

function state(over: Partial<MenuState> = {}): MenuState {
  return {
    t: (key) => EN[key] ?? key,
    tauri: true,
    lang: 'en',
    theme: 'dark',
    density: 'comfortable',
    sidebarVisible: true,
    maximized: false,
    canResume: false,
    canPause: false,
    canCancel: false,
    canRemove: false,
    openable: false,
    anyRunning: false,
    anyResumable: false,
    ...over,
  };
}

/** Actions that record their name (and arguments) when called. */
function recorder(): { actions: MenuActions; calls: string[] } {
  const calls: string[] = [];
  const handler: ProxyHandler<object> = {
    get:
      (_target, name: string) =>
      (...args: unknown[]) => {
        calls.push(args.length > 0 ? `${name}(${args.map(String).join(',')})` : name);
        return Promise.resolve();
      },
  };
  return { actions: new Proxy({}, handler) as MenuActions, calls };
}

function menu(menus: readonly MenuDef[], id: string): MenuDef {
  const m = menus.find((x) => x.id === id);
  if (m === undefined) {
    throw new Error(`no menu ${id}`);
  }
  return m;
}

function labels(m: MenuDef): string[] {
  return actionEntries(m.entries).map((e) => e.label);
}

describe('title bar menus', () => {
  it('has the six menus in order inside the desktop app', () => {
    const menus = buildMenus(state(), recorder().actions);
    expect(menus.map((m) => m.id)).toEqual(['file', 'downloads', 'tools', 'view', 'window', 'help']);
    expect(menus.map((m) => m.label)).toEqual(['menu.file', 'menu.downloads', 'menu.tools', 'menu.view', 'menu.window', 'menu.help']);
  });

  it('File lists the add, batch, grabber and import commands with their shortcuts, and Exit', () => {
    const file = menu(buildMenus(state(), recorder().actions), 'file');
    expect(labels(file)).toEqual(['Add download', 'Batch download', 'Site grabber', 'Import link list…', 'Open torrent file…', 'Open file', 'Show in folder', 'Exit']);
    const shortcuts = actionEntries(file.entries).map((e) => (e.kind === 'item' ? e.shortcut : undefined));
    expect(shortcuts.slice(0, 2)).toEqual(['Ctrl+N', 'Ctrl+Shift+N']);
  });

  it('opening the file or its folder needs one finished download to be selected', () => {
    const open = (openable: boolean): boolean[] =>
      actionEntries(menu(buildMenus(state({ openable }), recorder().actions), 'file').entries)
        .filter((e) => e.label === 'Open file' || e.label === 'Show in folder')
        .map(isEnabled);
    expect(open(false)).toEqual([false, false]);
    expect(open(true)).toEqual([true, true]);
  });

  it('Downloads enables each command only when it applies to the selection', () => {
    const enabled = (over: Partial<MenuState>): Record<string, boolean> => {
      const m = menu(buildMenus(state(over), recorder().actions), 'downloads');
      return Object.fromEntries(actionEntries(m.entries).map((e) => [e.label, isEnabled(e)]));
    };
    expect(enabled({})).toEqual({ 'Start / Resume': false, Pause: false, Cancel: false, Remove: false, 'Pause all': false, 'Resume all': false, 'New queue…': true });
    expect(enabled({ canPause: true, canCancel: true, canRemove: true, anyRunning: true })).toEqual(
      jasmine.objectContaining({ 'Start / Resume': false, Pause: true, Cancel: true, Remove: true, 'Pause all': true, 'Resume all': false }),
    );
    expect(enabled({ canResume: true, anyResumable: true })).toEqual(jasmine.objectContaining({ 'Start / Resume': true, 'Resume all': true }));
  });

  it('View has theme, language and density as radio groups with the current choice checked, and the sidebar switch', () => {
    const view = menu(buildMenus(state({ theme: 'light', lang: 'ar', density: 'compact', sidebarVisible: false }), recorder().actions), 'view');
    const checked = (kind: string): string[] =>
      actionEntries(view.entries)
        .filter((e) => e.kind === kind && 'checked' in e && e.checked)
        .map((e) => e.id);
    expect(checked('radio')).toEqual(['theme-light', 'lang-ar', 'density-compact']);
    expect(checked('check')).toEqual([]);
    expect(labels(view)).toEqual(['Light', 'Dark', 'System', 'English', 'العربية', 'Comfortable', 'Compact', 'Sidebar']);
    const shown = menu(buildMenus(state({ sidebarVisible: true }), recorder().actions), 'view');
    expect(actionEntries(shown.entries).filter((e) => e.kind === 'check' && e.checked).length).toBe(1);
  });

  it('radio rows are spoken with their group, and choosing one runs the matching command', () => {
    const { actions, calls } = recorder();
    const entries = actionEntries(menu(buildMenus(state(), actions), 'view').entries);
    const dark = entries.find((e) => e.id === 'theme-dark');
    expect(dark?.kind === 'radio' ? dark.group : '').toBe('Appearance');
    for (const id of ['theme-system', 'lang-ar', 'density-compact', 'sidebar']) {
      entries.find((e) => e.id === id)?.run();
    }
    expect(calls).toEqual(['setTheme(system)', 'setLanguage(ar)', 'setDensity(compact)', 'toggleSidebar']);
  });

  it('Window shows Restore instead of Maximize when maximized, and Close has Alt+F4', () => {
    const win = (maximized: boolean): MenuDef => menu(buildMenus(state({ maximized }), recorder().actions), 'window');
    expect(labels(win(false))).toEqual(['Minimize', 'Maximize', 'Close']);
    expect(labels(win(true))).toEqual(['Minimize', 'Restore', 'Close']);
    const close = actionEntries(win(false).entries).find((e) => e.label === 'Close');
    expect(close?.kind === 'item' ? close.shortcut : '').toBe('Alt+F4');
  });

  it('Help lists docs, GitHub, shortcuts, update check and About, each wired to its command', () => {
    const { actions, calls } = recorder();
    const help = menu(buildMenus(state(), actions), 'help');
    expect(labels(help)).toEqual(['Documentation', 'GitHub', 'Keyboard shortcuts', 'Check for updates', 'About Riptide']);
    actionEntries(help.entries).forEach((e) => e.run());
    expect(calls).toEqual(['openDocs', 'openRepo', 'showShortcuts', 'checkForUpdates', 'about']);
  });

  it('leaves out what needs the desktop shell when there is none (never a dead item)', () => {
    const menus = buildMenus(state({ tauri: false }), recorder().actions);
    expect(menus.map((m) => m.id)).toEqual(['file', 'downloads', 'tools', 'view', 'help']);
    expect(labels(menu(menus, 'file'))).not.toContain('Exit');
    expect(labels(menu(menus, 'tools'))).toEqual(['Research', 'Settings', 'Browser integration']);
    expect(labels(menu(menus, 'help'))).toEqual(['Documentation', 'GitHub', 'Keyboard shortcuts', 'About Riptide']);
  });

  it('Tools opens Settings and its integration section, and updates video support', () => {
    const { actions, calls } = recorder();
    const tools = menu(buildMenus(state(), actions), 'tools');
    expect(labels(tools)).toEqual(['Research', 'Settings', 'Browser integration', 'Update video support']);
    actionEntries(tools.entries).forEach((e) => e.run());
    expect(calls).toEqual(['openResearch', 'openSettings', 'openSettings(integration)', 'updateVideoSupport']);
  });

  describe('splitMnemonic', () => {
    it('splits around the first occurrence, ignoring case', () => {
      expect(splitMnemonic('File', 'F')).toEqual({ before: '', mn: 'F', after: 'ile' });
      expect(splitMnemonic('Downloads', 'o')).toEqual({ before: 'D', mn: 'o', after: 'wnloads' });
    });

    it('works for Arabic and leaves a label alone when the letter is not in it', () => {
      expect(splitMnemonic('ملف', 'م')).toEqual({ before: '', mn: 'م', after: 'لف' });
      expect(splitMnemonic('Help', 'z')).toEqual({ before: 'Help', mn: '', after: '' });
      expect(splitMnemonic('Help', '')).toEqual({ before: 'Help', mn: '', after: '' });
    });
  });
});
