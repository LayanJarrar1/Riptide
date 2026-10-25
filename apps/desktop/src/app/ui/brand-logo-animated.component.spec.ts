import { ChangeDetectionStrategy, Component, signal } from '@angular/core';
import { ComponentFixture, TestBed } from '@angular/core/testing';

import { REDUCED_MOTION_QUERY } from '../core/motion.service';
import { MemoryPreferenceStore, PREFERENCE_STORE } from '../core/preferences';
import { ThemeService } from '../theme/theme.service';
import { BrandLogoAnimatedComponent } from './brand-logo-animated.component';
import { LOGO_MARK_FILL, LOGO_MARK_PATH, LOGO_WORDMARK_FILL } from './brand-logo-paths';

@Component({
  changeDetection: ChangeDetectionStrategy.OnPush,
  imports: [BrandLogoAnimatedComponent],
  template: `<app-brand-logo-animated [replay]="replay()" [showWordmark]="word()" />`,
})
class HostComponent {
  readonly replay = signal(0);
  readonly word = signal(true);
}

describe('BrandLogoAnimatedComponent', () => {
  let fixture: ComponentFixture<HostComponent>;

  function setup(reduced: boolean): void {
    TestBed.configureTestingModule({
      providers: [
        { provide: PREFERENCE_STORE, useValue: new MemoryPreferenceStore() },
        {
          provide: REDUCED_MOTION_QUERY,
          useValue: { matches: reduced, addEventListener: () => undefined },
        },
      ],
    });
    fixture = TestBed.createComponent(HostComponent);
    fixture.detectChanges();
  }
  const root = (): HTMLElement => fixture.nativeElement as HTMLElement;

  it('draws the brand-kit mark path and is labelled', () => {
    setup(false);
    const svg = root().querySelector('svg');
    expect(svg?.getAttribute('role')).toBe('img');
    expect(svg?.getAttribute('aria-label')).toBe('Riptide');
    expect(root().querySelector('path.fill')?.getAttribute('d')).toBe(LOGO_MARK_PATH);
    expect(root().querySelectorAll('path.reveal').length).toBe(3);
  });

  it('skips the animation under reduced motion', () => {
    setup(true);
    expect(root().querySelector('svg.play')).toBeNull();
    expect(root().querySelector('path.fill')?.getAttribute('d')).toBe(LOGO_MARK_PATH);
  });

  it('plays when motion is allowed', () => {
    setup(false);
    expect(root().querySelector('svg.play')).not.toBeNull();
  });

  it('picks the wordmark variant for the theme', () => {
    setup(false);
    const theme = TestBed.inject(ThemeService);
    theme.setTheme('dark');
    fixture.detectChanges();
    expect(root().querySelector('path.fill')?.getAttribute('fill')).toBe(LOGO_MARK_FILL.dark);
    expect(root().querySelector('.word')?.getAttribute('fill')).toBe(LOGO_WORDMARK_FILL.dark);
    theme.setTheme('light');
    fixture.detectChanges();
    expect(root().querySelector('path.fill')?.getAttribute('fill')).toBe(LOGO_MARK_FILL.light);
    expect(root().querySelector('.word')?.getAttribute('fill')).toBe(LOGO_WORDMARK_FILL.light);
  });

  it('omits the wordmark when asked', () => {
    setup(false);
    fixture.componentInstance.word.set(false);
    fixture.detectChanges();
    expect(root().querySelector('.word')).toBeNull();
  });

  it('re-creates the svg only when replay changes', () => {
    setup(false);
    const first = root().querySelector('svg');
    fixture.detectChanges();
    expect(root().querySelector('svg')).toBe(first);
    fixture.componentInstance.replay.set(1);
    fixture.detectChanges();
    fixture.detectChanges();
    expect(root().querySelector('svg')).not.toBe(first);
  });
});
