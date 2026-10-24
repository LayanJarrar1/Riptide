import { afterNextRender, ChangeDetectionStrategy, Component, effect, ElementRef, inject, Injector, signal } from '@angular/core';
import { toSignal } from '@angular/core/rxjs-interop';
import { ActivatedRoute, Router } from '@angular/router';

import { PREFERENCE_STORE } from '../../core/preferences';
import { UpdateService } from '../../core/update.service';
import { I18nService } from '../../i18n/i18n.service';
import { IconComponent, IconName } from '../../ui/icon.component';
import { AiSettingsComponent } from './ai-settings.component';
import { ResearchSettingsComponent } from './research-settings.component';
import { BrowserSectionComponent } from './sections/browser-section.component';
import { DownloadsSectionComponent } from './sections/downloads-section.component';
import { GeneralSectionComponent } from './sections/general-section.component';
import { NetworkSectionComponent } from './sections/network-section.component';
import { PrivacySectionComponent } from './sections/privacy-section.component';
import { UpdatesSectionComponent } from './sections/updates-section.component';

export { isValidProxyUrl } from './sections/network-section.component';
export type { ProtocolStatus } from './sections/browser-section.component';
export type { MediaToolsInfo } from './sections/downloads-section.component';

/** The sections of the page, in the order of the list. The ids are the route fragments (`/settings#ai`). */
export const SETTINGS_SECTIONS = ['general', 'downloads', 'browser', 'network', 'ai', 'privacy', 'updates'] as const;
export type SettingsSection = (typeof SETTINGS_SECTIONS)[number];

/** Remembered selection. */
export const SETTINGS_SECTION_KEY = 'riptide.settings.section';

/**
 * Every id a link or script may aim at, and the section it lives in. The groups carry these ids,
 * so `/settings#research-sources` selects AI & Research and scrolls to the sources, and the old
 * names (`appearance`, `integration`, `about`, ...) keep working.
 */
export const SETTINGS_ANCHORS: Readonly<Record<string, SettingsSection>> = {
  appearance: 'general',
  behaviour: 'general',
  downloads: 'downloads',
  torrents: 'downloads',
  video: 'downloads',
  integration: 'browser',
  extension: 'browser',
  diagnostics: 'browser',
  network: 'network',
  ai: 'ai',
  'research-sources': 'ai',
  privacy: 'privacy',
  about: 'updates',
};

/** The section a fragment names: a section id or one of the anchors; `null` when it names nothing. */
export function sectionOf(fragment: string | null | undefined): SettingsSection | null {
  if (fragment === null || fragment === undefined || fragment === '') {
    return null;
  }
  const direct = SETTINGS_SECTIONS.find((s) => s === fragment);
  return direct ?? SETTINGS_ANCHORS[fragment] ?? null;
}

const SECTION_ICONS: Readonly<Record<SettingsSection, IconName>> = {
  general: 'settings',
  downloads: 'download',
  browser: 'link',
  network: 'globe',
  ai: 'sparkles',
  privacy: 'shieldCheck',
  updates: 'info',
};

/**
 * Settings: a section list on the start side and one section at a time in the content pane,
 * each a stack of titled groups of rows. Every control writes straight to its setting; there is no
 * save button because nothing here is dangerous to change (the destructive actions ask first).
 * The selected section is remembered and can be deep-linked with a route fragment.
 */
@Component({
  selector: 'app-settings-page',
  changeDetection: ChangeDetectionStrategy.OnPush,
  imports: [
    IconComponent,
    GeneralSectionComponent,
    DownloadsSectionComponent,
    BrowserSectionComponent,
    NetworkSectionComponent,
    AiSettingsComponent,
    ResearchSettingsComponent,
    PrivacySectionComponent,
    UpdatesSectionComponent,
  ],
  styles: [
    `
      :host {
        display: block;
        block-size: 100%;
        overflow: auto;
      }

      .shell {
        container: settings / inline-size;
        max-inline-size: 1040px;
        padding: var(--space-8) var(--space-8) var(--space-16);
        margin-inline: auto;
      }

      .page-title {
        font-size: var(--text-2xl);
        font-weight: var(--weight-semibold);
        letter-spacing: var(--tracking-tight);
      }

      .page-sub {
        margin-block-start: var(--space-1);
        color: var(--color-text-muted);
      }

      .layout {
        display: grid;
        grid-template-columns: 220px minmax(0, 1fr);
        align-items: start;
        gap: var(--space-8);
        margin-block-start: var(--space-6);
      }

      .nav {
        position: sticky;
        inset-block-start: var(--space-4);
        display: flex;
        flex-direction: column;
        gap: 2px;
      }

      .tab {
        display: flex;
        align-items: center;
        gap: var(--space-3);
        padding: 9px var(--space-3);
        border: 0;
        border-radius: var(--radius-sm);
        background: transparent;
        color: var(--color-text-secondary);
        font: inherit;
        font-weight: var(--weight-medium);
        text-align: start;
        cursor: pointer;
        transition: background-color var(--motion-fast) var(--ease-standard);
      }

      .tab:hover {
        background: var(--color-surface-hover);
        color: var(--color-text);
      }

      .tab[aria-selected='true'] {
        background: var(--color-surface-active);
        color: var(--color-text);
        box-shadow: inset 3px 0 0 var(--color-accent);
      }

      :host-context([dir='rtl']) .tab[aria-selected='true'] {
        box-shadow: inset -3px 0 0 var(--color-accent);
      }

      .tab:focus-visible {
        outline: 2px solid var(--color-focus);
        outline-offset: -2px;
      }

      .tab-label {
        flex: 1;
        min-inline-size: 0;
      }

      .badge {
        inline-size: 8px;
        block-size: 8px;
        border-radius: var(--radius-full);
        background: var(--color-accent);
      }

      .picker {
        display: none;
        inline-size: 100%;
        block-size: 38px;
        padding-inline: 12px;
        border: 1px solid var(--color-border-strong);
        border-radius: var(--radius-sm);
        background: var(--color-surface-2);
        color: var(--color-text);
        font: inherit;
      }

      .picker:focus-visible {
        outline: 2px solid var(--color-focus);
        outline-offset: 2px;
      }

      .pane {
        min-inline-size: 0;
        max-inline-size: 760px;
      }

      .pane:focus {
        outline: none;
      }

      .pane-title {
        font-size: var(--text-xl);
        font-weight: var(--weight-semibold);
        letter-spacing: var(--tracking-tight);
      }

      .pane-sub {
        margin: var(--space-1) 0 var(--space-6);
        color: var(--color-text-muted);
        font-size: var(--text-sm);
      }

      .update-banner {
        display: flex;
        align-items: center;
        gap: var(--space-2);
        padding: 10px 14px;
        margin-block-start: var(--space-4);
        border: 1px solid var(--color-accent);
        border-radius: var(--radius-md);
        color: var(--color-text);
        text-decoration: none;
      }

      .update-banner:focus-visible {
        outline: 2px solid var(--color-focus);
        outline-offset: 2px;
      }

      .update-dot {
        inline-size: 8px;
        block-size: 8px;
        border-radius: 50%;
        background: var(--color-accent);
      }

      .update-banner-link {
        margin-inline-start: auto;
        color: var(--color-accent);
      }

      @container settings (max-inline-size: 900px) {
        .layout {
          grid-template-columns: minmax(0, 1fr);
          gap: var(--space-4);
        }

        .nav {
          display: none;
        }

        .picker {
          display: block;
        }
      }

      @media (prefers-reduced-motion: reduce) {
        .tab {
          transition: none;
        }
      }
    `,
  ],
  template: `
    <div class="shell">
      <header>
        <h1 class="page-title">{{ i18n.t('settings.title') }}</h1>
        <p class="page-sub">{{ i18n.t('settings.subtitle') }}</p>
      </header>

      @if (updates.badgeVersion(); as version) {
        <a class="update-banner" href="#updates" role="status" (click)="goUpdates($event)">
          <span class="update-dot" aria-hidden="true"></span>
          <span>{{ i18n.t('settings.updateBanner').replace('{version}', version) }}</span>
          <span class="update-banner-link">{{ i18n.t('settings.updateBannerLink') }}</span>
        </a>
      }

      <div class="layout">
        <nav class="nav" role="tablist" aria-orientation="vertical" [attr.aria-label]="i18n.t('set.nav')">
          @for (id of sections; track id) {
            <button
              class="tab"
              type="button"
              role="tab"
              [id]="'settings-tab-' + id"
              [attr.aria-selected]="section() === id"
              [attr.aria-controls]="'settings-panel'"
              [attr.tabindex]="section() === id ? 0 : -1"
              (click)="select(id)"
              (keydown)="onKeydown($event)"
            >
              <app-icon [name]="icons[id]" [size]="18" />
              <span class="tab-label">{{ i18n.t('set.section.' + id) }}</span>
              @if (id === 'updates' && updates.badgeVersion() !== null) {
                <span class="badge" aria-hidden="true"></span>
              }
            </button>
          }
        </nav>

        <select class="picker" [attr.aria-label]="i18n.t('set.nav')" (change)="onPick($event)">
          @for (id of sections; track id) {
            <option [value]="id" [selected]="section() === id">{{ i18n.t('set.section.' + id) }}</option>
          }
        </select>

        <div class="pane" id="settings-panel" role="tabpanel" tabindex="-1" [attr.aria-labelledby]="'settings-tab-' + section()">
          <h2 class="pane-title">{{ i18n.t('set.section.' + section()) }}</h2>
          <p class="pane-sub">{{ i18n.t('set.sectionDesc.' + section()) }}</p>

          @switch (section()) {
            @case ('general') {
              <app-settings-general />
            }
            @case ('downloads') {
              <app-settings-downloads />
            }
            @case ('browser') {
              <app-settings-browser />
            }
            @case ('network') {
              <app-settings-network />
            }
            @case ('ai') {
              <app-ai-settings />
              <app-research-settings />
            }
            @case ('privacy') {
              <app-settings-privacy />
            }
            @case ('updates') {
              <app-settings-updates />
            }
          }
        </div>
      </div>

    </div>
  `,
})
export class SettingsPageComponent {
  protected readonly i18n = inject(I18nService);
  protected readonly updates = inject(UpdateService);
  private readonly store = inject(PREFERENCE_STORE);
  private readonly router = inject(Router);
  private readonly route = inject(ActivatedRoute);
  private readonly injector = inject(Injector);
  private readonly host = inject<ElementRef<HTMLElement>>(ElementRef);

  protected readonly sections = SETTINGS_SECTIONS;
  protected readonly icons = SECTION_ICONS;
  private readonly fragment = toSignal(this.route.fragment, { initialValue: null });

  /** The section showing; the route fragment wins over the remembered one. */
  readonly section = signal<SettingsSection>(sectionOf(this.route.snapshot.fragment) ?? this.remembered());

  constructor() {
    effect(() => {
      const fragment = this.fragment();
      if (sectionOf(fragment) !== null) {
        this.go(fragment as string, false);
      }
    });
  }

  private remembered(): SettingsSection {
    return sectionOf(this.store.get(SETTINGS_SECTION_KEY)) ?? 'general';
  }

  /** Shows a section from the list; the URL follows so the page can be linked and reloaded. */
  protected select(id: SettingsSection): void {
    this.go(id, true);
  }

  /** Shows the section an id belongs to and scrolls to the id (a group) after it rendered. */
  protected go(id: string, updateUrl = true): void {
    const target = sectionOf(id);
    if (target === null) {
      return;
    }
    this.section.set(target);
    this.store.set(SETTINGS_SECTION_KEY, target);
    if (updateUrl && this.fragment() !== id) {
      void this.router.navigate([], { relativeTo: this.route, fragment: id, replaceUrl: true });
    }
    afterNextRender(
      () => {
        const group = id === target ? null : document.getElementById(id);
        if (group !== null) {
          group.scrollIntoView({ block: 'start' });
        } else {
          this.host.nativeElement.scrollTop = 0;
        }
      },
      { injector: this.injector },
    );
  }

  protected onPick(event: Event): void {
    const id = sectionOf((event.target as HTMLSelectElement).value);
    if (id !== null) {
      this.select(id);
    }
  }

  /** Arrow keys, Home and End move through the list and open the section (automatic activation). */
  protected onKeydown(event: KeyboardEvent): void {
    const last = SETTINGS_SECTIONS.length - 1;
    const at = SETTINGS_SECTIONS.indexOf(this.section());
    let next: number;
    switch (event.key) {
      case 'ArrowDown':
      case 'ArrowRight':
        next = at === last ? 0 : at + 1;
        break;
      case 'ArrowUp':
      case 'ArrowLeft':
        next = at === 0 ? last : at - 1;
        break;
      case 'Home':
        next = 0;
        break;
      case 'End':
        next = last;
        break;
      default:
        return;
    }
    event.preventDefault();
    const id = SETTINGS_SECTIONS[next];
    this.select(id);
    this.host.nativeElement.querySelector<HTMLElement>(`#settings-tab-${id}`)?.focus();
  }

  protected goUpdates(event: Event): void {
    event.preventDefault();
    this.select('updates');
  }
}
