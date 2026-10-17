/** Options page: edits `settings` in `storage.local`; saves on every change. */
import browser from 'webextension-polyfill';

import { localiseDocument, resolveLang, t } from '../i18n';
import type { Lang, MessageKey } from '../i18n';
import { HOST_NAME } from '../protocol';
import { FILE_TYPE_KEYS, parseExtensionList, parseSiteList } from '../rules';
import type { FileTypeKey } from '../rules';
import { normaliseSettings } from '../settings';
import type { LanguagePref, Settings } from '../settings';
import { loadSettings, saveSettings } from '../storage';

let lang: Lang = 'en';
let settings: Settings;
let savedTimer: ReturnType<typeof setTimeout> | null = null;

function $<T extends HTMLElement>(id: string): T {
  const el = document.getElementById(id);
  if (!el) throw new Error(`options: #${id} missing`);
  return el as T;
}

function tr(key: MessageKey, params?: Record<string, string | number>): string {
  return t(lang, key, params);
}

function typeBox(key: FileTypeKey): HTMLInputElement {
  const el = document.querySelector<HTMLInputElement>(`input[data-type="${key}"]`);
  if (!el) throw new Error(`options: type ${key} missing`);
  return el;
}

function unitFor(bytes: number): number {
  for (const unit of [1024 ** 3, 1024 ** 2, 1024]) {
    if (bytes >= unit && bytes % unit === 0) return unit;
  }
  return bytes === 0 ? 1024 ** 2 : 1024;
}

function fill(): void {
  $<HTMLInputElement>('enabled').checked = settings.rules.enabled;
  const unit = unitFor(settings.rules.minSize);
  $<HTMLSelectElement>('minSizeUnit').value = String(unit);
  $<HTMLInputElement>('minSize').value = String(settings.rules.minSize / unit);
  for (const key of FILE_TYPE_KEYS) typeBox(key).checked = settings.rules.types[key];
  $<HTMLInputElement>('customExtensions').value = settings.rules.customExtensions.join(', ');
  $<HTMLTextAreaElement>('allowSites').value = settings.rules.allowSites.join('\n');
  $<HTMLTextAreaElement>('denySites').value = settings.rules.denySites.join('\n');
  $<HTMLTextAreaElement>('neverAskSites').value = settings.rules.neverAskSites.join('\n');
  $<HTMLInputElement>('altBypass').checked = settings.rules.altBypass;
  $<HTMLInputElement>('notify').checked = settings.notify;
  $<HTMLInputElement>('hoverBadge').checked = settings.hoverBadge;
  $<HTMLSelectElement>('language').value = settings.language;
  $('about-host').textContent = tr('options.about.host', { host: HOST_NAME });
  $('about-id').textContent = tr('options.about.id', { id: browser.runtime.id });
}

function read(): Settings {
  const unit = Number($<HTMLSelectElement>('minSizeUnit').value) || 1024 ** 2;
  const sizeValue = Number($<HTMLInputElement>('minSize').value);
  const types = {} as Record<FileTypeKey, boolean>;
  for (const key of FILE_TYPE_KEYS) types[key] = typeBox(key).checked;
  const language = $<HTMLSelectElement>('language').value as LanguagePref;
  return normaliseSettings({
    rules: {
      enabled: $<HTMLInputElement>('enabled').checked,
      minSize: Number.isFinite(sizeValue) && sizeValue >= 0 ? Math.round(sizeValue * unit) : settings.rules.minSize,
      types,
      customExtensions: parseExtensionList($<HTMLInputElement>('customExtensions').value),
      allowSites: parseSiteList($<HTMLTextAreaElement>('allowSites').value),
      denySites: parseSiteList($<HTMLTextAreaElement>('denySites').value),
      neverAskSites: parseSiteList($<HTMLTextAreaElement>('neverAskSites').value),
      altBypass: $<HTMLInputElement>('altBypass').checked,
    },
    language,
    notify: $<HTMLInputElement>('notify').checked,
    hoverBadge: $<HTMLInputElement>('hoverBadge').checked,
  });
}

function showSaved(): void {
  const el = $('saved');
  el.classList.add('show');
  if (savedTimer) clearTimeout(savedTimer);
  savedTimer = setTimeout(() => el.classList.remove('show'), 1_500);
}

async function save(relocalise: boolean): Promise<void> {
  settings = read();
  await saveSettings(settings);
  if (relocalise) {
    lang = resolveLang(settings.language, browser.i18n.getUILanguage());
    localiseDocument(lang);
    fill();
  }
  showSaved();
}

async function main(): Promise<void> {
  settings = await loadSettings();
  lang = resolveLang(settings.language, browser.i18n.getUILanguage());
  localiseDocument(lang);
  fill();

  const form = $<HTMLFormElement>('form');
  form.addEventListener('submit', (e) => e.preventDefault());
  form.addEventListener('change', (e) => {
    const relocalise = (e.target as HTMLElement | null)?.id === 'language';
    void save(relocalise);
  });
  let debounce: ReturnType<typeof setTimeout> | null = null;
  form.addEventListener('input', (e) => {
    const target = e.target as HTMLElement | null;
    if (!(target instanceof HTMLTextAreaElement || (target instanceof HTMLInputElement && target.type === 'text'))) return;
    if (debounce) clearTimeout(debounce);
    debounce = setTimeout(() => void save(false), 600);
  });
}

void main();
