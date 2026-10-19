#!/usr/bin/env node
// Checks relative links and #anchors in every tracked Markdown file. No dependencies.
//
//   node scripts/check-links.mjs
//
// External (http, https, mailto) links are not fetched. Exit code 1 when any link is broken.
import { execFileSync } from 'node:child_process';
import { readFileSync, existsSync, statSync } from 'node:fs';
import { dirname, resolve, join } from 'node:path';
import { fileURLToPath } from 'node:url';

const root = resolve(dirname(fileURLToPath(import.meta.url)), '..');
const files = execFileSync('git', ['ls-files', '-z', '--', '*.md'], { cwd: root, encoding: 'utf8' })
  .split('\0')
  .filter(Boolean);

/** GitHub's heading slug: lowercase, drop punctuation, spaces to hyphens. */
function slug(heading) {
  return heading
    .trim()
    .toLowerCase()
    .replace(/<[^>]*>/g, '')
    .replace(/[`*_~]/g, '')
    .replace(/[^\p{L}\p{N}\p{M}\s-]/gu, '')
    .replace(/\s/g, '-');
}

/** Removes fenced code blocks so their contents are not parsed. */
function stripFences(text) {
  return text.replace(/^(```|~~~)[^\n]*\n[\s\S]*?^\1[^\n]*$/gm, (m) => m.replace(/[^\n]/g, ''));
}

const anchorCache = new Map();
function anchorsOf(path) {
  if (anchorCache.has(path)) return anchorCache.get(path);
  const set = new Set();
  const seen = new Map();
  const text = stripFences(readFileSync(path, 'utf8'));
  const add = (s) => {
    const n = seen.get(s) ?? 0;
    seen.set(s, n + 1);
    set.add(n === 0 ? s : `${s}-${n}`);
  };
  for (const m of text.matchAll(/^ {0,3}#{1,6}\s+(.+?)\s*#*\s*$/gm)) add(slug(m[1]));
  for (const m of text.matchAll(/<(?:a|h[1-6])\b[^>]*\b(?:id|name)="([^"]+)"/g)) set.add(m[1]);
  anchorCache.set(path, set);
  return set;
}

const problems = [];
for (const rel of files) {
  const path = join(root, rel);
  if (!existsSync(path)) continue;
  const text = stripFences(readFileSync(path, 'utf8')).replace(/`[^`\n]*`/g, (m) => ' '.repeat(m.length));
  const lines = text.split('\n');
  const targets = [];
  lines.forEach((line, i) => {
    for (const m of line.matchAll(/!?\[[^\]]*\]\(\s*<?([^)\s>]+)>?(?:\s+"[^"]*")?\s*\)/g)) targets.push([i + 1, m[1]]);
    for (const m of line.matchAll(/<(?:a|img)\b[^>]*\b(?:href|src)="([^"]+)"/g)) targets.push([i + 1, m[1]]);
  });
  for (const [line, raw] of targets) {
    if (/^(?:[a-z][a-z0-9+.-]*:|\/\/)/i.test(raw)) continue;
    const [pathPart, hash] = raw.split('#');
    let target = path;
    if (pathPart) {
      target = resolve(dirname(path), decodeURIComponent(pathPart));
      if (!existsSync(target)) {
        problems.push(`${rel}:${line}: missing file ${raw}`);
        continue;
      }
    }
    if (hash && statSync(target).isFile() && target.endsWith('.md')) {
      const wanted = decodeURIComponent(hash).toLowerCase();
      if (!anchorsOf(target).has(wanted)) problems.push(`${rel}:${line}: no heading for ${raw}`);
    }
  }
}

if (problems.length > 0) {
  console.error(problems.join('\n'));
  console.error(`\n${problems.length} broken link(s) in ${files.length} Markdown file(s).`);
  process.exit(1);
}
console.log(`ok: relative links and anchors in ${files.length} Markdown file(s)`);
