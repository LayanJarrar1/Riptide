/**
 * Native Messaging client for the `app.riptide.host` binary.
 *
 * One port, opened on demand and closed after a short idle period so the host process (and the
 * service worker) can go away between downloads. Requests are correlated by `id`; responses are
 * validated with `parseResponse`. The connection status is derived from what happens:
 *
 * - `pong`                        -> `connected`
 * - `error` in answer to `ping`   -> `app_not_running` (host is there, the app's pipe is not)
 * - port disconnect               -> `host_missing` or `app_not_running` (see `classifyDisconnect`)
 */
import browser from 'webextension-polyfill';
import type { Runtime } from 'webextension-polyfill';

import { HOST_NAME, ProtocolError, makeRequest, parseResponse } from './protocol';
import type { RequestBody, Response } from './protocol';
import { classifyDisconnect } from './status';
import type { HostStatus } from './status';

/** A request that could not be answered. */
export class HostError extends Error {
  override readonly name = 'HostError';
  constructor(
    readonly status: HostStatus,
    message: string,
  ) {
    super(message);
  }
}

interface Pending {
  resolve: (r: Response) => void;
  reject: (e: Error) => void;
  timer: ReturnType<typeof setTimeout>;
}

function chromeLastError(): string {
  const g = globalThis as { chrome?: { runtime?: { lastError?: { message?: string } } } };
  return g.chrome?.runtime?.lastError?.message ?? '';
}

function errorText(e: unknown): string {
  return e instanceof Error ? e.message : String(e);
}

/** The client. One instance per background script. */
export class HostClient {
  /** Last known status. */
  status: HostStatus = 'unknown';
  /** App version from the last `pong`. */
  version: string | null = null;
  /** Called on every status change. */
  onStatus: (status: HostStatus) => void = () => undefined;

  private port: Runtime.Port | null = null;
  private nextId = 1;
  private readonly pending = new Map<number, Pending>();
  private idleTimer: ReturnType<typeof setTimeout> | null = null;
  private missingSince = 0;

  constructor(
    private readonly idleMs = 20_000,
    private readonly missingRetryMs = 60_000,
  ) {}

  /** Send a request and wait for its answer. */
  request(body: RequestBody, timeoutMs = 15_000): Promise<Response> {
    if (this.status === 'host_missing' && Date.now() - this.missingSince < this.missingRetryMs && !this.port) {
      return Promise.reject(new HostError('host_missing', 'native messaging host not registered'));
    }
    let port: Runtime.Port;
    try {
      port = this.connect();
    } catch (e) {
      this.setStatus(classifyDisconnect(errorText(e)));
      return Promise.reject(new HostError(this.status, errorText(e)));
    }
    const id = this.nextId++;
    return new Promise<Response>((resolve, reject) => {
      const timer = setTimeout(() => {
        this.pending.delete(id);
        reject(new HostError('app_not_running', 'no answer from the host'));
        this.scheduleIdle();
      }, timeoutMs);
      this.pending.set(id, { resolve, reject, timer });
      try {
        port.postMessage(makeRequest(id, body));
      } catch (e) {
        clearTimeout(timer);
        this.pending.delete(id);
        this.handleDisconnect(errorText(e) || chromeLastError());
        reject(new HostError(this.status, errorText(e)));
      }
    });
  }

  /** Liveness check; updates and returns the status. */
  async ping(timeoutMs = 5_000): Promise<HostStatus> {
    try {
      const r = await this.request({ type: 'ping' }, timeoutMs);
      if (r.type === 'pong') {
        this.version = r.version;
        this.setStatus('connected');
      } else if (r.type === 'error') {
        this.setStatus('app_not_running');
      }
    } catch (e) {
      // A disconnect already updated the status. A timeout does not, and an `unknown` status
      // would hide that the app never answered.
      if (e instanceof HostError && this.status === 'unknown') this.setStatus(e.status);
    }
    return this.status;
  }

  /** Close the port (the host process exits). Pending requests are rejected. */
  disconnect(): void {
    if (this.idleTimer) {
      clearTimeout(this.idleTimer);
      this.idleTimer = null;
    }
    const port = this.port;
    this.port = null;
    if (port) {
      port.onMessage.removeListener(this.onMessage);
      port.onDisconnect.removeListener(this.onDisconnect);
      try {
        port.disconnect();
      } catch {
        /* already gone */
      }
    }
    this.rejectAll(new HostError(this.status, 'disconnected'));
  }

  private connect(): Runtime.Port {
    if (this.port) return this.port;
    const port = browser.runtime.connectNative(HOST_NAME);
    port.onMessage.addListener(this.onMessage);
    port.onDisconnect.addListener(this.onDisconnect);
    this.port = port;
    return port;
  }

  private readonly onMessage = (raw: unknown): void => {
    let response: Response;
    try {
      response = parseResponse(raw);
    } catch (e) {
      if (e instanceof ProtocolError) console.warn('[riptide] dropped malformed frame:', e.message);
      return;
    }
    if (response.id === 0) return; // unsolicited event; the extension never subscribes
    const p = this.pending.get(response.id);
    if (!p) return;
    this.pending.delete(response.id);
    clearTimeout(p.timer);
    if (response.type !== 'error') this.setStatus('connected');
    p.resolve(response);
    this.scheduleIdle();
  };

  private readonly onDisconnect = (port: Runtime.Port): void => {
    const message = port.error?.message ?? chromeLastError();
    this.handleDisconnect(message);
  };

  private handleDisconnect(message: string): void {
    this.port = null;
    if (this.idleTimer) {
      clearTimeout(this.idleTimer);
      this.idleTimer = null;
    }
    // A clean disconnect with nothing pending (our own idle close) says nothing about the app.
    if (this.pending.size === 0 && !message) return;
    const status = classifyDisconnect(message);
    if (status === 'host_missing') this.missingSince = Date.now();
    this.setStatus(status);
    this.rejectAll(new HostError(status, message || 'host disconnected'));
  }

  private rejectAll(error: HostError): void {
    for (const p of this.pending.values()) {
      clearTimeout(p.timer);
      p.reject(error);
    }
    this.pending.clear();
  }

  private scheduleIdle(): void {
    if (this.idleTimer) clearTimeout(this.idleTimer);
    this.idleTimer = setTimeout(() => {
      this.idleTimer = null;
      if (this.pending.size === 0) this.disconnect();
    }, this.idleMs);
  }

  private setStatus(status: HostStatus): void {
    if (status === this.status) return;
    this.status = status;
    if (status !== 'connected') this.version = null;
    this.onStatus(status);
  }
}
