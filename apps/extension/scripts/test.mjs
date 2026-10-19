// Zero-config test runner: bundle every `src/**/*.test.ts` with esbuild (already a dev
// dependency) into `dist/test/` and run them with Node's built-in `node:test`.
import { build } from 'esbuild';
import { readdirSync, statSync, rmSync, mkdirSync } from 'node:fs';
import { join, relative, dirname } from 'node:path';
import { fileURLToPath } from 'node:url';
import { spawnSync } from 'node:child_process';

const root = dirname(dirname(fileURLToPath(import.meta.url)));
const srcDir = join(root, 'src');
const outDir = join(root, 'dist', 'test');

function walk(dir, out = []) {
  for (const name of readdirSync(dir)) {
    const p = join(dir, name);
    if (statSync(p).isDirectory()) walk(p, out);
    else if (name.endsWith('.test.ts')) out.push(p);
  }
  return out;
}

const tests = walk(srcDir);
if (tests.length === 0) {
  console.error('no *.test.ts files under src/');
  process.exit(1);
}

rmSync(outDir, { recursive: true, force: true });
mkdirSync(outDir, { recursive: true });

await build({
  entryPoints: tests,
  outdir: outDir,
  outbase: srcDir,
  outExtension: { '.js': '.mjs' },
  bundle: true,
  platform: 'node',
  format: 'esm',
  target: 'node20',
  packages: 'external',
  // the real polyfill refuses to load outside a browser; tests drive a fake port instead
  alias: { 'webextension-polyfill': join(srcDir, 'test-support', 'browser-stub.ts') },
  sourcemap: 'inline',
  logLevel: 'warning',
});

const files = tests.map((t) => join(outDir, relative(srcDir, t).replace(/\.ts$/, '.mjs')));
const result = spawnSync(process.execPath, ['--test', '--enable-source-maps', ...files], {
  stdio: 'inherit',
  cwd: root,
});
process.exit(result.status ?? 1);
