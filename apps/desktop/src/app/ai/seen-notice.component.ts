import { ChangeDetectionStrategy, Component, computed, inject, input, output } from '@angular/core';

import { FormatService } from '../core/format.service';
import { I18nService } from '../i18n/i18n.service';
import { TAURI_INVOKE } from '../tauri/tauri-bridge';
import { ButtonComponent } from '../ui/button.component';
import { IconComponent } from '../ui/icon.component';
import { ToastService } from '../ui/toast.service';
import { SeenMatch } from './ai.models';

/**
 * "You already downloaded this on 12 Oct 2026": shown by the Add dialog and the Research page
 * when the download memory knows the same address, DOI, arXiv id, file name and size, or file
 * contents. Offers Open file / Open folder while the file is still on disk, and Download again.
 */
@Component({
  selector: 'app-seen-notice',
  changeDetection: ChangeDetectionStrategy.OnPush,
  imports: [ButtonComponent, IconComponent],
  template: `
    @if (match(); as m) {
      <div class="notice" [class.is-stale]="m.stale" [class.is-compact]="compact()" role="status">
        <app-icon [name]="m.stale ? 'info' : 'check'" [size]="16" class="icon" />
        <div class="body">
          <span class="text">{{ text() }}</span>
          <span class="by">{{ by() }} · <span dir="auto">{{ m.title }}</span></span>
          @if (againChosen()) {
            <span class="by">{{ i18n.t('seen.againNote') }}</span>
          }
        </div>
        <div class="actions">
          @if (!m.stale) {
            <button appButton size="sm" variant="secondary" icon="file" type="button" (click)="openFile(m)">{{ i18n.t('seen.open') }}</button>
            <button appButton size="sm" variant="secondary" icon="folder" type="button" (click)="openFolder(m)">{{ i18n.t('seen.folder') }}</button>
          }
          @if (offerAgain()) {
            <button appButton size="sm" variant="ghost" icon="retry" type="button" [attr.aria-pressed]="againChosen()" (click)="again.emit()">{{ i18n.t('seen.again') }}</button>
          }
        </div>
      </div>
    }
  `,
  styles: `
    :host {
      display: block;
    }

    .notice {
      display: flex;
      align-items: center;
      gap: var(--space-3);
      padding: var(--space-2) var(--space-3);
      border: 1px solid var(--color-success);
      border-radius: var(--radius-sm);
      background: var(--state-completed-soft);
      font-size: var(--text-sm);
    }

    .notice.is-stale {
      border-color: var(--color-border-strong);
      background: var(--color-surface-2);
    }

    .icon {
      flex-shrink: 0;
      color: var(--color-success);
    }

    .is-stale .icon {
      color: var(--color-text-muted);
    }

    .body {
      display: flex;
      flex-direction: column;
      flex: 1;
      min-inline-size: 0;
    }

    .by {
      color: var(--color-text-muted);
      font-size: var(--text-xs);
      overflow-wrap: anywhere;
    }

    .actions {
      display: flex;
      flex-wrap: wrap;
      gap: var(--space-2);
      justify-content: flex-end;
    }

    .is-compact {
      padding: 4px var(--space-2);
    }

    @media (max-width: 640px) {
      .notice {
        flex-wrap: wrap;
      }
    }
  `,
})
export class SeenNoticeComponent {
  protected readonly i18n = inject(I18nService);
  private readonly format = inject(FormatService);
  private readonly invoke = inject(TAURI_INVOKE);
  private readonly toasts = inject(ToastService);

  readonly match = input.required<SeenMatch | null>();
  readonly compact = input(false);
  /** Show the "Download again" button (the Add dialog does; result cards use the checkbox). */
  readonly offerAgain = input(false);
  /** The user already chose to download again. */
  readonly againChosen = input(false);
  readonly again = output<void>();

  protected readonly text = computed(() => {
    const m = this.match();
    if (m === null) {
      return '';
    }
    return this.i18n.t(m.stale ? 'seen.stale' : 'seen.on').replace('{date}', this.format.dateTime(m.completedAt));
  });

  protected readonly by = computed(() => {
    const m = this.match();
    return m === null ? '' : this.i18n.t(`seen.by.${m.matchedBy}`);
  });

  protected openFile(m: SeenMatch): void {
    this.invoke<void>('open_file', { path: m.path }).catch((e: unknown) => this.toasts.show('danger', this.i18n.t('toast.engineError').replace('{error}', String(e))));
  }

  protected openFolder(m: SeenMatch): void {
    this.invoke<void>('show_in_folder', { path: m.path }).catch((e: unknown) => this.toasts.show('danger', this.i18n.t('toast.engineError').replace('{error}', String(e))));
  }
}
