import { inject, Pipe, PipeTransform } from '@angular/core';

import { I18nService } from './i18n.service';

/**
 * `{{ 'app.title' | t }}` — looks a key up in the active language.
 *
 * Deliberately impure: a pure pipe is memoised on its input (the key), so it would never
 * re-run when the language signal changes. The transform is a single map lookup, so running
 * it on every check is cheap, and reading `lang()` inside it registers the dependency that
 * makes OnPush views refresh on a language switch.
 */
@Pipe({ name: 't', pure: false })
export class TranslatePipe implements PipeTransform {
  private readonly i18n = inject(I18nService);

  transform(key: string): string {
    return this.i18n.t(key);
  }
}
