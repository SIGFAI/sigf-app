// Fails when a game id in the live catalog (host, guest, needs) has no entry in src/data/games.ts, or has an entry but
// no real art source (Steam id) and no generated art in public/art. Usage: node scripts/check-art.mjs [catalog-url]
import { readFileSync, existsSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';

const app = join(dirname(fileURLToPath(import.meta.url)), '..');
const url = process.argv[2] ?? 'https://sigf.ai/api/app/catalog';
const src = readFileSync(join(app, 'src/data/games.ts'), 'utf8');

const games = new Map();
for (const m of src.matchAll(/^\s*\{ id: '([^']+)', name: ('(?:[^'\\]|\\.)*'|"[^"]*"), short: [^,]+,(.*)$/gm)) {
  games.set(m[1], { name: m[2].slice(1, -1), steam: /\bsteam: \[/.test(m[3]) });
}
const gen = new Set([...(src.match(/const GENERATED = new Set\(\[([^\]]*)\]/)?.[1] ?? '').matchAll(/'([^']+)'/g)].map((m) => m[1]));

const res = await fetch(url);
if (!res.ok) throw new Error(`catalog ${res.status}`);
const body = await res.json();
const items = Array.isArray(body) ? body : body.items ?? body.mods ?? Object.values(body);
const ids = new Set();
for (const it of items) for (const id of [it.host, it.guest, ...(it.needs ?? [])]) if (id) ids.add(id);

const problems = [];
for (const id of [...ids].sort()) {
  const g = games.get(id);
  if (!g) { problems.push(`${id}: no entry in games.ts`); continue; }
  if (!g.name || g.name === id) problems.push(`${id}: no proper display name`);
  const hasGen = gen.has(id);
  if (hasGen) for (const k of ['tall', 'hero']) if (!existsSync(join(app, `public/art/${id}-${k}.webp`))) problems.push(`${id}: missing public/art/${id}-${k}.webp`);
  if (!g.steam && !hasGen) problems.push(`${id}: no Steam art and no generated art`);
}
console.log(`${items.length} catalog items, ${ids.size} game ids, ${games.size} games.ts entries, ${gen.size} with generated art`);
if (problems.length) {
  console.error(problems.join('\n'));
  process.exit(1);
}
console.log('ok: every catalog game has an entry and art');
