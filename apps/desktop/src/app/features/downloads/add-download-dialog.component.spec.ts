import { ComponentFixture, TestBed } from '@angular/core/testing';

import { MemoryPreferenceStore, NAVIGATOR_LANGUAGES, PREFERENCE_STORE } from '../../core/preferences';
import { DownloadEngine } from '../../data/download-engine';
import { MOCK_ENGINE_AUTOSTART, MockEngineService } from '../../data/mock-engine.service';
import { DownloadRequest } from '../../model/download';
import { AddDownloadDialogComponent, mediaErrorKey } from './add-download-dialog.component';

describe('mediaErrorKey', () => {
  it('turns the engine refusals into friendly keys', () => {
    expect(mediaErrorKey(new Error('video support is not installed (yt-dlp was not found)'))).toBe('add.mediaNoTools');
    expect(mediaErrorKey('protected content: this video uses DRM and cannot be downloaded')).toBe('add.mediaDrm');
    expect(mediaErrorKey('live streams are not supported')).toBe('add.mediaLive');
    expect(mediaErrorKey('anything else')).toBe('add.mediaUnsupported');
  });

  it('shows the friendly refusal messages instead of the raw yt-dlp line', () => {
    expect(mediaErrorKey('[rt:refused] [youtube] x: The page needs to be reloaded.')).toBe('add.mediaRefused');
    expect(mediaErrorKey('[rt:login] [youtube] x: Sign in to confirm your age.')).toBe('add.mediaLogin');
    // the tag decides even when the raw text mentions a live stream
    expect(mediaErrorKey('[rt:refused] is live')).toBe('add.mediaRefused');
  });
});

/** Lets the chained promises of the probe and the submit settle. */
async function flush(): Promise<void> {
  for (let i = 0; i < 20; i++) {
    await Promise.resolve();
  }
}

describe('AddDownloadDialogComponent video mode', () => {
  let fixture: ComponentFixture<AddDownloadDialogComponent>;
  let engine: MockEngineService;

  function el(): HTMLElement {
    return fixture.nativeElement as HTMLElement;
  }

  async function typeUrl(url: string): Promise<void> {
    const input = el().querySelector<HTMLInputElement>('input[name="url"]');
    expect(input).not.toBeNull();
    (input as HTMLInputElement).value = url;
    (input as HTMLInputElement).dispatchEvent(new Event('input'));
    jasmine.clock().tick(250);
    jasmine.clock().tick(1000);
    await flush();
    fixture.detectChanges();
  }

  beforeEach(async () => {
    jasmine.clock().install();
    await TestBed.configureTestingModule({
      imports: [AddDownloadDialogComponent],
      providers: [
        { provide: DownloadEngine, useClass: MockEngineService },
        { provide: MOCK_ENGINE_AUTOSTART, useValue: false },
        { provide: NAVIGATOR_LANGUAGES, useValue: ['en-US'] },
        { provide: PREFERENCE_STORE, useValue: new MemoryPreferenceStore() },
      ],
    }).compileComponents();
    engine = TestBed.inject(DownloadEngine) as MockEngineService;
    fixture = TestBed.createComponent(AddDownloadDialogComponent);
    fixture.componentRef.setInput('open', true);
    fixture.detectChanges();
  });

  afterEach(() => {
    jasmine.clock().uninstall();
  });

  it('shows the quality picker for a video page and submits it as media', async () => {
    const add = spyOn(engine, 'add').and.callThrough();
    await typeUrl('https://www.youtube.com/watch?v=abc');
    // one compact dropdown, only the formats the page offers, Best chosen
    const quality = el().querySelector<HTMLSelectElement>('select[name="quality"]') as HTMLSelectElement;
    expect(quality.options.length).toBe(6);
    expect(quality.value).toBe('best');
    expect(quality.options[0].textContent).toMatch(/Best|add\.qualityBest/);
    expect(el().querySelector('input[name="checksum"]')).toBeNull();
    const filename = el().querySelector<HTMLInputElement>('input[name="filename"]');
    expect(filename?.value).toBe('Big Buck Bunny.mp4');

    quality.value = quality.options[4].value;
    quality.dispatchEvent(new Event('change'));
    fixture.detectChanges();
    expect(filename?.value).toBe('Big Buck Bunny.m4a');

    const submit = Array.from(el().querySelectorAll<HTMLButtonElement>('button')).find((b) => b.className.includes('primary'));
    submit?.click();
    await flush();
    const request = add.calls.mostRecent().args[0] as DownloadRequest;
    expect(request.kind).toBe('media');
    expect(request.mediaFormat).toBe('audio-m4a');
    expect(request.category).toBe('music');
    expect(request.checksum).toBeUndefined();
  });

  it('probes a pasted video link at once and offers Best before the list arrives', async () => {
    const original = engine.probeMedia.bind(engine);
    let release: (() => void) | undefined;
    const probe = spyOn(engine, 'probeMedia').and.callFake(
      (url: string) =>
        new Promise((resolve, reject) => {
          release = () => original(url).then(resolve, reject);
        }),
    );
    const add = spyOn(engine, 'add').and.callThrough();
    const input = el().querySelector<HTMLInputElement>('input[name="url"]') as HTMLInputElement;
    input.dispatchEvent(new Event('paste'));
    input.value = 'https://www.youtube.com/watch?v=abc';
    input.dispatchEvent(new Event('input'));
    await flush(); // no timer ticked: a paste skips the typing pause
    fixture.detectChanges();
    expect(probe).toHaveBeenCalledTimes(1);

    // the list has not arrived, yet Best is there and the form can be submitted
    const quality = el().querySelector<HTMLSelectElement>('select[name="quality"]') as HTMLSelectElement;
    expect(quality.options.length).toBe(1);
    expect(quality.value).toBe('best');
    const submit = Array.from(el().querySelectorAll<HTMLButtonElement>('button')).find((b) => b.className.includes('primary'));
    expect(submit?.disabled).toBeFalse();
    submit?.click();
    await flush();
    const request = add.calls.mostRecent().args[0] as DownloadRequest;
    expect(request.kind).toBe('media');
    expect(request.mediaFormat).toBe('best');
    release?.();
  });

  it('stays a plain link for an ordinary file: no quality choice at all', async () => {
    await typeUrl('https://example.com/files/a.zip');
    expect(el().querySelector('.media')).toBeNull();
    expect(el().querySelector('select[name="quality"]')).toBeNull();
    expect(el().querySelector('input[name="checksum"]')).not.toBeNull();
  });

  it('keeps the rarely used options folded away and says when one of them is not the default', async () => {
    await typeUrl('https://example.com/files/a.zip');
    const details = el().querySelector<HTMLDetailsElement>('details.advanced') as HTMLDetailsElement;
    expect(details.open).toBeFalse();
    // connections and the start choice live inside it now
    expect(details.querySelector('[role="radiogroup"]')).not.toBeNull();
    expect(el().querySelector('.advanced-hint')).toBeNull();
    const later = Array.from(details.querySelectorAll<HTMLButtonElement>('.preset')).find((b) => b.getAttribute('aria-checked') === 'false' && !/^\d+$/.test(b.textContent?.trim() ?? ''));
    later?.click();
    fixture.detectChanges();
    expect(el().querySelector('.advanced-hint')?.textContent).toMatch(/starts later|add\.optLater/);
  });

  it('is the compact dialog, not the wide one', () => {
    expect(el().querySelector('dialog.is-wide')).toBeNull();
  });

  it('flags an address that is not a link', async () => {
    await typeUrl('not a link at all');
    expect(el().querySelector('.is-invalid')).not.toBeNull();
    const submit = Array.from(el().querySelectorAll<HTMLButtonElement>('button')).find((b) => b.className.includes('primary'));
    expect(submit?.disabled).toBeTrue();
  });

  it('accepts a very long link without breaking the layout', async () => {
    await typeUrl(`https://example.com/files/${'a'.repeat(1800)}.zip?token=${'b'.repeat(400)}`);
    const dialog = el().querySelector<HTMLElement>('dialog') as HTMLElement;
    expect(dialog.scrollWidth).toBeLessThanOrEqual(dialog.clientWidth + 1);
  });

  it('in RTL keeps the start of the LTR path field clear of the Browse button', () => {
    document.documentElement.dir = 'rtl';
    try {
      const input = el().querySelector<HTMLInputElement>('.input-wrap input[name="dir"]') as HTMLInputElement;
      const style = getComputedStyle(input);
      expect(parseFloat(style.paddingLeft)).toBeGreaterThanOrEqual(90);
      expect(parseFloat(style.paddingRight)).toBeLessThan(60);
    } finally {
      document.documentElement.dir = '';
    }
  });
});
