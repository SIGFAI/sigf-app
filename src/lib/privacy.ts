// Privacy choices (docs/PRIVACY.md): kept by the core in <SIGF_HOME>/privacy.json, which also enforces them for its
// own requests. Pictures load in the webview, so the UI applies the picture choice itself (`imageOk`). The browser
// preview keeps the choices in localStorage.
import { useSyncExternalStore } from 'react';
import { inTauri } from './api';

export type Privacy = {
  /** The first-start screen (or the installer) was answered: until then nothing but the catalog request goes out. */
  asked: boolean;
  /** Game, mashup and creator pictures from Steam, Epic and Modrinth image servers. */
  storeArt: boolean;
  /** The name of a game with no picture sent to the Steam store search. */
  artSearch: boolean;
  /** The ids of the games you own sent with lobby lists (off: the app filters the full public list itself). */
  lobbyGames: boolean;
  /** `ask`: the host form starts empty with a "Use my LAN address" button; `auto`: it fills in this PC's LAN address. */
  lanAddress: 'ask' | 'auto';
  /** The UI language the player picked (`en`, `zh-CN`, ...); absent: the system's. Not a request, never sent. */
  language?: string | null;
};

export const DEFAULT_PRIVACY: Privacy = { asked: false, storeArt: true, artSearch: true, lobbyGames: true, lanAddress: 'ask' };

/** Picture hosts that are not SIGF's own: shown only with `storeArt` (Steam Workshop previews and the mod sources' icons
 *  included: Thunderstore, CurseForge, Nexus Mods, GameBanana, mod.io, Modrinth). */
const THIRD_PARTY_IMG = /^https:\/\/(cdn\.cloudflare\.steamstatic\.com|shared\.cloudflare\.steamstatic\.com|shared\.akamai\.steamstatic\.com|cdn1\.epicgames\.com|cdn2\.unrealengine\.com|cdn\.modrinth\.com|images\.steamusercontent\.com|steamuserimages-a\.akamaihd\.net|gcdn\.thunderstore\.io|cdn\.thunderstore\.io|ccdn\.thunderstore\.io|media\.forgecdn\.net|staticdelivery\.nexusmods\.com|images\.gamebanana\.com|thumb\.modcdn\.io)\//i;
const SITE_IMG = /^https:\/\/sigf\.ai\//i;

let state: Privacy | null = null;
const subs = new Set<() => void>();
const emit = () => subs.forEach((f) => f());

async function invoke<T>(cmd: string, args?: Record<string, unknown>): Promise<T> {
  const { invoke } = await import('@tauri-apps/api/core');
  return invoke<T>(cmd, args);
}

const KEY = 'privacy';
function browserLoad(): Privacy {
  try {
    return { ...DEFAULT_PRIVACY, ...JSON.parse(localStorage.getItem(KEY) ?? '{}') };
  } catch {
    return DEFAULT_PRIVACY;
  }
}

/** Reads the saved choices once at start. */
export async function loadPrivacy(): Promise<Privacy> {
  let p: Privacy;
  try {
    p = inTauri ? { ...DEFAULT_PRIVACY, ...(await invoke<Privacy>('privacy_get')) } : browserLoad();
  } catch {
    p = DEFAULT_PRIVACY;
  }
  state = p;
  emit();
  return p;
}

/** Saves new choices (the core puts them in force for its own requests at once). */
export async function savePrivacy(p: Privacy): Promise<Privacy> {
  let saved = p;
  if (inTauri) saved = { ...DEFAULT_PRIVACY, ...(await invoke<Privacy>('privacy_set', { choices: p })) };
  else {
    try {
      localStorage.setItem(KEY, JSON.stringify(p));
    } catch {}
  }
  state = saved;
  emit();
  return saved;
}

/** The choices in force, or null while they are being read. */
export function getPrivacy(): Privacy | null {
  return state;
}

export function usePrivacy(): Privacy | null {
  return useSyncExternalStore(
    (f) => {
      subs.add(f);
      return () => subs.delete(f);
    },
    () => state,
  );
}

/**
 * Whether the webview may load this picture: local files and data always; sigf.ai once the choices are answered;
 * Steam, Epic and Modrinth image servers only with `storeArt`; anything else never (the CSP refuses it anyway).
 */
export function imageOk(src: string | null | undefined, p: Privacy | null): boolean {
  if (!src) return false;
  if (!/^https?:\/\//i.test(src)) return true;   // asset:, data:, http://asset.localhost (Steam's art cache on disk)
  if (/^http:\/\/asset\.localhost\//i.test(src)) return true;
  if (SITE_IMG.test(src)) return true;           // sigf.ai serves the catalog itself: its covers and clips need no extra choice
  if (!p?.asked) return false;
  return p.storeArt && THIRD_PARTY_IMG.test(src);
}
