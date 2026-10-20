#!/usr/bin/env node
// Downloads each entry of tests/fixtures/public-download-sources.json through the real
// `riptide fetch`, verifies size / checksum where known, records the observed SHA-256 and
// prints a result table. Manual or scheduled use only: it touches the public internet, so it is
// NOT part of scripts/check.ps1 or CI. Node 18+, no dependencies.
//
//   cargo build --release -p riptide-cli
//   node scripts/test-public-sources.mjs                  # run everything, print the table
//   node scripts/test-public-sources.mjs --update         # also write last_result into the JSON
//   node scripts/test-public-sources.mjs --only gutenberg # ids containing "gutenberg"
//   node scripts/test-public-sources.mjs --cli path\to\riptide.exe --delay 3000
//
// Politeness: strictly one download at a time, a pause between sources, the Riptide user agent
// (the CLI default), at most 2 connections per file, every file is <= 5 MB. Temp folders are
// removed afterwards.

import { spawn } from 'node:child_process';
import { createHash } from 'node:crypto';
import { createReadStream, existsSync, mkdirSync, mkdtempSync, readdirSync, readFileSync, rmSync, statSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

const here = dirname(fileURLToPath(import.meta.url));
const root = resolve(here, '..');
const fixture = join(root, 'tests', 'fixtures', 'public-download-sources.json');
const MAX_BYTES = 5 * 1024 * 1024;

function parseArgs(argv) {
  const o = { update: false, only: null, cli: null, delay: 2500, timeout: 120000, md: null };
  for (let i = 0; i < argv.length; i++) {
    const a = argv[i];
    if (a === '--update') o.update = true;
    else if (a === '--only') o.only = argv[++i];
    else if (a === '--cli') o.cli = argv[++i];
    else if (a === '--delay') o.delay = Number(argv[++i]);
    else if (a === '--timeout') o.timeout = Number(argv[++i]);
    else if (a === '--md') o.md = argv[++i];
    else if (a === '-h' || a === '--help') {
      console.log(readFileSync(fileURLToPath(import.meta.url), 'utf8').split('\n').slice(1, 16).join('\n'));
      process.exit(0);
    } else throw new Error(`unknown argument ${a}`);
  }
  return o;
}

function findCli(explicit) {
  const exe = process.platform === 'win32' ? 'riptide.exe' : 'riptide';
  const candidates = [explicit, join(root, 'target', 'release', exe)].filter(Boolean);
  for (const c of candidates) if (existsSync(c)) return resolve(c);
  throw new Error(`riptide CLI not found (tried ${candidates.join(', ')}); build it with: cargo build --release -p riptide-cli`);
}

function hashFile(path, algo) {
  return new Promise((res, rej) => {
    const h = createHash(algo);
    createReadStream(path).on('data', (d) => h.update(d)).on('error', rej).on('end', () => res(h.digest('hex')));
  });
}

function runCli(cli, args, timeoutMs) {
  return new Promise((res) => {
    const started = Date.now();
    const child = spawn(cli, args, { stdio: ['ignore', 'pipe', 'pipe'], windowsHide: true });
    let stdout = '';
    let stderr = '';
    let timedOut = false;
    child.stdout.on('data', (d) => (stdout += d));
    child.stderr.on('data', (d) => (stderr += d));
    const timer = setTimeout(() => {
      timedOut = true;
      child.kill();
    }, timeoutMs);
    child.on('close', (code) => {
      clearTimeout(timer);
      res({ code, stdout, stderr, timedOut, ms: Date.now() - started });
    });
    child.on('error', (e) => {
      clearTimeout(timer);
      res({ code: -1, stdout, stderr: String(e), timedOut, ms: Date.now() - started });
    });
  });
}

/** What kind of failure is this? Product defects are the ones we must fix. */
function classifyFailure(text, timedOut) {
  const t = text.toLowerCase();
  if (timedOut) return 'temporary-network';
  if (/\b429\b|too many requests|rate.?limit/.test(t)) return 'rate-limit';
  if (/\b(404|410)\b|not found|gone/.test(t)) return 'expired-or-removed';
  if (/\b401\b|\b403\b|forbidden|unauthori[sz]ed/.test(t)) return 'access-denied-or-expired';
  if (/\b5\d\d\b|bad gateway|unavailable|server error/.test(t)) return 'external-server';
  if (/dns|resolve|connect|connection|timed out|timeout|reset|refused|unreachable|network|tls|certificate|ssl/.test(t)) return 'temporary-network';
  if (/panick|unreachable code|stack overflow/.test(t)) return 'product-defect';
  if (/checksum|mismatch/.test(t)) return 'checksum-mismatch';
  return 'unclassified';
}

function expectedHex(entry) {
  const c = entry.checksum;
  if (!c) return null;
  if (c.hex) return { algo: c.algo, hex: c.hex.toLowerCase() };
  if (c.sri) {
    const [algo, b64] = c.sri.split('-', 2);
    return { algo, hex: Buffer.from(b64, 'base64').toString('hex') };
  }
  return null;
}

async function runOne(cli, entry, opts) {
  const work = mkdtempSync(join(tmpdir(), 'riptide-public-'));
  const out = join(work, 'out');
  const data = join(work, 'data');
  mkdirSync(out); // an existing folder tells the CLI to name the file itself
  const result = { date: new Date().toISOString(), status: 'fail', class: null, bytes: null, sha256: null, seconds: null, notes: [] };
  try {
    const args = ['--data-dir', data, '--lang', 'en', 'fetch', entry.url, '-o', out, '-c', '2', '-q'];
    const run = await runCli(cli, args, opts.timeout);
    result.seconds = Math.round(run.ms / 100) / 10;
    const files = existsSync(out) ? readdirSync(out) : [];
    const partials = files.filter((f) => f.endsWith('.rtpart'));
    const finals = files.filter((f) => !f.endsWith('.rtpart'));
    if (run.code !== 0 || run.timedOut) {
      result.class = classifyFailure(`${run.stderr}\n${run.stdout}`, run.timedOut);
      result.notes.push(`exit ${run.code}${run.timedOut ? ' (timeout)' : ''}: ${(run.stderr || run.stdout).trim().split('\n').slice(-2).join(' | ').slice(0, 200)}`);
      if (finals.length) {
        result.class = 'product-defect';
        result.notes.push(`failed run left a final file: ${finals.join(', ')}`);
      }
      return result;
    }
    if (finals.length !== 1) {
      result.class = 'product-defect';
      result.notes.push(`expected exactly 1 file, found ${finals.length}: ${finals.join(', ')}`);
      return result;
    }
    if (partials.length) {
      result.class = 'product-defect';
      result.notes.push(`partial file left after success: ${partials.join(', ')}`);
      return result;
    }
    const path = join(out, finals[0]);
    const size = statSync(path).size;
    result.bytes = size;
    result.sha256 = await hashFile(path, 'sha256');
    result.notes.push(`saved as ${finals[0]}`);
    if (size === 0) {
      result.class = 'product-defect';
      result.notes.push('empty file');
      return result;
    }
    if (size > MAX_BYTES) result.notes.push(`WARNING ${size} bytes exceeds the 5 MB budget`);
    let ok = true;
    if (entry.expected?.size != null && size !== entry.expected.size && !entry.volatile) {
      ok = false;
      result.class = 'size-mismatch';
      result.notes.push(`size ${size} != expected ${entry.expected.size}`);
    }
    if (entry.expected?.name && finals[0] !== entry.expected.name) {
      result.notes.push(`name differs from the expectation (${entry.expected.name})`);
    }
    const want = expectedHex(entry);
    if (want) {
      const got = want.algo === 'sha256' ? result.sha256 : await hashFile(path, want.algo);
      if (got !== want.hex) {
        ok = false;
        result.class = 'checksum-mismatch';
        result.notes.push(`${want.algo} ${got} != published ${want.hex}`);
      } else {
        result.notes.push(`${want.algo} matches the published value`);
      }
    } else if (entry.last_result?.sha256 && !entry.volatile) {
      if (entry.last_result.sha256 !== result.sha256) {
        result.notes.push('observed SHA-256 differs from the previous run (source changed?)');
      } else {
        result.notes.push('observed SHA-256 equals the previous run');
      }
    }
    result.status = ok ? 'pass' : 'fail';
    return result;
  } finally {
    rmSync(work, { recursive: true, force: true });
  }
}

const sleep = (ms) => new Promise((r) => setTimeout(r, ms));

async function main() {
  const opts = parseArgs(process.argv.slice(2));
  const cli = findCli(opts.cli);
  const doc = JSON.parse(readFileSync(fixture, 'utf8'));
  const entries = doc.sources.filter((s) => !opts.only || s.id.includes(opts.only));
  const rows = [];
  for (const [i, entry] of entries.entries()) {
    if (entry.auth_required) {
      rows.push({ entry, result: { status: 'skip', class: 'auth-required', notes: ['authentication is never bypassed'] } });
      continue;
    }
    process.stderr.write(`[${i + 1}/${entries.length}] ${entry.id} ... `);
    const result = await runOne(cli, entry, opts);
    process.stderr.write(`${result.status}${result.class ? ` (${result.class})` : ''}\n`);
    rows.push({ entry, result });
    if (opts.update) entry.last_result = { ...result, notes: result.notes.join('; ') };
    if (i < entries.length - 1) await sleep(opts.delay);
  }

  const lines = [
    '| Source | Category | Status | Class | Bytes | Seconds | Notes |',
    '|---|---|---|---|---|---|---|',
    ...rows.map(({ entry, result }) =>
      `| ${entry.id} | ${entry.category} | ${result.status} | ${result.class ?? ''} | ${result.bytes ?? ''} | ${result.seconds ?? ''} | ${(result.notes ?? []).join('; ').replace(/\|/g, '/')} |`),
  ];
  const pass = rows.filter((r) => r.result.status === 'pass').length;
  const summary = `\n${pass}/${rows.length} passed. Product defects: ${rows.filter((r) => r.result.class === 'product-defect').length}.`;
  console.log(lines.join('\n') + summary);
  if (opts.md) writeFileSync(opts.md, lines.join('\n') + summary + '\n');
  if (opts.update) writeFileSync(fixture, JSON.stringify(doc, null, 2) + '\n');
  // exit non-zero only for product defects: network trouble must not fail a scheduled job
  process.exit(rows.some((r) => r.result.class === 'product-defect') ? 1 : 0);
}

main().catch((e) => {
  console.error(e.message);
  process.exit(2);
});
