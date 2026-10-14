import { TestBed } from '@angular/core/testing';

import { isTauri, TauriService } from './tauri.service';

describe('TauriService (outside the Tauri webview)', () => {
  it('reports the bridge as unavailable in a plain browser', () => {
    expect(isTauri()).toBeFalse();
    expect(TestBed.inject(TauriService).available).toBeFalse();
  });

  it('loadAppInfo resolves and leaves appInfo null', async () => {
    const service = TestBed.inject(TauriService);
    await expectAsync(service.loadAppInfo()).toBeResolved();
    expect(service.appInfo()).toBeNull();
  });
});
