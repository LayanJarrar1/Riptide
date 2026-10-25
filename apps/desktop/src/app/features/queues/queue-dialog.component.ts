import { ChangeDetectionStrategy, Component, computed, effect, inject, input, output, signal, untracked } from '@angular/core';

import { NOW } from '../../core/clock';
import { IS_MAC } from '../../core/platform';
import { QueuesService } from '../../data/queues.service';
import { I18nService } from '../../i18n/i18n.service';
import {
  actionOf,
  blankSpec,
  kindOf,
  MAIN_QUEUE_ID,
  MAX_QUEUE_CONCURRENT,
  MAX_QUEUE_NAME_CHARS,
  POST_ACTION_KINDS,
  PostActionKind,
  specOf,
  UiQueue,
} from '../../model/queue';
import { scheduleIssues, toLocalDateTime } from '../../model/schedule';
import type { Schedule } from '../../tauri/bridge/Schedule';
import { ButtonComponent } from '../../ui/button.component';
import { DialogComponent } from '../../ui/dialog.component';
import { ToastService } from '../../ui/toast.service';
import { QueueScheduleEditorComponent } from './queue-schedule-editor.component';

/**
 * Create a queue or change one: name, how many downloads run at once, the schedule and what
 * happens when the queue finishes. The Main queue keeps its name and follows the limit in
 * Settings. Saving goes through `QueuesService`; a refusal (a name that is taken, a program
 * that does not exist) stays in the dialog next to the field.
 */
@Component({
  selector: 'app-queue-dialog',
  changeDetection: ChangeDetectionStrategy.OnPush,
  imports: [ButtonComponent, DialogComponent, QueueScheduleEditorComponent],
  template: `
    <app-dialog [open]="open()" [wide]="true" [title]="i18n.t(queue() === null ? 'queue.dialogCreate' : 'queue.dialogEdit')" (closed)="closed.emit()">
      <form class="form" (submit)="$event.preventDefault(); save()">
        <label class="field">
          <span class="label">{{ i18n.t('queue.name') }}</span>
          <input
            class="input"
            type="text"
            name="name"
            dir="auto"
            autocomplete="off"
            [maxLength]="maxName"
            [disabled]="isMain()"
            [value]="isMain() ? i18n.t('queue.main') : name()"
            [placeholder]="i18n.t('queue.namePlaceholder')"
            [attr.aria-invalid]="nameMissing() && tried()"
            (input)="name.set(text($event))"
          />
          @if (nameMissing() && tried()) {
            <span class="error" role="alert">{{ i18n.t('queue.nameRequired') }}</span>
          }
        </label>

        <label class="field narrow">
          <span class="label">{{ i18n.t('queue.maxConcurrent') }}</span>
          <input class="input" type="number" name="max" inputmode="numeric" min="1" [max]="maxConcurrent" step="1" dir="ltr" [disabled]="isMain()" [value]="max()" (input)="setMax($event)" />
          @if (isMain()) {
            <span class="hint">{{ i18n.t('queue.maxConcurrentMain') }}</span>
          }
        </label>

        <app-queue-schedule-editor [value]="schedule()" (valueChange)="schedule.set($event)" />

        <fieldset class="after">
          <legend class="label">{{ i18n.t('postaction.title') }}</legend>
          <div class="field">
            <select class="input select" name="after" [attr.aria-label]="i18n.t('postaction.title')" [value]="kind()" (change)="kind.set(kindValue($event))">
              @for (k of kinds; track k) {
                <option [value]="k" [selected]="k === kind()">{{ i18n.t('postaction.' + k) }}</option>
              }
            </select>
          </div>
          @if (kind() === 'run_command') {
            <label class="field">
              <span class="label">{{ i18n.t('postaction.command') }}</span>
              <input class="input mono" type="text" name="command" dir="ltr" spellcheck="false" autocomplete="off" [value]="command()" [placeholder]="i18n.t('postaction.commandPlaceholder')" (input)="command.set(text($event))" />
              <span class="hint">{{ i18n.t('postaction.commandHint') }}</span>
              @if (commandMissing() && tried()) {
                <span class="error" role="alert">{{ i18n.t('postaction.commandRequired') }}</span>
              }
            </label>
          }
        </fieldset>

        @if (error(); as message) {
          <p class="error" role="alert">{{ message }}</p>
        }
        <button type="submit" hidden></button>
      </form>
      <ng-container dialogActions>
        <button appButton variant="ghost" type="button" (click)="closed.emit()">{{ i18n.t('action.cancel') }}</button>
        <button appButton variant="primary" type="button" [busy]="saving()" [disabled]="saving()" (click)="save()">
          {{ i18n.t(queue() === null ? 'queue.create' : 'queue.save') }}
        </button>
      </ng-container>
    </app-dialog>
  `,
  styles: `
    .form {
      display: flex;
      flex-direction: column;
      gap: var(--space-4);
      padding-block: var(--space-2);
    }

    .field {
      display: flex;
      flex-direction: column;
      gap: 6px;
      min-inline-size: 0;
    }

    .field.narrow {
      max-inline-size: 180px;
    }

    .label {
      color: var(--color-text-secondary);
      font-size: var(--text-sm);
      font-weight: var(--weight-medium);
    }

    .input {
      inline-size: 100%;
      block-size: 36px;
      padding-inline: 12px;
      border: 1px solid var(--color-border-strong);
      border-radius: var(--radius-sm);
      background: var(--color-surface-2);
      color: var(--color-text);
      font: inherit;
      font-size: var(--text-sm);
    }

    .input:focus {
      outline: none;
      border-color: var(--color-accent);
      box-shadow: 0 0 0 3px var(--color-accent-ring);
    }

    .input:disabled {
      color: var(--color-text-muted);
    }

    .input[aria-invalid='true'] {
      border-color: var(--color-danger);
    }

    .mono {
      font-family: var(--font-mono);
    }

    .after {
      display: flex;
      flex-direction: column;
      gap: var(--space-3);
      min-inline-size: 0;
      margin: 0;
      padding: 0;
      border: 0;
    }

    .hint {
      color: var(--color-text-muted);
      font-size: var(--text-xs);
      line-height: var(--leading-normal);
    }

    .error {
      margin: 0;
      color: var(--color-danger);
      font-size: var(--text-xs);
    }
  `,
})
export class QueueDialogComponent {
  protected readonly i18n = inject(I18nService);
  private readonly queues = inject(QueuesService);
  private readonly toasts = inject(ToastService);
  private readonly now = inject(NOW);

  /** Whether the dialog is showing. */
  readonly open = input.required<boolean>();

  /** The queue being changed, or `null` to create a new one. */
  readonly queue = input<UiQueue | null>(null);

  /** The dialog should close (saved, cancelled, Escape). */
  readonly closed = output<void>();

  /** macOS has no separate hibernate (it decides itself): the choice is not offered there. */
  protected readonly kinds = inject(IS_MAC) ? POST_ACTION_KINDS.filter((k) => k !== 'hibernate') : POST_ACTION_KINDS;
  protected readonly maxName = MAX_QUEUE_NAME_CHARS;
  protected readonly maxConcurrent = MAX_QUEUE_CONCURRENT;

  protected readonly name = signal('');
  protected readonly max = signal(2);
  protected readonly schedule = signal<Schedule>(blankSpec().schedule);
  protected readonly kind = signal<PostActionKind>('nothing');
  protected readonly command = signal('');
  protected readonly tried = signal(false);
  protected readonly saving = signal(false);
  protected readonly error = signal<string | null>(null);

  protected readonly isMain = computed(() => this.queue()?.id === MAIN_QUEUE_ID);
  protected readonly nameMissing = computed(() => !this.isMain() && this.name().trim().length === 0);
  protected readonly commandMissing = computed(() => this.kind() === 'run_command' && this.command().trim().length === 0);

  constructor() {
    // every opening starts from the queue being edited (or a blank one)
    effect(() => {
      if (this.open()) {
        const queue = this.queue();
        untracked(() => this.reset(queue));
      }
    });
  }

  private reset(queue: UiQueue | null): void {
    const spec = queue === null ? blankSpec() : specOf(queue);
    this.name.set(spec.name);
    this.max.set(spec.maxConcurrent);
    this.schedule.set(spec.schedule);
    this.kind.set(kindOf(spec.onDone));
    this.command.set(spec.onDone?.action === 'run_command' ? spec.onDone.command : '');
    this.tried.set(false);
    this.saving.set(false);
    this.error.set(null);
  }

  protected text(event: Event): string {
    return (event.target as HTMLInputElement).value;
  }

  protected kindValue(event: Event): PostActionKind {
    return (event.target as HTMLSelectElement).value as PostActionKind;
  }

  protected setMax(event: Event): void {
    const n = Math.trunc(Number((event.target as HTMLInputElement).value));
    if (Number.isFinite(n)) {
      this.max.set(Math.min(MAX_QUEUE_CONCURRENT, Math.max(1, n)));
    }
  }

  /** Checks the form, then creates or updates; closes on success. */
  protected async save(): Promise<void> {
    if (this.saving()) {
      return;
    }
    this.tried.set(true);
    this.error.set(null);
    const issues = scheduleIssues(this.schedule(), toLocalDateTime(this.now()));
    if (this.nameMissing() || this.commandMissing() || issues.length > 0) {
      return;
    }
    const existing = this.queue();
    const onDone = actionOf(this.kind(), this.command().trim());
    const spec = {
      name: existing !== null && this.isMain() ? existing.name : this.name().trim(),
      maxConcurrent: existing !== null && this.isMain() ? existing.maxConcurrent : this.max(),
      schedule: this.schedule(),
      onDone,
    };
    this.saving.set(true);
    try {
      if (onDone !== null) {
        await this.queues.validateAction(onDone);
      }
      if (existing === null) {
        await this.queues.create(spec);
        this.toasts.show('success', this.i18n.t('toast.queueCreated').replace('{name}', spec.name));
      } else {
        await this.queues.update(existing.id, spec);
        this.toasts.show('success', this.i18n.t('toast.queueSaved'));
      }
      this.closed.emit();
    } catch (error: unknown) {
      this.error.set(error instanceof Error ? error.message : String(error));
    } finally {
      this.saving.set(false);
    }
  }
}
