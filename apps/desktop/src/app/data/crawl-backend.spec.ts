import { CrawlSpecInput, CrawlUpdate, WireCrawlBatch } from '../model/crawl';
import { CRAWL_EVENT, TauriCrawlBackend } from './crawl-backend';

const SPEC: CrawlSpecInput = { url: 'https://a.example/', depth: 1, scope: 'same-host', offsiteFiles: true, categories: [], include: [], exclude: [], minSize: null, maxSize: null };

describe('TauriCrawlBackend', () => {
  let handler: ((b: WireCrawlBatch) => void) | null;
  let unlistened: boolean;
  let calls: { cmd: string; args?: Record<string, unknown> }[];
  let beforeAnswer: (() => void) | null;

  const backend = (): TauriCrawlBackend =>
    new TauriCrawlBackend(
      <T>(cmd: string, args?: Record<string, unknown>) => {
        calls.push({ cmd, args });
        if (cmd === 'start_crawl') {
          beforeAnswer?.();
          return Promise.resolve(5 as unknown as T);
        }
        if (cmd === 'add_crawl_files') {
          return Promise.resolve({ added: [1, 2], skipped: [{ url: 'u', reason: 'duplicate', existing: 9 }] } as unknown as T);
        }
        return Promise.resolve(undefined as unknown as T);
      },
      <T>(event: string, h: (payload: T) => void) => {
        expect(event).toBe(CRAWL_EVENT);
        handler = h as unknown as (b: WireCrawlBatch) => void;
        return Promise.resolve(() => {
          unlistened = true;
        });
      },
    );

  beforeEach(() => {
    handler = null;
    unlistened = false;
    calls = [];
    beforeAnswer = null;
  });

  it('maps events of its own crawl, ignores other crawls, and stops listening at the end', async () => {
    const updates: CrawlUpdate[] = [];
    const id = await backend().start(SPEC, (u) => updates.push(u));
    expect(id).toBe(5);
    expect(calls[0]).toEqual({ cmd: 'start_crawl', args: { spec: SPEC } });
    handler?.({ id: 6, events: [{ type: 'skipped', url: 'x' }], end: null });
    handler?.({ id: 5, events: [{ type: 'skipped', url: 'y' }], end: null });
    expect(updates).toEqual([{ events: [{ kind: 'skipped', url: 'y' }], end: undefined }]);
    const end = { stop: 'finished', pageCapHit: false, fileCapHit: false, error: null } as const;
    handler?.({ id: 5, events: [], end });
    expect(updates.length).toBe(2);
    expect(updates[1].end).toEqual(end);
    expect(unlistened).toBeTrue();
    handler?.({ id: 5, events: [{ type: 'skipped', url: 'late' }], end: null });
    expect(updates.length).toBe(2);
  });

  it('replays events that arrive before start_crawl has answered with the id', async () => {
    const updates: CrawlUpdate[] = [];
    beforeAnswer = () => {
      handler?.({ id: 5, events: [{ type: 'page-visited', url: 'https://a.example/', depth: 0, links: 1 }], end: null });
      handler?.({ id: 4, events: [{ type: 'skipped', url: 'other crawl' }], end: null });
    };
    await backend().start(SPEC, (u) => updates.push(u));
    expect(updates.map((u) => u.events)).toEqual([[{ kind: 'page', url: 'https://a.example/', depth: 0 }]]);
  });

  it('rejects with the engine reason and stops listening when the spec is refused', async () => {
    const b = new TauriCrawlBackend(
      () => Promise.reject(new Error('bad spec')),
      () => Promise.resolve(() => (unlistened = true)),
    );
    await expectAsync(b.start(SPEC, () => undefined)).toBeRejectedWithError('bad spec');
    expect(unlistened).toBeTrue();
  });

  it('sends cancel, discard and add with the commands arguments', async () => {
    const b = backend();
    await b.cancel(5);
    await b.discard(5);
    const result = await b.add(5, ['u1', 'u2', 'u3'], { dir: 'D://dl', queueId: 2, startPaused: false });
    expect(calls).toEqual([
      { cmd: 'cancel_crawl', args: { id: 5 } },
      { cmd: 'discard_crawl', args: { id: 5 } },
      { cmd: 'add_crawl_files', args: { id: 5, urls: ['u1', 'u2', 'u3'], options: { dir: 'D://dl', queueId: 2, startPaused: false } } },
    ]);
    expect(result).toEqual({ added: [1, 2], skipped: [{ url: 'u', reason: 'duplicate' }] });
  });
});
