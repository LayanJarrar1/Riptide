import { ChangeDetectionStrategy, Component, computed, inject, input, output } from '@angular/core';

import { AiService } from '../../ai/ai.service';
import { SourceId } from '../../ai/research.models';
import { ResearchSession, SourceStatus } from '../../ai/research.session';
import { FormatService } from '../../core/format.service';
import { I18nService } from '../../i18n/i18n.service';
import { ButtonComponent } from '../../ui/button.component';
import { IconComponent } from '../../ui/icon.component';
import { ToastService } from '../../ui/toast.service';

/**
 * What a running (or finished) search is doing: the progress line with Cancel, one chip per source
 * (searching, how many results, failed with a retry button, stopped), the sources that need
 * setting up, the AI's query plan and any warning about the AI step.
 */
@Component({
  selector: 'app-research-status',
  changeDetection: ChangeDetectionStrategy.OnPush,
  imports: [ButtonComponent, IconComponent],
  template: `
    <section class="status" [attr.aria-label]="i18n.t('research.sources')">
      <div class="status-head" role="status" aria-live="polite">
        @if (session().busy()) {
          <div class="progress" aria-hidden="true"><span class="bar"></span></div>
        }
        <span class="status-text">{{ statusText() }}</span>
        @if (session().busy()) {
          <button appButton size="sm" variant="secondary" icon="stop" type="button" (click)="cancel()">{{ i18n.t('research.cancel') }}</button>
        }
      </div>

      <div class="chips" role="group" [attr.aria-label]="i18n.t('research.sources')">
        @for (s of session().sources(); track s.id) {
          <span class="chip" [class.is-busy]="s.state === 'searching'" [class.is-failed]="s.state === 'failed'" [class.is-stopped]="s.state === 'stopped'" [class.is-active]="session().onlySource() === s.id">
            <button type="button" class="chip-main" [attr.aria-pressed]="session().onlySource() === s.id" [attr.aria-label]="chipLabel(s)" [disabled]="s.found === 0" (click)="toggleOnly(s.id)">
              <span class="chip-dot" aria-hidden="true"></span>
              <span class="chip-name">{{ sourceLabel(s.id) }}</span>
              <span class="chip-state num">{{ chipState(s) }}</span>
            </button>
            @if (s.state === 'failed') {
              <button type="button" class="chip-retry" [attr.aria-label]="i18n.t('research.retry').replace('{source}', sourceLabel(s.id))" (click)="retry(s.id)">
                <app-icon name="retry" [size]="13" />
              </button>
            }
          </span>
        }
        @for (u of session().unavailable(); track u.source) {
          <span class="chip is-setup">
            <span class="chip-name">{{ sourceLabel(u.source) }}</span>
            <button type="button" class="chip-setup" (click)="openSettings.emit()">{{ i18n.t('research.unavailable.' + u.source) }}</button>
          </span>
        }
      </div>

      @if (session().plan().length > 0) {
        <p class="plan">
          <app-icon name="sparkles" [size]="14" />
          <span>{{ i18n.t('research.queries') }}:</span>
          @for (q of session().plan(); track q) {
            <span class="query" dir="auto">{{ q }}</span>
          }
        </p>
      }
      @if (session().ai(); as a) {
        @if (a.error; as e) {
          <p class="warn" role="alert"><app-icon name="alert" [size]="14" /> {{ i18n.t('research.ai.failed') }} {{ ai.failText(e) }}</p>
        }
        @if (a.dropped > 0) {
          <p class="warn"><app-icon name="shield" [size]="14" /> {{ i18n.t('research.ai.dropped').replace('{n}', fmt.int(a.dropped)) }}</p>
        }
      }
    </section>
  `,
  styles: `
    .status {
      margin-block-start: var(--space-4);
      padding: var(--space-3) var(--space-4);
      border: 1px solid var(--color-border);
      border-radius: var(--radius-md);
      background: var(--color-surface-1, var(--color-surface-2));
    }

    .status-head {
      position: relative;
      display: flex;
      align-items: center;
      justify-content: space-between;
      gap: var(--space-3);
      min-block-size: 32px;
    }

    .status-text {
      font-size: var(--text-sm);
      color: var(--color-text-secondary);
      flex: 1;
    }

    .progress {
      position: absolute;
      inset-inline: 0;
      inset-block-end: -6px;
      block-size: 2px;
      border-radius: 2px;
      background: var(--color-surface-3);
      overflow: hidden;
    }

    .bar {
      display: block;
      block-size: 100%;
      inline-size: 35%;
      background: var(--color-accent);
      animation: slide 1.1s var(--ease-standard) infinite;
    }

    @keyframes slide {
      from {
        transform: translateX(calc(var(--dir-sign) * -100%));
      }
      to {
        transform: translateX(calc(var(--dir-sign) * 290%));
      }
    }

    .chips {
      display: flex;
      flex-wrap: wrap;
      gap: var(--space-2);
      margin-block-start: var(--space-4);
    }

    .chip {
      display: inline-flex;
      align-items: center;
      border: 1px solid var(--color-border);
      border-radius: var(--radius-full);
      font-size: var(--text-xs);
      color: var(--color-text-secondary);
      background: transparent;
      overflow: hidden;
    }

    .chip.is-active {
      border-color: var(--color-accent);
      background: var(--color-accent-soft, var(--color-surface-3));
    }

    .chip.is-failed {
      border-color: var(--color-danger);
    }

    .chip.is-setup {
      border-style: dashed;
      gap: var(--space-2);
      padding-inline-start: 10px;
    }

    .chip-main,
    .chip-retry,
    .chip-setup {
      display: inline-flex;
      align-items: center;
      gap: 6px;
      block-size: 26px;
      padding-inline: 10px;
      border: 0;
      background: transparent;
      color: inherit;
      font: inherit;
      cursor: pointer;
    }

    .chip-main:disabled {
      cursor: default;
    }

    .chip-retry {
      padding-inline: 8px;
      border-inline-start: 1px solid var(--color-border);
      color: var(--color-danger);
    }

    .chip-setup {
      color: var(--color-accent);
      text-decoration: underline;
    }

    .chip-main:focus-visible,
    .chip-retry:focus-visible,
    .chip-setup:focus-visible {
      outline: 2px solid var(--color-accent);
      outline-offset: -2px;
    }

    .chip-name {
      font-weight: var(--weight-medium);
      color: var(--color-text);
    }

    .chip-dot {
      inline-size: 7px;
      block-size: 7px;
      border-radius: 50%;
      background: var(--color-success);
    }

    .chip.is-busy .chip-dot {
      background: var(--color-accent);
      animation: pulse 1.1s ease-in-out infinite;
    }

    .chip.is-failed .chip-dot {
      background: var(--color-danger);
    }

    .chip.is-stopped .chip-dot {
      background: var(--color-text-muted);
    }

    @keyframes pulse {
      50% {
        opacity: 0.25;
      }
    }

    .plan {
      display: flex;
      flex-wrap: wrap;
      align-items: center;
      gap: var(--space-2);
      margin-block-start: var(--space-3);
      font-size: var(--text-sm);
      color: var(--color-text-secondary);
    }

    .plan app-icon {
      color: var(--color-accent);
    }

    .query {
      padding: 1px 8px;
      border-radius: var(--radius-full);
      background: var(--color-surface-3);
    }


    .warn {
      display: flex;
      align-items: center;
      gap: var(--space-2);
      margin-block-start: var(--space-2);
      color: var(--color-text-secondary);
      font-size: var(--text-sm);
    }

    @media (prefers-reduced-motion: reduce) {
      .bar {
        animation: none;
        inline-size: 100%;
        opacity: 0.5;
      }

      .chip.is-busy .chip-dot {
        animation: none;
      }
    }
  `,
})
export class ResearchStatusComponent {
  protected readonly i18n = inject(I18nService);
  protected readonly ai = inject(AiService);
  protected readonly fmt = inject(FormatService);
  private readonly toasts = inject(ToastService);

  /** The search to describe. */
  readonly session = input.required<ResearchSession>();
  /** The user pressed the "set up" link of a source that needs a key or an address. */
  readonly openSettings = output<void>();

  protected chipState(s: SourceStatus): string {
    switch (s.state) {
      case 'searching':
        return s.found > 0 ? this.i18n.t('research.state.partial').replace('{n}', this.fmt.int(s.found)) : this.i18n.t('research.state.searching');
      case 'failed':
        return this.i18n.t(`research.err.${s.code ?? 'network'}`);
      case 'stopped':
        return this.i18n.t('research.state.stopped');
      default:
        return this.i18n.t('research.state.done').replace('{n}', this.fmt.int(s.found));
    }
  }

  protected chipLabel(s: SourceStatus): string {
    return `${this.sourceLabel(s.id)}: ${this.chipState(s)}`;
  }

  protected toggleOnly(id: SourceId): void {
    this.session().onlySource.set(this.session().onlySource() === id ? null : id);
  }

  protected retry(id: SourceId): void {
    void this.session().retry(id).catch((e: unknown) => this.toasts.show('danger', this.ai.errorText(e)));
  }

  protected cancel(): void {
    void this.session().cancel();
  }

  /** One line under the progress bar: what is going on right now. */
  protected readonly statusText = computed(() => {
    const total = this.session().sources().length;
    const done = this.session().sources().filter((s) => s.state !== 'searching').length;
    const phase = this.session().phase();
    if (phase === 'cancelled') {
      return this.i18n.t('research.cancelled');
    }
    if (phase === 'ranking') {
      return this.i18n.t('research.ai.ranking').replace('{n}', this.fmt.int(this.session().rankedCount()));
    }
    if (phase === 'searching') {
      const base = this.i18n.t('research.progress').replace('{done}', this.fmt.int(done)).replace('{total}', this.fmt.int(total));
      return this.session().aiActive() && this.session().plan().length === 0 ? `${base} · ${this.i18n.t('research.ai.planning')}` : base;
    }
    const a = this.session().ai();
    if (a?.used) {
      return `${this.i18n.t('research.done')} · ${this.i18n.t('research.ai.used').replace('{n}', this.fmt.int(a.tokens))}`;
    }
    return this.i18n.t('research.done');
  });

  protected sourceLabel(s: SourceId): string {
    return this.i18n.t(`research.source.${s}`);
  }
}
