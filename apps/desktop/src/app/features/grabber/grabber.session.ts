import { computed, inject, Injectable, signal } from '@angular/core';

import { CRAWL_BACKEND } from '../../data/crawl-backend';
import { QueuesService } from '../../data/queues.service';
import {
  CrawlAddOptions,
  CrawlCounters,
  CrawlEnd,
  CrawlFile,
  CrawlRow,
  CrawlScope,
  CrawlSpecInput,
  CrawlUpdate,
  groupRows,
  hostOf,
  isHttpLink,
  matchesFilter,
  MAX_CRAWL_DEPTH,
  megabytesToBytes,
  parsePatterns,
} from '../../model/crawl';
import { BatchResult, CategoryKey } from '../../model/download';
import { MAIN_QUEUE_ID } from '../../model/queue';
import { SettingsService } from '../settings/settings.service';

/** The dialog's three steps. */
export type GrabStep = 'spec' | 'crawl' | 'add';

/** Why the form cannot start a crawl, as a translation key; `null` when it can. */
export type SpecProblem = 'grab.error.url' | 'grab.error.size' | null;

/**
 * All the state and logic of the site grabber dialog, kept out of the template so a spec can
 * drive it with a fake {@link CrawlBackend}: the form (step 1), the live crawl (step 2: counters,
 * the file list, selection, filter) and the destination (step 3). One instance per dialog.
 */
@Injectable()
export class GrabberSession {
  private readonly backend = inject(CRAWL_BACKEND);
  private readonly settings = inject(SettingsService);
  private readonly queueList = inject(QueuesService);

  readonly step = signal<GrabStep>('spec');

  // step 1: what to crawl
  readonly url = signal('');
  readonly depth = signal(1);
  readonly scope = signal<CrawlScope>('same-host');
  readonly offsiteFiles = signal(true);
  readonly categories = signal<ReadonlySet<CategoryKey>>(new Set());
  readonly includeText = signal('');
  readonly excludeText = signal('');
  readonly minMb = signal('');
  readonly maxMb = signal('');

  // step 2: the crawl
  readonly running = signal(false);
  readonly pages = signal(0);
  readonly skipped = signal(0);
  readonly files = signal<readonly CrawlFile[]>([]);
  readonly end = signal<CrawlEnd | null>(null);
  readonly filter = signal('');
  /** Links the user unchecked; everything else, including files that arrive later, is checked. */
  readonly unchecked = signal<ReadonlySet<string>>(new Set());
  /** A start failure (the engine refused the spec), in the engine's words. */
  readonly startError = signal<string | null>(null);

  // step 3: where it goes
  readonly dir = signal('');
  readonly queueId = signal(MAIN_QUEUE_ID);
  readonly startPaused = signal(false);
  readonly adding = signal(false);

  private crawlId: number | null = null;
  /** Bumped by every start and reset, so late updates of an older crawl are ignored. */
  private generation = 0;

  readonly queues = this.queueList.queues;

  /** The start page's host, for the progress line. */
  readonly host = computed(() => hostOf(this.url().trim()));

  readonly specProblem = computed<SpecProblem>(() => {
    if (!isHttpLink(this.url())) {
      return 'grab.error.url';
    }
    const min = megabytesToBytes(this.minMb());
    const max = megabytesToBytes(this.maxMb());
    const unreadable = (text: string, value: number | null): boolean => text.trim().length > 0 && value === null;
    if (unreadable(this.minMb(), min) || unreadable(this.maxMb(), max) || (min !== null && max !== null && min > max)) {
      return 'grab.error.size';
    }
    return null;
  });

  readonly canStart = computed(() => this.specProblem() === null && !this.running());

  /** The spec as `start_crawl` takes it. */
  readonly spec = computed<CrawlSpecInput>(() => ({
    url: this.url().trim(),
    depth: this.depth(),
    scope: this.scope(),
    offsiteFiles: this.offsiteFiles(),
    categories: [...this.categories()],
    include: parsePatterns(this.includeText()),
    exclude: parsePatterns(this.excludeText()),
    minSize: megabytesToBytes(this.minMb()),
    maxSize: megabytesToBytes(this.maxMb()),
  }));

  readonly counters = computed<CrawlCounters>(() => ({ pages: this.pages(), files: this.files().length, skipped: this.skipped() }));

  /** The grouped rows that pass the filter: group headers and files, in display order. */
  readonly rows = computed<readonly CrawlRow[]>(() => groupRows(this.files(), this.filter()));

  /** The files that pass the filter. */
  readonly visible = computed(() => this.files().filter((f) => matchesFilter(f, this.filter())));

  /** The links that will be added: every checked file, whether or not the filter shows it. */
  readonly selection = computed<readonly string[]>(() => {
    const off = this.unchecked();
    return this.files()
      .filter((f) => !off.has(f.url))
      .map((f) => f.url);
  });

  readonly canNext = computed(() => !this.running() && this.selection().length > 0);
  readonly canAdd = computed(() => this.selection().length > 0 && this.dir().trim().length > 0 && !this.adding());

  /** Back to a blank form: a fresh dialog. Any crawl still known to the engine is dropped. */
  reset(): void {
    this.dropCrawl();
    this.generation++;
    this.step.set('spec');
    this.url.set('');
    this.depth.set(1);
    this.scope.set('same-host');
    this.offsiteFiles.set(true);
    this.categories.set(new Set());
    this.includeText.set('');
    this.excludeText.set('');
    this.minMb.set('');
    this.maxMb.set('');
    this.clearResults();
    this.dir.set(this.settings.settings().defaultDir);
    this.queueId.set(MAIN_QUEUE_ID);
    this.startPaused.set(false);
    this.adding.set(false);
  }

  setDepth(value: number): void {
    this.depth.set(Math.min(MAX_CRAWL_DEPTH, Math.max(0, Math.trunc(value))));
  }

  toggleCategory(category: CategoryKey): void {
    this.categories.update((set) => {
      const next = new Set(set);
      if (!next.delete(category)) {
        next.add(category);
      }
      return next;
    });
  }

  /** Starts the crawl and moves to step 2. A refused spec stays on step 1 with the reason. */
  async start(): Promise<void> {
    if (!this.canStart()) {
      return;
    }
    this.dropCrawl();
    this.clearResults();
    const generation = ++this.generation;
    this.running.set(true);
    this.step.set('crawl');
    try {
      const id = await this.backend.start(this.spec(), (update) => {
        if (generation === this.generation) {
          this.apply(update);
        }
      });
      if (generation !== this.generation) {
        // the dialog was reset while the engine was accepting the spec
        void this.backend.discard(id).catch(() => undefined);
        return;
      }
      this.crawlId = id;
    } catch (error: unknown) {
      if (generation === this.generation) {
        this.running.set(false);
        this.startError.set(String(error));
        this.step.set('spec');
      }
    }
  }

  /** Stops the crawl; the files found so far stay in the list. */
  async cancel(): Promise<void> {
    const id = this.crawlId;
    if (id === null || !this.running()) {
      return;
    }
    try {
      await this.backend.cancel(id);
    } catch (error: unknown) {
      this.end.set({ stop: null, pageCapHit: false, fileCapHit: false, error: String(error) });
      this.running.set(false);
    }
  }

  /** The dialog closes: stop and forget the crawl. */
  close(): void {
    this.dropCrawl();
    this.generation++;
    this.running.set(false);
  }

  toggle(url: string, on: boolean): void {
    this.unchecked.update((set) => {
      const next = new Set(set);
      if (on) {
        next.delete(url);
      } else {
        next.add(url);
      }
      return next;
    });
  }

  /** Checks or unchecks several links at once (a group header). */
  toggleMany(urls: readonly string[], on: boolean): void {
    this.unchecked.update((set) => {
      const next = new Set(set);
      for (const url of urls) {
        if (on) {
          next.delete(url);
        } else {
          next.add(url);
        }
      }
      return next;
    });
  }

  /** Checks every file the filter shows. */
  selectAll(): void {
    this.toggleMany(
      this.visible().map((f) => f.url),
      true,
    );
  }

  /** Unchecks every file the filter shows. */
  selectNone(): void {
    this.toggleMany(
      this.visible().map((f) => f.url),
      false,
    );
  }

  isChecked(url: string): boolean {
    return !this.unchecked().has(url);
  }

  next(): void {
    if (this.step() === 'crawl' && this.canNext()) {
      this.step.set('add');
    }
  }

  back(): void {
    this.step.set(this.step() === 'add' ? 'crawl' : 'spec');
  }

  /** Adds the checked files. Resolves with the engine's answer; `null` when nothing was sent. */
  async add(): Promise<BatchResult | null> {
    const id = this.crawlId;
    if (id === null || !this.canAdd()) {
      return null;
    }
    const options: CrawlAddOptions = { dir: this.dir().trim(), queueId: this.queueId(), startPaused: this.startPaused() };
    this.adding.set(true);
    try {
      const result = await this.backend.add(id, this.selection(), options);
      if (result.added.length > 0) {
        // the engine forgets the crawl once its files are in
        this.crawlId = null;
      }
      return result;
    } finally {
      this.adding.set(false);
    }
  }

  private apply(update: CrawlUpdate): void {
    const found: CrawlFile[] = [];
    let pages = 0;
    let skipped = 0;
    let final: CrawlCounters | null = null;
    for (const event of update.events) {
      if (event.kind === 'page') {
        pages++;
      } else if (event.kind === 'file') {
        found.push(event.file);
      } else if (event.kind === 'skipped') {
        skipped++;
      } else {
        final = event.counters;
      }
    }
    if (found.length > 0) {
      this.files.update((all) => all.concat(found));
    }
    // the engine's own totals, sent with the last update, are final
    this.pages.update((n) => (final ? final.pages : n + pages));
    this.skipped.update((n) => (final ? final.skipped : n + skipped));
    if (update.end) {
      this.end.set(update.end);
      this.running.set(false);
    }
  }

  private clearResults(): void {
    this.running.set(false);
    this.pages.set(0);
    this.skipped.set(0);
    this.files.set([]);
    this.end.set(null);
    this.filter.set('');
    this.unchecked.set(new Set());
    this.startError.set(null);
  }

  private dropCrawl(): void {
    const id = this.crawlId;
    this.crawlId = null;
    if (id !== null) {
      void this.backend.discard(id).catch(() => undefined);
    }
  }
}
