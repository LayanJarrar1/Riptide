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
    expect(el().querySelectorAll('.quality').length).toBe(6);
    expect(el().querySelector('input[name="checksum"]')).toBeNull();
    const filename = el().querySelector<HTMLInputElement>('input[name="filename"]');
    expect(filename?.value).toBe('Big Buck Bunny.mp4');

    el().querySelectorAll<HTMLButtonElement>('.quality')[4]?.click();
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

  it('stays a plain link for an ordinary file', async () => {
    await typeUrl('https://example.com/files/a.zip');
    expect(el().querySelector('.media')).toBeNull();
    expect(el().querySelector('input[name="checksum"]')).not.toBeNull();
  });
});
