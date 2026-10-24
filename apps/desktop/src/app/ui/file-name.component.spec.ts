import { ChangeDetectionStrategy, Component, signal } from '@angular/core';
import { TestBed } from '@angular/core/testing';

import { FileNameComponent, nameDirection, splitExtension } from './file-name.component';

@Component({
  changeDetection: ChangeDetectionStrategy.OnPush,
  imports: [FileNameComponent],
  template: `<app-file-name [name]="name()" />`,
})
class HostComponent {
  name = signal('التقرير-السنوي-2025.pdf');
}

describe('FileNameComponent', () => {
  it('detects direction from the first letter', () => {
    expect(nameDirection('التقرير-السنوي-2025.pdf')).toBe('rtl');
    expect(nameDirection('2025-التقرير.pdf')).toBe('rtl');
    expect(nameDirection('report-2025.pdf')).toBe('ltr');
    expect(nameDirection('1234.bin')).toBe('ltr');
  });

  it('splits the extension off, leaving dotfiles and odd suffixes whole', () => {
    expect(splitExtension('a.b.pdf')).toEqual(['a.b', '.pdf']);
    expect(splitExtension('.gitignore')).toEqual(['.gitignore', '']);
    expect(splitExtension('no-extension')).toEqual(['no-extension', '']);
    expect(splitExtension('Release notes. final copy')).toEqual(['Release notes. final copy', '']);
  });

  it('gives an Arabic name its own direction and keeps the extension a separate LTR isolate', () => {
    const fixture = TestBed.createComponent(HostComponent);
    fixture.detectChanges();
    const el: HTMLElement = fixture.nativeElement.querySelector('app-file-name');
    expect(el.getAttribute('dir')).toBe('rtl');
    expect(el.getAttribute('title')).toBe('التقرير-السنوي-2025.pdf');
    expect(el.querySelector('.stem')?.textContent).toBe('التقرير-السنوي-2025');
    const ext = el.querySelector('.ext');
    expect(ext?.textContent).toBe('.pdf');
    expect(ext?.getAttribute('dir')).toBe('ltr');
    expect(getComputedStyle(el).unicodeBidi).toBe('isolate');
  });

  it('keeps Latin names left to right', () => {
    const fixture = TestBed.createComponent(HostComponent);
    fixture.componentInstance.name.set('router-ax6000.bin');
    fixture.detectChanges();
    expect(fixture.nativeElement.querySelector('app-file-name').getAttribute('dir')).toBe('ltr');
  });
});
