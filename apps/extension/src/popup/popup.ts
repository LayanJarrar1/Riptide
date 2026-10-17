/** Toolbar popup: connection status, pause toggle, last hand-offs, Open / Options. */
import browser from 'webextension-polyfill';

import { localiseDocument, resolveLang, t } from '../i18n';
import type { Lang, MessageKey } from '../i18n';
import type { Message, RecentItem, StatusReply } from '../messages';
import { normaliseSettings } from '../settings';

const IS_FIREFOX = __BROWSER__ === 'firefox';

let lang: Lang = 'en';

function $<T extends HTMLElement>(id: string): T {
  const el = document.getElementById(id);
  if (!el) throw new Error(`popup: #${id} missing`);
  return el as T;
}

function tr(key: MessageKey, params?: Record<string, string | number>): string {
  return t(lang, key, params);
}

function send(message: Message): Promise<StatusReply | undefined> {
  return browser.runtime.sendMessage(message).then(
    (r) => r as StatusReply | undefined,
    () => undefined,
  );
}

function timeOf(ms: number): string {
  return new Intl.DateTimeFormat(lang === 'ar' ? 'ar' : 'en', { hour: '2-digit', minute: '2-digit' }).format(new Date(ms));
}

function renderRecent(items: RecentItem[], listAvailable: boolean): void {
  const list = $<HTMLUListElement>('recent-list');
  list.replaceChildren();
  for (const item of items) {
    const li = document.createElement('li');
    li.className = 'item';
    const name = document.createElement('span');
    name.className = 'name';
    name.textContent = item.filename;
    name.title = item.url;
    const chip = document.createElement('span');
    chip.className = 'chip';
    const state = item.state?.state ?? 'sent';
    chip.dataset['state'] = state;
    chip.textContent = tr(`state.${state}`);
    const time = document.createElement('span');
    time.className = 'time';
    time.textContent = timeOf(item.at);
    li.append(name, chip, time);
    list.append(li);
  }
  $('recent-empty').hidden = items.length > 0;
  $('recent-note').hidden = listAvailable || items.length === 0;
}

function render(reply: StatusReply | undefined): void {
  const status = reply?.status ?? 'unknown';
  const section = $('status');
  section.dataset['status'] = status;
  $('status-label').textContent = tr(`popup.status.${status}`);
  const hint = $('status-hint');
  hint.textContent =
    status === 'app_not_running'
      ? tr('popup.status.hint.app_not_running')
      : status === 'host_missing'
        ? tr('popup.status.hint.host_missing')
        : '';
  $('version').textContent = reply?.version ? `v${reply.version}` : '';

  const label = $('capture-label');
  const pauseBtn = $<HTMLButtonElement>('pause');
  if (reply && !reply.rulesEnabled) {
    label.textContent = tr('popup.capture.disabled');
    label.dataset['on'] = 'false';
    pauseBtn.hidden = true;
  } else {
    const paused = reply?.paused ?? false;
    label.textContent = tr(paused ? 'popup.capture.paused' : 'popup.capture.on');
    label.dataset['on'] = String(!paused);
    pauseBtn.hidden = false;
    pauseBtn.textContent = tr(paused ? 'popup.capture.resume' : 'popup.capture.pause');
    pauseBtn.dataset['paused'] = String(paused);
  }

  renderRecent(reply?.recent ?? [], reply?.listAvailable ?? true);
  $('captured').textContent = tr('popup.captured', { n: reply?.captured ?? 0 });
}

async function checkFirefoxAccess(): Promise<void> {
  if (!IS_FIREFOX) return;
  try {
    const has = await browser.permissions.contains({ origins: ['<all_urls>'] });
    $('grant').hidden = has;
  } catch {
    $('grant').hidden = true;
  }
}

async function main(): Promise<void> {
  const got = await browser.storage.local.get('settings');
  const settings = normaliseSettings(got['settings']);
  lang = resolveLang(settings.language, browser.i18n.getUILanguage());
  localiseDocument(lang);
  render(undefined);
  void checkFirefoxAccess();

  $('open').addEventListener('click', () => {
    void send({ kind: 'open-app' }).then(render);
  });
  $('options').addEventListener('click', () => {
    void browser.runtime.openOptionsPage();
    window.close();
  });
  $('refresh').addEventListener('click', () => {
    void send({ kind: 'refresh' }).then(render);
  });
  $('pause').addEventListener('click', (e) => {
    const paused = (e.currentTarget as HTMLButtonElement).dataset['paused'] === 'true';
    void send({ kind: 'set-paused', paused: !paused }).then(render);
  });
  $('grant-btn').addEventListener('click', () => {
    void browser.permissions
      .request({ origins: ['<all_urls>'] })
      .then((granted) => {
        $('grant').hidden = granted;
      })
      .catch(() => undefined);
  });

  render(await send({ kind: 'get-status' }));
}

void main();
