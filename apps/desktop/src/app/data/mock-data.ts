/**
 * Seed data for the mock engine: a realistic, mixed list so every state, category and
 * direction (Arabic and Latin names) is on screen during a demo.
 */

import { categoryOf, Download, Segment, SegmentState } from '../model/download';

const KB = 1024;
const MB = 1024 * KB;
const GB = 1024 * MB;

/** Deterministic pseudo-random generator so the demo looks the same on every launch. */
export function mulberry32(seed: number): () => number {
  let a = seed >>> 0;
  return () => {
    a = (a + 0x6d2b79f5) >>> 0;
    let t = a;
    t = Math.imul(t ^ (t >>> 15), t | 1);
    t ^= t + Math.imul(t ^ (t >>> 7), t | 61);
    return ((t ^ (t >>> 14)) >>> 0) / 4294967296;
  };
}

/** Equal ranges covering `[0, size)`, like `riptide_core::plan_segments`. */
export function planSegments(size: number, connections: number, minSegment = MB): Segment[] {
  if (size <= 0) {
    return [{ index: 0, start: 0, end: 0, written: 0, state: 'pending' }];
  }
  const n = Math.max(1, Math.min(connections, Math.floor(size / minSegment)));
  const base = Math.floor(size / n);
  const extra = size % n;
  const out: Segment[] = [];
  let start = 0;
  for (let i = 0; i < n; i++) {
    const len = base + (i < extra ? 1 : 0);
    out.push({ index: i, start, end: start + len, written: 0, state: 'pending' });
    start += len;
  }
  return out;
}

/** Segments with a given overall fraction written, spread unevenly like a real transfer. */
function filled(
  size: number,
  connections: number,
  fraction: number,
  rng: () => number,
  activeState: SegmentState = 'active',
): Segment[] {
  return planSegments(size, connections).map((s) => {
    const len = s.end - s.start;
    const jitter = 0.65 + rng() * 0.7;
    const written = Math.min(len, Math.floor(len * fraction * jitter));
    const state: SegmentState =
      written >= len ? 'done' : written > 0 ? activeState : fraction > 0 ? activeState : 'pending';
    return { ...s, written, state };
  });
}

function history(rng: () => number, around: number, n = 60): number[] {
  const out: number[] = [];
  let v = around * (0.6 + rng() * 0.4);
  for (let i = 0; i < n; i++) {
    v += (around - v) * 0.25 + (rng() - 0.5) * around * 0.35;
    out.push(Math.max(0, v));
  }
  return out;
}

interface Seed {
  readonly url: string;
  readonly filename?: string;
  readonly size: number | null;
  readonly state: Download['state'];
  readonly fraction?: number;
  readonly connections?: number;
  readonly speed?: number;
  readonly dir?: string;
  readonly failure?: Download['failure'];
  readonly minutesAgo: number;
  readonly checksum?: Download['checksum'];
  readonly supportsRanges?: boolean;
  readonly queueId?: number;
  readonly priority?: Download['priority'];
}

const DOWNLOADS = 'D:\\Downloads';

const SEEDS: readonly Seed[] = [
  {
    url: 'https://releases.ubuntu.com/24.04.2/ubuntu-24.04.2-desktop-amd64.iso',
    size: 5.8 * GB,
    state: 'downloading',
    fraction: 0.62,
    connections: 8,
    speed: 48.2 * MB,
    minutesAgo: 3,
    checksum: { algo: 'sha256', hex: 'd7fe3d6a0419667d2f8eff12796996328daa0efcaa5e7a1e5a1a2b4b9d5b8c3f' },
  },
  {
    url: 'https://cdn.example.org/courses/physics/محاضرة-الفيزياء-الحديثة-07.mp4',
    size: 1.4 * GB,
    state: 'queued',
    connections: 6,
    dir: 'D:\\Lectures',
    minutesAgo: 1,
    priority: 'high',
  },
  {
    url: 'https://download.blender.org/release/Blender4.3/blender-4.3.2-windows-x64.msi',
    size: 330 * MB,
    state: 'downloading',
    fraction: 0.91,
    connections: 4,
    speed: 9.8 * MB,
    minutesAgo: 6,
  },
  {
    url: 'https://mirror.example.net/datasets/city-sounds-2025-flac.zip',
    size: 2.1 * GB,
    state: 'queued',
    connections: 8,
    minutesAgo: 2,
  },
  {
    url: 'https://files.example.com/reports/التقرير-السنوي-2025.pdf',
    size: 18 * MB,
    state: 'queued',
    connections: 2,
    minutesAgo: 2,
    priority: 'high',
  },
  {
    url: 'https://cdn.example.org/firmware/router-ax6000-v2.3.1.bin',
    size: 96 * MB,
    state: 'queued',
    connections: 4,
    minutesAgo: 1,
  },
  {
    url: 'https://archive.example.org/photos/trip-amman-raw.7z',
    size: 7.3 * GB,
    state: 'paused',
    fraction: 0.44,
    connections: 8,
    dir: 'E:\\Photos',
    minutesAgo: 48,
  },
  {
    url: 'https://static.example.com/ide/riptide-sdk-0.1.0-win64.zip',
    size: 212 * MB,
    state: 'verifying',
    fraction: 1,
    connections: 8,
    minutesAgo: 9,
    checksum: { algo: 'sha256', hex: '2c26b46b68ffc68ff99b453c1d30413413422d706483bfa0f98a5e886266e7ae' },
  },
  {
    url: 'https://cdn.example.org/games/soundtrack-vol2.flac',
    size: 640 * MB,
    state: 'failed',
    fraction: 0.37,
    connections: 4,
    failure: 'expired',
    minutesAgo: 120,
  },
  {
    url: 'https://mirror.example.net/images/debian-12.9.0-amd64-netinst.iso',
    size: 631 * MB,
    state: 'failed',
    fraction: 1,
    connections: 8,
    failure: 'checksum',
    minutesAgo: 35,
    checksum: { algo: 'sha256', hex: '9f181ae12b25840a508786b1756c6352a0e58484998669288c4eec2ab16b19d8' },
  },
  {
    url: 'https://fonts.example.org/ibm-plex-sans-arabic-1.1.0.zip',
    size: 4.2 * MB,
    state: 'completed',
    fraction: 1,
    connections: 4,
    minutesAgo: 200,
  },
  {
    url: 'https://cdn.example.org/video/keynote-2025-1080p.mkv',
    size: 3.9 * GB,
    state: 'completed',
    fraction: 1,
    connections: 8,
    dir: 'D:\\Video',
    minutesAgo: 1440,
  },
  {
    url: 'https://files.example.com/design/brand-guidelines-v4.pdf',
    size: 42 * MB,
    state: 'completed',
    fraction: 1,
    connections: 4,
    minutesAgo: 2880,
  },
  {
    url: 'https://files.example.com/wallpapers/الصحراء-عند-الغروب.png',
    size: 24 * MB,
    state: 'completed',
    fraction: 1,
    connections: 2,
    dir: 'E:\\Photos',
    minutesAgo: 4300,
  },
  {
    url: 'https://static.example.com/apps/vscode-user-setup-x64.exe',
    size: 98 * MB,
    state: 'completed',
    fraction: 1,
    connections: 8,
    minutesAgo: 7200,
  },
  {
    url: 'https://stream.example.org/live/segment-latest.ts',
    size: null,
    state: 'downloading',
    fraction: 0,
    connections: 1,
    speed: 2.4 * MB,
    supportsRanges: false,
    minutesAgo: 0,
  },
];

/** Builds the seed list; `now` keeps timestamps relative to launch. */
export function seedDownloads(now: number, seed = 7): Download[] {
  const rng = mulberry32(seed);
  return SEEDS.map((s, i) => {
    const filename = s.filename ?? decodeURIComponent(s.url.split('/').pop() ?? 'download');
    const size = s.size === null ? null : Math.round(s.size);
    const connections = s.connections ?? 8;
    const fraction = s.state === 'completed' ? 1 : (s.fraction ?? 0);
    const active = s.state === 'downloading' ? 'active' : s.state === 'failed' ? 'failed' : 'pending';
    let segments: Segment[];
    if (size === null) {
      // unknown length: one streaming segment that only grows
      const written = Math.floor(rng() * 180 * MB);
      segments = [{ index: 0, start: 0, end: written, written, state: 'active' }];
    } else {
      segments = filled(size, connections, fraction, rng, active);
      if (s.state === 'completed' || s.state === 'verifying' || (s.state === 'failed' && fraction >= 1)) {
        segments = segments.map((g) => ({ ...g, written: g.end - g.start, state: 'done' }));
      }
      if (s.state === 'paused' || s.state === 'queued') {
        segments = segments.map((g) => ({ ...g, state: g.written >= g.end - g.start ? 'done' : 'pending' }));
      }
    }
    const addedAt = now - s.minutesAgo * 60_000;
    const startedAt = s.state === 'queued' ? null : addedAt + 2_000;
    const completedAt = s.state === 'completed' ? addedAt + 60_000 * (3 + Math.floor(rng() * 20)) : null;
    const speed = s.state === 'downloading' ? (s.speed ?? 5 * MB) : 0;
    return {
      id: i + 1,
      url: s.url,
      filename,
      dir: s.dir ?? DOWNLOADS,
      size,
      supportsRanges: s.supportsRanges ?? true,
      state: s.state,
      category: categoryOf(filename),
      queueId: s.queueId ?? 1,
      queuePosition: i,
      priority: s.priority ?? 'normal',
      connections,
      segments,
      speedBps: speed,
      speedHistory: speed > 0 ? history(rng, speed) : new Array<number>(60).fill(0),
      addedAt,
      startedAt,
      completedAt,
      failure: s.failure ?? null,
      checksum: s.checksum ?? null,
    };
  });
}

const GEN_NAMES = [
  'ubuntu-desktop', 'blender-release', 'city-sounds', 'مشروع-التخرج', 'lecture-physics', 'backup-photos',
  'dataset-train', 'التقرير-السنوي', 'game-patch', 'movie-trailer', 'podcast-episode', 'invoice-scan',
  'مسلسل-الحلقة', 'source-snapshot', 'driver-pack', 'ebook-collection',
] as const;
const GEN_EXTS = ['iso', 'zip', 'mp4', 'pdf', 'mp3', 'exe', 'jpg', '7z', 'mkv', 'docx'] as const;
const GEN_SIZES = [180 * KB, 4 * MB, 38 * MB, 220 * MB, 1.2 * GB, 4.7 * GB] as const;
/** One shared, immutable history for every download that is not moving. */
const IDLE_HISTORY: readonly number[] = new Array<number>(60).fill(0);

/**
 * `count` believable downloads for the list-performance harness (`?mockCount=N`): about 60 %
 * completed, the rest spread over queued, paused, failed and cancelled, with `active` of them
 * downloading. Built to be cheap at 50 000 rows: finished and waiting rows share their segment
 * and history arrays instead of owning copies.
 */
export function generateDownloads(count: number, active: number, now: number, seed = 11): Download[] {
  const rng = mulberry32(seed);
  const out: Download[] = new Array<Download>(count);
  const sharedSegments = new Map<number, readonly Segment[]>();
  const done = (size: number): readonly Segment[] => {
    let segs = sharedSegments.get(size);
    if (segs === undefined) {
      segs = planSegments(size, 4).map((g) => ({ ...g, written: g.end - g.start, state: 'done' as const }));
      sharedSegments.set(size, segs);
    }
    return segs;
  };
  const fresh = (size: number): readonly Segment[] => planSegments(size, 4);
  for (let i = 0; i < count; i++) {
    const name = GEN_NAMES[Math.floor(rng() * GEN_NAMES.length)];
    const ext = GEN_EXTS[Math.floor(rng() * GEN_EXTS.length)];
    const filename = `${name}-${String(i + 1).padStart(5, '0')}.${ext}`;
    const size = Math.round(GEN_SIZES[Math.floor(rng() * GEN_SIZES.length)] * (0.5 + rng()));
    const roll = rng();
    // the newest `active` rows are the moving ones, so a list sorted newest-first shows them at the top
    const isActive = i >= count - active;
    const state: Download['state'] = isActive
      ? 'downloading'
      : roll < 0.6
        ? 'completed'
        : roll < 0.75
          ? 'queued'
          : roll < 0.87
            ? 'paused'
            : roll < 0.96
              ? 'failed'
              : 'cancelled';
    let segments: readonly Segment[];
    let speed = 0;
    let history: readonly number[] = IDLE_HISTORY;
    if (state === 'completed') {
      segments = done(size);
    } else if (isActive) {
      const f = 0.05 + rng() * 0.8;
      segments = filled(size, 4, f, rng, 'active');
      speed = (2 + rng() * 40) * MB;
      history = new Array<number>(60).fill(speed);
    } else {
      segments = fresh(size);
    }
    const addedAt = now - (count - i) * 1000;
    out[i] = {
      id: i + 1,
      url: `https://files.example.com/${encodeURIComponent(filename)}`,
      filename,
      dir: DOWNLOADS,
      size,
      supportsRanges: true,
      state,
      category: categoryOf(filename),
      queueId: 1,
      queuePosition: i,
      priority: 'normal',
      connections: 4,
      segments,
      speedBps: speed,
      speedHistory: history,
      addedAt,
      startedAt: state === 'queued' ? null : addedAt + 1000,
      completedAt: state === 'completed' ? addedAt + 60_000 : null,
      failure: state === 'failed' ? 'network' : null,
      checksum: null,
    };
  }
  return out;
}

export { GB, KB, MB };
