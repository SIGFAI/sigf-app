// Play on Mac (app/docs/MAC-PLAY.md): Windows games run from their game page on macOS. Recipes come from sigf.ai
// (`GET /api/app/mac/games`); the core (src-tauri/src/macplay) downloads the engine, prepares a Windows folder (a Wine
// prefix with Windows Steam inside), opens that Steam so the player signs in and installs the game, then plays and stops
// it. Like lib/mods.ts, every call goes through one backend: the core in the app, or a simulation in a plain browser
// (`?mac=1` shows the macOS app, `?macstate=fresh|engine|steam|installed|running` picks where each game starts,
// `?macfail=download|sha|steam|unsupported` makes the next setup or launch fail that way).

import { useEffect, useSyncExternalStore } from 'react';
import { appPlatform, inTauri } from './api';
import { GAME } from '../data/games';

// ---------- Contract types (MAC-PLAY.md sections 1 and 2) ----------

export type MacEngine = { id: string; url: string; sha256: string; size: number; kind: 'wine' | 'renderer' | 'native'; name?: string; version?: string };
export type MacRoute = 'wine' | 'native-port' | 'native';
export type MacTest = { by: string; on: string; result: 'works' | 'menu' | 'broken' };

export type MacGame = {
  game: string;
  name: string;
  route: MacRoute;
  wine?: {
    engine: string;
    renderer: string;
    store: 'steam-win';
    appid: string;
    exe: string;
    env: Record<string, string>;
    dllOverrides: string;
    args: string[];
    verbs?: string[];
    online: 'ok' | 'offline-only' | 'none';
  };
  port?: {
    engine: string;
    data: { from: 'steam-win' | 'gog-installer' | 'folder'; appid?: string; files: string[] };
    launch: string[];
  };
  native?: { steamAppId: string };
  minMacos: string;
  notes: string[];
  tested: MacTest[];
  /** The engines this recipe names, when the site sends them with it (or as `{ games, engines }`). */
  engines?: MacEngine[];
};

export type MacStatus = {
  route: MacRoute;
  engine: 'missing' | 'ready';
  prefix: 'missing' | 'ready';
  store: 'missing' | 'installed' | 'running';
  game: 'missing' | 'installed';
  data?: 'missing' | 'ready';
  running: boolean;
  disk: { engineBytes: number; prefixBytes: number };
};

export type MacProgress = { game: string; step: string; done: number; total: number };
export type RemoveWhat = 'prefix' | 'engine' | 'all';

/** What went wrong, typed for the UI (the core answers `{ code, message }`). */
export type MacErrorKind = 'unsupported' | 'download' | 'sha' | 'steam_missing' | 'not_installed' | 'data_missing' | 'macos_old' | 'in_use' | 'network' | 'other';
export class MacError extends Error {
  constructor(public kind: MacErrorKind, message: string, public code: string = kind) {
    super(message);
  }
}

function macError(e: unknown): MacError {
  if (e instanceof MacError) return e;
  const o = (e && typeof e === 'object' ? e : {}) as { code?: unknown; message?: unknown };
  const code = typeof o.code === 'string' ? o.code : '';
  const message = typeof o.message === 'string' ? o.message : e instanceof Error ? e.message : String(e ?? '');
  const c = code.toLowerCase();
  // The core's codes (src-tauri/src/macplay/mod.rs): unsupported, badRecipe, download, sha, io, unpack, wine, timeout,
  // notReady, notSetUp, dataMissing, macosTooOld, inUse, launch. `unsupported` also answers recipe features the core
  // does not handle yet: only "macOS only" means this system.
  const kind: MacErrorKind =
    c === 'unsupported' ? (/macos only/i.test(message) ? 'unsupported' : 'other')
    : c === 'datamissing' ? 'data_missing'
    : c === 'macostooold' ? 'macos_old'
    : c === 'inuse' ? 'in_use'
    : /sha|hash|checksum|integrity/.test(c) ? 'sha'
    : /steam/.test(c) && /missing|not_installed|not_found|no_/.test(c) ? 'steam_missing'
    : /not_installed|game_missing|no_game/.test(c) ? 'not_installed'
    : /download|http|fetch/.test(c) ? 'download'
    : /network|timeout|offline/.test(c) ? 'network'
    : 'other';
  return new MacError(kind, message || code || 'failed', code || kind);
}

// ---------- Backends ----------

type Backend = {
  games: () => Promise<MacGame[]>;
  status: (g: MacGame) => Promise<MacStatus>;
  setup: (g: MacGame) => Promise<void>;
  store: (game: string) => Promise<void>;
  play: (g: MacGame) => Promise<void>;
  pickData: (game: string) => Promise<void>;
  stop: (game: string) => Promise<void>;
  remove: (game: string, what: RemoveWhat) => Promise<void>;
};

const tauriInvoke = async <T,>(cmd: string, args?: Record<string, unknown>): Promise<T> => (await import('@tauri-apps/api/core')).invoke<T>(cmd, args);
const call = async <T,>(cmd: string, args?: Record<string, unknown>): Promise<T> => {
  try {
    return await tauriInvoke<T>(cmd, args);
  } catch (e) {
    throw macError(e);
  }
};

/** The site's answer: `MacGame[]`, or `{ games, engines }`; each game gets the engines it names. */
function normalize(body: unknown): MacGame[] {
  const o = body as { games?: MacGame[]; engines?: MacEngine[] } | MacGame[];
  const games = Array.isArray(o) ? o : o?.games ?? [];
  const pool = Array.isArray(o) ? [] : o?.engines ?? [];
  return games
    .filter((g) => g && typeof g.game === 'string' && typeof g.name === 'string')
    .map((g) => {
      const want = engineIds(g);
      const engines = g.engines?.length ? g.engines : pool.filter((e) => want.includes(e.id));
      return { ...g, notes: g.notes ?? [], tested: g.tested ?? [], engines };
    });
}

const engineIds = (g: MacGame) => [g.wine?.engine, g.wine?.renderer, g.port?.engine].filter((x): x is string => !!x);

const tauri: Backend = {
  games: async () => {
    let r: { status: number; body: string };
    try {
      r = await tauriInvoke<{ status: number; body: string }>('lobby_api', { method: 'GET', path: '/api/app/mac/games', body: null, secret: null });
    } catch (e) {
      throw new MacError('network', e instanceof Error ? e.message : String(e));
    }
    if (r.status >= 400) throw new MacError('network', `HTTP ${r.status}`);
    return normalize(JSON.parse(r.body));
  },
  status: (g) => call<MacStatus>('mac_status', { game: g.game, gameJson: JSON.stringify(g) }),
  setup: async (g) => {
    listenProgress();
    await call<void>('mac_setup', { gameJson: JSON.stringify(g) });
  },
  store: (game) => call<void>('mac_store', { game }),
  play: (g) => call<void>('mac_play', { game: g.game, gameJson: JSON.stringify(g), direct: null }),
  pickData: async (game) => {
    await call<unknown>('mac_pick_data', { game });
  },
  stop: (game) => call<void>('mac_stop', { game }),
  remove: (game, what) => call<void>('mac_remove', { game, what }),
};

const backend: Promise<Backend> = inTauri ? Promise.resolve(tauri) : Promise.resolve(simBackend());

let progressOn: Promise<unknown> | null = null;
function listenProgress() {
  progressOn ??= import('@tauri-apps/api/event').then(({ listen }) => listen<MacProgress>('macplay://progress', (e) => onProgress(e.payload)));
}

// ---------- Store ----------

/** What the app is doing for a game right now. `waitInstall`: Windows Steam was opened for the player to install it. */
export type MacRun = { kind: 'setup' | 'store' | 'play' | 'stop' | 'remove'; step?: string; done?: number; total?: number };
export type MacState = { status: MacStatus | null; run: MacRun | null; error: MacError | null; storeOpened: boolean };

const blank: MacState = { status: null, run: null, error: null, storeOpened: false };
const states = new Map<string, MacState>();
const subs = new Set<() => void>();
let version = 0;
let queued = false;
function emit() {
  if (queued) return;
  queued = true;
  queueMicrotask(() => {
    queued = false;
    version++;
    subs.forEach((f) => f());
  });
}
const subscribe = (f: () => void) => {
  subs.add(f);
  return () => void subs.delete(f);
};
function patch(game: string, p: Partial<MacState>) {
  states.set(game, { ...(states.get(game) ?? blank), ...p });
  emit();
}
export const macState = (game: string): MacState => states.get(game) ?? blank;

function onProgress(p: MacProgress) {
  const s = states.get(p.game);
  if (s?.run?.kind !== 'setup') return;
  patch(p.game, { run: { kind: 'setup', step: p.step, done: p.done, total: p.total } });
}

// ---------- Recipes ----------

let gamesList: MacGame[] | null = null;
let gamesP: Promise<MacGame[]> | null = null;
let gamesError: MacError | null = null;
let platform: string | null = null;
void appPlatform().then((p) => {
  platform = p;
  emit();
});

/** Whether the app runs on macOS (null until the core says). */
export function useIsMac(): boolean | null {
  return useSyncExternalStore(subscribe, () => (platform === null ? null : platform === 'macos'));
}

/** Every Mac recipe (read once, on macOS only). Null while loading; [] off macOS or when the site did not answer. */
export function useMacGames(): MacGame[] | null {
  const mac = useIsMac();
  useEffect(() => {
    if (!mac || gamesP) return;
    gamesP = backend.then((b) => b.games()).then(
      (l) => {
        gamesList = l;
        gamesError = null;
        emit();
        return l;
      },
      (e) => {
        gamesP = null;
        gamesList = [];
        gamesError = macError(e);
        emit();
        return [];
      },
    );
  }, [mac]);
  return useSyncExternalStore(subscribe, () => (mac === false ? EMPTY : gamesList));
}
const EMPTY: MacGame[] = [];
export const macGamesError = () => gamesError;

/** The recipe for a game key, once the recipes are read. */
export function useMacGame(key: string | null | undefined): MacGame | undefined {
  const list = useMacGames();
  return key ? list?.find((g) => g.game === key) : undefined;
}
export const macGameOf = (key: string | null | undefined) => (key ? gamesList?.find((g) => g.game === key) : undefined);

/** The Steam app id that shows a recipe's art and identifies its Windows (or Mac) build. */
export const macAppid = (g: MacGame) => g.wine?.appid ?? g.port?.data.appid ?? g.native?.steamAppId ?? GAME[g.game]?.steam?.[0] ?? '';

/** The engines a recipe downloads, and their total size (0 when the site sent no sizes). */
export function macEngines(g: MacGame): { list: MacEngine[]; bytes: number } {
  const list = engineIds(g).map((id) => g.engines?.find((e) => e.id === id)).filter((e): e is MacEngine => !!e);
  return { list, bytes: list.reduce((s, e) => s + (e.size || 0), 0) };
}

// ---------- Status ----------

const inflight = new Map<string, Promise<void>>();
export function refreshStatus(game: string): Promise<void> {
  let p = inflight.get(game);
  if (p) return p;
  const g = macGameOf(game);
  if (!g) return Promise.resolve();
  p = backend
    .then((b) => b.status(g))
    .then(
      (status) => {
        // Waiting for the player's install ends once the game is there (or the Windows folder is gone).
        const s = macState(game);
        patch(game, { status, storeOpened: s.storeOpened && status.game !== 'installed' && status.data !== 'ready' && (status.route === 'native-port' || status.store !== 'missing') });
      },
      (e) => patch(game, { error: macError(e) }),
    )
    .finally(() => inflight.delete(game));
  inflight.set(game, p);
  return p;
}

/**
 * A game's Mac state, re-rendering on change. Polls the core every `every` ms while asked (`every` > 0), and every 3 s
 * whatever `every` says while the game runs or Windows Steam is open for its install (so the steps tick by themselves).
 */
export function useMacState(game: string | null | undefined, every = 0): MacState {
  const s = useSyncExternalStore(subscribe, () => (game ? macState(game) : blank));
  const busy = !!s.status?.running || s.storeOpened || s.status?.store === 'running';
  const ms = busy ? 3000 : every;
  useEffect(() => {
    if (!game) return;
    if (!macState(game).status) void refreshStatus(game);
    if (!ms) return;
    const id = setInterval(() => void refreshStatus(game), ms);
    return () => clearInterval(id);
  }, [game, ms]);
  return s;
}

/** Re-renders on any Mac play change. */
export const useMacVersion = () => useSyncExternalStore(subscribe, () => version);

// ---------- Actions ----------

async function act(game: string, run: MacRun, f: (b: Backend) => Promise<void>, after?: Partial<MacState>): Promise<boolean> {
  patch(game, { run, error: null });
  try {
    await f(await backend);
    patch(game, { run: null, ...after });
    await refreshStatus(game);
    return true;
  } catch (e) {
    patch(game, { run: null, error: macError(e) });
    void refreshStatus(game);
    return false;
  }
}

/** Downloads the engine and prepares the Windows folder (or copies the game data, once the game is installed). */
export const setupMac = (g: MacGame) => act(g.game, { kind: 'setup' }, (b) => b.setup(g));
/** Opens Windows Steam in the game's folder: the player signs in and installs the game; the status polls pick it up. */
export const openMacStore = (game: string) => act(game, { kind: 'store' }, (b) => b.store(game), { storeOpened: true });
export const playMac = (game: string) => act(game, { kind: 'play' }, (b) => b.play(macGameOf(game)!));
/** A port whose data comes from a folder: the core asks for it in a native dialog and copies what the engine reads. */
export const pickMacData = (game: string) => act(game, { kind: 'setup' }, (b) => b.pickData(game));
export const stopMac = (game: string) => act(game, { kind: 'stop' }, (b) => b.stop(game));
export const removeMac = (game: string, what: RemoveWhat) => act(game, { kind: 'remove' }, (b) => b.remove(game, what), { storeOpened: false });
/** The player was sent to a store to install the game (Steam for Mac): poll until it shows up. */
export const waitForInstall = (game: string) => patch(game, { storeOpened: true, error: null });
export const clearMacError = (game: string) => macState(game).error && patch(game, { error: null });

// ---------- Where a game stands ----------

/** The guided setup's steps for a route; `play` is always last. */
export type MacStepId = 'engine' | 'windows' | 'steam' | 'data' | 'install' | 'play';
export function macSteps(g: MacGame): MacStepId[] {
  if (g.route === 'native') return ['install', 'play'];
  if (g.route === 'native-port') return g.port?.data.from === 'steam-win' ? ['engine', 'steam', 'data', 'play'] : ['engine', 'data', 'play'];
  return ['engine', 'windows', 'steam', 'play'];
}

/** Whether a step is done for this status. */
export function stepDone(id: MacStepId, s: MacStatus | null, g: MacGame): boolean {
  if (!s) return false;
  switch (id) {
    case 'engine': return s.engine === 'ready';
    case 'windows': return s.prefix === 'ready' && s.store !== 'missing';
    case 'steam': return s.game === 'installed' || (g.route === 'native-port' && s.data === 'ready');
    case 'data': return s.data === 'ready';
    case 'install': return s.game === 'installed';
    case 'play': return false;
  }
}

/** The first step not done yet (the one the sheet opens on). */
export function currentStep(g: MacGame, s: MacStatus | null): MacStepId {
  return macSteps(g).find((id) => !stepDone(id, s, g)) ?? 'play';
}

/** Ready to play: every step before Play is done. */
export const macReady = (g: MacGame, s: MacStatus | null) => !!s && currentStep(g, s) === 'play';

/** Which setup step a progress event belongs to (the core's step names are free text: matched by keyword). */
export function stepOfProgress(step: string | undefined, g: MacGame): MacStepId {
  const s = (step ?? '').toLowerCase();
  if (/copy|data/.test(s)) return 'data';
  if (g.route === 'wine' && /prefix|wineboot|mono|verb|vcrun|steam|dll|renderer-install|registry/.test(s)) return 'windows';
  return 'engine';
}

// ---------- Browser preview ----------

/** Sample recipes for the five first-batch games (the preview only; the real ones come from sigf.ai). */
export const SAMPLE_MAC_GAMES: MacGame[] = (() => {
  const wine: MacEngine = { id: 'wine-11.18', name: 'Wine', version: '11.18', kind: 'wine', size: 191e6, sha256: '0'.repeat(64), url: 'https://github.com/Gcenx/macOS_Wine_builds/releases/download/11.18/wine-devel-11.18-osx64.tar.xz' };
  const dxmt: MacEngine = { id: 'dxmt-0.72', name: 'DXMT', version: '0.72', kind: 'renderer', size: 9.4e6, sha256: '1'.repeat(64), url: 'https://github.com/3Shain/dxmt/releases/download/v0.72/dxmt-v0.72-builtin.tar.gz' };
  const vcmi: MacEngine = { id: 'vcmi-1.7', name: 'VCMI', version: '1.7.0', kind: 'native', size: 64e6, sha256: '2'.repeat(64), url: 'https://github.com/vcmi/vcmi/releases/download/1.7.0/VCMI-macOS-arm.dmg' };
  const gen: MacEngine = { id: 'generalsx-0.4', name: 'GeneralsX', version: '0.4', kind: 'native', size: 48e6, sha256: '3'.repeat(64), url: 'https://github.com/SIGFAI/generals-mac/releases/download/v0.4/GeneralsX-macOS.zip' };
  const base = { env: {}, dllOverrides: 'd3d11,dxgi=n,b', args: [], store: 'steam-win' as const };
  return [
    {
      game: 'skyrim', name: 'The Elder Scrolls V: Skyrim Special Edition', route: 'wine', minMacos: '26.0',
      wine: { ...base, engine: wine.id, renderer: dxmt.id, appid: '489830', exe: 'SkyrimSE.exe', verbs: ['vcrun2022'], online: 'offline-only' },
      notes: ['Mods from the Mods tab install into the Windows folder, SKSE included.', 'Creation Club downloads need Bethesda.net: play offline here.'],
      tested: [], engines: [wine, dxmt],
    },
    {
      game: 'witcher3', name: 'The Witcher 3: Wild Hunt', route: 'wine', minMacos: '26.0',
      wine: { ...base, engine: wine.id, renderer: dxmt.id, appid: '292030', exe: 'bin/x64/witcher3.exe', args: ['-dx11'], verbs: ['vcrun2022'], online: 'none' },
      notes: ['Runs the DirectX 11 build: the DirectX 12 option in the launcher is not supported.', 'Expect long shader compiles the first time a region loads.'],
      tested: [{ by: 'SIGF', on: 'M3 Pro · macOS 26.1', result: 'menu' }], engines: [wine, dxmt],
    },
    {
      game: 'homm3', name: 'Heroes of Might and Magic 3: Complete', route: 'native-port', minMacos: '26.0',
      port: { engine: vcmi.id, data: { from: 'gog-installer', files: ['Data', 'Maps', 'Mp3'] }, launch: ['VCMI.app/Contents/MacOS/vcmilauncher'] },
      notes: ['VCMI is an open-source engine for Heroes 3: it reads the maps, art and music from your GOG copy.', 'The Steam HD edition does not carry the files VCMI needs: use the GOG Complete edition.'],
      tested: [{ by: 'SIGF', on: 'M2 · macOS 26.0', result: 'works' }], engines: [vcmi],
    },
    {
      game: 'generalszh', name: 'Command & Conquer Generals Zero Hour', route: 'native-port', minMacos: '26.0',
      port: { engine: gen.id, data: { from: 'steam-win', appid: '2732960', files: ['Data', 'ZH_Generals', 'BIG files'] }, launch: ['GeneralsX.app/Contents/MacOS/GeneralsX'] },
      notes: ['A native port of EA\'s GPL source release, with the game files from your copy.'],
      tested: [], engines: [gen],
    },
    {
      game: 'aoe2de', name: 'Age of Empires II: Definitive Edition', route: 'native', minMacos: '26.0',
      native: { steamAppId: '813780' },
      notes: ['The publisher\'s own Mac version: SIGF only starts it through Steam for Mac.'],
      tested: [{ by: 'SIGF', on: 'M1 · macOS 26.0', result: 'works' }],
    },
  ];
})();

/** A simulation of the core on sample recipes: setup, Windows Steam, play and remove move through their states. */
function simBackend(): Backend {
  const q = typeof location === 'undefined' ? new URLSearchParams() : new URLSearchParams(location.search);
  const start = q.get('macstate') ?? 'fresh';
  let fail = q.get('macfail');
  const sizes: Record<string, number> = { skyrim: 14.2e9, witcher3: 49.6e9, homm3: 1.1e9, generalszh: 3.4e9, aoe2de: 0 };
  const sim = new Map<string, MacStatus>();
  const at = (game: string): MacStatus => {
    let s = sim.get(game);
    if (!s) {
      const g = SAMPLE_MAC_GAMES.find((x) => x.game === game);
      const route = g?.route ?? 'wine';
      const port = route === 'native-port';
      const eng = macEngines(g ?? SAMPLE_MAC_GAMES[0]).bytes;
      const level = ['fresh', 'engine', 'steam', 'installed', 'running'].indexOf(start);
      s = {
        route,
        engine: level >= 1 ? 'ready' : 'missing',
        prefix: level >= 2 ? 'ready' : 'missing',
        store: level >= 2 ? 'installed' : 'missing',
        game: level >= 3 ? 'installed' : 'missing',
        ...(port ? { data: level >= 3 ? 'ready' : 'missing' } : {}),
        running: level >= 4,
        disk: { engineBytes: level >= 1 ? eng : 0, prefixBytes: level >= 2 ? 1.4e9 + (level >= 3 ? sizes[game] ?? 0 : 0) : 0 },
      } as MacStatus;
      if (route === 'native') s = { ...s, engine: 'ready', prefix: 'ready', store: 'installed', disk: { engineBytes: 0, prefixBytes: 0 } };
      sim.set(game, s);
    }
    return s;
  };
  const set = (game: string, p: Partial<MacStatus>) => sim.set(game, { ...at(game), ...p });
  const sleep = (ms: number) => new Promise((r) => setTimeout(r, ms));
  const failNow = (where: string[]) => {
    if (!fail || !where.includes(fail)) return;
    const f = fail;
    fail = null;
    const e: Record<string, [string, string]> = {
      download: ['download_failed', 'github.com answered HTTP 503 for wine-devel-11.18-osx64.tar.xz'],
      sha: ['sha_mismatch', 'wine-devel-11.18-osx64.tar.xz: sha256 does not match the recipe'],
      steam: ['steam_not_installed', 'Steam for Mac is not installed'],
      unsupported: ['unsupported', 'Play on Mac runs on macOS only'],
    };
    throw { code: e[f][0], message: e[f][1] };
  };
  return {
    games: () => sleep(350).then(() => normalize(SAMPLE_MAC_GAMES)),
    status: async ({ game }) => {
      await sleep(120);
      if (start === 'unsupported') throw { code: 'unsupported', message: 'Play on Mac runs on macOS only' };
      return at(game);
    },
    setup: async (g) => {
      const s = at(g.game);
      const emitP = (step: string, done: number, total: number) => onProgress({ game: g.game, step, done, total });
      if (g.route === 'native-port' && s.game === 'installed') {
        for (let i = 0; i <= 10; i++) {
          emitP('copy-data', i, 10);
          await sleep(220);
        }
        set(g.game, { data: 'ready' });
        return;
      }
      const total = macEngines(g).bytes || 200e6;
      for (let i = 0; i <= 12; i++) {
        emitP('download-engine', (total * i) / 12, total);
        await sleep(160);
        if (i === 7) failNow(['download', 'sha']);
      }
      set(g.game, { engine: 'ready', disk: { engineBytes: total, prefixBytes: 0 } });
      if (g.route === 'native-port' && g.port?.data.from !== 'steam-win') return;
      const steps = ['wineboot', 'wine-mono', 'vcrun2022', 'renderer-dlls', 'steam-setup'];
      for (let i = 0; i < steps.length; i++) {
        emitP(steps[i], i, steps.length);
        await sleep(450);
      }
      set(g.game, { prefix: 'ready', store: 'installed', disk: { engineBytes: total, prefixBytes: 1.4e9 } });
    },
    store: async (game) => {
      await sleep(500);
      set(game, { store: 'running' });
      // The player signs in and installs: the game appears a few polls later.
      setTimeout(() => set(game, { game: 'installed', disk: { ...at(game).disk, prefixBytes: 1.4e9 + (sizes[game] ?? 0) } }), 7000);
    },
    pickData: async (game) => {
      await sleep(1200);
      set(game, { data: 'ready', game: 'installed' });
    },
    play: async ({ game }) => {
      await sleep(600);
      failNow(['steam']);
      const st = at(game);
      if (st.route === 'native-port' && st.data !== 'ready') {
        setTimeout(() => set(game, { data: 'ready', game: 'installed', disk: { ...at(game).disk, prefixBytes: sizes[game] ?? 0 } }), 7000);
        return;
      }
      set(game, { running: true });
    },
    stop: async (game) => {
      await sleep(700);
      set(game, { running: false, store: at(game).store === 'running' ? 'installed' : at(game).store });
    },
    remove: async (game, what) => {
      await sleep(900);
      const s = at(game);
      if (what === 'engine') set(game, { engine: 'missing', disk: { ...s.disk, engineBytes: 0 } });
      else if (what === 'prefix') set(game, { prefix: 'missing', store: 'missing', game: 'missing', disk: { ...s.disk, prefixBytes: 0 } });
      else set(game, { engine: 'missing', prefix: 'missing', store: 'missing', game: 'missing', ...(s.route === 'native-port' ? { data: 'missing' } : {}), disk: { engineBytes: 0, prefixBytes: 0 } });
    },
  };
}
