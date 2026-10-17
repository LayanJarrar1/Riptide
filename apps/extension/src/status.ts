/** Connection status to the Riptide app, as the popup and badge show it. Pure; no browser APIs. */
export type HostStatus = 'unknown' | 'connected' | 'app_not_running' | 'host_missing';

/** Every status, for exhaustive UI tables. */
export const HOST_STATUSES: readonly HostStatus[] = ['unknown', 'connected', 'app_not_running', 'host_missing'];

/**
 * Classify a native-messaging port disconnect by the browser's error text.
 *
 * Chrome: "Specified native messaging host not found.", "Access to the specified native
 * messaging host is forbidden.", "Invalid native messaging host name specified.", "Native host
 * has exited.", "Error when communicating with the native messaging host."
 * Firefox: "No such native application app.riptide.host", "An unexpected error occurred".
 *
 * Anything that says the host binary is not registered for this extension means "host missing";
 * everything else (host present but it exited or the pipe to the app failed) is "app not
 * running", which is the actionable state for the user.
 */
export function classifyDisconnect(message: string): HostStatus {
  const m = message.toLowerCase();
  if (
    m.includes('not found') ||
    m.includes('not registered') ||
    m.includes('forbidden') ||
    m.includes('invalid native messaging host') ||
    m.includes('no such native application')
  ) {
    return 'host_missing';
  }
  return 'app_not_running';
}
