// Builds dist/<target> for chrome, edge and firefox from one codebase.
//
//   node scripts/build.mjs [chrome|edge|firefox ...] [--watch]
//
// Per target: manifest = manifest/base.json deep-merged with manifest/<target>.json (arrays are
// unioned), esbuild bundles the four entry points with `__BROWSER__` defined, static pages and
// locales are copied, and the icons are rendered.
import { build, context } from 'esbuild';
import { cpSync, existsSync, mkdirSync, readFileSync, rmSync, writeFileSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';

import { writeIcons } from './make-icons.mjs';

export const TARGETS = ['chrome', 'edge', 'firefox'];
const root = dirname(dirname(fileURLToPath(import.meta.url)));

const args = process.argv.slice(2);
const watch = args.includes('--watch');
const targets = args.filter((a) => !a.startsWith('--'));
for (const t of targets) {
  if (!TARGETS.includes(t)) {
    console.error(`unknown target "${t}"; expected one of ${TARGETS.join(', ')}`);
    process.exit(2);
  }
}

function readJson(path) {
  return JSON.parse(readFileSync(path, 'utf8'));
}

export function mergeManifest(base, overlay) {
  const out = { ...base };
  for (const [key, value] of Object.entries(overlay)) {
    const current = out[key];
    if (Array.isArray(value) && Array.isArray(current)) {
      out[key] = [...current, ...value.filter((v) => !current.includes(v))];
    } else if (
      value &&
      typeof value === 'object' &&
      !Array.isArray(value) &&
      current &&
      typeof current === 'object' &&
      !Array.isArray(current)
    ) {
      out[key] = mergeManifest(current, value);
    } else {
      out[key] = value;
    }
  }
  return out;
}

async function buildTarget(target) {
  const out = join(root, 'dist', target);
  rmSync(out, { recursive: true, force: true });
  mkdirSync(out, { recursive: true });

  const manifest = mergeManifest(
    readJson(join(root, 'manifest', 'base.json')),
    readJson(join(root, 'manifest', `${target}.json`)),
  );
  writeFileSync(join(out, 'manifest.json'), JSON.stringify(manifest, null, 2) + '\n');

  cpSync(join(root, '_locales'), join(out, '_locales'), { recursive: true });
  for (const page of ['popup', 'options']) {
    mkdirSync(join(out, page), { recursive: true });
    for (const ext of ['html', 'css']) {
      const src = join(root, 'src', page, `${page}.${ext}`);
      if (existsSync(src)) cpSync(src, join(out, page, `${page}.${ext}`));
    }
  }
  writeIcons(join(out, 'icons'));

  const options = {
    entryPoints: {
      background: join(root, 'src', 'background.ts'),
      'content/content': join(root, 'src', 'content', 'content.ts'),
      'popup/popup': join(root, 'src', 'popup', 'popup.ts'),
      'options/options': join(root, 'src', 'options', 'options.ts'),
    },
    outdir: out,
    bundle: true,
    format: 'iife',
    platform: 'browser',
    target: ['chrome116', 'firefox128'],
    define: { __BROWSER__: JSON.stringify(target) },
    minify: false,
    sourcemap: false,
    legalComments: 'none',
    loader: { '.svg': 'text' },
    charset: 'utf8',
    logLevel: 'warning',
  };

  if (watch) {
    const ctx = await context(options);
    await ctx.watch();
    console.log(`watching ${target} -> dist/${target}`);
  } else {
    const result = await build({ ...options, metafile: true });
    const files = Object.keys(result.metafile.outputs).length;
    console.log(`built ${target} -> dist/${target} (${files} bundles)`);
  }
}

for (const target of targets.length ? targets : TARGETS) {
  await buildTarget(target);
}
