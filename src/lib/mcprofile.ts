// Minecraft profiles (docs/GAME-HUB.md section 4, "Minecraft"): Modrinth and CurseForge mods install into a
// SIGF-managed Prism instance per (Minecraft version, loader), "SIGF <version> <Loader>". The player picks the profile
// on the Minecraft page (remembered on this PC; the latest release with Fabric by default); search and plans carry it
// (`mc`, `loader`), the core writes the instance on the first install, and Play starts it through Prism.

import { useSyncExternalStore } from 'react';
import { inTauri } from './api';

export type McLoader = 'fabric' | 'neoforge' | 'forge' | 'quilt';
export const MC_LOADERS: McLoader[] = ['fabric', 'neoforge', 'forge', 'quilt'];
/** Product names: the same in every language. */
export const LOADER_NAME: Record<McLoader, string> = { fabric: 'Fabric', neoforge: 'NeoForge', forge: 'Forge', quilt: 'Quilt' };

export type McProfile = { mc: string; loader: McLoader };
export type McVersions = { latest: string; versions: { id: string; loaders: McLoader[] }[] };
/** The profile on this PC: Prism found, its program found (Play), the instance written (after a first install). */
export type McProfileState = { prism: boolean; launcher: boolean; exists: boolean; name: string };

const KEY = 'mc-profile';
const isLoader = (v: unknown): v is McLoader => typeof v === 'string' && (MC_LOADERS as string[]).includes(v);
const VERSION_RE = /^[0-9][0-9A-Za-z._+-]{0,31}$/;

function load(): McProfile | null {
  try {
    const v = JSON.parse(localStorage.getItem(KEY) ?? 'null') as Partial<McProfile> | null;
    return v && typeof v.mc === 'string' && VERSION_RE.test(v.mc) && isLoader(v.loader) ? { mc: v.mc, loader: v.loader } : null;
  } catch {
    return null;
  }
}

let picked: McProfile | null = load();
let versions: McVersions | null = null;
let versionsP: Promise<McVersions | null> | null = null;
let rev = 0;
const subs = new Set<() => void>();
const emit = () => {
  rev++;
  subs.forEach((f) => f());
};
const subscribe = (f: () => void) => {
  subs.add(f);
  return () => void subs.delete(f);
};

/** The browser preview has no sigf.ai proxy: a few versions to click through. */
const SAMPLE: McVersions = {
  latest: '26.3',
  versions: [
    { id: '26.3', loaders: ['fabric', 'neoforge', 'quilt'] },
    { id: '1.21.1', loaders: ['fabric', 'neoforge', 'forge', 'quilt'] },
    { id: '1.20.1', loaders: ['fabric', 'forge', 'quilt'] },
  ],
};

async function fetchVersions(): Promise<McVersions> {
  if (!inTauri) return SAMPLE;
  const { invoke } = await import('@tauri-apps/api/core');
  const r = await invoke<{ status: number; body: string }>('lobby_api', { method: 'GET', path: '/api/app/mods/mc-versions', body: null, secret: null });
  if (r.status >= 400) throw new Error(`HTTP ${r.status}`);
  const v = JSON.parse(r.body) as McVersions;
  return {
    latest: String(v.latest ?? ''),
    versions: (Array.isArray(v.versions) ? v.versions : [])
      .filter((x) => typeof x?.id === 'string' && VERSION_RE.test(x.id))
      .map((x) => ({ id: x.id, loaders: (Array.isArray(x.loaders) ? x.loaders : []).filter(isLoader) })),
  };
}

/** The versions the picker offers (read once per session); null when sigf.ai did not answer. */
function readVersions(): Promise<McVersions | null> {
  versionsP ??= fetchVersions().then(
    (v) => ((versions = v), emit(), v),
    () => {
      versionsP = null;
      return null;
    },
  );
  return versionsP;
}

/** The picked profile, else the latest release with Fabric (or its first loader); null until either is known. */
export function currentMcProfile(): McProfile | null {
  if (picked) return picked;
  const v = versions?.versions.find((x) => x.id === versions?.latest) ?? versions?.versions[0];
  return v ? { mc: v.id, loader: v.loaders.includes('fabric') ? 'fabric' : v.loaders[0] ?? 'fabric' } : null;
}

/** `1.21.1-fabric`: what installed mods are matched on (see `profileOfDir`). */
export const profileKey = (p: McProfile | null) => (p ? `${p.mc}-${p.loader}` : '');
export const profileName = (p: McProfile) => `SIGF ${p.mc} ${LOADER_NAME[p.loader]}`;

/** The profile an installed mod went into, from its game folder (`.../instances/sigf-<mc>-<loader>/.minecraft`). */
export function profileOfDir(dir: string | null | undefined): string | undefined {
  const m = /[\\/]instances[\\/]sigf-([0-9][0-9A-Za-z._+-]*)-(fabric|neoforge|forge|quilt)(?:[\\/]|$)/.exec(dir ?? '');
  return m ? `${m[1]}-${m[2]}` : undefined;
}

/** The `mc` and `loader` parameters of a Minecraft search or plan (nothing for another game, or before a profile is
 *  known: sigf.ai then answers links, as for apps without profiles). */
export async function mcParams(game: string): Promise<Record<string, string>> {
  if (game !== 'minecraft') return {};
  if (!picked && !versions) await readVersions();
  const p = currentMcProfile();
  // Without a profile sigf.ai answers links: an Install click must never turn into opening a web page.
  if (!p) throw new Error('Minecraft versions could not be read from sigf.ai');
  return { mc: p.mc, loader: p.loader };
}

/** Picks a profile (kept on this PC). */
export function setMcProfile(p: McProfile) {
  picked = p;
  try {
    localStorage.setItem(KEY, JSON.stringify(p));
  } catch {}
  emit();
}

/** The profile, the versions on offer, and a key that changes with the profile (for effects). */
export function useMcProfile(): { profile: McProfile | null; versions: McVersions | null; key: string } {
  void readVersions();
  useSyncExternalStore(subscribe, () => rev);
  const profile = currentMcProfile();
  return { profile, versions, key: profileKey(profile) };
}

/** A key that changes when the Minecraft profile does ('' for other games). */
export function useMcProfileKey(game: string): string {
  useSyncExternalStore(subscribe, () => rev);
  if (game !== 'minecraft') return '';
  void readVersions();
  return profileKey(currentMcProfile());
}

export async function mcProfileState(p: McProfile): Promise<McProfileState> {
  if (!inTauri) return { prism: true, launcher: true, exists: p.loader === 'fabric', name: profileName(p) };
  const { invoke } = await import('@tauri-apps/api/core');
  return invoke<McProfileState>('mc_profile_state', { mc: p.mc, loader: p.loader });
}

/** Starts the profile's instance through Prism. Rejects with the core's `{ code, message }` (`needs_launcher`,
 *  `no_profile`). */
export async function playMcProfile(p: McProfile): Promise<void> {
  if (!inTauri) return console.info('play', profileName(p));
  const { invoke } = await import('@tauri-apps/api/core');
  await invoke<void>('mc_profile_play', { mc: p.mc, loader: p.loader });
}
