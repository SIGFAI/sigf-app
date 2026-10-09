// Mods from every source for one game (docs/GAME-HUB.md): search and item details through sigf.ai's mods proxy, install
// plans the core checks and installs with the engine's snapshot/restore (`mods_install`, registry id `mod/<ref>`), the
// Nexus Mods account and `nxm://` links, and libraries applied across sources (Steam Workshop items through the Steam
// helper, everything else through engine plans). Like lib/workshop.ts, every call goes through one backend: the core in
// the app, or a simulation on sample data in a plain browser (mods-sample.ts).

import { useEffect, useSyncExternalStore } from 'react';
import { GAMES } from '../data/games';
import { appPlatform, inTauri, openUrl, prismDownload } from './api';
import { currentMcProfile, mcParams, profileKey, profileOfDir, type McProfile } from './mcprofile';
import { isInstallError, type InstallError } from './install';
import {
  browse as wsBrowse, cachedItem as wsCached, getItems as wsGetItems, isSubscribed, readState, saveLibrary, statusOf as wsStatusOf, subscribe as wsSubscribe,
  unsubscribe as wsUnsubscribe, wsIdOf, type Library, type Sort as WsSort, type WorkshopItem,
} from './workshop';

// ---------- Contract types (docs/GAME-HUB.md sections 2 to 4) ----------

export type Source = 'ts' | 'cf' | 'nx' | 'mio' | 'gb' | 'mr' | 'ws';
/** Display order of the source chips. */
export const SOURCES: Source[] = ['ts', 'nx', 'cf', 'mio', 'gb', 'mr', 'ws'];
/** Product names: shown as they are in every language. */
export const SOURCE_NAME: Record<Source, string> = {
  ts: 'Thunderstore', nx: 'Nexus Mods', cf: 'CurseForge', mio: 'mod.io', gb: 'GameBanana', mr: 'Modrinth', ws: 'Steam Workshop',
};
/** Short names for the badge on a card. */
export const SOURCE_SHORT: Record<Source, string> = { ts: 'Thunderstore', nx: 'Nexus', cf: 'CurseForge', mio: 'mod.io', gb: 'GameBanana', mr: 'Modrinth', ws: 'Workshop' };

export type Target = { source?: string; match?: string; dst: string; root?: string; unpack?: boolean };

export type ModGame = {
  game: string;
  name: string;
  steam?: string;
  sources: {
    ts?: { community: string };
    cf?: { gameId: number; classId?: number };
    nx?: { domain: string };
    mio?: { gameId: number };
    gb?: { gameId: number };
    mr?: Record<string, never>;
  };
  targets: Target[];
};

/** One downloadable file of an item (`/item` answers them for sources with several files: Nexus, CurseForge, mod.io). */
export type ModFile = { id: string; name: string; version?: string; sizeBytes?: number; updated?: number; primary?: boolean; category?: string };

export type ModItem = {
  ref: string; source: Source;
  game: string; title: string; summary: string; author?: string;
  icon?: string;
  downloads?: number; likes?: number; updated?: number; created?: number;
  version?: string; sizeBytes?: number; tags: string[];
  url: string;
  nsfw?: boolean;
  installable: boolean;
  why?: string;
  /** `/item` only: plain text, at most 2000 chars. */
  description?: string;
  files?: ModFile[];
};

export type ModSort = 'popular' | 'updated' | 'new';
export type SearchPage = { items: ModItem[]; next: string | null; total?: number };

export type PlanFile = {
  url: string; size?: number;
  hash?: { sha256?: string; sha512?: string; sha1?: string; md5?: string };
  name: string; unpack: boolean; dst: string; root?: string; of: string;
};
export type InstallPlan = {
  ref: string; game: string; name: string; version: string;
  files: PlanFile[];
  deps: { ref: string; name: string; version: string }[];
  link?: { url: string; why: string };
  needs?: 'nxm';
  /** Nexus plans (`needs: 'nxm'`): where the "Mod manager download" click happens, and the file the plan is for. */
  nx?: { domain: string; modId: string | number; fileId?: string | number | null; filesUrl: string; files?: ModFile[] };
  /** Dependencies no source could resolve (the mod may not start without them). */
  missing?: string[];
  /** Minecraft plans: the SIGF Prism instance (lib/mcprofile.ts) the files go to (`dst` starts with `{instance}`). */
  instance?: McProfile & { loaderVersion: string };
};

/** The player's Nexus Mods account as the core holds it (`<SIGF_HOME>/nexus.json`). */
export type NexusStatus = { loggedIn: boolean; name?: string; premium?: boolean };

/** A mod installed by the engine (registry id `mod/<ref>`). */
export type InstalledModRow = {
  ref: string; game: string; name: string; version: string; at: number;
  /** Minecraft: the profile it went into (`1.21.1-fabric`, lib/mcprofile.ts). */
  profile?: string;
};

type ModsErrorCode =
  | 'link_only' | 'no_mc_version' | 'nxm_expired' | 'rate_limited' | 'cancelled' | 'source_unavailable' | 'no_source' | 'not_found' | 'network' | 'not_on_pc' | 'nexus_unavailable' | 'needs_nexus_login' | 'nxm_unknown' | 'install' | 'failed';

/** A refusal from the proxy or the core, typed. `install`: `detail` is the engine's InstallError. */
export class ModsError extends Error {
  constructor(public code: ModsErrorCode, message: string, public detail?: InstallError) {
    super(message);
  }
}

// ---------- Backends ----------

export type ModsBackend = {
  games: () => Promise<ModGame[]>;
  search: (game: string, source: Source, o: { q?: string; sort: ModSort; cursor?: string | null }) => Promise<SearchPage>;
  item: (ref: string) => Promise<ModItem>;
  plan: (ref: string, game: string, file?: string) => Promise<InstallPlan>;
  /** Installs a checked plan; progress comes as `install://progress` with id `mod/<ref>`. */
  install: (plan: InstallPlan, gameDirs: Record<string, string>) => Promise<void>;
  uninstall: (ref: string, force: boolean) => Promise<void>;
  installed: () => Promise<InstalledModRow[]>;
  nexusStatus: () => Promise<NexusStatus>;
  nexusLogin: () => Promise<NexusStatus>;
  nexusLogout: () => Promise<void>;
  /** "Use my Nexus API key": the core checks the player's personal key with Nexus and keeps it (`nexus_key_invalid`). */
  nexusSetKey: (key: string) => Promise<NexusStatus>;
  /** A download URL for a Nexus file: with the `nxm://` link's key and expiry (free accounts), without (Premium). */
  nexusLink: (domain: string, modId: string, fileId: string, key?: string, expires?: string) => Promise<string>;
  nxmHandler: (enable: boolean) => Promise<boolean>;
  /** Whether this system can answer `nxm://` links, whether SIGF does, and whether another mod manager had them. */
  nxmState: () => Promise<NxmState>;
};

export type NxmState = { supported: boolean; enabled: boolean; other: boolean };

const tauriInvoke = async <T,>(cmd: string, args?: Record<string, unknown>): Promise<T> => (await import('@tauri-apps/api/core')).invoke<T>(cmd, args);

/** The core's install codes as the engine's InstallError kinds (their texts are in i18n/errors.ts). */
const INSTALL_KIND: Record<string, string> = {
  download: 'download', hash_mismatch: 'shaMismatch', tampered: 'tampered', snapshot_corrupt: 'snapshotCorrupt', io: 'io', bad_plan: 'recipe', bad_host: 'recipe',
  needs_launcher: 'needsLauncher',
};
/** The core's other codes (mods.rs, nexus.rs) as the UI's. */
const CORE_CODE: Record<string, ModsErrorCode> = {
  nexus_unavailable: 'nexus_unavailable', nexus_sso: 'nexus_unavailable', unsupported: 'nexus_unavailable',
  nexus_login_required: 'needs_nexus_login', nexus_key_invalid: 'needs_nexus_login', needs_nxm: 'needs_nexus_login',
  missing_game_dir: 'not_on_pc', unsupported_archive: 'link_only', link_only: 'link_only', nxm_expired: 'nxm_expired',
  rate_limited: 'rate_limited', not_found: 'not_found', network: 'network', timeout: 'network', cancelled: 'cancelled',
};

/** Core refusals: `{ code, message }` (every mods and Nexus command), an InstallError (`{ kind, message }`), or text. */
function coreError(e: unknown): ModsError {
  if (e instanceof ModsError) return e;
  if (isInstallError(e)) return new ModsError('install', e.message, e);
  if (e && typeof e === 'object' && 'code' in e) {
    const o = e as { code: unknown; message?: unknown; files?: unknown };
    const code = String(o.code);
    const message = String(o.message ?? code);
    const kind = INSTALL_KIND[code];
    if (kind) {
      const files = Array.isArray(o.files) ? o.files.map(String) : [];
      return new ModsError('install', message, { kind, message, files, file: files[0] ?? '', url: '' } as unknown as InstallError);
    }
    return new ModsError(CORE_CODE[code] ?? 'failed', message);
  }
  const text = e instanceof Error ? e.message : String(e);
  return new ModsError(/nexus_unavailable/.test(text) ? 'nexus_unavailable' : 'failed', text);
}

async function invoke<T>(cmd: string, args?: Record<string, unknown>): Promise<T> {
  try {
    return await tauriInvoke<T>(cmd, args);
  } catch (e) {
    throw coreError(e);
  }
}

/** One GET on sigf.ai's mods proxy through the core (`lobby_api`). */
/** The Minecraft profile parameters, as a network error when sigf.ai's version list could not be read. */
async function mcQuery(game: string): Promise<Record<string, string>> {
  try {
    return await mcParams(game);
  } catch (e) {
    throw new ModsError('network', e instanceof Error ? e.message : String(e));
  }
}

async function getJson<T>(path: string): Promise<T> {
  let r: { status: number; body: string };
  try {
    r = await tauriInvoke<{ status: number; body: string }>('lobby_api', { method: 'GET', path: `/api/app/mods${path}`, body: null, secret: null });
  } catch (e) {
    throw new ModsError('network', e instanceof Error ? e.message : String(e));
  }
  let json: ({ error?: unknown; detail?: unknown } & Record<string, unknown>) | undefined;
  try {
    json = JSON.parse(r.body);
  } catch {}
  if (r.status < 400 && json) return json as T;
  const message = String(json?.detail ?? json?.error ?? `HTTP ${r.status}`);
  if (r.status === 503 && json?.error === 'source_unavailable') throw new ModsError('source_unavailable', message);
  // The games map says the game has no such source (a stale map in the app): the chip stays out of the way.
  if (r.status === 400 && json?.error === 'no_source_for_game') throw new ModsError('no_source', message);
  if (r.status === 404) throw new ModsError('not_found', message);   // unknown_game, unknown item
  // 502 mods_upstream (the source itself failed), anything else: a network problem, retried by the player.
  throw new ModsError('network', message);
}

/** `nexus_status` as the core answers it, whatever its exact field names. */
function asNexus(v: unknown): NexusStatus {
  const o = (v ?? {}) as Record<string, unknown>;
  const user = (o.user ?? null) as Record<string, unknown> | null;
  return {
    loggedIn: Boolean(o.connected ?? o.loggedIn ?? o.logged_in ?? user),
    name: (o.name ?? user?.name) as string | undefined,
    premium: Boolean(o.premium ?? o.isPremium ?? user?.isPremium ?? user?.is_premium),
  };
}

const tauri: ModsBackend = {
  games: () => getJson<ModGame[]>('/games'),
  search: async (game, source, o) => {
    // Minecraft: only what runs on the picked profile (version + loader).
    const p = new URLSearchParams({ game, source, sort: o.sort, ...(source === 'mr' || source === 'cf' ? await mcQuery(game) : {}) });
    if (o.q?.trim()) p.set('q', o.q.trim());
    if (o.cursor) p.set('cursor', o.cursor);
    return getJson<SearchPage>(`/search?${p}`);
  },
  item: (ref) => getJson<ModItem>(`/item?${new URLSearchParams({ ref })}`),
  plan: async (ref, game, file) => getJson<InstallPlan>(`/plan?${new URLSearchParams({ ref, game, ...(file ? { file } : {}), ...(await mcQuery(game)) })}`),
  install: async (plan, gameDirs) => {
    listenProgress();
    await invoke<unknown>('mods_install', { planJson: JSON.stringify(plan), gameDirs });
  },
  uninstall: (ref, force) => invoke<void>('restore', { id: `mod/${ref}`, force }),
  installed: async () => {
    const list = await invoke<{ id: string; name: string; version: string; installedAt: number; games: { game: string; gameDir?: string | null }[] }[]>('installed');
    return list.filter((m) => m.id.startsWith('mod/')).map((m) => {
      const row: InstalledModRow = { ref: m.id.slice(4), game: m.games[0]?.game ?? '', name: m.name, version: m.version, at: m.installedAt };
      const profile = row.game === 'minecraft' ? profileOfDir(m.games[0]?.gameDir) : undefined;
      return profile ? { ...row, profile } : row;
    });
  },
  nexusStatus: async () => asNexus(await invoke<unknown>('nexus_status')),
  nexusLogin: async () => asNexus(await invoke<unknown>('nexus_login')),
  nexusLogout: () => invoke<void>('nexus_logout'),
  nexusSetKey: async (key) => asNexus(await invoke<unknown>('nexus_set_key', { key })),
  nexusLink: async (domain, modId, fileId, key, expires) => {
    // The core takes numbers (u64) for the ids and the expiry.
    const n = (v: string | undefined) => (v === undefined || !/^\d{1,15}$/.test(v) ? null : Number(v));
    if (n(modId) === null || n(fileId) === null) throw new ModsError('failed', 'bad Nexus file');
    const r = await invoke<unknown>('nexus_download_links', { domain, modId: n(modId), fileId: n(fileId), key: key ?? null, expires: n(expires) });
    // Nexus answers a list of CDN mirrors ([{ name, short_name, URI }]); the core may pass it on or keep plain URLs.
    const first = Array.isArray(r) ? r[0] : r;
    const o = first as Record<string, unknown> | undefined;
    const url = typeof first === 'string' ? first : (o?.uri ?? o?.URI ?? o?.url);
    if (typeof url !== 'string' || !/^https:\/\//.test(url)) throw new ModsError('failed', 'Nexus gave no download link');
    return url;
  },
  nxmHandler: async (enable) => {
    const r = await invoke<unknown>('nxm_handler', { enable });
    if (typeof r === 'boolean') return r;
    const o = r as Record<string, unknown> | null;
    return typeof o?.enabled === 'boolean' ? o.enabled : enable;
  },
  nxmState: async () => {
    const o = (await invoke<Record<string, unknown>>('nxm_handler_state')) ?? {};
    return { supported: o.supported !== false, enabled: !!o.enabled, other: !!o.other };
  },
};

const backend: Promise<ModsBackend> = inTauri ? Promise.resolve(tauri) : import('./mods-sample').then((m) => m.simModsBackend(onProgress));

type ProgressEvent = { id: string; phase: 'download' | 'verify' | 'build' | 'install' | 'ready'; pct: number };
let progressOn: Promise<unknown> | null = null;
function listenProgress() {
  progressOn ??= import('@tauri-apps/api/event').then(({ listen }) => listen<ProgressEvent>('install://progress', (e) => onProgress(e.payload)));
}

// ---------- Change signal ----------

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
const subscribeStore = (f: () => void) => {
  subs.add(f);
  return () => void subs.delete(f);
};

/** Re-renders on any mods change (items fetched, installs, Nexus account). With `select`, only when its value changes. */
export function useMods<T = number>(select?: () => T): T {
  return useSyncExternalStore(subscribeStore, select ?? (() => version as T));
}

// ---------- Games map ----------

let gamesList: ModGame[] | null = null;
let gamesP: Promise<ModGame[]> | null = null;
/** Every game the proxy knows mod sources for (read once). Null while loading; [] when the proxy did not answer. */
export function useModGames(): ModGame[] | null {
  gamesP ??= backend.then((b) => b.games()).then(
    (l) => ((gamesList = l), emit(), l),
    () => {
      gamesP = null;
      gamesList = [];
      emit();
      return [];
    },
  );
  return useMods(() => gamesList);
}
export const modGames = () => gamesList ?? [];
export const modGameOf = (canon: string | null | undefined) => (canon ? gamesList?.find((g) => g.game === canon) : undefined);

/** The sources a game's Mods tab asks (the Workshop when `workshop`: a Steam game on Windows). */
export function sourcesOf(g: ModGame | undefined, workshop: boolean): Source[] {
  const have = g ? (Object.keys(g.sources) as Source[]) : [];
  return SOURCES.filter((s) => (s === 'ws' ? workshop : have.includes(s)));
}

// ---------- Items ----------

const items = new Map<string, ModItem>();
const full = new Set<string>();
const remember = (list: ModItem[]) => {
  for (const it of list) if (!full.has(it.ref)) items.set(it.ref, it);
  if (list.length) emit();
};
export const cachedMod = (ref: string) => items.get(ref);

/** A Workshop item as a mod card. */
export function fromWorkshop(w: WorkshopItem, game: string): ModItem {
  return {
    ref: `ws:${w.id}`, source: 'ws', game, title: w.title, summary: w.description, author: w.author, icon: w.preview,
    downloads: w.subs, likes: w.favs, updated: w.updated, created: w.created, sizeBytes: w.sizeBytes, tags: w.tags, url: w.url,
    installable: true, description: w.description,
  };
}

const WS_SORT: Record<ModSort, WsSort> = { popular: 'top', updated: 'updated', new: 'new' };

/** One page of one source. The Workshop goes through its own proxy (`appid` needed). */
export async function searchSource(game: string, source: Source, o: { q?: string; sort: ModSort; cursor?: string | null }, appid?: string): Promise<SearchPage> {
  if (source === 'ws') {
    if (!appid) return { items: [], next: null, total: 0 };
    try {
      const p = await wsBrowse(appid, { sort: WS_SORT[o.sort], q: o.q, cursor: o.cursor });
      return { items: p.items.filter((i) => i.kind === 'item').map((i) => fromWorkshop(i, game)), next: p.next, total: p.total };
    } catch (e) {
      const code = (e as { code?: string }).code;
      throw new ModsError(code === 'search_unavailable' ? 'source_unavailable' : 'network', e instanceof Error ? e.message : String(e));
    }
  }
  const page = await (await backend).search(game, source, o);
  remember(page.items);
  return page;
}

/** Full details (description, files) of an item; cached for the session. */
export async function getMod(ref: string): Promise<ModItem> {
  const ws = wsIdOf(ref);
  if (ws) {
    const [w] = await wsGetItems([ws]);
    if (!w) throw new ModsError('not_found', ref);
    return fromWorkshop(w, '');
  }
  if (full.has(ref)) return items.get(ref)!;
  const it = await (await backend).item(ref);
  items.set(ref, it);
  full.add(ref);
  emit();
  return it;
}

const fetching = new Map<string, Promise<unknown>>();
/** These refs as cards (Workshop ids through the Workshop proxy), fetched once; undefined until known. */
export function useRefItems(refs: string[]): (ModItem | undefined)[] {
  const key = refs.join(',');
  useEffect(() => {
    const ws = refs.map(wsIdOf).filter((x): x is string => !!x && !wsCached(x));
    if (ws.length) wsGetItems(ws).then(emit, () => {});
    for (const r of refs) {
      if (wsIdOf(r) || items.has(r) || fetching.has(r)) continue;
      fetching.set(r, getMod(r).catch(() => {}).finally(() => fetching.delete(r)));
    }
  }, [key]);
  useMods();
  return refs.map(refItem);
}

/** What the app knows of a ref now (Workshop cache or mods cache). */
export function refItem(ref: string): ModItem | undefined {
  const ws = wsIdOf(ref);
  if (ws) {
    const w = wsCached(ws);
    return w ? fromWorkshop(w, '') : undefined;
  }
  return items.get(ref);
}

/** The source of a ref. */
export const sourceOfRef = (ref: string): Source => (wsIdOf(ref) ? 'ws' : (ref.split(':')[0] as Source));

/** Library refs keep Workshop items bare (older libraries, share links). */
export const libRef = (ref: string) => wsIdOf(ref) ?? ref;

// ---------- What installs here ----------

/**
 * Whether the app can install an item now, and why not: the proxy's `installable`/`why`, plus what only the app knows
 * (Nexus files need the player's Nexus key: SSO or "Use my Nexus API key"). Re-reads on `useMods()`.
 */
export function canInstall(it: Pick<ModItem, 'source' | 'installable' | 'why'>): { ok: boolean; why?: string } {
  if (!it.installable) return { ok: false, why: it.why };
  if (it.source === 'nx' && !nexus?.loggedIn) return { ok: false, why: 'needs_nexus_login' };
  return { ok: true };
}
/** Nexus installs for free accounts go through the "Mod manager download" click on the site. */
export const viaNexusClick = (it: Pick<ModItem, 'source'>) => it.source === 'nx' && !nexus?.premium;

// ---------- Merging sources ----------

/** A card of the merged list: the same mod found on other sources rides along (`also`). */
export type Hit = ModItem & { also?: ModItem[] };

const norm = (s?: string) => (s ?? '').toLowerCase().normalize('NFKD').replace(/[^a-z0-9]+/g, '');
const dedupKey = (it: ModItem) => `${norm(it.title)}|${norm(it.author)}`;
const sortKey: Record<ModSort, (it: ModItem) => number> = {
  popular: (it) => it.downloads ?? 0,
  updated: (it) => it.updated ?? 0,
  new: (it) => it.created ?? 0,
};

/**
 * Interleaves the sources' pages (each already in the source's own order), so one source never buries another; the same title by the same author on two sources is one card (the first stays, the other
 * rides in `also`). `seen` carries the cards already shown (load more).
 */
export function mergePages(lists: ModItem[][], sort: ModSort, seen: Map<string, Hit> = new Map()): Hit[] {
  const at = lists.map(() => 0);
  const out: Hit[] = [];
  const key = sortKey[sort];
  // Popularity is not comparable across sources (Thunderstore counts every update, GameBanana barely counts), so
  // "popular" interleaves by rank within each source (each source's best first, in proportion to its page); dates are
  // comparable and merge head against head.
  const better = (i: number, j: number) => {
    if (sort !== 'popular') return key(lists[i][at[i]]) > key(lists[j][at[j]]);
    const qi = at[i] / lists[i].length;
    const qj = at[j] / lists[j].length;
    return qi < qj || (qi === qj && key(lists[i][at[i]]) > key(lists[j][at[j]]));
  };
  for (;;) {
    let best = -1;
    for (let i = 0; i < lists.length; i++) {
      if (at[i] >= lists[i].length) continue;
      if (best < 0 || better(i, best)) best = i;
    }
    if (best < 0) break;
    const it = lists[best][at[best]++];
    const k = dedupKey(it);
    const twin = seen.get(k);
    if (twin) {
      if (twin.ref !== it.ref && !twin.also?.some((a) => a.ref === it.ref)) twin.also = [...(twin.also ?? []), it];
      continue;
    }
    const hit: Hit = { ...it };
    seen.set(k, hit);
    out.push(hit);
  }
  return out;
}

// ---------- Installed mods and installs in flight ----------

export type ModLive =
  | { phase: 'plan' }
  | { phase: 'download' | 'verify' | 'install'; pct: number }
  | { phase: 'nxm'; since: number; filesUrl: string }
  | { phase: 'remove' }
  | { phase: 'failed'; error: string };

const installed = new Map<string, InstalledModRow>();
let installedRead = false;
const live = new Map<string, ModLive>();
/** Nexus downloads waiting for the player's "Mod manager download" click: ref -> the game. */
const waiting = new Map<string, { game: string; item?: ModItem }>();

const setLive = (ref: string, l: ModLive | null) => {
  if (l) live.set(ref, l);
  else live.delete(ref);
  emit();
};

let gameDirs: Record<string, string> = {};
/** Game id -> install folder from the scan (App keeps it current): installs started by an `nxm://` link need it. */
export function setGameDirs(d: Record<string, string>) {
  gameDirs = d;
}

/** Reads what the engine has installed (once, then after every install and removal). */
export async function refreshInstalledMods(): Promise<void> {
  try {
    const list = await (await backend).installed();
    installed.clear();
    for (const r of list) installed.set(r.ref, r);
    installedRead = true;
    emit();
  } catch {}
}
let firstRead: Promise<void> | null = null;
/** A Minecraft mod counts on the game page while its profile is the one picked (every other mod always does). */
const inProfile = (r: InstalledModRow) => r.game !== 'minecraft' || !r.profile || r.profile === profileKey(currentMcProfile());

/** Installed mods of one game (null: all; Minecraft: the picked profile's), read once and kept current. */
export function useInstalledMods(game?: string | null): InstalledModRow[] | null {
  firstRead ??= refreshInstalledMods();
  useMods();
  if (!installedRead) return null;
  return [...installed.values()].filter((r) => !game || (r.game === game && inProfile(r)));
}
export const isModInstalled = (ref: string) => installed.has(ref);

export type ModStatus =
  | { kind: 'none' }
  | { kind: 'planning' }
  | { kind: 'installing'; phase: 'download' | 'verify' | 'install'; pct: number }
  | { kind: 'nxm'; filesUrl: string }
  | { kind: 'removing' }
  | { kind: 'installed'; version: string }
  | { kind: 'failed'; error: string };

/** A mod's state on this PC (engine mods; Workshop refs through the Steam helper's state). */
export function modStatus(ref: string, appid?: string): ModStatus {
  const ws = wsIdOf(ref);
  if (ws) {
    if (!appid) return { kind: 'none' };
    const s = wsStatusOf(appid, ws);
    switch (s.kind) {
      case 'subscribing': return { kind: 'planning' };
      case 'unsubscribing': return { kind: 'removing' };
      case 'downloading': return { kind: 'installing', phase: 'download', pct: s.pct ?? 0 };
      case 'subscribed':
      case 'installed': return { kind: 'installed', version: '' };
      case 'failed': return { kind: 'failed', error: s.error ?? '' };
      default: return { kind: 'none' };
    }
  }
  const l = live.get(ref);
  if (l) {
    if (l.phase === 'plan') return { kind: 'planning' };
    if (l.phase === 'nxm') return { kind: 'nxm', filesUrl: l.filesUrl };
    if (l.phase === 'remove') return { kind: 'removing' };
    if (l.phase === 'failed') return { kind: 'failed', error: l.error };
    return { kind: 'installing', phase: l.phase, pct: l.pct };
  }
  // Minecraft: installed into another profile is not installed here (Install moves it).
  const row = [installed.get(ref)].find((r) => r && inProfile(r));
  return row ? { kind: 'installed', version: row.version } : { kind: 'none' };
}

function onProgress(p: ProgressEvent) {
  if (!p.id.startsWith('mod/')) return;
  const ref = p.id.slice(4);
  if (p.phase === 'ready') return;
  setLive(ref, { phase: p.phase === 'build' ? 'install' : p.phase, pct: p.pct });
}

/** Where a Nexus mod's files are listed (the page with the "Mod manager download" buttons). */
const filesPage = (it: { url: string }) => `${it.url.replace(/[?#].*$/, '')}?tab=files`;

/** What one Install did: installed, sent to its source's page (`link`), or waiting for the Nexus click. */
export type InstallOutcome = { kind: 'installed' | 'link' | 'nxm'; url?: string };

/** Throws `not_on_pc` when the game has no scanned folder. */
function dirsFor(game: string): Record<string, string> {
  // Minecraft mods go to a Prism instance, not a game folder; the browser preview has no folders at all.
  if (inTauri && game !== 'minecraft' && !gameDirs[game]) throw new ModsError('not_on_pc', game);
  return gameDirs;
}

/**
 * Install: asks sigf.ai for the plan (dependencies resolved), then the core checks and installs it. A mod the app cannot
 * install opens its page (`link`); a Nexus mod for a free account opens its files page and waits for the `nxm://`
 * click (`nxm`). Workshop refs subscribe through Steam (`appid` needed). Throws a ModsError.
 */
export async function installMod(ref: string, game: string, o: { file?: string; appid?: string } = {}): Promise<InstallOutcome> {
  const ws = wsIdOf(ref);
  if (ws) {
    if (!o.appid) throw new ModsError('not_on_pc', game);
    await wsSubscribe(o.appid, [ws]);
    return { kind: 'installed' };
  }
  setLive(ref, { phase: 'plan' });
  try {
    const b = await backend;
    const plan = await b.plan(ref, game, o.file);
    if (plan.link) {
      setLive(ref, null);
      // Minecraft: a link means no version of the mod for the picked profile. Say so; never open a page on Install.
      if (game === 'minecraft') {
        const p = currentMcProfile();
        throw new ModsError('no_mc_version', p ? `${p.mc} ${p.loader}` : '');
      }
      void openUrl(plan.link.url);
      return { kind: 'link', url: plan.link.url };
    }
    if (plan.needs === 'nxm') {
      const nx = await b.nexusStatus();
      if (!nx.loggedIn) throw new ModsError('needs_nexus_login', ref);
      const [, domain, modId] = ref.split(':');
      if (nx.premium) {
        const fileId = o.file ?? (plan.nx?.fileId != null ? String(plan.nx.fileId) : undefined) ?? (await getMod(ref)).files?.find((f) => f.primary)?.id;
        if (!fileId) throw new ModsError('not_found', ref);
        return await runPlan(withUrl(plan, ref, await b.nexusLink(domain, modId, fileId)), game);
      }
      const it = items.get(ref) ?? { url: `https://www.nexusmods.com/${domain}/mods/${modId}` };
      const url = plan.nx?.filesUrl && /^https:\/\//.test(plan.nx.filesUrl) ? plan.nx.filesUrl : filesPage(it);
      // Without an nxm:// handler (macOS), the click on Nexus cannot reach the app: the files page is all there is.
      const handler = await b.nxmState().catch(() => null);
      if (handler && !handler.supported) {
        setLive(ref, null);
        void openUrl(url);
        return { kind: 'link', url };
      }
      waiting.set(ref, { game });
      setLive(ref, { phase: 'nxm', since: Date.now(), filesUrl: url });
      void openUrl(url);
      return { kind: 'nxm', url };
    }
    return await runPlan(plan, game);
  } catch (e) {
    const err = coreError(e);
    // The core will not install it (an archive it cannot unpack...): the mod's own page, like a link plan.
    if (err.code === 'link_only') {
      setLive(ref, null);
      const url = items.get(ref)?.url;
      if (url) {
        void openUrl(url);
        return { kind: 'link', url };
      }
    }
    // Not a failure of this mod: the panel or the page says what to do.
    // Minecraft without Prism Launcher: its download page, like a mashup that needs it.
    if (err.detail?.kind === 'needsLauncher') void appPlatform().then((p) => openUrl(prismDownload(p)));
    if (err.code === 'needs_nexus_login' || err.code === 'not_on_pc' || err.code === 'cancelled') setLive(ref, null);
    else setLive(ref, { phase: 'failed', error: err.message });
    throw err;
  }
}

/** The plan with the item's own file pointing at `url` (a Nexus download link). */
function withUrl(plan: InstallPlan, ref: string, url: string): InstallPlan {
  let done = false;
  const files = plan.files.map((f) => (!done && f.of === ref && !f.url ? ((done = true), { ...f, url }) : f));
  if (!done && files.length) files[files.length - 1] = { ...files[files.length - 1], url };
  return { ...plan, files };
}

async function runPlan(plan: InstallPlan, game: string): Promise<InstallOutcome> {
  const dirs = dirsFor(game);
  setLive(plan.ref, { phase: 'download', pct: 0 });
  await (await backend).install(plan, dirs);
  await refreshInstalledMods();
  setLive(plan.ref, null);
  return { kind: 'installed' };
}

const plans = new Map<string, Promise<InstallPlan>>();
/** The plan an Install would run (the mod sheet lists what it brings along); asked once per session. */
export function previewPlan(ref: string, game: string): Promise<InstallPlan> {
  const k = `${game}|${ref}|${game === 'minecraft' ? profileKey(currentMcProfile()) : ''}`;
  let p = plans.get(k);
  if (!p) {
    p = backend.then((b) => b.plan(ref, game));
    p.catch(() => plans.delete(k));
    plans.set(k, p);
  }
  return p;
}

/** Stops waiting for a Nexus click. */
export function cancelNxm(ref: string) {
  waiting.delete(ref);
  if (live.get(ref)?.phase === 'nxm') setLive(ref, null);
}

/** Uninstall: the engine restores the files the mod changed. `force` restores over files changed since. */
export async function uninstallMod(ref: string, o: { appid?: string; force?: boolean } = {}): Promise<void> {
  const ws = wsIdOf(ref);
  if (ws) {
    if (o.appid) await wsUnsubscribe(o.appid, [ws]);
    return;
  }
  setLive(ref, { phase: 'remove' });
  try {
    await (await backend).uninstall(ref, !!o.force);
    installed.delete(ref);
    await refreshInstalledMods();
  } catch (e) {
    throw coreError(e);
  } finally {
    setLive(ref, null);
  }
}

/** A failed install's error stays on its card until the next try; this clears it. */
export const clearFailed = (ref: string) => live.get(ref)?.phase === 'failed' && setLive(ref, null);

// ---------- Nexus Mods ----------

let nexus: NexusStatus | null = null;
let nexusErr: ModsError | null = null;
let nexusP: Promise<void> | null = null;
/** Reads the account once. */
function readNexus() {
  nexusP ??= backend
    .then((b) => b.nexusStatus())
    .then(
      (s) => {
        nexus = s;
        nexusErr = null;
        emit();
      },
      (e) => {
        nexusErr = coreError(e);
        nexus = { loggedIn: false };
        emit();
      },
    );
  return nexusP;
}
/** The Nexus account (null while reading) and why it cannot be used (`nexus_unavailable`: login is not open yet). */
export function useNexus(): { status: NexusStatus | null; error: ModsError | null } {
  void readNexus();
  useMods();
  return { status: nexus, error: nexusErr };
}
export async function nexusLogin(): Promise<NexusStatus> {
  try {
    nexus = await (await backend).nexusLogin();
    nexusErr = null;
    emit();
    return nexus;
  } catch (e) {
    const err = coreError(e);
    if (err.code === 'nexus_unavailable') nexusErr = err;
    emit();
    throw err;
  }
}
/** "Use my Nexus API key". A key Nexus refuses throws `needs_nexus_login` (core `nexus_key_invalid`). */
export async function nexusSetKey(key: string): Promise<NexusStatus> {
  try {
    nexus = await (await backend).nexusSetKey(key);
    emit();
    return nexus;
  } catch (e) {
    throw coreError(e);
  }
}
export async function nexusLogout(): Promise<void> {
  await (await backend).nexusLogout().catch((e) => { throw coreError(e); });
  nexus = { ...(nexus ?? {}), loggedIn: false, name: undefined, premium: false };
  emit();
}

let nxm: NxmState | null = null;
let nxmP: Promise<void> | null = null;
/** Who answers Nexus "Mod manager download" links on this PC (null while reading). */
export function useNxmState(): NxmState | null {
  nxmP ??= backend.then((b) => b.nxmState()).then(
    (s) => ((nxm = s), emit()),
    () => ((nxm = { supported: false, enabled: false, other: false }), emit()),
  );
  useMods();
  return nxm;
}
export async function setNxmHandler(enable: boolean): Promise<boolean> {
  const on = await (await backend).nxmHandler(enable).catch((e) => { throw coreError(e); });
  nxm = { ...(nxm ?? { supported: true, other: false }), enabled: on };
  emit();
  return on;
}

type Nxm = { domain: string; modId: string; fileId: string; key?: string; expires?: string };
/** `nxm://<domain>/mods/<mod>/files/<file>?key=&expires=&user_id=`, or null. */
export function parseNxm(text: string): Nxm | null {
  const m = /^nxm:\/\/([a-z0-9]{1,60})\/mods\/(\d{1,12})\/files\/(\d{1,14})\/?(?:\?(.*))?$/i.exec(text.trim());
  if (!m) return null;
  const q = new URLSearchParams(m[4] ?? '');
  return { domain: m[1].toLowerCase(), modId: m[2], fileId: m[3], key: q.get('key') ?? undefined, expires: q.get('expires') ?? undefined };
}

/**
 * A "Mod manager download" click on Nexus reached the app: the plan for that file, the download link with the click's
 * key, then the install. Works for a mod the app was waiting for and for one clicked straight on the site. Resolves
 * with the mod's name; throws a ModsError.
 */
export async function handleNxm(link: string): Promise<{ name: string; game: string }> {
  const n = parseNxm(link);
  if (!n) throw new ModsError('nxm_unknown', link);
  const ref = `nx:${n.domain}:${n.modId}`;
  if (!gamesList) await backend.then((b) => b.games()).then((l) => ((gamesList = l), emit())).catch(() => {});
  const game = waiting.get(ref)?.game ?? gamesList?.find((g) => g.sources.nx?.domain.toLowerCase() === n.domain)?.game;
  if (!game) throw new ModsError('nxm_unknown', n.domain);
  waiting.delete(ref);
  setLive(ref, { phase: 'plan' });
  try {
    const b = await backend;
    const plan = await b.plan(ref, game, n.fileId);
    if (plan.link) throw new ModsError('failed', plan.link.why);
    const url = await b.nexusLink(n.domain, n.modId, n.fileId, n.key, n.expires);
    await runPlan(withUrl(plan, ref, url), game);
    return { name: plan.name, game };
  } catch (e) {
    const err = coreError(e);
    setLive(ref, err.code === 'not_on_pc' ? null : { phase: 'failed', error: err.message });
    throw err;
  }
}

// ---------- Libraries across sources (docs/GAME-HUB.md section 6) ----------

/** The canonical game of a Steam app id, else `steam:<appid>`. */
export function gameKeyOfAppid(appid: string, scanCanon?: string | null): string {
  return scanCanon ?? GAMES.find((g) => g.steam?.includes(appid))?.id ?? `steam:${appid}`;
}
/** A library's game key: its `game`, else from its Steam app id. */
export const libGame = (lib: Pick<Library, 'appid' | 'game'>) => lib.game ?? (lib.appid ? gameKeyOfAppid(lib.appid) : '');

/** Whether a library item is on this PC. */
export function refHave(ref: string, appid: string): boolean {
  const ws = wsIdOf(ref);
  return ws ? !!appid && isSubscribed(appid, ws) : installed.has(ref);
}

/**
 * Apply: brings in every item not on the PC yet. Workshop items subscribe through Steam (Windows only, `workshop`);
 * the others install through their plans, one after the other. What it brings in is recorded in `addedByUs` (saved
 * before anything starts) so Remove takes back only that. Nexus items for free accounts wait for their click; items
 * that fail are skipped and counted. Resolves with how many were brought in, waiting and failed.
 */
export async function applyLibrary(lib: Library, workshop: boolean): Promise<{ added: number; waiting: number; failed: number }> {
  const game = libGame(lib);
  const ws = lib.items.map(wsIdOf).filter((x): x is string => !!x);
  if (ws.length && lib.appid && workshop) await readState(lib.appid, ws).catch(() => {});
  if (!installedRead) await refreshInstalledMods();
  const missing = lib.items.filter((r) => !refHave(r, lib.appid) && (!wsIdOf(r) || (workshop && lib.appid)));
  const addedByUs = [...new Set([...lib.addedByUs.filter((r) => lib.items.includes(r)), ...missing])];
  const cur = { ...lib, addedByUs, applied: false };
  await saveLibrary(cur);
  const wsMissing = missing.map(wsIdOf).filter((x): x is string => !!x);
  let failed = 0;
  let waits = 0;
  if (wsMissing.length) await wsSubscribe(lib.appid, wsMissing);
  for (const ref of missing.filter((r) => !wsIdOf(r))) {
    try {
      const r = await installMod(ref, game, { appid: lib.appid });
      if (r.kind === 'nxm' || r.kind === 'link') waits++;
    } catch (e) {
      if (e instanceof ModsError && e.code === 'not_on_pc') throw e;
      failed++;
    }
  }
  await saveLibrary({ ...cur, applied: failed === 0 });
  return { added: missing.length - failed - waits, waiting: waits, failed };
}

/**
 * Remove: takes back what this library added. An item another applied library of the same game also holds stays, and
 * that library takes it over (its own Remove takes it back). Returns how many were kept.
 */
export async function removeLibrary(lib: Library, all: Library[]): Promise<number> {
  const game = libGame(lib);
  const others = all.filter((o) => o.id !== lib.id && libGame(o) === game && o.applied);
  const kept = lib.addedByUs.filter((r) => others.some((o) => o.items.includes(r)));
  const drop = lib.addedByUs.filter((r) => !kept.includes(r));
  const ws = drop.map(wsIdOf).filter((x): x is string => !!x);
  if (ws.length && lib.appid) await wsUnsubscribe(lib.appid, ws);
  for (const ref of drop.filter((r) => !wsIdOf(r))) {
    if (installed.has(ref)) await uninstallMod(ref);
    else cancelNxm(ref);
  }
  for (const o of others) {
    const take = kept.filter((r) => o.items.includes(r) && !o.addedByUs.includes(r));
    if (take.length) await saveLibrary({ ...o, addedByUs: [...o.addedByUs, ...take] });
  }
  await saveLibrary({ ...lib, applied: false, addedByUs: [] });
  return kept.length;
}
