// Steam Workshop (docs/WORKSHOP.md): browse and item details through sigf.ai's proxy, subscribe / unsubscribe / state
// through the core (which runs the Steam helper), and the player's libraries (named, shareable lists of items).
// Every call goes through one backend chosen at start: the core in the app, or a simulation on sample data in a plain
// browser (vite dev without Tauri, workshop-sample.ts) so the UI can be designed and screenshotted.

import { useEffect, useSyncExternalStore } from 'react';
import { inTauri } from './api';

// ---------- Contract types ----------

export type WorkshopItem = {
  id: string; appid: string; title: string; description: string;
  preview?: string;
  author?: string;
  subs: number; favs: number; votesUp?: number; votesDown?: number; score?: number;
  sizeBytes?: number; updated: number; created: number;
  tags: string[]; kind: 'item' | 'collection';
  children?: number;
  requires?: string[];
  url: string;
};

export type Sort = 'trend' | 'top' | 'new' | 'updated';
type BrowseQuery = { sort: Sort; q?: string; tag?: string | null; cursor?: string | null };
export type BrowsePage = { items: WorkshopItem[]; next: string | null; total: number };
type Collection = { collection: WorkshopItem; items: WorkshopItem[] };

/** What Steam says about one item on this PC (`workshop_state`). */
export type ItemState = { id: string; subscribed: boolean; installed: boolean; downloading: boolean; needsUpdate: boolean; sizeBytes?: number };

export type Library = {
  id: string;
  appid: string; name: string; items: string[];
  applied: boolean;
  addedByUs: string[];
  source?: { kind: 'collection' | 'link'; id?: string };
  created: number; updated: number;
};

export type ProgressEvent = { appid: string; id: string; state: 'subscribing' | 'downloading' | 'installed' | 'failed'; done: number; total: number; error?: string };

type WorkshopErrorCode =
  | 'steam_not_running' | 'not_owned' | 'not_steam_game' | 'helper_missing' | 'init_failed' | 'failed' | 'library'
  | 'search_unavailable' | 'not_found' | 'network';

/** A refusal from the core (`{ code, message }`) or the proxy, typed. */
export class WorkshopError extends Error {
  constructor(public code: WorkshopErrorCode, message: string) {
    super(message);
  }
}

/** Refusals that stop the whole game page (shown as a banner, not a toast): Steam itself is not usable for this game. */
export const STEAM_BLOCKING: ReadonlySet<WorkshopErrorCode> = new Set<WorkshopErrorCode>(['steam_not_running', 'not_owned', 'helper_missing', 'init_failed', 'not_steam_game']);
/** The blocking refusals that opening the Steam client fixes. */
export const STEAM_CLOSED: ReadonlySet<WorkshopErrorCode> = new Set<WorkshopErrorCode>(['steam_not_running', 'init_failed']);
export const isCode = (e: unknown, codes: ReadonlySet<WorkshopErrorCode>) => e instanceof WorkshopError && codes.has(e.code);

const CORE_CODES = new Set<string>(['steam_not_running', 'not_owned', 'not_steam_game', 'helper_missing', 'init_failed', 'failed', 'library']);
function asError(e: unknown): WorkshopError {
  if (e instanceof WorkshopError) return e;
  if (e && typeof e === 'object' && 'code' in e) {
    const o = e as { code: unknown; message?: unknown };
    const code = CORE_CODES.has(String(o.code)) ? (String(o.code) as WorkshopErrorCode) : 'failed';
    return new WorkshopError(code, String(o.message ?? o.code));
  }
  return new WorkshopError('failed', e instanceof Error ? e.message : String(e));
}

const tauriInvoke = async <T,>(cmd: string, args?: Record<string, unknown>): Promise<T> => (await import('@tauri-apps/api/core')).invoke<T>(cmd, args);

/** workshop_* and libraries_* refuse with `{ code, message }`. */
async function invoke<T>(cmd: string, args?: Record<string, unknown>): Promise<T> {
  try {
    return await tauriInvoke<T>(cmd, args);
  } catch (e) {
    throw asError(e);
  }
}

// ---------- Backends ----------

/** Everything that differs between the app and the browser preview. Errors are WorkshopErrors. */
export type Backend = {
  browse: (appid: string, o: BrowseQuery) => Promise<BrowsePage>;
  /** At most 100 ids. */
  items: (ids: string[]) => Promise<WorkshopItem[]>;
  collection: (id: string) => Promise<Collection>;
  state: (appid: string, ids: string[] | null) => Promise<ItemState[]>;
  subscribe: (appid: string, ids: string[]) => Promise<void>;
  unsubscribe: (appid: string, ids: string[]) => Promise<void>;
  libraries: () => Promise<Library[]>;
  saveLibrary: (lib: Library) => Promise<Library[]>;
  deleteLibrary: (id: string) => Promise<Library[]>;
};

/** One GET on sigf.ai's Workshop proxy through the core (`lobby_api`: no CORS, no webview origin involved). */
async function getJson<T>(path: string): Promise<T> {
  let r: { status: number; body: string };
  try {
    r = await tauriInvoke<{ status: number; body: string }>('lobby_api', { method: 'GET', path: `/api/app/workshop${path}`, body: null, secret: null });
  } catch (e) {
    throw new WorkshopError('network', e instanceof Error ? e.message : String(e));
  }
  let json: ({ error?: unknown; detail?: unknown } & Record<string, unknown>) | undefined;
  try {
    json = JSON.parse(r.body);
  } catch {}
  if (r.status < 400 && json) return json as T;
  const message = String(json?.detail ?? json?.error ?? `HTTP ${r.status}`);
  // 503 workshop_search_unavailable: the proxy runs without its Steam Web API key.
  if (r.status === 503 && json?.error === 'workshop_search_unavailable') throw new WorkshopError('search_unavailable', message);
  if (r.status === 404) throw new WorkshopError('not_found', message);
  throw new WorkshopError('network', message);
}

const tauri: Backend = {
  browse: (appid, o) => {
    const p = new URLSearchParams({ appid, sort: o.sort });
    if (o.q?.trim()) p.set('q', o.q.trim());
    if (o.tag) p.set('tag', o.tag);
    if (o.cursor) p.set('cursor', o.cursor);
    return getJson<BrowsePage>(`/browse?${p}`);
  },
  items: async (ids) => (await getJson<{ items: WorkshopItem[] }>(`/items?ids=${ids.join(',')}`)).items,
  collection: (id) => getJson<Collection>(`/collection/${id}`),
  state: (appid, ids) => invoke<ItemState[]>('workshop_state', { appid, ids }),
  subscribe: (appid, ids) => {
    listenProgress();
    return invoke<void>('workshop_subscribe', { appid, ids });
  },
  unsubscribe: (appid, ids) => invoke<void>('workshop_unsubscribe', { appid, ids }),
  libraries: () => invoke<Library[]>('libraries_list'),
  saveLibrary: (lib) => invoke<Library[]>('libraries_save', { lib }),
  deleteLibrary: (id) => invoke<Library[]>('libraries_delete', { id }),
};

const backend: Promise<Backend> = inTauri ? Promise.resolve(tauri) : import('./workshop-sample').then((m) => m.simBackend(onProgress));

let progressOff: Promise<() => void> | null = null;
/** Listens to `workshop://progress` once for the app's life (the store keeps every game's progress). */
function listenProgress() {
  progressOff ??= import('@tauri-apps/api/event').then(({ listen }) => listen<ProgressEvent>('workshop://progress', (e) => onProgress(e.payload)));
}

// ---------- Change signals ----------

/** A version number and its listeners; changes made in one burst notify once (next microtask). */
function signal() {
  const subs = new Set<() => void>();
  let queued = false;
  const s = {
    version: 0,
    subscribe: (f: () => void) => {
      subs.add(f);
      return () => void subs.delete(f);
    },
    emit: () => {
      if (queued) return;
      queued = true;
      queueMicrotask(() => {
        queued = false;
        s.version++;
        subs.forEach((f) => f());
      });
    },
  };
  return s;
}

// ---------- Proxy (sigf.ai) ----------

const cache = new Map<string, WorkshopItem>();
const itemsChanged = signal();
const remember = (items: WorkshopItem[]) => {
  items.forEach((i) => cache.set(i.id, i));
  if (items.length) itemsChanged.emit();
};
/** An item already fetched this session (cards, libraries), or undefined. */
export const cachedItem = (id: string) => cache.get(id);

export async function browse(appid: string, o: BrowseQuery): Promise<BrowsePage> {
  const page = await (await backend).browse(appid, o);
  remember(page.items);
  return page;
}

/** Fetches in flight, per id: two views asking for the same items share one request. */
const fetching = new Map<string, Promise<void>>();

/** Item details by id, in the order asked; unknown ids are left out. Cached for the session. */
export async function getItems(ids: string[]): Promise<WorkshopItem[]> {
  const uniq = [...new Set(ids)];
  const want = uniq.filter((id) => !cache.has(id) && !fetching.has(id));
  for (let i = 0; i < want.length; i += 100) {
    const part = want.slice(i, i + 100);
    const p = backend
      .then((b) => b.items(part))
      .then(remember)
      .finally(() => part.forEach((id) => fetching.delete(id)));
    part.forEach((id) => fetching.set(id, p));
  }
  await Promise.all(uniq.map((id) => fetching.get(id)));
  return ids.map((id) => cache.get(id)).filter((x): x is WorkshopItem => !!x);
}

/** These items (undefined until fetched, or when the proxy does not know them), fetched once and kept current. */
export function useItems(ids: string[]): (WorkshopItem | undefined)[] {
  const key = ids.join(',');
  useEffect(() => {
    if (ids.length) getItems(ids).catch(() => {});
  }, [key]);
  useSyncExternalStore(itemsChanged.subscribe, () => itemsChanged.version);
  return ids.map((id) => cache.get(id));
}

/** A collection and its items (a plain item's id answers with that one item). */
export async function getCollection(id: string): Promise<Collection> {
  const r = await (await backend).collection(id);
  remember(r.items);
  return r;
}

export const steamUrl = (id: string) => `https://steamcommunity.com/sharedfiles/filedetails/?id=${id}`;

// ---------- Item state on this PC (core + Steam helper) ----------

type Live = { state: ProgressEvent['state'] | 'unsubscribing'; done: number; total: number; error?: string };
type Entry = { state?: ItemState; live?: Live };

const states = new Map<string, Map<string, Entry>>();
const known = new Set<string>();   // app ids whose full subscribed list was read
const changed = signal();
const entries = (appid: string) => {
  let m = states.get(appid);
  if (!m) states.set(appid, (m = new Map()));
  return m;
};
const patch = (appid: string, id: string, f: (e: Entry) => Entry) => {
  const m = entries(appid);
  m.set(id, f(m.get(id) ?? {}));
  changed.emit();
};

/**
 * Re-renders on Workshop state changes (subscribe progress, state reads). With `select`, only when what it returns
 * changes (it must return a primitive or a stable value).
 */
export function useWorkshop<T = number>(select?: () => T): T {
  return useSyncExternalStore(changed.subscribe, select ?? (() => changed.version as T));
}

export type Status =
  | { kind: 'none' }
  | { kind: 'subscribing' }
  | { kind: 'downloading'; pct: number | null }
  | { kind: 'subscribed' }       // subscribed, Steam has not downloaded it yet
  | { kind: 'installed'; needsUpdate: boolean }
  | { kind: 'unsubscribing' }
  | { kind: 'failed'; error?: string };

export function statusOf(appid: string, id: string): Status {
  const e = states.get(appid)?.get(id);
  const l = e?.live;
  if (l) {
    if (l.state === 'subscribing') return { kind: 'subscribing' };
    if (l.state === 'unsubscribing') return { kind: 'unsubscribing' };
    if (l.state === 'downloading') return { kind: 'downloading', pct: l.total > 0 ? Math.min(100, (l.done / l.total) * 100) : null };
    if (l.state === 'failed') return { kind: 'failed', error: l.error };
  }
  const s = e?.state;
  if (!s?.subscribed) return { kind: 'none' };
  if (s.downloading) return { kind: 'downloading', pct: null };
  if (s.installed) return { kind: 'installed', needsUpdate: s.needsUpdate };
  return { kind: 'subscribed' };
}

export const isSubscribed = (appid: string, id: string) => !!states.get(appid)?.get(id)?.state?.subscribed;

/** Stored states of these items (null: every subscribed one). */
function fromStore(appid: string, ids: string[] | null): ItemState[] {
  const want = ids && new Set(ids);
  const out: ItemState[] = [];
  for (const [id, e] of states.get(appid) ?? []) if (e.state && (want ? want.has(id) : e.state.subscribed)) out.push(e.state);
  return out;
}

/** Subscribed item ids for a game, as last read (null: never read). */
export const subscribedIds = (appid: string): string[] | null => (known.has(appid) ? fromStore(appid, null).map((s) => s.id) : null);

function put(appid: string, list: ItemState[], all: boolean) {
  const m = entries(appid);
  if (all) {
    known.add(appid);
    // Everything not in the full list is not subscribed (unsubscribed in Steam since).
    const got = new Set(list.map((s) => s.id));
    for (const [id, e] of m) if (!got.has(id) && e.state) m.set(id, { ...e, state: { ...e.state, subscribed: false, installed: false, downloading: false } });
  }
  for (const s of list) patch(appid, s.id, (e) => ({ ...e, state: s, live: e.live?.state === 'failed' ? undefined : e.live }));
  changed.emit();
}

/** Subscribe calls running per game: the core runs one helper at a time, so `workshop_state` would wait for the whole
 *  download. While one runs, state reads answer from the store (kept current by `workshop://progress`). */
const running = new Map<string, number>();
/** State reads in flight (same game and ids share one helper call), and when each game's full list was last read. */
const reading = new Map<string, Promise<ItemState[]>>();
const fullAt = new Map<string, number>();
const FULL_FRESH_MS = 5000;

/** Reads what Steam has for these items (null: every subscribed item of the game; a full read under 5 s old is reused). */
export function readState(appid: string, ids: string[] | null): Promise<ItemState[]> {
  if (inTauri && (running.get(appid) ?? 0) > 0) return Promise.resolve(fromStore(appid, ids));
  if (ids === null && Date.now() - (fullAt.get(appid) ?? 0) < FULL_FRESH_MS) return Promise.resolve(fromStore(appid, null));
  const key = `${appid}:${ids ? [...ids].sort().join(',') : '*'}`;
  let p = reading.get(key);
  if (!p) {
    p = backend
      .then((b) => b.state(appid, ids))
      .then((list) => {
        put(appid, list, ids === null);
        if (ids === null) fullAt.set(appid, Date.now());
        return list;
      })
      .finally(() => reading.delete(key));
    reading.set(key, p);
  }
  return p;
}

function onProgress(p: ProgressEvent) {
  patch(p.appid, p.id, (e) => {
    if (p.state === 'installed') {
      return { state: { ...(e.state ?? { id: p.id, needsUpdate: false }), subscribed: true, installed: true, downloading: false, needsUpdate: false, sizeBytes: p.total || e.state?.sizeBytes } };
    }
    return { ...e, live: { state: p.state, done: p.done, total: p.total, error: p.error } };
  });
}

/** Subscribes and waits for Steam to download them; the store follows progress. Throws a WorkshopError. */
export async function subscribe(appid: string, ids: string[]): Promise<void> {
  if (!ids.length) return;
  for (const id of ids) patch(appid, id, (e) => ({ ...e, live: { state: 'subscribing', done: 0, total: 0 } }));
  running.set(appid, (running.get(appid) ?? 0) + 1);
  try {
    await (await backend).subscribe(appid, ids);
  } finally {
    running.set(appid, (running.get(appid) ?? 1) - 1);
    fullAt.delete(appid);
    // Whatever the events said, Steam's own answer wins (failed items keep their error until the next try).
    await readState(appid, ids).catch(() => {});
    for (const id of ids) patch(appid, id, (x) => ({ ...x, live: x.live?.state === 'failed' ? x.live : undefined }));
  }
}

export async function unsubscribe(appid: string, ids: string[]): Promise<void> {
  if (!ids.length) return;
  for (const id of ids) patch(appid, id, (e) => ({ ...e, live: { state: 'unsubscribing', done: 0, total: 0 } }));
  try {
    await (await backend).unsubscribe(appid, ids);
    for (const id of ids) patch(appid, id, (e) => ({ state: { ...(e.state ?? { id, needsUpdate: false }), subscribed: false, installed: false, downloading: false } }));
  } finally {
    fullAt.delete(appid);
    for (const id of ids) patch(appid, id, (x) => ({ ...x, live: x.live?.state === 'unsubscribing' ? undefined : x.live }));
  }
}

// ---------- Libraries ----------

let libs: Library[] | null = null;
const libSubs = new Set<() => void>();
const setLibs = (l: Library[]) => {
  libs = l;
  libSubs.forEach((f) => f());
};
let libsP: Promise<Library[]> | null = null;

/** Every library on this PC (all games), read once and kept current by save/delete. Null while loading. */
export function useLibraries(): Library[] | null {
  libsP ??= backend
    .then((b) => b.libraries())
    .then(
      (l) => (setLibs(l), l),
      (e) => {
        libsP = null;
        setLibs([]);
        throw e;
      },
    );
  return useSyncExternalStore(
    (f) => {
      libSubs.add(f);
      return () => libSubs.delete(f);
    },
    () => libs,
  );
}

export async function saveLibrary(lib: Library): Promise<void> {
  setLibs(await (await backend).saveLibrary({ ...lib, updated: Date.now() }));
}

export async function deleteLibrary(id: string): Promise<void> {
  setLibs(await (await backend).deleteLibrary(id));
}

const ID_CHARS = 'abcdefghijkmnopqrstuvwxyz23456789';   // 33 symbols: no l, 0, 1
/** A library id: 10 chars of [a-km-z2-9], unbiased (bytes past the last whole multiple of 33 are dropped). */
function newLibraryId(): string {
  let id = '';
  while (id.length < 10) for (const b of crypto.getRandomValues(new Uint8Array(16))) if (b < ID_CHARS.length * 7 && id.length < 10) id += ID_CHARS[b % ID_CHARS.length];
  return id;
}

export function newLibrary(appid: string, name: string, items: string[] = [], source?: Library['source']): Library {
  const now = Date.now();
  return { id: newLibraryId(), appid, name, items: [...new Set(items)], applied: false, addedByUs: [], source, created: now, updated: now };
}

/**
 * Apply: subscribes every item of the library that is not subscribed yet. The ones it subscribed are recorded in
 * `addedByUs` (saved before Steam is asked, so a crash still knows them) so Remove takes back only those.
 */
export async function applyLibrary(lib: Library): Promise<void> {
  if (!lib.items.every((id) => states.get(lib.appid)?.get(id)?.state)) await readState(lib.appid, lib.items).catch(() => {});
  const missing = lib.items.filter((id) => !isSubscribed(lib.appid, id));
  const addedByUs = [...new Set([...lib.addedByUs.filter((id) => lib.items.includes(id)), ...missing])];
  const cur = { ...lib, addedByUs, applied: false };
  await saveLibrary(cur);
  await subscribe(lib.appid, missing);
  await saveLibrary({ ...cur, applied: true });
}

/**
 * Remove: unsubscribes what this library added. An item another applied library of the same game also holds stays
 * subscribed, and that library takes it over (its own Remove will take it back). Returns how many were kept.
 */
export async function removeLibrary(lib: Library, all: Library[]): Promise<number> {
  const others = all.filter((o) => o.id !== lib.id && o.appid === lib.appid && o.applied);
  const kept = lib.addedByUs.filter((id) => others.some((o) => o.items.includes(id)));
  const drop = lib.addedByUs.filter((id) => !kept.includes(id));
  await unsubscribe(lib.appid, drop);
  for (const o of others) {
    const take = kept.filter((id) => o.items.includes(id) && !o.addedByUs.includes(id));
    if (take.length) await saveLibrary({ ...o, addedByUs: [...o.addedByUs, ...take] });
  }
  await saveLibrary({ ...lib, applied: false, addedByUs: [] });
  return kept.length;
}

// ---------- Links ----------

export const MAX_SHARE = 200;

/** `sigf://library/{appid}/<ids>?name=<name>` (at most 200 items). */
export function shareLink(lib: Pick<Library, 'appid' | 'items' | 'name'>): string {
  return `sigf://library/${lib.appid}/${lib.items.slice(0, MAX_SHARE).join(',')}?name=${encodeURIComponent(lib.name)}`;
}

type SharedLibrary = { appid: string; ids: string[]; name: string };

/** A library link as sent (`sigf://library/...`) or pasted from the web (`https://sigf.ai/library/...`). */
export function parseShare(text: string): SharedLibrary | null {
  const m = text.trim().match(/^(?:sigf:\/\/library\/|https?:\/\/(?:www\.)?sigf\.ai\/library\/)(\d{1,10})\/([\d,]+)\/?(?:\?(.*))?$/i);
  if (!m) return null;
  const ids = [...new Set(m[2].split(',').filter((x) => /^\d{1,20}$/.test(x)))].slice(0, MAX_SHARE);
  if (!ids.length) return null;
  let name = '';
  try {
    name = new URLSearchParams(m[3] ?? '').get('name')?.trim().slice(0, 80) ?? '';
  } catch {}
  return { appid: m[1], ids, name };
}

/** A Steam Workshop link (`steamcommunity.com/sharedfiles/filedetails/?id=…`, `/workshop/filedetails/?id=…`): its id. */
function parseSteamLink(text: string): string | null {
  const s = text.trim();
  const m = s.match(/^(?:https?:\/\/)?(?:www\.)?steamcommunity\.com\/(?:sharedfiles|workshop)\/filedetails\/?\?(?:.*&)?id=(\d{1,20})/i);
  if (m) return m[1];
  return /^\d{6,20}$/.test(s) ? s : null;
}

type Pasted = { kind: 'share'; share: SharedLibrary } | { kind: 'collection'; id: string } | null;
export function parsePasted(text: string): Pasted {
  const share = parseShare(text);
  if (share) return { kind: 'share', share };
  const id = parseSteamLink(text);
  return id ? { kind: 'collection', id } : null;
}

// ---------- Small formatters ----------

/** Bytes as KB / MB / GB with the locale's units. */
export function fmtBytes(n: number | undefined, locale?: string): string {
  if (!n || n < 0) return '';
  const [v, unit] = n >= 1e9 ? [n / 1e9, 'gigabyte'] : n >= 1e6 ? [n / 1e6, 'megabyte'] : [Math.max(1, n / 1e3), 'kilobyte'];
  return new Intl.NumberFormat(locale, { style: 'unit', unit, unitDisplay: 'short', maximumFractionDigits: v < 10 ? 1 : 0 }).format(v);
}
