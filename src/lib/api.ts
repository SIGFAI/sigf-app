// Bridge to the Rust core. In a plain browser (vite dev without Tauri) it falls back to a sample PC,
// so the UI can be designed and screenshotted without building the app.

import { convertFileSrc } from '@tauri-apps/api/core';
import { canonOf } from '../data/games';

export type Game = {
  key: string;
  store: 'steam' | 'epic' | 'ubisoft' | 'gog' | 'minecraft';
  storeId: string;
  name: string;
  installDir?: string;
  build?: string;
  sizeBytes?: number;
  launch?: string;
  art?: string;
  artWide?: string;
  /** Steam's cached library art on disk; show through `localSrc`. */
  artLocal?: string;
  heroLocal?: string;
  wideLocal?: string;
  canon?: string | null;
};

export type Launcher = { kind: 'prism' | 'modrinth' | 'official'; exe?: string; dataDir: string; instances: string[] };
export type Scan = { games: Game[]; launchers: Launcher[]; stores: string[]; millis: number };

export const inTauri = typeof window !== 'undefined' && '__TAURI_INTERNALS__' in window;

async function invoke<T>(cmd: string, args?: Record<string, unknown>): Promise<T> {
  const { invoke } = await import('@tauri-apps/api/core');
  return invoke<T>(cmd, args);
}

const steam = (id: string, name: string, build: string, gb: number): Game => ({
  key: `steam:${id}`, store: 'steam', storeId: id, name, build, sizeBytes: gb * 1e9,
  launch: `steam://rungameid/${id}`,
  art: `https://cdn.cloudflare.steamstatic.com/steam/apps/${id}/library_600x900.jpg`,
  artWide: `https://cdn.cloudflare.steamstatic.com/steam/apps/${id}/header.jpg`,
});

const SAMPLE: Scan = {
  games: [
    steam('3240220', 'Grand Theft Auto V Enhanced', '20112468', 105),
    steam('489830', 'The Elder Scrolls V: Skyrim Special Edition', '17998431', 14),
    steam('440', 'Team Fortress 2', '19644912', 31),
    steam('4000', "Garry's Mod", '19720304', 6),
    steam('2280', 'DOOM + DOOM II', '15009911', 2),
    steam('105600', 'Terraria', '14400302', 1),
    steam('1966720', 'Lethal Company', '16031212', 1),
    steam('2379780', 'Balatro', '18100223', 1),
    { key: 'epic:Ginger', store: 'epic', storeId: 'Ginger', name: 'Cyberpunk 2077', build: '2.31', sizeBytes: 70e9, launch: 'com.epicgames.launcher://apps/Ginger?action=launch&silent=true' },
    { key: 'ubisoft:13504', store: 'ubisoft', storeId: '13504', name: "Assassin's Creed Valhalla", launch: 'uplay://launch/13504/0' },
    { key: 'minecraft:java', store: 'minecraft', storeId: 'java', name: 'Minecraft: Java Edition' },
  ],
  launchers: [{ kind: 'prism', exe: 'prismlauncher.exe', dataDir: '%APPDATA%/PrismLauncher', instances: ['1.21.1 Fabric', 'Vanilla 26.3'] }],
  stores: ['steam', 'epic', 'ubisoft', 'minecraft'],
  millis: 41,
};

/** A file from Steam's art cache as a webview URL (asset protocol); nothing outside the app. */
export function localSrc(path?: string | null): string | undefined {
  if (!path || !inTauri) return undefined;
  return convertFileSrc(path);
}

export type SteamArt = { appid: string; art?: string; hero?: string; wide?: string };
const lookups = new Map<string, Promise<SteamArt | null>>();

/** Steam art for a game matched by name, for stores with no public art (Ubisoft, some Epic). Once per name. */
export function steamLookup(name: string): Promise<SteamArt | null> {
  if (!inTauri) return Promise.resolve(null);
  let p = lookups.get(name);
  if (!p) {
    p = invoke<SteamArt | null>('steam_lookup', { name }).catch(() => null);
    lookups.set(name, p);
  }
  return p;
}

export async function scanGames(): Promise<Scan> {
  const s = inTauri ? await invoke<Scan>('scan_games') : await new Promise<Scan>((r) => setTimeout(() => r(SAMPLE), 650));
  return { ...s, games: s.games.map((g) => ({ ...g, canon: canonOf(g) })) };
}

/** Plays an installed mashup: Prism first for Minecraft sides, then the host through its store with the mod's args. */
export async function playInstalled(id: string, games: Game[]) {
  const stores: Record<string, [string, string, string | null]> = {};
  for (const g of games) if (g.canon && !stores[g.canon]) stores[g.canon] = [g.store, g.storeId, g.launch ?? null];
  return invoke<void>('play', { id, stores });
}

export async function launchGame(uri: string) {
  if (inTauri) return invoke<void>('launch', { uri });
  console.info('launch', uri);
}

export async function openUrl(url: string) {
  if (inTauri) {
    const { openUrl } = await import('@tauri-apps/plugin-opener');
    return openUrl(url);
  }
  window.open(url, '_blank');
}

export async function windowAction(a: 'minimize' | 'toggleMaximize' | 'close') {
  if (!inTauri) return;
  const { getCurrentWindow } = await import('@tauri-apps/api/window');
  await getCurrentWindow()[a]();
}

// Agents built on the launchpad: live data from sigf.ai.
export type Agent = {
  ticker: string; name: string; host: string; guest: string; status: string; live: boolean;
  image?: string; crossover?: boolean; model?: { label: string }; createdAt: string;
  /** The mix it is building now (it can move on from its first one). */
  mixNow?: { host: string; guest: string | null; crossover?: boolean } | null;
  /** Its machine: `frame` is its latest picture (a site path), `progress` the build's steps. */
  machine?: { state: string; frame: string | null; progress: { done: number; total: number; current: string | null } | null };
};

/** GET through the Rust core in the app (sigf.ai sends no CORS headers), plain fetch in a browser. */
export async function getText(url: string): Promise<string> {
  if (inTauri) return invoke<string>('fetch_text', { url });
  const r = await fetch(url);
  if (!r.ok) throw new Error(`HTTP ${r.status}`);
  return r.text();
}

export async function fetchAgents(): Promise<Agent[]> {
  try {
    const list = JSON.parse(await getText('https://sigf.ai/api/studio/agents')) as Agent[];
    return list.map((a) => ({ ...a, image: a.image ? `https://sigf.ai${a.image}` : undefined }));
  } catch {
    return [];
  }
}
