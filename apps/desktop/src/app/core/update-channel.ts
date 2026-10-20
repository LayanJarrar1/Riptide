import type { Settings } from '../features/settings/settings.service';

/** The update channels the UI offers (`Settings['channel']`). */
export type UpdateChannel = Settings['channel'];

/**
 * Whether `version` is a semver pre-release (`0.1.0-alpha.4`): a `-` in the part before any
 * `+build` metadata. Unknown or empty versions are not pre-releases.
 */
export function isPrerelease(version: string | null | undefined): boolean {
  const core = (version ?? '').trim().split('+')[0] ?? '';
  return core.includes('-');
}

/**
 * The channel a build really follows. Every Riptide release so far is a GitHub pre-release, so
 * the stable feed has nothing for a pre-release build: those always follow the beta feed,
 * whatever was saved. A stable build keeps honouring the saved channel. The Rust side applies
 * the same rule (`Channel::effective` in `src-tauri/src/updater.rs`); the saved setting itself
 * is never rewritten.
 */
export function effectiveChannel(saved: UpdateChannel, version: string | null | undefined): UpdateChannel {
  return isPrerelease(version) ? 'beta' : saved;
}
