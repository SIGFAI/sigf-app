import { useEffect, useMemo, useRef, useState } from 'react';
import { CATALOG, installedConflict, type Mashup } from './data/catalog';
import { GAME } from './data/games';
import { appPlatform, fetchAgents, getText, inTauri, launchGame, openUrl, playInstalled, prismDownload, runsOn, scanGames, windowAction, type Agent, type Scan } from './lib/api';
import { findOwnCopies, installMashup, installedMods, isInstallError, onInstallProgress, restoreMashup, type OwnFound, type PlayError } from './lib/install';
import { Icon, STORE_LABEL } from './ui';
import { Home } from './views/Home';
import { Library } from './views/Library';
import { Build } from './views/Build';
import { Live } from './views/Live';
import { Queue } from './views/Queue';
import { Detail } from './views/Detail';
import { Picker } from './views/Picker';
import { JoinSheet, Lobbies, type Joining } from './views/Lobbies';
import { OwnCopySheet, type OwnAsk } from './views/OwnCopy';
import { PrivacyPanel } from './views/Privacy';
import { ReportSheet } from './views/Report';
import type { LastError } from './lib/api';
import { UpdateBanner } from './views/Update';
import { UPDATE_EVERY_MS, checkUpdate, type Available } from './lib/update';
import { loadPrivacy, usePrivacy } from './lib/privacy';
import { list, setLocale, t, tx, useLocale, type Key } from './i18n';
import { installErrorText, joinErrorText, playErrorText } from './i18n/errors';
import {
  ApiError, SERVER_ACTIVE, closeLobby, createLobby, getLobby, getServer, heartbeat, hostedForget, hostedList, hostedSave, hostingInfo, isJoinError, joinLobby,
  minecraftPlayers, onInviteLinks, onJoinProgress, startServer, stopServer, worldLink, type HostedEntry, type HostedServer, type Hosted, type Hosting, type Lobby, type Target,
} from './lib/lobbies';

const SITE = 'https://sigf.ai';
const HEARTBEAT_MS = 30_000;
const SERVER_POLL_MS = 10_000;
const SERVER_WARN_MS = 15 * 60_000;   // a gentle warning when the 8 h session has this much left

export type View = 'mix' | 'library' | 'live' | 'together' | 'build' | 'queue';
export type Phase = 'download' | 'verify' | 'build' | 'install' | 'ready';
/** `version`: the installed version, once the engine has it (lobbies pin one). */
export type Install = { phase: Phase; pct: number; started: number; real?: boolean; version?: string };
/** `region`: start a free hosted server there (the mashup's recipe has a `server` block); else the host's own game serves. */
export type HostOptions = { mode: 'invite' | 'public'; maxPlayers: number; name: string; targets: Target[]; region?: string };

/** What the app knows about the player: built once per scan, read by every view. */
export type Ctx = {
  scan: Scan | null;
  catalog: Mashup[];
  /** Live catalog mashups that do not run on this system, by id -> the systems they run on (`windows`...). The catalog
   *  above leaves them out; lobby lists show their lobbies disabled ("Windows only"). */
  elsewhere: Map<string, string[]>;
  owned: Set<string>;
  installs: Record<string, Install>;
  pair: [string | null, string | null];
  setPair: (p: [string | null, string | null]) => void;
  pick: (slot: 0 | 1) => void;
  open: (m: Mashup) => void;
  get: (m: Mashup) => void;
  play: (m: Mashup) => void;
  restore: (m: Mashup) => void;
  /** The installed mashup `m` cannot be installed next to (catalog `conflicts`, both ways), or null. */
  conflictOf: (m: Mashup) => Mashup | null;
  /** Restores the conflicting mashup `other`, then installs `m` (the "Restore <other> first" button). */
  restoreThenGet: (m: Mashup, other: Mashup) => void;
  agents: Agent[];
  go: (v: View) => void;
  /** Installs a local mashup.json (creators, tests): same engine, no catalog. */
  sideload: (recipeJson: string) => void;
  /** The lobby this app hosts (one at a time), its heartbeat running. */
  hosted: Hosted | null;
  host: (m: Mashup, o: HostOptions) => Promise<void>;
  /** Sends a heartbeat now with this player count (games without an automatic count). */
  hostUpdate: (players: number) => void;
  closeHost: () => void;
  /** Free hosted servers on the site (regions, limits), or null when it offers none. */
  hosting: Hosting | null;
  /** The hosted lobby's free server: null when the host's own game serves. */
  server: HostedServer | null;
  /** Why the last start failed (all servers busy...), shown on the lobby card. */
  serverError: string | null;
  startServer: (region: string) => Promise<void>;
  stopServer: () => Promise<void>;
  /** Hosted worlds still downloadable (host secrets kept by the core for 7 days). */
  worlds: HostedEntry[];
  downloadWorld: (lobby: string) => Promise<void>;
  forgetWorld: (lobby: string) => void;
  /** Joins a lobby by id: the join sheet takes it from there. An invite link always asks first; a Join from a lobby
   *  list (`fromList`) asks when it would install something. */
  join: (id: string, fromList?: boolean) => void;
  /** "Report a bug" for a mashup, or for the app itself (null): the report sheet, read before anything opens. */
  report: (m: Mashup | null) => void;
};

const NAV: { id: View; label: Key; icon: string }[] = [
  { id: 'mix', label: 'nav.mix', icon: 'mix' },
  { id: 'library', label: 'nav.library', icon: 'library' },
  { id: 'live', label: 'nav.live', icon: 'tv' },
  { id: 'together', label: 'nav.lobbies', icon: 'people' },
  { id: 'build', label: 'nav.build', icon: 'build' },
  { id: 'queue', label: 'nav.installs', icon: 'queue' },
];

const loadInstalls = (): Record<string, Install> => {
  try {
    return JSON.parse(localStorage.getItem('installs') ?? '{}');
  } catch {
    return {};
  }
};

export default function App() {
  const [scan, setScan] = useState<Scan | null>(null);
  const [view, setView] = useState<View>('mix');
  const [pair, setPair] = useState<[string | null, string | null]>([null, null]);
  const [picking, setPicking] = useState<0 | 1 | null>(null);
  const [detail, setDetail] = useState<Mashup | null>(null);
  const [installs, setInstalls] = useState<Record<string, Install>>(loadInstalls);
  const [agents, setAgents] = useState<Agent[]>([]);
  // The seed only dresses the browser preview; the real app shows the live catalog alone.
  const [catalog, setCatalog] = useState<Mashup[]>(inTauri ? [] : CATALOG);
  const [elsewhere, setElsewhere] = useState<Map<string, string[]>>(new Map());
  const [query, setQuery] = useState('');
  const [toast, setToast] = useState<string | null>(null);
  const [hosted, setHosted] = useState<Hosted | null>(null);
  const [joining, setJoining] = useState<Joining | null>(null);
  const [ownAsk, setOwnAsk] = useState<OwnAsk | null>(null);
  const [hosting, setHosting] = useState<Hosting | null>(null);
  const [server, setServer] = useState<HostedServer | null>(null);
  const [serverError, setServerError] = useState<string | null>(null);
  const [worlds, setWorlds] = useState<HostedEntry[]>([]);
  const privacy = usePrivacy();
  // The language: the saved choice once the privacy file is read, the system's until then. App re-renders on a switch.
  useLocale();
  useEffect(() => setLocale(privacy?.language), [privacy?.language]);
  const [privacyOpen, setPrivacyOpen] = useState(false);
  // Report a bug: the sheet's subject (null: the app), and the last error the app showed per mashup ('' = any), this
  // session only. They stay on the PC unless the player puts them in a report and submits it.
  const [reporting, setReporting] = useState<{ m: Mashup | null } | null>(null);
  const lastErrors = useRef<Record<string, LastError>>({});
  const noteError = (id: string, message: string, kind: LastError['kind']) => {
    lastErrors.current[id] = { message, kind };
    lastErrors.current[''] = { message, kind };
  };
  const [update, setUpdate] = useState<Available | null>(null);
  const [updateLater, setUpdateLater] = useState<string | null>(null);
  const warned = useRef<string | null>(null);
  const search = useRef<HTMLInputElement>(null);
  // Refresh (button, F5 / Ctrl+R, window focus): one at a time; the last good catalog read for the focus refetch.
  const [refreshing, setRefreshing] = useState(false);
  const refreshRun = useRef<Promise<void> | null>(null);
  const catalogAt = useRef(0);
  // Read by the long-lived listeners and timers below without re-subscribing them.
  const latest = useRef({ scan, owned: new Set<string>(), installs, hosted, server: null as HostedServer | null, join: (_id: string) => {} });

  useEffect(() => {
    // Until the privacy choices are answered (first start), the catalog below is the only request that goes out.
    void loadPrivacy();
    scanGames().then(setScan);
    hostedList().then(setWorlds).catch(() => {});
    void loadCatalog();
    void refreshInstalled();
    const offs: (() => void)[] = [];
    onInstallProgress((p) => setInstalls((a) => ({ ...a, [p.id]: { ...a[p.id], phase: p.phase, pct: p.pct, started: a[p.id]?.started ?? Date.now(), real: true } }))).then((f) => offs.push(f));
    onJoinProgress((p) => setJoining((j) => (j && j.id === p.lobby && j.step !== 'error' ? { ...j, step: p.step } : j))).then((f) => offs.push(f));
    return () => offs.forEach((f) => f());
  }, []);

  // Everything else that goes online by itself waits for the privacy answer. Invite links that arrived before it wait
  // in the core's queue and are read here.
  const answered = !!privacy?.asked;
  useEffect(() => {
    if (!answered) return;
    fetchAgents().then(setAgents);
    hostingInfo().then(setHosting);
    let off: (() => void) | null = null;
    let live = true;
    // Invite links from outside (sigf:// clicks, a second launch), the one the app was started with included.
    onInviteLinks((id) => latest.current.join(id)).then((f) => (live ? (off = f) : f()));
    // Updates: asked now and every 6 hours (GitHub, docs/PRIVACY.md). "Later" hides that version until the next check.
    const ask = () => checkUpdate().then((u) => {
      if (!live) return;
      setUpdate(u);
      setUpdateLater(null);
    }, () => {});
    const first = setTimeout(ask, 3000);
    const every = setInterval(ask, UPDATE_EVERY_MS);
    return () => {
      live = false;
      off?.();
      clearTimeout(first);
      clearInterval(every);
    };
  }, [answered]);

  // The hosted lobby's heartbeat: every 30 s, with the live player count (Minecraft: read from the host's own world).
  useEffect(() => {
    if (!hosted) return;
    const beat = async () => {
      const h = latest.current.hosted;
      if (!h) return;
      let players = h.lobby.players;
      const mc = h.lobby.targets.find((t) => t.game === 'minecraft');
      const srv = latest.current.server;
      if (srv?.state === 'running' && typeof srv.players === 'number') players = Math.max(1, Math.min(h.lobby.maxPlayers, srv.players));
      else if (mc) {
        const st = await minecraftPlayers(mc.address).catch(() => null);
        if (st) players = Math.max(1, Math.min(h.lobby.maxPlayers, st[0]));
      }
      try {
        const lobby = await heartbeat(h, players);
        setHosted((cur) => (cur && cur.lobby.id === lobby.id ? { ...cur, lobby } : cur));
      } catch (e) {
        if (e instanceof ApiError && (e.status === 410 || e.status === 404)) {
          setHosted(null);
          flash(t('toast.lobbyEnded'));
        }
      }
    };
    const timer = setInterval(beat, HEARTBEAT_MS);
    const first = setTimeout(beat, 4000);   // a first count soon after opening
    return () => {
      clearInterval(timer);
      clearTimeout(first);
    };
  }, [hosted?.lobby.id]);

  // The hosted lobby's free server: asked every 10 s while it is queued, starting or running. Running: the lobby is
  // read again so its card shows the address the server filled in (friends join with the invite link, as in V1).
  useEffect(() => {
    const id = hosted?.lobby.id;
    if (!id || !server || !SERVER_ACTIVE.includes(server.state)) return;
    const poll = async () => {
      const h = latest.current.hosted;
      if (!h || h.lobby.id !== id) return;
      let s: HostedServer;
      try {
        s = await getServer(id);
      } catch {
        return;
      }
      const before = latest.current.server?.state;
      setServer(s);
      if (s.worldUntil) void keepWorld(h, s);
      if (s.state === 'running' && before !== 'running') {
        flash(t('toast.serverReady'));
        getLobby(id).then((lobby) => setHosted((cur) => (cur && cur.lobby.id === id ? { ...cur, lobby } : cur)), () => {});
      }
      if (s.state === 'stopped' || s.state === 'failed') {
        flash(s.state === 'failed' ? t('toast.serverFailed') : t('toast.serverStopped'));
        getLobby(id).then((lobby) => setHosted((cur) => (cur && cur.lobby.id === id ? { ...cur, lobby } : cur)), () => {});
      }
      const left = s.expiresAt ? Date.parse(s.expiresAt) - Date.now() : Infinity;
      if (s.state === 'running' && left < SERVER_WARN_MS && warned.current !== id) {
        warned.current = id;
        flash(t('toast.serverStopsSoon', { count: Math.max(1, Math.round(left / 60_000)) }));
      }
    };
    const timer = setInterval(poll, SERVER_POLL_MS);
    const first = setTimeout(poll, 1500);
    return () => {
      clearInterval(timer);
      clearTimeout(first);
    };
  }, [hosted?.lobby.id, server?.state]);

  // Closing the app closes its lobby (best effort; without heartbeats it closes by itself within 90 s anyway).
  useEffect(() => {
    const bye = () => {
      const h = latest.current.hosted;
      if (h) void closeLobby(h).catch(() => {});
    };
    window.addEventListener('beforeunload', bye);
    return () => window.removeEventListener('beforeunload', bye);
  }, []);

  useEffect(() => {
    try {
      localStorage.setItem('installs', JSON.stringify(Object.fromEntries(Object.entries(installs).filter(([, v]) => v.phase === 'ready'))));
    } catch {}
  }, [installs]);

  useEffect(() => {
    const k = (e: KeyboardEvent) => {
      if ((e.ctrlKey || e.metaKey) && e.key.toLowerCase() === 'k') {
        e.preventDefault();
        search.current?.focus();
      }
      if (e.key === 'Escape') {
        if (picking !== null) setPicking(null);
        else if (joining) setJoining(null);
        else if (detail) setDetail(null);
      }
    };
    window.addEventListener('keydown', k);
    return () => window.removeEventListener('keydown', k);
  }, [picking, detail, joining]);

  // Browser preview (no Rust core): installs are simulated.
  useEffect(() => {
    const busy = Object.values(installs).some((i) => i.phase !== 'ready' && !i.real);
    if (!busy) return;
    const timer = setInterval(() => {
      setInstalls((all) => {
        const next = { ...all };
        for (const [id, i] of Object.entries(all)) {
          if (i.phase === 'ready' || i.real) continue;
          const m = catalog.find((c) => c.id === id);
          const speed = m ? 100 / Math.max(3, m.installSeconds / 4) : 10;
          let pct = i.pct + speed * (0.6 + Math.random() * 0.8);
          let phase: Phase = i.phase;
          if (pct >= 100) {
            pct = 0;
            phase = phase === 'download' ? 'verify' : phase === 'verify' ? 'install' : 'ready';
            if (phase === 'ready') flash(t('toast.ready', { name: m?.name ?? id }));
          }
          next[id] = { ...i, phase, pct: phase === 'ready' ? 100 : pct };
        }
        return next;
      });
    }, 250);
    return () => clearInterval(timer);
  }, [installs]);

  /** A catalog mashup by id; one the catalog no longer lists (taken down, other system) as a stub restore can use. */
  const mashupById = (id: string, name: string | null, like: Mashup): Mashup =>
    catalog.find((c) => c.id === id) ?? { ...like, id, name: name ?? id.replace(/^sigf\//, ''), conflicts: [], recipeUrl: undefined };

  /** Forgets an install in the UI (restored, failed). */
  const dropInstall = (id: string) =>
    setInstalls((a) => {
      const n = { ...a };
      delete n[id];
      return n;
    });

  /** Restore vanilla for one mashup; true when it is gone. Asks before restoring over game files changed since the install. */
  const restoreOne = async (m: Mashup): Promise<boolean> => {
    if (installs[m.id]?.real) {
      try {
        try {
          await restoreMashup(m.id);
        } catch (e) {
          if (!(isInstallError(e) && e.kind === 'tampered')) throw e;
          if (!confirm(t('confirm.tampered', { count: e.files.length }))) return false;
          await restoreMashup(m.id, true);
        }
      } catch (e) {
        const message = isInstallError(e) ? e.message : String(e);
        noteError(m.id, `Restore: ${message}`, 'restore');
        flash(installErrorText(e));
        return false;
      }
    }
    dropInstall(m.id);
    flash(t('toast.restored', { game: GAME[m.host]?.short ?? m.host }));
    return true;
  };

  /** Two mashups that change the same game files are never installed together: offer to restore the other one first. */
  const offerRestoreFirst = async (m: Mashup, other: Mashup) => {
    if (!confirm(t('confirm.restoreFirst', { name: m.name, other: other.name }))) return;
    if (await restoreOne(other)) await getMashup(m, true);
  };
  const flash = (s: string) => {
    setToast(s);
    setTimeout(() => setToast((t) => (t === s ? null : t)), 3200);
  };

  /** A prerequisite the player installs is missing (the recipe's `requires_files`): say so and offer its page. Nothing
   *  was installed or started. */
  const askPrerequisite = (message: string, page: string | undefined) => {
    if (page && /^https:\/\//.test(page)) {
      if (confirm(t('confirm.prerequisite', { message, page }))) void openUrl(page);
    } else flash(message);
  };

  /** Install and Play refuse with `missingFile` when a prerequisite is missing: true when `e` was one (handled). */
  const onMissingFile = (e: unknown) => {
    if (!isInstallError<PlayError>(e) || e.kind !== 'missingFile') return false;
    askPrerequisite(e.message, e.page);
    return true;
  };

  /** Keeps the host secret of a hosted lobby in the core's store (world downloads for 7 days, restarts included). */
  async function keepWorld(h: Hosted, s: HostedServer) {
    const until = s.worldUntil ? Math.floor(Date.parse(s.worldUntil) / 1000) : null;
    const known = worlds.find((w) => w.lobby === h.lobby.id);
    if (known && known.worldUntil === until) return;
    try {
      setWorlds(await hostedSave({
        lobby: h.lobby.id, secret: h.secret, mashupId: h.lobby.mashup.id, name: h.lobby.mashup.name,
        startedAt: Math.floor(Date.parse(s.startedAt ?? new Date().toISOString()) / 1000), worldUntil: until,
      }));
    } catch {}
  }

  /** Starts a free hosted server for the lobby this app hosts; a refusal (all servers busy...) stays on its card. */
  async function startHosted(h: Hosted, region: string) {
    setServerError(null);
    try {
      const s = await startServer(h, region);
      setServer(s);
      await keepWorld(h, s);
      const lobby = await getLobby(h.lobby.id).catch(() => null);
      if (lobby) setHosted((cur) => (cur && cur.lobby.id === lobby.id ? { ...cur, lobby } : cur));
    } catch (e) {
      const busy = e instanceof ApiError && e.code === 'servers_busy';
      setServerError(busy ? (e.position ? t('server.busyLine', { position: String(e.position) }) : t('lobbyApi.servers_busy')) : e instanceof Error ? e.message : String(e));
    }
  }

  const owned = useMemo(() => new Set((scan?.games ?? []).map((g) => g.canon).filter(Boolean) as string[]), [scan]);
  // Game id -> install folder from the scan: the core keeps the games of installed mashups and checks whether one runs.
  const gameDirs = useMemo(() => {
    const dirs: Record<string, string> = {};
    for (const g of scan?.games ?? []) if (g.canon && g.installDir && !dirs[g.canon]) dirs[g.canon] = g.installDir;
    return dirs;
  }, [scan]);

  function refreshInstalled() {
    return installedMods().then((list) =>
      setInstalls((a) => ({ ...a, ...Object.fromEntries(list.map((m) => [m.id, { phase: 'ready' as Phase, pct: 100, started: m.installedAt * 1000, real: true, version: m.version }])) })),
    ).catch(() => {});
  }

  /** Reads the live catalog, keeping the mashups that run on this system (a Mac never lists a Windows-only one); seed
   *  entries stay for layout (browser preview only) until the API has enough mashups. */
  function loadCatalog() {
    return Promise.all([getText(`${SITE}/api/app/catalog`), appPlatform()])
      .then(([text, platform]) => {
        const all = JSON.parse(text) as Partial<Mashup>[] as Mashup[];
        const live = all.filter((m) => runsOn(m, platform));
        setElsewhere(new Map(all.filter((m) => !runsOn(m, platform)).map((m) => [m.id, m.platforms ?? ['windows']])));
        const ids = new Set(live.map((m) => m.id));
        setCatalog([...live.map((m) => ({ ...m, steps: m.steps ?? [], strategy: m.strategy ?? "", installSeconds: m.installSeconds ?? 30, sizeMb: m.sizeMb ?? 0, needs: m.needs ?? [m.host] })), ...(inTauri ? [] : CATALOG.filter((m) => !ids.has(m.id)))]);
        catalogAt.current = Date.now();
      })
      .catch(() => {});
  }

  /** Refresh: the catalog, the installed mods and the game scan again. A click while one runs joins it. */
  function refreshAll() {
    refreshRun.current ??= (async () => {
      setRefreshing(true);
      try {
        await Promise.allSettled([loadCatalog(), refreshInstalled(), scanGames().then(setScan)]);
      } finally {
        setRefreshing(false);
        refreshRun.current = null;
      }
    })();
    return refreshRun.current;
  }

  // F5 / Ctrl+R refresh the data instead of reloading the webview; back on the window, a catalog older than 2 min is read again.
  useEffect(() => {
    const key = (e: KeyboardEvent) => {
      if (e.key === 'F5' || ((e.ctrlKey || e.metaKey) && e.key.toLowerCase() === 'r')) {
        e.preventDefault();
        if (!e.repeat) void refreshAll();
      }
    };
    const focus = () => {
      if (!refreshRun.current && Date.now() - catalogAt.current > 120_000) void loadCatalog();
    };
    window.addEventListener('keydown', key);
    window.addEventListener('focus', focus);
    return () => {
      window.removeEventListener('keydown', key);
      window.removeEventListener('focus', focus);
    };
  }, []);

  /**
   * From a lobby id to the game: read the lobby (metadata only) and check the library, then ask the player. Nothing is
   * downloaded, installed or launched before they click Install & join in the sheet (`confirmJoin`). A Join from a
   * lobby list skips the question only when the exact version is already installed.
   */
  const join = async (id: string, fromList = false) => {
    setView('together');
    setDetail(null);
    setJoining({ id, step: 'lobby' });
    const fail = (error: string, prism = false) => setJoining((j) => ({ ...(j ?? { id }), id, step: 'error', error, prism }));
    let lobby: Lobby;
    try {
      lobby = await getLobby(id);
    } catch (e) {
      return fail(isJoinError(e) ? joinErrorText(e) : e instanceof Error ? e.message : String(e));
    }
    setJoining({ id, step: 'lobby', lobby });
    // A cold start from a link: the library scan may still be running.
    for (let i = 0; i < 60 && !latest.current.scan; i++) await new Promise((r) => setTimeout(r, 250));
    const { scan: sc, owned: own } = latest.current;
    if (!sc) return fail(t('join.stillScanning'));
    const miss = lobby.games.filter((g) => !own.has(g));
    if (miss.length) return fail(t('join.needGames', { games: list(miss.map((g) => GAME[g]?.name ?? g)) }));
    if (lobby.state === 'full') return fail(t('err.join.lobbyFull'));
    if (lobby.state === 'waiting') return fail(lobby.server.provider === 'controller' ? t('join.serverStarting', { host: lobby.host }) : t('join.hostNotReady', { host: lobby.host }));
    const have = latest.current.installs[lobby.mashup.id];
    const install = !(have?.phase === 'ready' && have.real && have.version === lobby.mashup.version);
    if (!fromList || install) return setJoining({ id, step: 'confirm', lobby, install });
    await confirmJoin(id, lobby);
  };

  /**
   * Bring your own copy: true when every copy the recipe needs is on this PC (found by the core) or the player picked
   * it in the sheet; false when they cancel. A recipe without own copies is ready at once.
   */
  const ownCopiesReady = async (m: Mashup, text: string): Promise<boolean> => {
    let needs: unknown;
    try {
      needs = (JSON.parse(text) as { own_copies?: unknown }).own_copies;
    } catch {
      return true; // the core refuses a broken recipe with its own error
    }
    if (!Array.isArray(needs) || needs.length === 0) return true;
    const missing: OwnFound[] = (await findOwnCopies(text)).filter((f) => !f.found);
    if (!missing.length) return true;
    return new Promise<boolean>((resolve) => setOwnAsk({ m, recipe: text, missing, resolve }));
  };

  /** The player said yes: the core installs the pinned version if needed, then launches with the address they saw. */
  const confirmJoin = async (id: string, lobby: Lobby) => {
    const fail = (error: string, prism = false) => setJoining((j) => ({ ...(j ?? { id }), id, step: 'error', error, prism }));
    const sc = latest.current.scan;
    if (!sc) return fail(t('join.stillScanning'));
    setJoining({ id, step: 'lobby', lobby });
    if (!inTauri) {
      setJoining({ id, step: 'install', lobby });
      await new Promise((r) => setTimeout(r, 1200));
      setJoining({ id, step: 'launch', lobby });
    }
    try {
      await joinLobby(id, sc.games, lobby);
      setJoining((j) => (j && j.id === id ? { ...j, step: 'done' } : j));
      refreshInstalled();
    } catch (e) {
      const prism = isJoinError(e) && e.kind === 'needsLauncher';
      fail(prism ? t('join.needsPrism') : isJoinError(e) ? joinErrorText(e) : String(e), prism);
    }
  };

  latest.current = { scan, owned, installs, hosted, server, join };

  /** Get: fetch the recipe, check own copies, install. `afterRestore`: the conflicting mashup was just restored (the
   *  installs state above is not updated yet; the core checks the pair again anyway). */
  const getMashup = async (m: Mashup, afterRestore = false) => {
    const clash = afterRestore ? null : installedConflict(m, (id) => installs[id]?.phase === 'ready');
    if (clash) return offerRestoreFirst(m, mashupById(clash, null, m));
    if (!inTauri) {
      setInstalls((a) => ({ ...a, [m.id]: { phase: 'download', pct: 0, started: Date.now() } }));
      return;
    }
    if (!m.recipeUrl) return flash(t('toast.notInCatalog', { name: m.name }));
    // The catalog serves recipeUrl relative to the site (/api/app/recipe/<id>@<version>).
    let text: string;
    try {
      text = await getText(new URL(m.recipeUrl, SITE).href);
      // A mashup on the player's own copy (a ROM): found on this PC or picked, before anything is downloaded.
      if (!(await ownCopiesReady(m, text))) return;
    } catch (e) {
      noteError(m.id, installErrorText(e, { loc: 'en' }), 'install');
      return flash(installErrorText(e));
    }
    setInstalls((a) => ({ ...a, [m.id]: { phase: 'download', pct: 0, started: Date.now(), real: true } }));
    try {
      const dirs: Record<string, string> = {};
      for (const g of scan?.games ?? []) if (g.canon && g.installDir && !dirs[g.canon]) dirs[g.canon] = g.installDir;
      const done = await installMashup(text, dirs);
      setInstalls((a) => ({ ...a, [m.id]: { phase: 'ready', pct: 100, started: Date.now(), real: true, version: done?.version ?? m.version } }));
      flash(t('toast.ready', { name: m.name }));
    } catch (e) {
      dropInstall(m.id);
      if (isInstallError(e) && e.kind === 'conflict') return offerRestoreFirst(m, mashupById(e.with, e.withName, m));
      if (onMissingFile(e)) return;
      noteError(m.id, installErrorText(e, { loc: 'en' }), 'install');
      flash(installErrorText(e));
      if (isInstallError(e) && e.kind === 'needsLauncher') void appPlatform().then((p) => openUrl(prismDownload(p)));
    }
  };

  const ctxInstalls = () => installs;
  const ctx: Ctx = {
    scan,
    catalog,
    elsewhere,
    owned,
    installs,
    pair,
    setPair,
    pick: (slot) => setPicking(slot),
    open: setDetail,
    get: (m) => void getMashup(m),
    play: async (m) => {
      if (ctxInstalls()[m.id]?.real) {
        try {
          await playInstalled(m.id, scan?.games ?? []);
        } catch (e) {
          const message = isInstallError<PlayError>(e) ? e.message : String(e);
          noteError(m.id, `Play: ${message}`, 'play');
          if (onMissingFile(e)) return;
          return flash(playErrorText(message));
        }
      } else {
        const g = scan?.games.find((x) => x.canon === m.host);
        if (g?.launch) launchGame(g.launch);
      }
      flash(m.kind === 'passthrough' ? t('toast.launchingPair', { name: m.name, first: GAME[m.guest!]?.short ?? m.guest!, then: GAME[m.host]?.short ?? m.host }) : t('toast.launching', { name: m.name }));
    },
    restore: (m) => void restoreOne(m),
    conflictOf: (m) => {
      const id = installedConflict(m, (x) => installs[x]?.phase === 'ready');
      return id ? mashupById(id, null, m) : null;
    },
    restoreThenGet: (m, other) => void restoreOne(other).then((ok) => { if (ok) return getMashup(m, true); }),
    agents,
    go: setView,
    sideload: async (text) => {
      let r: { id: string; name: string; tagline?: string; kind?: Mashup['kind']; games?: { game: string; role: string }[] };
      try {
        r = JSON.parse(text);
      } catch {
        return flash(t('toast.badRecipe'));
      }
      const host = r.games?.find((g) => g.role === 'host')?.game ?? r.games?.[0]?.game ?? 'unknown';
      const guest = r.games?.find((g) => g.role === 'guest')?.game;
      const m: Mashup = {
        id: r.id, name: r.name, tagline: r.tagline ?? t('sideload.tagline'), kind: r.kind ?? 'mod', host, guest,
        needs: (r.games ?? []).map((g) => g.game), by: { name: t('sideload.by') }, license: '', sizeMb: 0, installSeconds: 0,
        strategy: t('sideload.strategy'), steps: [], updated: '', recipeUrl: 'local',
      };
      setCatalog((c) => [m, ...c.filter((x) => x.id !== m.id)]);
      setInstalls((a) => ({ ...a, [m.id]: { phase: 'download', pct: 0, started: Date.now(), real: true } }));
      try {
        const dirs: Record<string, string> = {};
        for (const g of scan?.games ?? []) if (g.canon && g.installDir && !dirs[g.canon]) dirs[g.canon] = g.installDir;
        await installMashup(text, dirs);
        setInstalls((a) => ({ ...a, [m.id]: { phase: 'ready', pct: 100, started: Date.now(), real: true } }));
        flash(t('toast.installed', { name: m.name }));
      } catch (e) {
        dropInstall(m.id);
        flash(installErrorText(e, { asSent: true }));
      }
    },
    hosted,
    host: async (m, o) => {
      try {
        const made = await createLobby({ mashup: `${m.id}@${m.version ?? '1.0.0'}`, host: o.name, mode: o.mode, maxPlayers: o.maxPlayers, targets: o.region ? [] : o.targets });
        // The browser sample does not know the mashup's name or games: the card does.
        const h = { ...made, lobby: { ...made.lobby, mashup: { ...made.lobby.mashup, name: made.lobby.mashup.name || m.name }, games: made.lobby.games.length ? made.lobby.games : m.needs } };
        setServer(null);
        setServerError(null);
        setHosted(h);
        if (o.region) {
          flash(t('toast.lobbyOpenServer'));
          await startHosted(h, o.region);
        } else flash(o.mode === 'public' ? t('toast.lobbyOpenPublic') : t('toast.inviteReady'));
      } catch (e) {
        flash(e instanceof Error ? e.message : String(e));
      }
    },
    hostUpdate: (players) => {
      const h = latest.current.hosted;
      if (!h) return;
      setHosted({ ...h, lobby: { ...h.lobby, players } });
      heartbeat(h, players)
        .then((lobby) => setHosted((cur) => (cur && cur.lobby.id === lobby.id ? { ...cur, lobby } : cur)))
        .catch(() => {});
    },
    closeHost: () => {
      const h = latest.current.hosted;
      setHosted(null);
      setServer(null);
      setServerError(null);
      if (h) closeLobby(h).then(() => flash(t('toast.lobbyClosed')), () => flash(t('toast.lobbyClosedLocal')));
    },
    hosting,
    server,
    serverError,
    startServer: async (region) => {
      const h = latest.current.hosted;
      if (h) await startHosted(h, region);
    },
    stopServer: async () => {
      const h = latest.current.hosted;
      if (!h) return;
      try {
        const s = await stopServer(h.lobby.id, h.secret);
        setServer(s);
        await keepWorld(h, s);
        const lobby = await getLobby(h.lobby.id).catch(() => null);
        if (lobby) setHosted((cur) => (cur && cur.lobby.id === lobby.id ? { ...cur, lobby } : cur));
        flash(t('toast.serverStoppedByYou'));
      } catch (e) {
        flash(e instanceof Error ? e.message : String(e));
      }
    },
    worlds,
    downloadWorld: async (lobby) => {
      const w = worlds.find((x) => x.lobby === lobby) ?? (hosted?.lobby.id === lobby ? { secret: hosted.secret } : null);
      if (!w) return flash(t('toast.worldGone'));
      try {
        const link = await worldLink(lobby, w.secret);
        await openUrl(link.url);
        flash(t('toast.worldDownloading'));
      } catch (e) {
        flash(e instanceof Error ? e.message : String(e));
      }
    },
    forgetWorld: (lobby) => {
      hostedForget(lobby).then(setWorlds, () => {});
    },
    join: (id, fromList) => void join(id, fromList),
    report: (m) => setReporting({ m }),
  };

  const active = Object.values(installs).filter((i) => i.phase !== 'ready');
  const avg = active.length ? active.reduce((s, i) => s + (i.phase === 'download' ? i.pct * 0.7 : i.phase === 'verify' ? 70 + i.pct * 0.1 : 80 + i.pct * 0.2), 0) / active.length : 0;

  return (
    <div className="shell">
      <header className="titlebar" data-tauri-drag-region>
        <div className="brand" data-tauri-drag-region>
          <span className="wordmark">SIGF</span>
          <span className="tag">{t('app.tagline')}</span>
        </div>
        <label className="search">
          <Icon name="search" size={15} />
          <input ref={search} value={query} onChange={(e) => setQuery(e.target.value)} placeholder={t('app.search')} onFocus={() => setView('mix')} />
          <kbd>Ctrl K</kbd>
        </label>
        <div className="stores" data-tauri-drag-region>
          {(['steam', 'epic', 'ubisoft', 'gog', 'minecraft'] as const).map((s) => {
            const on = scan?.stores.includes(s);
            const n = scan?.games.filter((g) => g.store === s).length ?? 0;
            return (
              <span key={s} className={`store ${on ? 'on' : ''}`} title={on ? t('app.storeGames', { store: STORE_LABEL[s], count: n }) : t('app.storeMissing', { store: STORE_LABEL[s] })}>
                <i />
                {STORE_LABEL[s]}
              </span>
            );
          })}
        </div>
        <button className={`refresh ${refreshing ? 'spin' : ''}`} onClick={() => void refreshAll()} disabled={refreshing} title={t('app.refresh')} aria-label={t('app.refresh')}>
          <Icon name="refresh" size={15} />
        </button>
        <div className="winctl">
          <button onClick={() => windowAction('minimize')} aria-label={t('app.minimize')}><Icon name="min" size={14} /></button>
          <button onClick={() => windowAction('toggleMaximize')} aria-label={t('app.maximize')}><Icon name="max" size={12} /></button>
          <button
            className="close"
            onClick={async () => {
              if (hosted) await closeLobby(hosted).catch(() => {});
              windowAction('close');
            }}
            aria-label={t('common.close')}
          ><Icon name="x" size={14} /></button>
        </div>
      </header>

      <nav className="rail">
        {NAV.map((n) => (
          <button key={n.id} className={view === n.id ? 'on' : ''} onClick={() => setView(n.id)}>
            <Icon name={n.icon} size={22} />
            <span>{t(n.label)}</span>
            {n.id === 'queue' && active.length > 0 && (
              <svg className="ring" viewBox="0 0 36 36"><circle cx="18" cy="18" r="16" pathLength="100" strokeDasharray={`${avg} 100`} /></svg>
            )}
          </button>
        ))}
        <div className="rail-foot">
          <span className="count">{scan ? scan.games.length : '…'}</span>
          <span>{tx('app.gamesFound', { br: <br /> })}</span>
          <button className="rail-privacy" onClick={() => setPrivacyOpen(true)} title={t('privacy.titleSheet')}>
            <Icon name="shield" size={16} />
            <span>{t('privacy.eyebrow')}</span>
          </button>
        </div>
      </nav>

      <main className="main" key={view}>
        {view === 'mix' && <Home ctx={ctx} query={query} />}
        {view === 'library' && <Library ctx={ctx} />}
        {view === 'live' && <Live />}
        {view === 'together' && <Lobbies ctx={ctx} />}
        {view === 'build' && <Build ctx={ctx} />}
        {view === 'queue' && <Queue ctx={ctx} />}
      </main>

      {detail && <Detail ctx={ctx} m={detail} onClose={() => setDetail(null)} />}
      {picking !== null && (
        <Picker
          ctx={ctx}
          slot={picking}
          onPick={(id) => {
            const p: [string | null, string | null] = [...pair];
            p[picking] = id;
            if (p[0] && p[0] === p[1]) p[picking === 0 ? 1 : 0] = null;
            setPair(p);
            setPicking(null);
          }}
          onClose={() => setPicking(null)}
        />
      )}
      {ownAsk && <OwnCopySheet ask={ownAsk} onDone={(ok) => { setOwnAsk(null); ownAsk.resolve(ok); }} />}
      {joining && <JoinSheet ctx={ctx} j={joining} onClose={() => setJoining(null)} onConfirm={(l) => void confirmJoin(joining.id, l)} />}
      {privacy && !privacy.asked && <PrivacyPanel first initial={privacy} onDone={() => {}} />}
      {privacy?.asked && privacyOpen && <PrivacyPanel initial={privacy} onDone={() => { setPrivacyOpen(false); flash(t('toast.privacySaved')); }} onClose={() => setPrivacyOpen(false)} onReport={() => { setPrivacyOpen(false); setReporting({ m: null }); }} />}
      {reporting && (
        <ReportSheet
          ctx={ctx}
          m={reporting.m}
          lastError={(reporting.m ? lastErrors.current[reporting.m.id] : lastErrors.current['']) ?? null}
          onClose={() => setReporting(null)}
          flash={flash}
        />
      )}
      {update && answered && updateLater !== update.version && (
        <UpdateBanner
          update={update}
          gameDirs={gameDirs}
          beforeInstall={async () => {
            const h = latest.current.hosted;
            if (h) await closeLobby(h).catch(() => {});
          }}
          onLater={() => setUpdateLater(update.version)}
        />
      )}
      {toast && <div className="toast" key={toast}><Icon name="check" size={16} />{toast}</div>}
    </div>
  );
}
