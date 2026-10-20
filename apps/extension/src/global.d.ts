/** Target browser, injected by `scripts/build.mjs` (`define`) so dead branches are dropped. */
declare const __BROWSER__: 'chrome' | 'edge' | 'firefox';

/** Brand-kit SVG files are bundled as text (esbuild `text` loader). */
declare module '*.svg' {
  const content: string;
  export default content;
}
