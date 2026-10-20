// Copies the Riptide brand-kit app icon PNGs (brand/icon-<size>.png, committed exactly as
// delivered by the brand kit: no re-drawing, no recolouring) into an icons folder.
// No dependencies.
//
//   node scripts/make-icons.mjs [outDir]      (default: icons/)
//   import { writeIcons } from './make-icons.mjs'   (used by build.mjs per target)
import { copyFileSync, mkdirSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';

export const ICON_SIZES = [16, 32, 48, 128];

const root = dirname(dirname(fileURLToPath(import.meta.url)));

/** Copy `icon-<size>.png` for every size from brand/ into `dir`. Returns the file names. */
export function writeIcons(dir) {
  mkdirSync(dir, { recursive: true });
  const names = [];
  for (const size of ICON_SIZES) {
    const name = `icon-${size}.png`;
    copyFileSync(join(root, 'brand', name), join(dir, name));
    names.push(name);
  }
  return names;
}

if (process.argv[1] && fileURLToPath(import.meta.url) === process.argv[1]) {
  const dir = process.argv[2] ? process.argv[2] : join(root, 'icons');
  const names = writeIcons(dir);
  console.log(`icons: copied ${names.join(', ')} to ${dir}`);
}
