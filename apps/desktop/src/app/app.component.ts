import { ChangeDetectionStrategy, Component, computed, inject, OnInit } from '@angular/core';

import { I18nService, Lang, LANGS } from './i18n/i18n.service';
import { TranslatePipe } from './i18n/translate.pipe';
import { TauriService } from './tauri/tauri.service';
import { Theme, THEMES, ThemeService } from './theme/theme.service';
import { SegmentedControlComponent, SegmentedOption } from './ui/segmented-control.component';

/** Milestone 0 shell: top bar with language/theme switches, a placeholder card, a footer. */
@Component({
  selector: 'app-root',
  changeDetection: ChangeDetectionStrategy.OnPush,
  imports: [TranslatePipe, SegmentedControlComponent],
  templateUrl: './app.component.html',
  styleUrl: './app.component.css',
})
export class AppComponent implements OnInit {
  protected readonly i18n = inject(I18nService);
  protected readonly theme = inject(ThemeService);
  protected readonly tauri = inject(TauriService);

  /** Language names are shown in their own language, whatever the UI language is. */
  protected readonly langOptions = computed<readonly SegmentedOption<Lang>[]>(() =>
    LANGS.map((value) => ({ value, label: this.i18n.t(`lang.${value}`) })),
  );

  protected readonly langOptionLang: Readonly<Record<Lang, string>> = { ar: 'ar', en: 'en' };

  protected readonly themeOptions = computed<readonly SegmentedOption<Theme>[]>(() =>
    THEMES.map((value) => ({ value, label: this.i18n.t(`theme.${value}`) })),
  );

  ngOnInit(): void {
    void this.tauri.loadAppInfo();
  }

  protected onLang(lang: Lang): void {
    this.i18n.setLang(lang);
  }

  protected onTheme(theme: Theme): void {
    this.theme.setTheme(theme);
  }
}
