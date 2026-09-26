#!/usr/bin/env node
// 版本号单一来源：Cargo.toml 的 [workspace.package].version
// 用法：node scripts/sync-version.mjs [x.y.z] [--check]
import { readFileSync, writeFileSync } from 'node:fs';
import { fileURLToPath } from 'node:url';
import path from 'node:path';

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');
const F = {
  cargo: path.join(root, 'Cargo.toml'),
  tauri: path.join(root, 'apps/drive/src-tauri/tauri.conf.json'),
  pkg: path.join(root, 'apps/drive/ui/package.json'),
  lock: path.join(root, 'apps/drive/ui/package-lock.json'),
};
const args = process.argv.slice(2);
const check = args.includes('--check');
const cargoSrc = readFileSync(F.cargo, 'utf8');
const block = cargoSrc.match(/\[workspace\.package\][\s\S]*?version\s*=\s*"([^"]+)"/);
if (!block) throw new Error('Cargo.toml 缺少 [workspace.package].version');
const version = args.find((a) => /^\d+\.\d+\.\d+/.test(a)) || block[1];

const rows = [];
const save = (file, next) => {
  const same = readFileSync(file, 'utf8') === next;
  if (!same && !check) writeFileSync(file, next);
  rows.push([path.relative(root, file).replace(/\\/g, '/'), same ? 'ok' : check ? 'MISMATCH' : 'updated']);
};

save(F.cargo, check ? cargoSrc : cargoSrc.replace(block[0], block[0].replace(/version\s*=\s*"[^"]+"/, `version = "${version}"`)));
if (check && block[1] !== version) rows[rows.length - 1][1] = 'MISMATCH';

for (const [key, file] of [['tauri', F.tauri], ['pkg', F.pkg]]) {
  const json = JSON.parse(readFileSync(file, 'utf8'));
  json.version = version;
  save(file, JSON.stringify(json, null, 2) + '\n');
}

// package-lock：只改前两处（根 + packages[""]）与 ui 包名相邻的 version
let count = 0;
const lock = readFileSync(F.lock, 'utf8').replace(/("(?:name": "webdav-drive-ui",\s*\n\s*)?)"version": "\d+\.\d+\.\d+"/g, (m, prefix) => {
  if (count < 2 && (prefix || count === 0)) { count += 1; return (prefix || '') + `"version": "${version}"`; }
  return m;
});
save(F.lock, lock);

console.log(`版本（来源 Cargo.toml）: ${version}`);
for (const [file, state] of rows) console.log(`  ${state.padEnd(10)} ${file}`);
if (rows.some((row) => row[1] === 'MISMATCH')) process.exit(1);
