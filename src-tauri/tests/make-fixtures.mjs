#!/usr/bin/env node
// Writes the synthetic test fixtures in tests/fixtures/<case>/: one mashup.json recipe per case plus its release
// assets (zip, mrpack, pk3), as the SIGF publisher lays them out. Every archive holds placeholder bytes only
// ("MZ fixture: <path>" for a DLL, a tiny zip for a jar): no real mod, loader or game file is in this repository.
// The tests in tests/recipes.rs check layout, hashes, launch arguments and Restore, so dummy bytes are enough.
//
// Output is deterministic (stored zip entries, fixed timestamps), so re-running this script changes nothing unless a
// case below changes. Run it from anywhere: `node src-tauri/tests/make-fixtures.mjs`, then commit tests/fixtures/.
// Asset URLs are `file:///FIXTURE/<case>/<file>`; tests/recipes.rs re-points them at the checkout.

import { createHash } from 'node:crypto';
import { mkdirSync, rmSync, writeFileSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';

const OUT = join(dirname(fileURLToPath(import.meta.url)), 'fixtures');
const sha256 = (b) => createHash('sha256').update(b).digest('hex');

// ---- minimal deterministic zip writer (stored entries, DOS time 1980-01-01 00:00) ----
const CRC_TABLE = Array.from({ length: 256 }, (_, n) => {
  let c = n;
  for (let k = 0; k < 8; k++) c = c & 1 ? 0xedb88320 ^ (c >>> 1) : c >>> 1;
  return c >>> 0;
});
function crc32(buf) {
  let c = 0xffffffff;
  for (const b of buf) c = CRC_TABLE[(c ^ b) & 0xff] ^ (c >>> 8);
  return (c ^ 0xffffffff) >>> 0;
}
/** entries: [path, string | Buffer][] -> zip bytes */
function zip(entries) {
  const locals = [];
  const centrals = [];
  let offset = 0;
  for (const [path, body] of entries) {
    const name = Buffer.from(path, 'utf8');
    const data = Buffer.isBuffer(body) ? body : Buffer.from(body, 'utf8');
    const crc = crc32(data);
    const local = Buffer.alloc(30);
    local.writeUInt32LE(0x04034b50, 0);
    local.writeUInt16LE(20, 4); // version needed
    local.writeUInt16LE(0x0800, 6); // UTF-8 names
    local.writeUInt16LE(0, 8); // stored
    local.writeUInt16LE(0, 10); // time
    local.writeUInt16LE(0x21, 12); // date 1980-01-01
    local.writeUInt32LE(crc, 14);
    local.writeUInt32LE(data.length, 18);
    local.writeUInt32LE(data.length, 22);
    local.writeUInt16LE(name.length, 26);
    local.writeUInt16LE(0, 28);
    locals.push(local, name, data);
    const central = Buffer.alloc(46);
    central.writeUInt32LE(0x02014b50, 0);
    central.writeUInt16LE(20, 4);
    central.writeUInt16LE(20, 6);
    central.writeUInt16LE(0x0800, 8);
    central.writeUInt16LE(0, 10);
    central.writeUInt16LE(0, 12);
    central.writeUInt16LE(0x21, 14);
    central.writeUInt32LE(crc, 16);
    central.writeUInt32LE(data.length, 20);
    central.writeUInt32LE(data.length, 24);
    central.writeUInt16LE(name.length, 28);
    central.writeUInt32LE(offset, 42);
    centrals.push(central, name);
    offset += 30 + name.length + data.length;
  }
  const cd = Buffer.concat(centrals);
  const end = Buffer.alloc(22);
  end.writeUInt32LE(0x06054b50, 0);
  end.writeUInt16LE(entries.length, 8);
  end.writeUInt16LE(entries.length, 10);
  end.writeUInt32LE(cd.length, 12);
  end.writeUInt32LE(offset, 16);
  return Buffer.concat([...locals, cd, end]);
}

// ---- placeholder contents ----
const dll = (path) => `MZ fixture: ${path}\n`; // a PE file starts with MZ; the tests only check that
const text = (path) => `fixture: ${path}\n`;
const jar = (id) => zip([['fabric.mod.json', JSON.stringify({ schemaVersion: 1, id, version: '0.0.0' })]]);
const mrpack = (name, versionId, mc, overrides) =>
  zip([
    [
      'modrinth.index.json',
      JSON.stringify({ formatVersion: 1, game: 'minecraft', versionId, name, summary: 'Test fixture.', files: [], dependencies: { minecraft: mc, 'fabric-loader': '0.19.5' } }, null, 2),
    ],
    ...overrides,
  ]);
const fake = (paths) => paths.map((p) => [p, p.endsWith('.dll') ? dll(p) : text(p)]);

const BEPINEX = 'BepInEx_win_x64_5.4.23.5.zip';
const bepinexZip = () =>
  zip(
    fake([
      '.doorstop_version',
      'changelog.txt',
      'doorstop_config.ini',
      'winhttp.dll',
      'BepInEx/core/0Harmony.dll',
      'BepInEx/core/BepInEx.dll',
      'BepInEx/core/BepInEx.Preloader.dll',
      'BepInEx/core/Mono.Cecil.dll',
      'BepInEx/core/MonoMod.Utils.dll',
    ]),
  );
const bepinexRequire = { id: 'bepinex', version: '5.4.23.5', license: 'LGPL-2.1', page: 'https://github.com/BepInEx/BepInEx/releases/tag/v5.4.23.5', source: { asset: BEPINEX } };

const sigfMod = (slug, kit) => ({
  source: { repo: `https://github.com/SIGFAI/${slug}`, license: 'MIT' },
  media: {},
  built_by: { agent: 'SIGF' },
  idea_by: 'example',
  ...(kit ? { kit: { id: kit, status: null } } : {}),
  built_at: '2026-01-01T00:00:00.000Z',
});
const upstream = (slug, extra = {}) => ({
  source: {
    repo: `https://github.com/example/${slug}`,
    license: 'MIT',
    upstream_license: 'MIT',
    tag: 'v0.1.0',
    commit: '0000000000000000000000000000000000000000',
    hosted: `https://github.com/SIGFAI/${slug}`,
    ...extra,
  },
  media: {},
  built_by: { author: 'example', authors: ['example'], packaged_by: 'SIGF' },
  idea_by: 'example',
  built_at: '2026-01-01T00:00:00.000Z',
});
const server = (mc, pack) => ({ game: 'minecraft', mc, loader: 'fabric@0.19.5', ram_gb: 2, max_players: 10, pack });

// ---- the cases ----
// `assets`: file name -> bytes. In `recipe`, an install file `{ src, ... }` or a pack `{ src }` gets its url, sha256,
// size (and `contents` when unpacked) filled in; `files` (the release asset list) is filled from `assets`.
const CASES = {
  // A mod for a game started with arguments: the .pk3 goes into the app folder, launch args point at it.
  doom: {
    assets: { 'doom-dragon-shout.pk3': zip([['zscript.zs', 'version "4.14"\n']]) },
    recipe: {
      id: 'sigf/doom-dragon-shout',
      version: '1.0.0',
      name: 'Dragon Shout',
      tagline: 'Test fixture.',
      kind: 'mashup',
      games: [
        { game: 'doom', role: 'host', engine: 'GZDoom + Freedoom 2' },
        { game: 'skyrim', role: 'guest', label: 'Skyrim' },
      ],
      requires: [{ id: 'gzdoom' }, { id: 'freedoom2' }],
      install: [{ game: 'doom', strategy: 'args', files: [{ src: 'doom-dragon-shout.pk3', dst: '{app}/mod.pk3', unpack: false }] }],
      launch: [{ game: 'doom', args: ['-file', '{app}/mod.pk3'] }],
      ...sigfMod('doom-dragon-shout', 'doom'),
    },
  },

  // A Minecraft mod: the .mrpack becomes a Prism instance.
  minecraft: {
    assets: {
      'minecraft-banana.mrpack': mrpack('Banana Blocks', '1.0.0', '26.3', [['overrides/mods/sigf-minecraft-banana.jar', jar('sigf-minecraft-banana')]]),
    },
    recipe: {
      id: 'sigf/minecraft-banana',
      version: '1.0.0',
      name: 'Banana Blocks',
      tagline: 'Test fixture.',
      kind: 'mod',
      games: [{ game: 'minecraft', role: 'host', engine: 'Minecraft Java 26.3 + Fabric Loader 0.19.5', mc: '26.3', loader: 'fabric@0.19.5' }],
      requires: [{ id: 'fabric-loader' }],
      install: [{ game: 'minecraft', strategy: 'mrpack', pack: { src: 'minecraft-banana.mrpack' } }],
      launch: [{ game: 'minecraft', args: [] }],
      ...sigfMod('minecraft-banana', 'minecraft'),
      server: server('26.3', 'minecraft-banana.mrpack'),
    },
  },

  // Two layers unpacked into tf/custom under `profile`.
  tf2: {
    assets: {
      'tf2-dragon-shout.zip': zip([
        ['cfg/sigf.cfg', 'echo mod'],
        ['scripts/vscripts/sigf_mod.nut', 'print("hi")'],
      ]),
      'tf2-dragon-shout-game-kit.zip': zip([
        ['cfg/sigf.cfg', 'echo kit'],
        ['scripts/vscripts/mapspawn.nut', 'IncludeScript("sigf_mod")'],
      ]),
    },
    recipe: {
      id: 'sigf/tf2-dragon-shout',
      version: '1.0.0',
      name: 'Dragon Shout',
      tagline: 'Test fixture.',
      kind: 'mod',
      games: [{ game: 'tf2', role: 'host', engine: null, apps: { steam: '440' } }],
      requires: [],
      install: [
        {
          game: 'tf2',
          strategy: 'profile',
          files: [
            { src: 'tf2-dragon-shout.zip', dst: '{game}/tf/custom/sigf_dragon_shout', unpack: true },
            { src: 'tf2-dragon-shout-game-kit.zip', dst: '{game}/tf/custom/sigf_kit', unpack: true },
          ],
        },
      ],
      launch: [{ game: 'tf2', args: ['-game', 'tf', '-insecure'] }],
      ...sigfMod('tf2-dragon-shout', 'tf2'),
    },
  },

  // A FiveM resource into the FiveM server data folder (`{fivem}`).
  gta5: {
    assets: {
      'gta5-rain-of-cows.zip': zip([
        ['client.lua', 'print("cows")'],
        ['fxmanifest.lua', "fx_version 'cerulean'\ngame 'gta5'\nclient_script 'client.lua'\n"],
      ]),
    },
    recipe: {
      id: 'sigf/gta5-rain-of-cows',
      version: '1.0.0',
      name: 'Rain Of Cows',
      tagline: 'Test fixture.',
      kind: 'mod',
      games: [{ game: 'gta5', role: 'host', engine: 'GTA V + FiveM (local FXServer, Lua resource)', apps: { steam: '271590' } }],
      requires: [{ id: 'fivem' }],
      install: [{ game: 'gta5', strategy: 'profile', runtime: 'fivem', files: [{ src: 'gta5-rain-of-cows.zip', dst: '{fivem}/resources/sigf_rain_of_cows', unpack: true }] }],
      launch: [{ game: 'gta5', args: [] }],
      ...sigfMod('gta5-rain-of-cows', 'gta5'),
    },
  },

  // A two-game passthrough: files into the game folder (snapshot) plus a Prism instance; Minecraft starts first.
  'gta5-minecraft': {
    assets: {
      'gta5-blocky.mrpack': mrpack('Blocky Los Santos', '1.0.0', '26.3', [['overrides/mods/sigf-crossover-minecraft.jar', jar('sigf-crossover-minecraft')]]),
      'gta5-blocky-gta5.zip': zip([
        ['MCPassthrough.asi', 'asi'],
        ['reshade-shaders/Shaders/MCPassthrough.fx', 'fx'],
      ]),
    },
    recipe: {
      id: 'sigf/gta5-blocky',
      version: '1.0.0',
      name: 'Blocky Los Santos',
      tagline: 'Test fixture.',
      kind: 'passthrough',
      games: [
        { game: 'gta5', role: 'host', engine: null, apps: { steam: '271590' } },
        { game: 'minecraft', role: 'guest', label: 'minecraft', mc: '26.3', loader: 'fabric@0.19.5' },
      ],
      requires: [{ id: 'fabric-loader' }, { id: 'scripthookv' }, { id: 'reshade' }],
      install: [
        { game: 'gta5', strategy: 'game-dir-snapshot', files: [{ src: 'gta5-blocky-gta5.zip', dst: '{game}', unpack: true }] },
        { game: 'minecraft', strategy: 'mrpack', pack: { src: 'gta5-blocky.mrpack' } },
      ],
      launch: [{ game: 'minecraft', wait: 'port:25599' }, { game: 'gta5', args: [] }],
      ...sigfMod('gta5-blocky', 'crossover-gta5-minecraft'),
      server: server('26.3', 'gta5-blocky.mrpack'),
    },
  },

  // An upstream fusion: a script-extender plugin into {game}/Data plus a Prism instance, the game started through its
  // script-extender loader. The plugin's .ini names the app's Prism instance.
  'skse-fusion': {
    assets: {
      'skse-fusion-skyrim.zip': zip([
        ['SKSE/Plugins/Fusion.dll', dll('SKSE/Plugins/Fusion.dll')],
        ['SKSE/Plugins/Fusion.ini', '; Test fixture.\n[Minecraft]\nbStartWithSkyrim = 1\nsLauncher =\nsArguments = --launch sigf-skse-fusion\n'],
        ...fake(['SKSE/Plugins/Fusion/LICENSE.txt', 'SKSE/Plugins/Fusion/SOURCE.txt', 'SKSE/Plugins/Fusion/THIRD-PARTY-NOTICES.md']),
      ]),
      'skse-fusion.mrpack': mrpack('SKSE Fusion', '0.1.0', '26.3', [
        ['overrides/mods/fusion-fabric-0.1.0.jar', jar('fusion')],
        ['overrides/licenses/Fusion-LICENSE.txt', text('Fusion-LICENSE.txt')],
      ]),
    },
    recipe: {
      id: 'sigf/skse-fusion',
      version: '0.1.0',
      name: 'SKSE Fusion',
      tagline: 'Test fixture.',
      kind: 'passthrough',
      games: [
        { game: 'skyrim', role: 'host', engine: 'Skyrim Special Edition + SKSE64 plugin', apps: { steam: '489830' } },
        { game: 'minecraft', role: 'guest', label: 'Minecraft', mc: '26.3', loader: 'fabric@0.19.5', java: '25' },
      ],
      requires: [
        { id: 'skse64', page: 'https://skse.silverlock.org/', note: 'start Skyrim with skse64_loader.exe' },
        { id: 'fabric-loader', version: '0.19.5' },
      ],
      install: [
        { game: 'skyrim', strategy: 'game-dir-snapshot', files: [{ src: 'skse-fusion-skyrim.zip', dst: '{game}/Data', unpack: true }] },
        { game: 'minecraft', strategy: 'mrpack', pack: { src: 'skse-fusion.mrpack' }, jvm_args: ['-Dfusion.startHidden=true'] },
      ],
      launch: [{ game: 'minecraft' }, { game: 'skyrim', args: [], exe: 'skse64_loader.exe' }],
      ...upstream('skse-fusion', { license: 'MIT AND GPL-3.0-or-later', rebuilt: { file: 'Fusion.dll' } }),
      server: { ...server('26.3', 'skse-fusion.mrpack'), load_on_server: ['fusion'] },
    },
  },

  'f4se-fusion': {
    assets: {
      'f4se-fusion-fallout4.zip': zip([
        ['F4SE/Plugins/F4Fusion.dll', dll('F4SE/Plugins/F4Fusion.dll')],
        ...fake(['F4SE/Plugins/F4Fusion/LICENSE.txt', 'F4SE/Plugins/F4Fusion/README.txt', 'F4SE/Plugins/F4Fusion/SOURCE.txt']),
      ]),
      'f4se-fusion.mrpack': mrpack('F4SE Fusion', '0.1.0', '26.3', [
        ['overrides/mods/f4fusion-0.1.0.jar', jar('f4fusion')],
        ['overrides/licenses/F4Fusion-LICENSE.txt', text('F4Fusion-LICENSE.txt')],
      ]),
    },
    recipe: {
      id: 'sigf/f4se-fusion',
      version: '0.1.0',
      name: 'F4SE Fusion',
      tagline: 'Test fixture.',
      kind: 'passthrough',
      games: [
        { game: 'fallout4', role: 'host', engine: 'Fallout 4 + F4SE plugin', apps: { steam: '377160' } },
        { game: 'minecraft', role: 'guest', label: 'Minecraft', mc: '26.3', loader: 'fabric@0.19.5', java: '25' },
      ],
      requires: [
        { id: 'f4se', page: 'https://f4se.silverlock.org/', note: 'start Fallout 4 with f4se_loader.exe' },
        { id: 'fabric-loader', version: '0.19.5' },
      ],
      install: [
        { game: 'fallout4', strategy: 'game-dir-snapshot', files: [{ src: 'f4se-fusion-fallout4.zip', dst: '{game}/Data', unpack: true, contents: false }] },
        { game: 'minecraft', strategy: 'mrpack', pack: { src: 'f4se-fusion.mrpack' } },
      ],
      launch: [{ game: 'minecraft' }, { game: 'fallout4', args: [], exe: 'f4se_loader.exe' }],
      ...upstream('f4se-fusion', { license: 'MIT AND GPL-3.0', rebuilt: { file: 'F4Fusion.dll' } }),
      server: server('26.3', 'f4se-fusion.mrpack'),
    },
  },

  // A BepInEx fusion: the BepInEx zip unpacked into the game folder first, then the plugin, both under the snapshot.
  'bepinex-fusion': {
    assets: {
      [BEPINEX]: bepinexZip(),
      'bepinex-fusion-slimerancher.zip': zip(fake(['LICENSE.txt', 'Fusion.dll', 'THIRD_PARTY_NOTICES.md'])),
    },
    recipe: {
      id: 'sigf/bepinex-fusion',
      version: '1.0.0',
      name: 'BepInEx Fusion',
      tagline: 'Test fixture.',
      kind: 'mashup',
      games: [
        { game: 'slimerancher', role: 'host', label: 'Slime Rancher', engine: 'Unity (Mono) + BepInEx 5 plugin', apps: { steam: '433340' } },
        { game: 'minecraft', role: 'guest', label: 'Minecraft', mc: '26.1.2', uses: 'assets of the player\'s own install, not launched' },
      ],
      requires: [bepinexRequire, { id: 'minecraft-java', version: '26.1.2' }],
      install: [
        {
          game: 'slimerancher',
          strategy: 'game-dir-snapshot',
          loader: 'bepinex',
          files: [
            { src: BEPINEX, dst: '{game}', unpack: true },
            { src: 'bepinex-fusion-slimerancher.zip', dst: '{game}/BepInEx/plugins/Fusion', unpack: true },
          ],
        },
      ],
      launch: [{ game: 'slimerancher', args: [] }],
      ...upstream('bepinex-fusion', { license: 'MIT AND LGPL-2.1', bundled: [{ name: 'BepInEx', version: '5.4.23.5', repo: 'https://github.com/BepInEx/BepInEx', license: 'LGPL-2.1' }] }),
      status: 'beta',
    },
  },

  // A BepInEx bridge in the guest game plus the Minecraft host as a Prism instance; only Minecraft is started.
  'bepinex-mrpack': {
    assets: {
      [BEPINEX]: bepinexZip(),
      'bepinex-mrpack-ultrakill.zip': zip(fake(['LICENSE.txt', 'Bridge.dll'])),
      'bepinex-mrpack.mrpack': mrpack('BepInEx Bridge', '0.1.0', '1.21.11', [
        ['overrides/mods/bridge-fabric-0.1.0.jar', jar('bridge')],
        ['overrides/licenses/Bridge-LICENSE.txt', text('Bridge-LICENSE.txt')],
      ]),
    },
    recipe: {
      id: 'sigf/bepinex-mrpack',
      version: '0.1.0',
      name: 'BepInEx Bridge',
      tagline: 'Test fixture.',
      kind: 'passthrough',
      games: [
        { game: 'minecraft', role: 'host', label: 'Minecraft', mc: '1.21.11', loader: 'fabric@0.19.5', java: '21' },
        { game: 'ultrakill', role: 'guest', label: 'ULTRAKILL', engine: 'Unity (Mono) + BepInEx 5 plugin', apps: { steam: '1229490' } },
      ],
      requires: [bepinexRequire, { id: 'fabric-loader', version: '0.19.5' }],
      install: [
        {
          game: 'ultrakill',
          strategy: 'game-dir-snapshot',
          loader: 'bepinex',
          files: [
            { src: BEPINEX, dst: '{game}', unpack: true },
            { src: 'bepinex-mrpack-ultrakill.zip', dst: '{game}/BepInEx/plugins/Bridge', unpack: true },
          ],
        },
        { game: 'minecraft', strategy: 'mrpack', pack: { src: 'bepinex-mrpack.mrpack' } },
      ],
      launch: [{ game: 'minecraft' }],
      ...upstream('bepinex-mrpack', { license: 'MIT AND LGPL-2.1', bundled: [{ name: 'BepInEx', version: '5.4.23.5', repo: 'https://github.com/BepInEx/BepInEx', license: 'LGPL-2.1' }] }),
      status: 'beta',
    },
  },
};

// ---- fill in hashes, sizes, urls and contents; write everything ----
/** The entries of a stored zip written by zip() above: [path, bytes][] */
function entries(buf) {
  const out = [];
  let p = 0;
  while (buf.readUInt32LE(p) === 0x04034b50) {
    const size = buf.readUInt32LE(p + 18);
    const n = buf.readUInt16LE(p + 26);
    const path = buf.subarray(p + 30, p + 30 + n).toString('utf8');
    out.push([path, buf.subarray(p + 30 + n, p + 30 + n + size)]);
    p += 30 + n + size;
  }
  return out;
}

rmSync(OUT, { recursive: true, force: true });
for (const [name, c] of Object.entries(CASES)) {
  const dir = join(OUT, name);
  mkdirSync(dir, { recursive: true });
  const asset = (file) => {
    const b = c.assets[file];
    if (!b) throw new Error(`${name}: no asset ${file}`);
    return { url: `file:///FIXTURE/${name}/${file}`, sha256: sha256(b), size: b.length };
  };
  const r = structuredClone(c.recipe);
  for (const step of r.install) {
    for (const f of step.files ?? []) {
      const listContents = f.contents !== false;
      delete f.contents;
      if (f.unpack && listContents) f.contents = entries(c.assets[f.src]).map(([path, b]) => ({ path, sha256: sha256(b) }));
      Object.assign(f, asset(f.src));
    }
    if (step.pack) Object.assign(step.pack, asset(step.pack.src));
  }
  for (const req of r.requires) {
    if (req.source?.asset) {
      const a = asset(req.source.asset);
      req.source = { url: a.url, sha256: a.sha256 };
    }
  }
  r.files = Object.keys(c.assets).map((file) => ({ name: file, ...asset(file) }));
  // Keep `files` before the metadata, as the publisher writes it.
  const ordered = {};
  for (const [k, v] of Object.entries(r)) {
    if (k === 'files') continue;
    ordered[k] = v;
    if (k === 'launch') ordered.files = r.files;
  }
  writeFileSync(join(dir, 'mashup.json'), JSON.stringify(ordered, null, 2) + '\n');
  for (const [file, b] of Object.entries(c.assets)) writeFileSync(join(dir, file), b);
}
console.log(`wrote ${Object.keys(CASES).length} fixture cases to ${OUT}`);
