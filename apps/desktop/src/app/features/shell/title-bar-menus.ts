import type { Lang } from '../../i18n/i18n.service';
import type { Theme } from '../../theme/theme.service';

/** The six top-level menus, in bar order. */
export type MenuId = 'file' | 'downloads' | 'tools' | 'view' | 'window' | 'help';

/** Which row takes focus when a menu appears: the first, the last, or the menu itself. */
export type MenuFocus = 'first' | 'last' | 'menu';

/** A command row. */
export interface ItemEntry {
  readonly kind: 'item';
  readonly id: string;
  readonly label: string;
  /** Hint drawn at the end of the row (`Ctrl+N`). */
  readonly shortcut?: string;
  readonly disabled?: boolean;
  readonly run: () => void;
}

/** One choice of a group where exactly one is on (theme, language, density). */
export interface RadioEntry {
  readonly kind: 'radio';
  readonly id: string;
  readonly label: string;
  /** The group's name; spoken before the label (`Theme: Dark`). */
  readonly group: string;
  readonly checked: boolean;
  readonly run: () => void;
}

/** A switch (sidebar shown or hidden). */
export interface CheckEntry {
  readonly kind: 'check';
  readonly id: string;
  readonly label: string;
  readonly shortcut?: string;
  readonly checked: boolean;
  readonly run: () => void;
}

export interface SeparatorEntry {
  readonly kind: 'separator';
}

/** A caption above a group of radios; not focusable. */
export interface HeadingEntry {
  readonly kind: 'heading';
  readonly label: string;
}

export type MenuEntry = ItemEntry | RadioEntry | CheckEntry | SeparatorEntry | HeadingEntry;

/** Rows that can take focus and be chosen. */
export type ActionEntry = ItemEntry | RadioEntry | CheckEntry;

export interface MenuDef {
  readonly id: MenuId;
  readonly label: string;
  /** The letter Alt + letter opens the menu with (it must occur in the label to be underlined). */
  readonly mnemonic: string;
  readonly entries: readonly MenuEntry[];
}

/** What the menus show: the app's current state. */
export interface MenuState {
  readonly t: (key: string) => string;
  /** Inside the Tauri shell (a native window, the Rust commands). */
  readonly tauri: boolean;
  readonly lang: Lang;
  readonly theme: Theme;
  readonly density: 'comfortable' | 'compact';
  readonly sidebarVisible: boolean;
  readonly maximized: boolean;
  readonly canResume: boolean;
  readonly canPause: boolean;
  readonly canCancel: boolean;
  readonly canRemove: boolean;
  /** Exactly one finished download is selected. */
  readonly openable: boolean;
  readonly anyRunning: boolean;
  readonly anyResumable: boolean;
}

/** What the menus can do (implemented by `AppCommands`; specs pass a recording fake). */
export interface MenuActions {
  addDownload(): void;
  batchDownload(): void;
  siteGrabber(): void;
  importLinkList(): void;
  openTorrentFile(): void;
  openFile(): void;
  openFolder(): void;
  exit(): void;
  resumeSelected(): void;
  pauseSelected(): void;
  cancelSelected(): void;
  removeSelected(): void;
  pauseAll(): void;
  resumeAll(): void;
  newQueue(): void;
  openSettings(section?: string): void;
  openResearch(): void;
  updateVideoSupport(): Promise<void>;
  setTheme(theme: Theme): void;
  setLanguage(lang: Lang): void;
  toggleSidebar(): void;
  setDensity(density: 'comfortable' | 'compact'): void;
  minimize(): void;
  toggleMaximize(): void;
  closeWindow(): void;
  showShortcuts(): void;
  openDocs(): void;
  openRepo(): void;
  checkForUpdates(): Promise<void>;
  about(): void;
}

const SEP: SeparatorEntry = { kind: 'separator' };

/**
 * The menu bar's content for the current state. Pure: the title bar calls it inside a
 * `computed`, so enabled states follow the selection live. An entry whose feature does not
 * exist in the running environment is left out (never shown dead): the window commands and
 * Exit need the native shell, Update video support and update checks need the Rust side.
 */
export function buildMenus(s: MenuState, a: MenuActions): readonly MenuDef[] {
  const t = s.t;
  const item = (id: string, label: string, run: () => void, extra: { shortcut?: string; disabled?: boolean } = {}): ItemEntry => ({
    kind: 'item',
    id,
    label,
    run,
    ...extra,
  });

  const file: MenuEntry[] = [
    item('add', t('action.add'), () => a.addDownload(), { shortcut: 'Ctrl+N' }),
    item('batch', t('action.batch'), () => a.batchDownload(), { shortcut: 'Ctrl+Shift+N' }),
    item('grabber', t('action.grabber'), () => a.siteGrabber()),
    item('importList', t('menu.importList'), () => a.importLinkList()),
    item('openTorrent', t('menu.openTorrent'), () => a.openTorrentFile()),
    SEP,
    item('openFile', t('action.openFile'), () => a.openFile(), { disabled: !s.openable }),
    item('openFolder', t('action.openFolder'), () => a.openFolder(), { disabled: !s.openable }),
  ];
  if (s.tauri) {
    file.push(SEP, item('exit', t('menu.exit'), () => a.exit()));
  }

  const downloads: MenuEntry[] = [
    item('resume', t('menu.startResume'), () => a.resumeSelected(), { disabled: !s.canResume }),
    item('pause', t('action.pause'), () => a.pauseSelected(), { disabled: !s.canPause }),
    item('cancel', t('action.cancel'), () => a.cancelSelected(), { disabled: !s.canCancel }),
    item('remove', t('action.remove'), () => a.removeSelected(), { shortcut: 'Del', disabled: !s.canRemove }),
    SEP,
    item('pauseAll', t('toolbar.pauseAll'), () => a.pauseAll(), { disabled: !s.anyRunning }),
    item('resumeAll', t('toolbar.resumeAll'), () => a.resumeAll(), { disabled: !s.anyResumable }),
    SEP,
    item('newQueue', t('menu.newQueue'), () => a.newQueue()),
  ];

  const tools: MenuEntry[] = [
    item('research', t('menu.research'), () => a.openResearch()),
    item('settings', t('nav.settings'), () => a.openSettings(), { shortcut: 'Ctrl+,' }),
    item('integration', t('menu.browserIntegration'), () => a.openSettings('integration')),
  ];
  if (s.tauri) {
    tools.push(SEP, item('video', t('settings.videoUpdate'), () => void a.updateVideoSupport()));
  }

  const themeGroup = t('theme.label');
  const langGroup = t('lang.label');
  const densityGroup = t('menu.density');
  const radio = (id: string, group: string, label: string, checked: boolean, run: () => void): RadioEntry => ({ kind: 'radio', id, group, label, checked, run });
  const view: MenuEntry[] = [
    { kind: 'heading', label: themeGroup },
    radio('theme-light', themeGroup, t('theme.light'), s.theme === 'light', () => a.setTheme('light')),
    radio('theme-dark', themeGroup, t('theme.dark'), s.theme === 'dark', () => a.setTheme('dark')),
    radio('theme-system', themeGroup, t('theme.system'), s.theme === 'system', () => a.setTheme('system')),
    SEP,
    { kind: 'heading', label: langGroup },
    // each language in its own script, whatever the UI language is
    radio('lang-en', langGroup, t('lang.en'), s.lang === 'en', () => a.setLanguage('en')),
    radio('lang-ar', langGroup, t('lang.ar'), s.lang === 'ar', () => a.setLanguage('ar')),
    SEP,
    { kind: 'heading', label: densityGroup },
    radio('density-comfortable', densityGroup, t('menu.densityComfortable'), s.density === 'comfortable', () => a.setDensity('comfortable')),
    radio('density-compact', densityGroup, t('menu.densityCompact'), s.density === 'compact', () => a.setDensity('compact')),
    SEP,
    { kind: 'check', id: 'sidebar', label: t('menu.sidebar'), shortcut: 'Ctrl+B', checked: s.sidebarVisible, run: () => a.toggleSidebar() },
  ];

  const windowMenu: MenuEntry[] = s.tauri
    ? [
        item('minimize', t('window.minimize'), () => a.minimize()),
        item('maximize', s.maximized ? t('window.restore') : t('window.maximize'), () => a.toggleMaximize()),
        SEP,
        item('close', t('window.close'), () => a.closeWindow(), { shortcut: 'Alt+F4' }),
      ]
    : [];

  const help: MenuEntry[] = [
    item('docs', t('menu.docs'), () => a.openDocs()),
    item('github', t('menu.github'), () => a.openRepo()),
    item('shortcuts', t('shortcuts.title'), () => a.showShortcuts(), { shortcut: '?' }),
  ];
  if (s.tauri) {
    help.push(SEP, item('updates', t('settings.checkUpdates'), () => void a.checkForUpdates()));
  }
  help.push(item('about', t('menu.about'), () => a.about()));

  const defs: MenuDef[] = [
    { id: 'file', label: t('menu.file'), mnemonic: t('menu.file.key'), entries: file },
    { id: 'downloads', label: t('menu.downloads'), mnemonic: t('menu.downloads.key'), entries: downloads },
    { id: 'tools', label: t('menu.tools'), mnemonic: t('menu.tools.key'), entries: tools },
    { id: 'view', label: t('menu.view'), mnemonic: t('menu.view.key'), entries: view },
    { id: 'window', label: t('menu.window'), mnemonic: t('menu.window.key'), entries: windowMenu },
    { id: 'help', label: t('menu.help'), mnemonic: t('menu.help.key'), entries: help },
  ];
  // a menu with nothing to offer is not shown (the Window menu in the browser preview)
  return defs.filter((m) => m.entries.length > 0);
}

/** `label` split around the first occurrence of `mnemonic` (case-insensitive) for underlining. */
export function splitMnemonic(label: string, mnemonic: string): { before: string; mn: string; after: string } {
  const i = mnemonic === '' ? -1 : label.toLowerCase().indexOf(mnemonic.toLowerCase());
  return i < 0 ? { before: label, mn: '', after: '' } : { before: label.slice(0, i), mn: label.slice(i, i + mnemonic.length), after: label.slice(i + mnemonic.length) };
}

/** The entries that can take focus, in order. */
export function actionEntries(entries: readonly MenuEntry[]): readonly ActionEntry[] {
  return entries.filter((e): e is ActionEntry => e.kind === 'item' || e.kind === 'radio' || e.kind === 'check');
}

/** Whether a row can be chosen. */
export function isEnabled(entry: ActionEntry): boolean {
  return entry.kind !== 'item' || entry.disabled !== true;
}
