import { ChangeDetectionStrategy, Component, signal } from '@angular/core';
import { ComponentFixture, TestBed } from '@angular/core/testing';

import { DownloadState, Segment } from '../model/download';
import { ProgressFlowComponent } from './progress-flow.component';

@Component({
  changeDetection: ChangeDetectionStrategy.OnPush,
  imports: [ProgressFlowComponent],
  template: `<app-progress-flow [segments]="segments()" [size]="size()" [state]="state()" />`,
})
class HostComponent {
  readonly segments = signal<readonly Segment[]>([]);
  readonly size = signal<number | null>(100);
  readonly state = signal<DownloadState>('downloading');
}

describe('ProgressFlowComponent', () => {
  let fixture: ComponentFixture<HostComponent>;
  let host: HostComponent;

  function pieces(): HTMLElement[] {
    return Array.from((fixture.nativeElement as HTMLElement).querySelectorAll<HTMLElement>('.piece'));
  }

  beforeEach(async () => {
    await TestBed.configureTestingModule({ imports: [HostComponent] }).compileComponents();
    fixture = TestBed.createComponent(HostComponent);
    host = fixture.componentInstance;
  });

  it('lays out one piece per segment at its true position with its own fill', () => {
    host.segments.set([
      { index: 0, start: 0, end: 50, written: 25, state: 'active' },
      { index: 1, start: 50, end: 100, written: 50, state: 'done' },
    ]);
    fixture.detectChanges();
    const p = pieces();
    expect(p.length).toBe(2);
    expect(p[0].style.insetInlineStart).toBe('0%');
    expect(p[0].style.inlineSize).toBe('50%');
    expect(p[0].querySelector<HTMLElement>('.fill')?.style.transform).toBe('scaleX(0.5)');
    expect(p[1].style.insetInlineStart).toBe('50%');
    expect(p[1].classList).toContain('p-done');
    const bar = (fixture.nativeElement as HTMLElement).querySelector('[role=progressbar]');
    expect(bar?.getAttribute('aria-valuenow')).toBe('75');
  });

  it('renders an indeterminate stream when the size is unknown', () => {
    host.size.set(null);
    host.segments.set([{ index: 0, start: 0, end: 10, written: 10, state: 'active' }]);
    fixture.detectChanges();
    expect(pieces().length).toBe(0);
    expect((fixture.nativeElement as HTMLElement).querySelector('.stream')).toBeTruthy();
    expect((fixture.nativeElement as HTMLElement).querySelector('[role=progressbar]')?.getAttribute('aria-valuenow')).toBeNull();
  });

  it('collapses to one full bar when completed and changes hue with the state', () => {
    host.segments.set([
      { index: 0, start: 0, end: 50, written: 50, state: 'done' },
      { index: 1, start: 50, end: 100, written: 50, state: 'done' },
    ]);
    host.state.set('completed');
    fixture.detectChanges();
    expect(pieces().length).toBe(1);
    expect(pieces()[0].style.inlineSize).toBe('100%');
    const el = (fixture.nativeElement as HTMLElement).querySelector('app-progress-flow');
    expect(el?.classList).toContain('s-done');
    host.state.set('paused');
    fixture.detectChanges();
    expect(el?.classList).toContain('s-paused');
  });
});
