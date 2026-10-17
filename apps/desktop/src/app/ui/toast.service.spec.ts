import { TestBed } from '@angular/core/testing';

import { TOAST_LEAVE_MS, ToastService } from './toast.service';

describe('ToastService', () => {
  let toasts: ToastService;

  beforeEach(() => {
    jasmine.clock().install();
    jasmine.clock().mockDate(new Date(1_760_000_000_000));
    toasts = TestBed.inject(ToastService);
  });

  afterEach(() => {
    jasmine.clock().uninstall();
  });

  it('shows a toast, marks it leaving after its time and drops it once the exit has played', () => {
    const id = toasts.show('info', 'hello');
    expect(toasts.toasts().map((t) => t.id)).toEqual([id]);
    expect(toasts.toasts()[0].ms).toBe(4000);
    expect(toasts.toasts()[0].leaving).toBeFalse();
    jasmine.clock().tick(3999);
    expect(toasts.toasts()[0].leaving).toBeFalse();
    jasmine.clock().tick(1);
    expect(toasts.toasts()[0].leaving).toBeTrue();
    jasmine.clock().tick(TOAST_LEAVE_MS);
    expect(toasts.toasts().length).toBe(0);
  });

  it('pauses the clock and resumes with the time that was left', () => {
    const id = toasts.show('success', 'done');
    jasmine.clock().tick(1000);
    toasts.pause(id);
    jasmine.clock().tick(60_000);
    expect(toasts.toasts()[0].leaving).toBeFalse();
    toasts.resume(id);
    jasmine.clock().tick(2999);
    expect(toasts.toasts()[0].leaving).toBeFalse();
    jasmine.clock().tick(1);
    expect(toasts.toasts()[0].leaving).toBeTrue();
  });

  it('accepts one action or a list, keeps at most two, and gives action toasts longer', () => {
    const run = (): void => undefined;
    toasts.show('info', 'one', { label: 'Undo', run });
    toasts.show('info', 'many', [
      { label: 'A', run },
      { label: 'B', run },
      { label: 'C', run },
    ]);
    const [one, many] = toasts.toasts();
    expect(one.actions.map((a) => a.label)).toEqual(['Undo']);
    expect(one.ms).toBe(7000);
    expect(many.actions.map((a) => a.label)).toEqual(['A', 'B']);
    const early = toasts.show('danger', 'early');
    toasts.dismiss(early);
    toasts.dismiss(early); // a second dismiss while leaving is a no-op
    expect(toasts.toasts().find((t) => t.id === early)?.leaving).toBeTrue();
    jasmine.clock().tick(TOAST_LEAVE_MS);
    expect(toasts.toasts().map((t) => t.text)).toEqual(['one', 'many']);
  });
});
