import { ChangeDetectionStrategy, Component, signal } from '@angular/core';
import { ComponentFixture, TestBed } from '@angular/core/testing';

import { MemoryPreferenceStore, NAVIGATOR_LANGUAGES, PREFERENCE_STORE } from '../core/preferences';
import { DownloadState, FailureKind } from '../model/download';
import { StatusPillComponent } from './status-pill.component';

@Component({
  changeDetection: ChangeDetectionStrategy.OnPush,
  imports: [StatusPillComponent],
  template: `<app-status-pill [state]="state()" [failure]="failure()" [summary]="summary()" />`,
})
class HostComponent {
  readonly state = signal<DownloadState>('downloading');
  readonly failure = signal<FailureKind | null>(null);
  readonly summary = signal(false);
}

describe('StatusPillComponent', () => {
  let fixture: ComponentFixture<HostComponent>;
  let host: HostComponent;

  function pill(): HTMLElement {
    const el = (fixture.nativeElement as HTMLElement).querySelector<HTMLElement>('app-status-pill');
    if (!el) {
      throw new Error('no pill');
    }
    return el;
  }

  beforeEach(async () => {
    jasmine.clock().install();
    await TestBed.configureTestingModule({
      imports: [HostComponent],
      providers: [
        { provide: NAVIGATOR_LANGUAGES, useValue: ['en-US'] },
        { provide: PREFERENCE_STORE, useValue: new MemoryPreferenceStore() },
      ],
    }).compileComponents();
    fixture = TestBed.createComponent(HostComponent);
    host = fixture.componentInstance;
    fixture.detectChanges();
  });

  afterEach(() => {
    jasmine.clock().uninstall();
  });

  it('renders the state with its icon and label and plays nothing on first render', () => {
    expect(pill().dataset['state']).toBe('downloading');
    expect(pill().textContent?.trim()).toBe('Downloading');
    jasmine.clock().tick(1);
    fixture.detectChanges();
    expect(pill().className).not.toContain('m-');
  });

  it('draws the check in when a download completes and clears the morph when it ends', () => {
    host.state.set('completed');
    fixture.detectChanges();
    jasmine.clock().tick(1);
    fixture.detectChanges();
    expect(pill().classList).toContain('m-draw');
    expect(pill().querySelector('svg')?.classList).toContain('is-drawing');
    pill().dispatchEvent(new AnimationEvent('animationend', { bubbles: true }));
    fixture.detectChanges();
    expect(pill().classList).not.toContain('m-draw');
    expect(pill().querySelector('svg')?.classList).not.toContain('is-drawing');
  });

  it('in summary mode says "Failed" and keeps the cause as the tooltip', () => {
    host.state.set('failed');
    host.failure.set('network');
    host.summary.set(true);
    fixture.detectChanges();
    expect(pill().textContent?.trim()).toBe('Failed');
    expect(pill().getAttribute('title')).toBe('Connection lost');
  });

  it('shakes once on failure, names the cause, and ignores the icon\'s own animationend', () => {
    host.state.set('failed');
    host.failure.set('expired');
    fixture.detectChanges();
    jasmine.clock().tick(1);
    fixture.detectChanges();
    expect(pill().classList).toContain('m-shake');
    expect(pill().textContent?.trim()).toBe('Link expired');
    pill().querySelector('path')?.dispatchEvent(new AnimationEvent('animationend', { bubbles: true }));
    fixture.detectChanges();
    expect(pill().classList).toContain('m-shake');
  });
});
