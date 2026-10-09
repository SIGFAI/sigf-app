// Multiplayer lobbies (docs/RECIPE-FORMAT.md section 9): the API on sigf.ai through the Rust core, joins through
// the core's join path (install the pinned version, then launch with the join args). In a plain browser (vite dev)
// every call works on local sample data, so the screens can be designed without the app or the API.

import { inTauri, type Game } from './api';
import { t, type Key } from '../i18n';
import { en } from '../i18n/en';

export type JoinKind = 'prism' | 'connect' | 'mod';
export type Target = { game: string; address: string; join?: JoinKind };
export type PublicLobby = {
  id: string;
  mashup: { id: string; version: string; name: string };
  games: string[];
  host: string;
  mode: 'invite' | 'public';
  players: number;
  maxPlayers: number;
  state: 'waiting' | 'open' | 'full';
  createdAt: string;
};
export type Lobby = PublicLobby & {
  targets: Target[];
  server: { provider: 'local-host' | 'controller'; state: 'ready' | 'starting' | 'failed' };
  expiresAt: string;
  invite: string;
  url: string;
};
/** A lobby this app hosts: the secret signs the heartbeats and the close. Kept in memory only. */
export type Hosted = { lobby: Lobby; secret: string };

export type JoinStep = 'lobby' | 'install' | 'launch' | 'done';
export type JoinError = { kind: string; message: string };

/** Canonical games whose engine joins with `+connect`; kept in step with join.rs and the sigf.ai lobby API. */
export const CONNECT_GAMES = new Set(['tf2', 'gmod', 'portal2', 'cs16', 'css', 'hl2dm', 'l4d2', 'quake']);
export const DEFAULT_PORT = (game: string) => (game === 'minecraft' ? 25565 : CONNECT_GAMES.has(game) ? 27015 : 7777);

/** `host:port` the API will accept (same rule as the core and the API). */
export function validAddress(s: string): boolean {
  const m = /^(\[[0-9a-f:.]{2,45}\]|[a-z0-9](?:[a-z0-9.-]{0,251}[a-z0-9])?):(\d{1,5})$/i.exec(s.trim());
  if (!m || m[2].startsWith('0') || Number(m[2]) > 65535) return false;
  return m[1].startsWith('[') || m[1].split('.').every((l) => l.length > 0 && l.length <= 63 && !l.startsWith('-') && !l.endsWith('-'));
}

const ID_RE = /^[a-km-z2-9]{12}$/;
/** The lobby id of an invite (`sigf://join/<id>`, `https://sigf.ai/join/<id>`, or the id), or null. */
export function parseInvite(text: string): string | null {
  const s = text.trim();
  const m = /^(?:sigf:\/\/join\/|https:\/\/(?:www\.)?sigf\.ai\/join\/)?([^/?#\s]+)\/?(?:[?#].*)?$/i.exec(s);
  return m && ID_RE.test(m[1]) ? m[1] : null;
}

async function invoke<T>(cmd: string, args?: Record<string, unknown>): Promise<T> {
  const { invoke } = await import('@tauri-apps/api/core');
  return invoke<T>(cmd, args);
}

export class ApiError extends Error {
  /** servers_busy: the place in the free servers' queue, when the controller gives one. */
  position?: number;
  constructor(public status: number, public code: string, message: string) {
    super(message);
  }
}

/** The lobby service's error code in the player's language, when the dictionaries have it. */
const readable = (code: string): Key | null => (`lobbyApi.${code}` in en ? (`lobbyApi.${code}` as Key) : null);

/** One call to /api/app/lobbies* through the core (no CORS, no webview origin involved). */
async function api<T>(method: 'GET' | 'POST' | 'DELETE', path: string, body?: unknown, secret?: string): Promise<T> {
  const r = await invoke<{ status: number; body: string }>('lobby_api', { method, path, body: body === undefined ? null : JSON.stringify(body), secret: secret ?? null });
  let json: { error?: string; detail?: string } & Record<string, unknown> = {};
  try {
    json = JSON.parse(r.body);
  } catch {}
  if (r.status >= 400) {
    const code = json.error ?? `http_${r.status}`;
    const key = readable(code);
    const status = String(r.status);
    const e = new ApiError(r.status, code, key ? t(key, { status }) : json.detail ?? t('lobbyApi.status', { status }));
    if (typeof json.position === 'number') e.position = json.position;
    throw e;
  }
  return json as T;
}

// ---------- browser samples ----------
const now = () => new Date().toISOString();
const SAMPLE: PublicLobby[] = [
  { id: 'k3m9xq2wa7fd', mashup: { id: 'sigf/example-passthrough', version: '1.0.0', name: 'Example Passthrough' }, games: ['gta5', 'minecraft'], host: 'nightowl', mode: 'public', players: 6, maxPlayers: 20, state: 'open', createdAt: now() },
  { id: 'p8r2tz4hc6vb', mashup: { id: 'example/skyrim-minecraft', version: '1.2.0', name: 'Example Fusion' }, games: ['skyrim', 'minecraft'], host: 'Dovah K', mode: 'public', players: 41, maxPlayers: 100, state: 'open', createdAt: now() },
  { id: 'h4c8vt2nb7xe', mashup: { id: 'sigf/example-lethal-mod', version: '0.4.0', name: 'Example Crew Mod' }, games: ['lethal'], host: 'quota_or_bust', mode: 'public', players: 3, maxPlayers: 8, state: 'open', createdAt: now() },
  { id: 'w5n7jq3es9ga', mashup: { id: 'sigf/example-tf2-mod', version: '0.3.1', name: 'Example Mod' }, games: ['tf2'], host: 'medic_main', mode: 'public', players: 12, maxPlayers: 12, state: 'full', createdAt: now() },
];
const fakeLobby = (p: PublicLobby, targets: Target[] = []): Lobby => ({
  ...p, targets, server: { provider: 'local-host', state: 'ready' }, expiresAt: now(), invite: `sigf://join/${p.id}`, url: `https://sigf.ai/join/${p.id}`,
});
const fakeId = () => Array.from({ length: 12 }, () => 'abcdefghijkmnopqrstuvwxyz23456789'[Math.floor(Math.random() * 33)]).join('');

// ---------- the API ----------
/**
 * Public lobbies for games this PC owns. With `sendGames` (privacy choice "lobbyGames") the owned ids go with the
 * request and sigf.ai filters; without it the app asks for the whole public list and filters it here.
 */
export async function listLobbies(owned: Set<string>, mashup?: string, sendGames = true): Promise<PublicLobby[]> {
  if (!inTauri) return SAMPLE.filter((l) => (!mashup || l.mashup.id === mashup));
  const q = new URLSearchParams();
  if (mashup) q.set('mashup', mashup);
  if (sendGames && owned.size) q.set('games', [...owned].join(','));
  const list = await api<PublicLobby[]>('GET', `/api/app/lobbies?${q}`);
  return sendGames || !owned.size ? list : list.filter((l) => l.games.every((g) => owned.has(g)));
}

export async function getLobby(id: string): Promise<Lobby> {
  if (!inTauri) {
    const s = SAMPLE.find((l) => l.id === id) ?? { ...SAMPLE[0], id };
    return fakeLobby(s, [{ game: 'minecraft', address: 'sample.invalid:25565', join: 'prism' }]);
  }
  return api<Lobby>('GET', `/api/app/lobbies/${id}`);
}

export type CreateLobby = { mashup: string; host: string; mode: 'invite' | 'public'; maxPlayers: number; targets: Target[] };
export async function createLobby(req: CreateLobby): Promise<Hosted> {
  if (!inTauri) {
    const [id, version] = req.mashup.split('@');
    const p: PublicLobby = { id: fakeId(), mashup: { id, version, name: '' }, games: [], host: req.host, mode: req.mode, players: 1, maxPlayers: req.maxPlayers, state: req.targets.length ? 'open' : 'waiting', createdAt: now() };
    return { lobby: fakeLobby(p, req.targets), secret: 'browser' };
  }
  return api<Hosted>('POST', '/api/app/lobbies', req);
}

export async function heartbeat(h: Hosted, players: number, targets?: Target[]): Promise<Lobby> {
  if (!inTauri) return { ...h.lobby, players, targets: targets ?? h.lobby.targets, state: players >= h.lobby.maxPlayers ? 'full' : 'open' };
  return api<Lobby>('POST', `/api/app/lobbies/${h.lobby.id}/heartbeat`, { players, ...(targets ? { targets: targets.map(({ game, address }) => ({ game, address })) } : {}) }, h.secret);
}

export async function closeLobby(h: Hosted): Promise<void> {
  if (!inTauri) return;
  await api('DELETE', `/api/app/lobbies/${h.lobby.id}`, undefined, h.secret);
}

// ---------- free hosted servers (docs/RECIPE-FORMAT.md section 9.5) ----------
export type Region = { id: string; label: string; available: boolean; default: boolean };
export type Hosting = { regions: Region[]; limits: { maxPlayers: number; hours: number; worldDays: number } };
export type ServerState = 'none' | 'queued' | 'starting' | 'running' | 'stopped' | 'failed';
export type HostedServer = {
  lobby: string | null;
  mashup?: { id: string; version: string; name: string };
  state: ServerState;
  region?: string;
  address?: string | null;
  players?: number;
  maxPlayers?: number;
  etaS?: number | null;
  position?: number | null;
  startedAt?: string;
  expiresAt?: string;
  stoppedAt?: string | null;
  worldUntil?: string;
};
/** A lobby this app ran a hosted server for: its secret, kept by the core in <SIGF_HOME>/hosted.json. */
export type HostedEntry = { lobby: string; secret: string; mashupId: string; name: string; startedAt: number; worldUntil?: number | null };
export const SERVER_ACTIVE: ServerState[] = ['queued', 'starting', 'running'];

const SAMPLE_HOSTING: Hosting = {
  regions: [
    { id: 'eu-west-1', label: 'Europe', available: true, default: true },
    { id: 'us-east-1', label: 'North America', available: false, default: false },
  ],
  limits: { maxPlayers: 10, hours: 8, worldDays: 7 },
};
const browserServers = new Map<string, HostedServer>();

/** Free hosted servers on sigf.ai: regions and limits, or null when the site offers none (the option stays hidden). */
export async function hostingInfo(): Promise<Hosting | null> {
  if (!inTauri) return SAMPLE_HOSTING;
  try {
    return await api<Hosting>('GET', '/api/app/lobbies/hosting');
  } catch {
    return null;
  }
}

/** The open region nearest to this PC (the Americas: us-east-1), else the site's default. */
export function nearestRegion(h: Hosting): string {
  let tz = '';
  try {
    tz = Intl.DateTimeFormat().resolvedOptions().timeZone ?? '';
  } catch {}
  const want = tz.startsWith('America/') ? 'us-east-1' : 'eu-west-1';
  const open = h.regions.filter((r) => r.available);
  return (open.find((r) => r.id === want) ?? open.find((r) => r.default) ?? open[0] ?? h.regions[0]).id;
}

export async function startServer(h: Hosted, region: string): Promise<HostedServer> {
  if (!inTauri) {
    const at = Date.now();
    const s: HostedServer = {
      lobby: h.lobby.id, state: 'starting', region, players: 0, maxPlayers: 10, etaS: 6, startedAt: now(),
      expiresAt: new Date(at + 8 * 3600e3).toISOString(), worldUntil: new Date(at + 8 * 3600e3 + 7 * 86400e3).toISOString(),
    };
    browserServers.set(h.lobby.id, s);
    setTimeout(() => browserServers.set(h.lobby.id, { ...s, state: 'running', address: 'sample.invalid:25565', players: 1, etaS: null }), 6000);
    return s;
  }
  return api<HostedServer>('POST', `/api/app/lobbies/${h.lobby.id}/server`, { region }, h.secret);
}

export async function getServer(lobby: string): Promise<HostedServer> {
  if (!inTauri) return browserServers.get(lobby) ?? { lobby, state: 'none' };
  return api<HostedServer>('GET', `/api/app/lobbies/${lobby}/server`);
}

export async function stopServer(lobby: string, secret: string): Promise<HostedServer> {
  if (!inTauri) {
    const s: HostedServer = { ...(browserServers.get(lobby) ?? { lobby }), state: 'stopped', address: null, stoppedAt: now() };
    browserServers.set(lobby, s);
    return s;
  }
  return api<HostedServer>('DELETE', `/api/app/lobbies/${lobby}/server`, undefined, secret);
}

/** A presigned download link of the lobby's world (host secret): during the session and for 7 days after it. */
export async function worldLink(lobby: string, secret: string): Promise<{ url: string; expiresAt: string | null; size: number | null }> {
  if (!inTauri) return { url: 'https://example.invalid/world.zip', expiresAt: null, size: null };
  return api('GET', `/api/app/lobbies/${lobby}/world`, undefined, secret);
}

const HOSTED_KEY = 'hosted-worlds';
const browserList = (): HostedEntry[] => {
  try {
    return JSON.parse(localStorage.getItem(HOSTED_KEY) ?? '[]');
  } catch {
    return [];
  }
};
const browserKeep = (list: HostedEntry[]) => {
  try {
    localStorage.setItem(HOSTED_KEY, JSON.stringify(list));
  } catch {}
  return list;
};
/** The kept host secrets, newest first (the core's hosted.json; the browser preview keeps them in localStorage). */
export async function hostedList(): Promise<HostedEntry[]> {
  if (inTauri) return invoke<HostedEntry[]>('hosted_list');
  return browserList();
}
export async function hostedSave(e: HostedEntry): Promise<HostedEntry[]> {
  if (inTauri) return invoke<HostedEntry[]>('hosted_save', { entry: e });
  return browserKeep([e, ...browserList().filter((x) => x.lobby !== e.lobby)]);
}
export async function hostedForget(lobby: string): Promise<HostedEntry[]> {
  if (inTauri) return invoke<HostedEntry[]>('hosted_forget', { lobby });
  return browserKeep(browserList().filter((x) => x.lobby !== lobby));
}

// ---------- this PC ----------
/** This PC's LAN address, for the host's default join address. Browser: a documentation address. */
export async function lanAddress(): Promise<string> {
  if (!inTauri) return '192.168.1.20';
  return (await invoke<string | null>('lan_address')) ?? '127.0.0.1';
}

/** Players on the host's own Minecraft world, or null when it does not answer (not opened yet). */
export async function minecraftPlayers(address: string): Promise<[number, number] | null> {
  if (!inTauri) return null;
  return invoke<[number, number] | null>('minecraft_players', { address });
}

/** Joins: the core reads the lobby, installs its pinned version if needed, launches with the join args. */
/**
 * Joins after the player confirmed `seen` (the lobby as the confirm sheet showed it): the core refuses when the lobby's
 * mashup, version or server addresses changed since, so a host cannot swap the address after the click.
 */
export async function joinLobby(id: string, games: Game[], seen: Lobby): Promise<Lobby> {
  if (!inTauri) {
    await new Promise((r) => setTimeout(r, 900));
    return getLobby(id);
  }
  const dirs: Record<string, string> = {};
  const stores: Record<string, [string, string, string | null]> = {};
  for (const g of games) {
    if (g.canon && g.installDir && !dirs[g.canon]) dirs[g.canon] = g.installDir;
    if (g.canon && !stores[g.canon]) stores[g.canon] = [g.store, g.storeId, g.launch ?? null];
  }
  const confirmed = {
    mashup: seen.mashup.id,
    version: seen.mashup.version,
    targets: Object.fromEntries(seen.targets.map((t) => [t.game, t.address])),
  };
  return invoke<Lobby>('join_lobby', { lobby: id, gameDirs: dirs, stores, confirmed });
}

export function isJoinError(e: unknown): e is JoinError {
  return typeof e === 'object' && e !== null && 'kind' in e && 'message' in e;
}

/** `join://progress` from the core; returns the unsubscribe function. Browser: never fires. */
export async function onJoinProgress(cb: (p: { lobby: string; step: JoinStep }) => void): Promise<() => void> {
  if (!inTauri) return () => {};
  const { listen } = await import('@tauri-apps/api/event');
  return listen<{ lobby: string; step: JoinStep }>('join://progress', (e) => cb(e.payload));
}

/**
 * Invite links opened from outside (a sigf:// click, a second launch): `cb` gets each lobby id once, the ones that
 * arrived before the UI was listening included. Browser: `?join=<id>` in the page URL, for design work.
 */
export async function onInviteLinks(cb: (id: string) => void): Promise<() => void> {
  if (!inTauri) {
    const id = parseInvite(new URLSearchParams(location.search).get('join') ?? '');
    if (id) setTimeout(() => cb(id), 600);
    // `?link=<sigf://library/...>`: a Workshop library link, as the core would hand it over.
    const link = new URLSearchParams(location.search).get('link');
    if (link) setTimeout(() => cb(link), 600);
    return () => {};
  }
  const drain = async () => (await invoke<string[]>('take_links')).forEach(cb);
  const { listen } = await import('@tauri-apps/api/event');
  const off = await listen('link://open', () => void drain());
  await drain();
  return off;
}
