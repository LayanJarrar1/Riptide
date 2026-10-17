import { Signal } from '@angular/core';

import { Download, DownloadRequest, ProbeResult } from '../model/download';

/**
 * What the UI needs from a download engine. Two implementations exist: `MockEngineService`
 * (plain browser and Karma: a simulation) and `TauriEngineService` (inside the Tauri webview:
 * the real engine over IPC). `provideDownloadEngine()` picks one at startup; components, the
 * store and dialogs inject this class and never a concrete one.
 *
 * Every command resolves when the engine has accepted it; the list itself changes through the
 * `downloads` signal, so callers do not need to await anything to see the effect.
 */
export abstract class DownloadEngine {
  /** Every download, oldest first. */
  abstract readonly downloads: Signal<readonly Download[]>;

  /** Downloads holding a queue slot. */
  abstract readonly active: Signal<readonly Download[]>;

  /** Sum of current speeds. */
  abstract readonly totalSpeedBps: Signal<number>;

  /** Last 60 global speed samples, one per second, newest last. */
  abstract readonly totalSpeedHistory: Signal<readonly number[]>;

  /** How many downloads run at once (the Main queue's limit). */
  abstract readonly maxActive: Signal<number>;

  /** One download by id. */
  abstract get(id: number): Download | undefined;

  /** Asks the engine what `url` points at before adding it. */
  abstract probe(url: string): Promise<ProbeResult>;

  /** Adds a download and resolves with its id. `probe` lets a simulation skip its own probe. */
  abstract add(request: DownloadRequest, probe?: ProbeResult): Promise<number>;

  /**
   * A download that would duplicate a new one: the same link (fragment ignored) or the same file
   * name in the same folder; `undefined` when there is none. `filename` and `dir` default to what
   * `add` would choose.
   */
  abstract findDuplicate(url: string, filename?: string, dir?: string): Promise<Download | undefined>;

  /** Starts a download over from byte zero; everything downloaded so far is discarded. */
  abstract restart(id: number): Promise<void>;

  /** Pauses a queued or running download. */
  abstract pause(id: number): Promise<void>;

  /** Resumes a paused download; it re-enters the queue. */
  abstract resume(id: number): Promise<void>;

  /** Retries a failed download (the same transition as `resume`). */
  abstract retry(id: number): Promise<void>;

  /** Cancels a non-terminal download; the row stays in the list. */
  abstract cancel(id: number): Promise<void>;

  /** Removes a download from the list, optionally deleting the file on disk. */
  abstract remove(id: number, deleteFile?: boolean): Promise<void>;

  /** Pauses every queued or running download. */
  abstract pauseAll(): Promise<void>;

  /** Resumes every paused or failed download. */
  abstract resumeAll(): Promise<void>;

  /** Gives an expired download a fresh address. */
  abstract refresh(id: number, url: string): Promise<void>;
}
