/**
 * Stand-in for `webextension-polyfill` in unit tests (`scripts/test.mjs` aliases the package to
 * this file): a fake `runtime.connectNative` whose ports the test drives by hand.
 */

type Listener<A extends unknown[]> = (...args: A) => void;

/** A fake native-messaging port. */
export class FakePort {
  readonly sent: unknown[] = [];
  error: { message: string } | null = null;
  disconnected = false;
  private readonly messageListeners: Listener<[unknown]>[] = [];
  private readonly disconnectListeners: Listener<[FakePort]>[] = [];

  readonly onMessage = {
    addListener: (l: Listener<[unknown]>): void => void this.messageListeners.push(l),
    removeListener: (l: Listener<[unknown]>): void => {
      const i = this.messageListeners.indexOf(l);
      if (i >= 0) this.messageListeners.splice(i, 1);
    },
  };
  readonly onDisconnect = {
    addListener: (l: Listener<[FakePort]>): void => void this.disconnectListeners.push(l),
    removeListener: (l: Listener<[FakePort]>): void => {
      const i = this.disconnectListeners.indexOf(l);
      if (i >= 0) this.disconnectListeners.splice(i, 1);
    },
  };

  postMessage(message: unknown): void {
    this.sent.push(message);
  }

  disconnect(): void {
    this.disconnected = true;
  }

  /** The host sends a frame. */
  receive(message: unknown): void {
    for (const l of [...this.messageListeners]) l(message);
  }

  /** The browser reports the port closed (`message` is Chrome's error text, `''` for a clean close). */
  drop(message: string): void {
    this.error = message ? { message } : null;
    for (const l of [...this.disconnectListeners]) l(this);
  }
}

/** Ports opened so far, oldest first. */
export const ports: FakePort[] = [];

/** `connectNative` fails like Chrome does when the host is not registered. */
export const behaviour: { throwOnConnect: string | null } = { throwOnConnect: null };

const browser = {
  runtime: {
    connectNative(_name: string): FakePort {
      if (behaviour.throwOnConnect !== null) throw new Error(behaviour.throwOnConnect);
      const port = new FakePort();
      ports.push(port);
      return port;
    },
  },
};

export default browser;
