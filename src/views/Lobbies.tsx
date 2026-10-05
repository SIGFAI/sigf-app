// Multiplayer (docs/RECIPE-FORMAT.md section 9): the Lobbies page, the "Play with friends" panel of a mashup, and
// the join sheet that walks a player from an invite link to the game.
import { useEffect, useState } from 'react';
import type { Ctx } from '../App';
import type { Mashup } from '../data/catalog';
import { GAME } from '../data/games';
import { inTauri, openUrl } from '../lib/api';
import { CONNECT_GAMES, DEFAULT_PORT, SERVER_ACTIVE, lanAddress, listLobbies, nearestRegion, parseInvite, validAddress, type HostedServer, type Lobby, type PublicLobby, type Target } from '../lib/lobbies';
import { Icon, MashupCover } from '../ui';
import { usePrivacy } from '../lib/privacy';
import { Section, gameName } from './shared';

const missingOf = (ctx: Ctx, games: string[]) => games.filter((g) => !ctx.owned.has(g));

/** Copies to the clipboard; true when it worked. */
async function copy(text: string) {
  try {
    await navigator.clipboard.writeText(text);
    return true;
  } catch {
    return false;
  }
}

function Players({ n, max, state }: { n: number; max: number; state: PublicLobby['state'] }) {
  return (
    <span className={`players players-${state}`} title={`${n} of ${max} players`}>
      <i style={{ ['--p' as string]: `${Math.min(100, (n / Math.max(1, max)) * 100)}%` }} />
      <b>{n}</b>/{max}
    </span>
  );
}

function LobbyRow({ ctx, l, i }: { ctx: Ctx; l: PublicLobby; i: number }) {
  const miss = missingOf(ctx, l.games);
  const [host, guest] = [l.games[0], l.games[1]];
  const known = ctx.catalog.find((m) => m.id === l.mashup.id);
  return (
    <div className="lobby" style={{ ['--i' as string]: i }}>
      <MashupCover host={known?.host ?? host} guest={known?.guest ?? guest} className="qcover" />
      <div className="qinfo">
        <b>{l.mashup.name}</b>
        <span>
          hosted by {l.host} · {l.games.map(gameName).join(' + ')} · v{l.mashup.version}
        </span>
      </div>
      <Players n={l.players} max={l.maxPlayers} state={l.state} />
      {miss.length ? (
        <button className="act act-miss" onClick={() => known && ctx.open(known)}>Needs {miss.map(gameName).join(' + ')}</button>
      ) : l.state === 'open' ? (
        <button className="act act-get" onClick={() => ctx.join(l.id, true)}>Join</button>
      ) : (
        <button className="act act-busy" disabled>{l.state === 'full' ? 'Full' : 'Starting'}</button>
      )}
    </div>
  );
}

/** Public lobbies, refreshed every 10 s while shown. */
function useLobbies(ctx: Ctx, mashup?: string) {
  const [list, setList] = useState<PublicLobby[] | null>(null);
  const [error, setError] = useState<string | null>(null);
  const owned = [...ctx.owned].sort().join(',');
  const sendGames = usePrivacy()?.lobbyGames ?? false;
  useEffect(() => {
    let live = true;
    const load = () =>
      listLobbies(ctx.owned, mashup, sendGames)
        .then((l) => live && (setList(l), setError(null)))
        .catch((e) => live && setError(e instanceof Error ? e.message : String(e)));
    load();
    const t = setInterval(load, 10_000);
    return () => {
      live = false;
      clearInterval(t);
    };
  }, [owned, mashup, sendGames]);
  return { list, error };
}

export function Lobbies({ ctx }: { ctx: Ctx }) {
  const { list, error } = useLobbies(ctx);
  const [link, setLink] = useState('');
  const id = parseInvite(link);
  const players = (list ?? []).reduce((s, l) => s + l.players, 0);

  return (
    <div className="page">
      <section className="together-hero">
        <span className="eyebrow">Play together</span>
        <h1>
          Your mashup. <span className="chrome">Your friends in it.</span>
        </h1>
        <p>Open a lobby from any mashup you play, send the link, and everyone lands in the same world on the exact same version. Joining installs it first if needed.</p>
        <form
          className="invite-field"
          onSubmit={(e) => {
            e.preventDefault();
            if (id) ctx.join(id);
          }}
        >
          <Icon name="link" size={16} />
          <input value={link} onChange={(e) => setLink(e.target.value)} placeholder="Paste an invite link: sigf://join/… or sigf.ai/join/…" spellCheck={false} />
          <button className="act act-get" disabled={!id}>Join</button>
        </form>
      </section>

      {ctx.hosted && (
        <Section title="Your lobby" sub="Open while the app runs. Friends join with the link.">
          <HostCard ctx={ctx} />
        </Section>
      )}

      <HostedWorlds ctx={ctx} />

      <Section
        title="Public lobbies"
        sub="Mashups you can play right now, hosted by other players. Only names and player counts are public."
        aside={list && <span className="live-pill"><Icon name="live" size={10} /> {list.length} open · {players} playing</span>}
      >
        {error && <div className="empty">Lobbies are not reachable right now. {error}</div>}
        {!error && list === null && <div className="empty">Looking for lobbies…</div>}
        {!error && list?.length === 0 && <div className="empty">No public lobby for your games right now. Open one from any mashup: Play with friends.</div>}
        <div className="queue">{list?.map((l, i) => <LobbyRow key={l.id} ctx={ctx} l={l} i={i} />)}</div>
      </Section>
    </div>
  );
}

/** "7 h 42 min", "12 min", "under a minute". */
function timeLeft(ms: number) {
  if (ms < 60_000) return 'under a minute';
  const min = Math.floor(ms / 60_000);
  return min >= 60 ? `${Math.floor(min / 60)} h ${String(min % 60).padStart(2, '0')} min` : `${min} min`;
}
const day = (iso: string | number) => new Date(iso).toLocaleDateString(undefined, { month: 'short', day: 'numeric' });

/** Re-renders every `ms` while mounted (countdowns). */
function useTick(ms: number) {
  const [, set] = useState(0);
  useEffect(() => {
    const t = setInterval(() => set((n) => n + 1), ms);
    return () => clearInterval(t);
  }, [ms]);
}

/** The free hosted server of the lobby this app hosts: starting -> ready, players, time left, stop, world. */
function ServerPanel({ ctx, s }: { ctx: Ctx; s: HostedServer }) {
  useTick(15_000);
  const [busy, setBusy] = useState(false);
  const left = s.expiresAt ? Date.parse(s.expiresAt) - Date.now() : null;
  const warn = s.state === 'running' && left !== null && left < 15 * 60_000;
  const region = ctx.hosting?.regions.find((r) => r.id === s.region)?.label ?? s.region;
  const max = s.maxPlayers ?? ctx.hosting?.limits.maxPlayers ?? 10;
  const title =
    s.state === 'queued' ? `In line for a free server${s.position ? ` · #${s.position}` : ''}`
    : s.state === 'starting' ? `Starting your server${s.etaS ? ` · about ${Math.max(1, Math.round(s.etaS / 60))} min` : ''}`
    : s.state === 'running' ? 'Server ready'
    : s.state === 'failed' ? 'The server stopped unexpectedly'
    : 'Server stopped';
  const act = async (f: () => Promise<void>) => {
    setBusy(true);
    await f();
    setBusy(false);
  };
  return (
    <div className={`srv srv-${s.state}${warn ? ' srv-warn' : ''}`}>
      <div className="srv-top">
        <span className="srv-icon"><Icon name="server" size={16} /></span>
        <div>
          <b>{title}</b>
          <small>Hosted on SIGF · {region} · free</small>
        </div>
        {s.state === 'running' && <Players n={s.players ?? 0} max={max} state={(s.players ?? 0) >= max ? 'full' : 'open'} />}
      </div>
      {(s.state === 'queued' || s.state === 'starting') && <i className="srv-bar" />}
      {s.state === 'running' && (
        <p className="host-hint">
          The server address is in the lobby: friends join with the invite link, nothing to type.{' '}
          {left !== null && (
            <span className={`srv-left ${warn ? 'on' : ''}`}>
              <Icon name="clock" size={12} /> {warn ? `Stops in ${timeLeft(left)}: download the world to keep it` : `${timeLeft(left)} left of ${ctx.hosting?.limits.hours ?? 8} h`}
            </span>
          )}
        </p>
      )}
      {(s.state === 'stopped' || s.state === 'failed') && s.worldUntil && <p className="host-hint">Your world is kept until {day(s.worldUntil)}.</p>}
      <div className="host-actions">
        {SERVER_ACTIVE.includes(s.state) ? (
          <button className="act act-ghost" disabled={busy} onClick={() => act(ctx.stopServer)}>
            <Icon name="x" size={14} /> Stop server
          </button>
        ) : (
          <button className="act act-ghost" disabled={busy} onClick={() => act(() => ctx.startServer(s.region ?? 'eu-west-1'))}>
            <Icon name="server" size={14} /> Start again
          </button>
        )}
        {s.state !== 'queued' && (
          <button className="act act-ghost" disabled={busy} onClick={() => act(() => ctx.downloadWorld(s.lobby ?? ctx.hosted!.lobby.id))}>
            <Icon name="download" size={14} /> Download world
          </button>
        )}
      </div>
    </div>
  );
}

/** Worlds of free servers this app ran, downloadable for 7 days after the session (secrets kept by the core). */
export function HostedWorlds({ ctx }: { ctx: Ctx }) {
  const list = ctx.worlds.filter((w) => w.lobby !== ctx.hosted?.lobby.id || !ctx.server || !SERVER_ACTIVE.includes(ctx.server.state));
  if (!list.length) return null;
  return (
    <Section title="Your hosted worlds" sub="Worlds from your free servers, kept 7 days after the session. Download one to keep it.">
      <div className="queue">
        {list.map((w, i) => {
          const known = ctx.catalog.find((m) => m.id === w.mashupId);
          return (
            <div key={w.lobby} className="lobby" style={{ ['--i' as string]: i }}>
              <MashupCover host={known?.host ?? 'minecraft'} guest={known?.guest} cover={known?.cover} className="qcover" />
              <div className="qinfo">
                <b>{w.name || known?.name || w.mashupId}</b>
                <span>
                  hosted {day(w.startedAt * 1000)}
                  {w.worldUntil ? ` · kept until ${day(w.worldUntil * 1000)}` : ''}
                </span>
              </div>
              <button className="act act-get" onClick={() => ctx.downloadWorld(w.lobby)}>
                <Icon name="download" size={14} /> Download world
              </button>
              <button className="act act-ghost" onClick={() => ctx.forgetWorld(w.lobby)} title="Forget this world here">
                <Icon name="x" size={14} />
              </button>
            </div>
          );
        })}
      </div>
    </Section>
  );
}

/** The lobby this app hosts: link, live count, its free server if any, close. */
function HostCard({ ctx }: { ctx: Ctx }) {
  const h = ctx.hosted!;
  const l = h.lobby;
  const [copied, setCopied] = useState(false);
  const mc = l.targets.some((t) => t.game === 'minecraft');
  const srv = ctx.server && ctx.server.state !== 'none' ? ctx.server : null;
  return (
    <div className="host-card">
      <div className="host-top">
        <div>
          <span className={`state-dot state-${l.state}`} />
          <b>{l.mashup.name}</b>
          <small>{l.mode === 'public' ? 'Public lobby' : 'Invite only'} · v{l.mashup.version}</small>
        </div>
        <Players n={l.players} max={l.maxPlayers} state={l.state} />
      </div>
      <div className="invite-row">
        <code>{l.url.replace(/^https:\/\//, '')}</code>
        <button
          className="act act-get"
          onClick={async () => {
            if (await copy(l.url)) {
              setCopied(true);
              setTimeout(() => setCopied(false), 1800);
            }
          }}
        >
          <Icon name={copied ? 'check' : 'copy'} size={14} /> {copied ? 'Copied' : 'Copy invite'}
        </button>
      </div>
      {srv && <ServerPanel ctx={ctx} s={srv} />}
      {ctx.serverError && (
        <div className="join-error">
          <span>{ctx.serverError}</span>
          <div className="host-actions">
            <button className="act act-get" onClick={() => ctx.startServer(srv?.region ?? (ctx.hosting ? nearestRegion(ctx.hosting) : 'eu-west-1'))}>Try again</button>
          </div>
        </div>
      )}
      {!srv && !ctx.serverError && l.state === 'waiting' && <p className="host-hint">Waiting for your game. {mc ? 'Start your world, then type /publish true survival 25565 in chat.' : 'Start the game and open your server.'}</p>}
      {!mc && !srv && (
        <div className="stepper" title="Players in your game (counted automatically for Minecraft)">
          <span>Players</span>
          <button onClick={() => ctx.hostUpdate(Math.max(1, l.players - 1))} aria-label="One less">−</button>
          <b>{l.players}</b>
          <button onClick={() => ctx.hostUpdate(Math.min(l.maxPlayers, l.players + 1))} aria-label="One more">+</button>
        </div>
      )}
      <div className="host-actions">
        <button className="act act-ghost" onClick={() => ctx.closeHost()} title={srv && SERVER_ACTIVE.includes(srv.state) ? 'Also stops your free server (the world is kept 7 days)' : undefined}>
          <Icon name="x" size={14} /> Close lobby
        </button>
      </div>
    </div>
  );
}

const NAME_KEY = 'lobby-name';
const loadName = () => {
  try {
    return localStorage.getItem(NAME_KEY) ?? '';
  } catch {
    return '';
  }
};

/** The games of a mashup that need a join address: Minecraft and connect engines, else the host (its mod's own networking). */
const addressGames = (m: Mashup) => {
  const sides = [...new Set([...m.needs, ...(m.kind === 'passthrough' && m.guest ? [m.guest] : [])])];
  const net = sides.filter((g) => g === 'minecraft' || CONNECT_GAMES.has(g));
  return net.length ? net : [m.host];
};

/** "Play with friends" on a mashup: open a lobby (invite or public), or join one of its public lobbies. */
export function PlayTogether({ ctx, m }: { ctx: Ctx; m: Mashup }) {
  const inst = ctx.installs[m.id];
  const ready = inst?.phase === 'ready';
  const mine = ctx.hosted?.lobby.mashup.id === m.id;
  const [open, setOpen] = useState(false);
  const [mode, setMode] = useState<'invite' | 'public'>('invite');
  const [max, setMax] = useState(8);
  const [name, setName] = useState(loadName);
  const [addr, setAddr] = useState<Record<string, string>>({});
  const [busy, setBusy] = useState(false);
  const canHost = !!m.server && !!ctx.hosting;
  const [where, setWhere] = useState<'own' | 'sigf'>('own');
  const [region, setRegion] = useState(() => (ctx.hosting ? nearestRegion(ctx.hosting) : 'eu-west-1'));
  const onSigf = canHost && where === 'sigf';
  const cap = m.server?.maxPlayers ?? ctx.hosting?.limits.maxPlayers ?? 10;
  const games = addressGames(m);
  const { list } = useLobbies(ctx, m.id);
  // The LAN address goes into the lobby only when the player puts it there (privacy choice "lanAddress": ask), or
  // is filled in by itself when they chose that (auto).
  const autoLan = usePrivacy()?.lanAddress === 'auto';
  const [lan, setLan] = useState<string | null>(null);
  const fillLan = async () => {
    const ip = lan ?? (await lanAddress());
    setLan(ip);
    setAddr((a) => Object.fromEntries(games.map((g) => [g, a[g]?.trim() ? a[g] : `${ip}:${DEFAULT_PORT(g)}`])));
  };
  const others = (list ?? []).filter((l) => l.id !== ctx.hosted?.lobby.id);

  useEffect(() => {
    if (!open || !autoLan) return;
    void fillLan();
  }, [open, autoLan]);

  const outdated = ready && inst.version && m.version && inst.version !== m.version;
  const nameOk = /^[\p{L}\p{N} _.'-]{1,32}$/u.test(name.trim());
  const addrOk = onSigf || games.every((g) => validAddress(addr[g] ?? ''));

  return (
    <div className="together">
      <h4>Play with friends</h4>
      {mine ? (
        <HostCard ctx={ctx} />
      ) : inTauri && (!m.version || !m.recipeUrl) ? (
        <p className="host-hint">Multiplayer opens once this mashup is in the live catalog.</p>
      ) : !ready ? (
        <p className="host-hint">Get it first: you host from your own game.</p>
      ) : outdated ? (
        <p className="host-hint">
          You have v{inst.version}; lobbies run v{m.version}. <a onClick={() => ctx.get(m)}>Update</a> to host.
        </p>
      ) : !open ? (
        <button className="act act-ghost together-open" onClick={() => setOpen(true)} disabled={!!ctx.hosted} title={ctx.hosted ? 'Close your other lobby first' : undefined}>
          <Icon name="people" size={15} /> Open a lobby
        </button>
      ) : (
        <form
          className="host-form"
          onSubmit={async (e) => {
            e.preventDefault();
            if (!nameOk || !addrOk || busy) return;
            try {
              localStorage.setItem(NAME_KEY, name.trim());
            } catch {}
            setBusy(true);
            const targets: Target[] = onSigf ? [] : games.map((g) => ({ game: g, address: addr[g].trim() }));
            await ctx.host(m, { mode, maxPlayers: onSigf ? Math.min(max, cap) : max, name: name.trim(), targets, region: onSigf ? region : undefined });
            setBusy(false);
            setOpen(false);
          }}
        >
          {canHost && (
            <div className="where">
              <button type="button" className={where === 'own' ? 'on' : ''} onClick={() => setWhere('own')}>
                <b>Use my own game</b>
                <small>Your world is the server. Unlimited, you keep it running.</small>
              </button>
              <button type="button" className={where === 'sigf' ? 'on' : ''} onClick={() => { setWhere('sigf'); setMax((v) => Math.min(v, cap)); }}>
                <b><Icon name="server" size={13} /> Host on SIGF <em>free</em></b>
                <small>A server online in about a minute. Up to {cap} players, {ctx.hosting?.limits.hours ?? 8} h, world kept {ctx.hosting?.limits.worldDays ?? 7} days.</small>
              </button>
            </div>
          )}
          <div className="seg">
            <button type="button" className={mode === 'invite' ? 'on' : ''} onClick={() => setMode('invite')}>Invite link</button>
            <button type="button" className={mode === 'public' ? 'on' : ''} onClick={() => setMode('public')}>Public lobby</button>
          </div>
          <label className="field">
            <small>Your name</small>
            <input value={name} onChange={(e) => setName(e.target.value)} maxLength={32} placeholder="What your friends see" />
          </label>
          <label className="field">
            <small>Players</small>
            <input type="range" min={2} max={onSigf ? cap : m.needs.includes('minecraft') || m.guest === 'minecraft' ? 100 : 32} value={onSigf ? Math.min(max, cap) : max} onChange={(e) => setMax(Number(e.target.value))} />
            <b className="field-val">{onSigf ? Math.min(max, cap) : max}</b>
          </label>
          {onSigf && ctx.hosting && (
            <div className="field">
              <small>Region</small>
              <div className="seg">
                {ctx.hosting.regions.map((r) => (
                  <button key={r.id} type="button" className={region === r.id ? 'on' : ''} disabled={!r.available} title={r.available ? undefined : 'Opens tomorrow'} onClick={() => setRegion(r.id)}>
                    {r.label}{r.available ? '' : ' · tomorrow'}
                  </button>
                ))}
              </div>
            </div>
          )}
          {!onSigf && games.map((g) => (
            <label key={g} className={`field ${addr[g] && !validAddress(addr[g]) ? 'bad' : ''}`}>
              <small>{GAME[g]?.short ?? g} address</small>
              <input value={addr[g] ?? ''} onChange={(e) => setAddr((a) => ({ ...a, [g]: e.target.value }))} spellCheck={false} placeholder="host:port" />
            </label>
          ))}
          {!onSigf && !games.every((g) => addr[g]?.trim()) && (
            <p className="host-hint">
              <button type="button" className="act act-ghost" onClick={() => void fillLan()}>
                <Icon name="link" size={13} /> Use my LAN address
              </button>{' '}
              Fills in this PC's local network address. It is shared with everyone who has the invite link. Or type a tunnel or public address.
            </p>
          )}
          <p className="host-hint">
            {onSigf ? 'SIGF starts a Minecraft server with this exact version. Its address goes into the lobby by itself: friends join with the invite link, nothing to forward. The server runs while this lobby is open.'
              : games.includes('minecraft')
              ? 'Your world is the server: start it, then type /publish true survival 25565 in chat. Friends outside your network need a forwarded port or a tunnel address (playit.gg) instead of a LAN address.'
              : 'Your game is the server: start it and open it to others. Friends outside your network need a forwarded port or a tunnel address.'}{' '}
            The address goes only to people with the link, never on a public list.
          </p>
          <div className="host-actions">
            <button type="button" className="act act-ghost" onClick={() => setOpen(false)}>Cancel</button>
            <button className="act act-get" disabled={!nameOk || !addrOk || busy}>
              {busy ? 'Opening…' : onSigf ? 'Start free server' : mode === 'public' ? 'Open public lobby' : 'Create invite'}
            </button>
          </div>
        </form>
      )}
      {others.length > 0 && (
        <div className="queue together-list">
          {others.slice(0, 4).map((l, i) => <LobbyRow key={l.id} ctx={ctx} l={l} i={i} />)}
        </div>
      )}
    </div>
  );
}

export type Joining = {
  id: string;
  /** `confirm`: waiting for the player's yes; nothing is downloaded or launched before it. */
  step: 'lobby' | 'confirm' | 'install' | 'launch' | 'done' | 'error';
  lobby?: Lobby;
  /** `confirm`: the exact version is not installed yet, so joining installs it first. */
  install?: boolean;
  error?: string;
  /** Prism is missing: the error offers its download. */
  prism?: boolean;
};

const STEPS: { id: Joining['step']; label: string }[] = [
  { id: 'lobby', label: 'Checking the lobby' },
  { id: 'install', label: 'Installing the exact version' },
  { id: 'launch', label: 'Launching' },
  { id: 'done', label: 'In the game' },
];

/** What joining will do, before anything happens: the mashup, who made it, the games it changes, where it connects. */
function JoinConfirm({ ctx, l, install, onConfirm, onClose }: { ctx: Ctx; l: Lobby; install: boolean; onConfirm: () => void; onClose: () => void }) {
  const known = ctx.catalog.find((m) => m.id === l.mashup.id);
  const author = known?.by.name;
  return (
    <div className="join-confirm">
      <dl className="join-facts">
        <div><dt>Mashup</dt><dd>{l.mashup.name} <span className="muted">v{l.mashup.version}</span></dd></div>
        <div><dt>Made by</dt><dd>{author ?? <span className="muted">Not in your catalog: unknown author</span>}</dd></div>
        <div><dt>Invited by</dt><dd>{l.host}</dd></div>
        <div>
          <dt>{install ? 'Changes' : 'Starts'}</dt>
          <dd>{l.games.map(gameName).join(' + ')}</dd>
        </div>
        <div>
          <dt>Connects to</dt>
          <dd className="join-addr">
            {l.targets.length ? l.targets.map((t) => <code key={t.game}>{l.targets.length > 1 ? `${gameName(t.game)}: ` : ''}{t.address}</code>) : <span className="muted">No server address yet</span>}
          </dd>
        </div>
      </dl>
      <p className="host-hint">
        {install
          ? 'Joining downloads this exact version, installs it into your game folders (Restore vanilla undoes it), then starts the game connected to the server above. Nothing is downloaded until you click Install & join.'
          : 'This version is already installed. Joining starts the game connected to the server above.'}
      </p>
      <div className="host-actions">
        <button className="act act-ghost" onClick={onClose}>Cancel</button>
        <button className="act act-get" onClick={onConfirm} disabled={!l.targets.length} autoFocus>{install ? 'Install & join' : 'Join'}</button>
      </div>
    </div>
  );
}

/** From an invite to the game: one sheet, one state at a time. */
export function JoinSheet({ ctx, j, onClose, onConfirm }: { ctx: Ctx; j: Joining; onClose: () => void; onConfirm: (l: Lobby) => void }) {
  const l = j.lobby;
  const known = l && ctx.catalog.find((m) => m.id === l.mashup.id);
  const pct = l ? Math.round(ctx.installs[l.mashup.id]?.pct ?? 0) : 0;
  const at = STEPS.findIndex((s) => s.id === j.step);
  return (
    <div className="scrim scrim-center" onClick={j.step === 'done' || j.step === 'error' || j.step === 'confirm' ? onClose : undefined}>
      <div className="join-sheet" onClick={(e) => e.stopPropagation()}>
        <MashupCover host={known?.host ?? l?.games[0] ?? 'minecraft'} guest={known?.guest ?? l?.games[1]} className="join-cover" />
        <button className="detail-close" onClick={onClose} aria-label="Close"><Icon name="x" size={16} /></button>
        <div className="join-body">
          <span className="eyebrow">{l ? `${l.host} invites you` : 'Invite'}</span>
          <h2>{l?.mashup.name ?? 'Opening the lobby…'}</h2>
          {l && <p className="muted">v{l.mashup.version} · {l.players}/{l.maxPlayers} players · {l.games.map(gameName).join(' + ')}</p>}
          {j.step === 'confirm' && l ? (
            <JoinConfirm ctx={ctx} l={l} install={!!j.install} onConfirm={() => onConfirm(l)} onClose={onClose} />
          ) : j.step === 'error' ? (
            <div className="join-error">
              <span>{j.error}</span>
              <div className="host-actions">
                {j.prism && <button className="act act-ghost" onClick={() => openUrl('https://prismlauncher.org/download/windows/')}>Get Prism Launcher <Icon name="ext" size={12} /></button>}
                <button className="act act-get" onClick={() => ctx.join(j.id)}>Try again</button>
              </div>
            </div>
          ) : (
            <ol className="join-steps">
              {STEPS.map((s, i) => (
                <li key={s.id} className={i < at || j.step === 'done' ? 'done' : i === at ? 'on' : ''}>
                  <i>{i < at || j.step === 'done' ? <Icon name="check" size={12} /> : null}</i>
                  <span>{s.label}</span>
                  {s.id === 'install' && i === at && <small>{pct}%</small>}
                  {s.id === 'install' && i === at && <em className="bar" style={{ ['--p' as string]: `${pct}%` }} />}
                </li>
              ))}
            </ol>
          )}
          {j.step === 'done' && <p className="host-hint">The game is starting and connects to the lobby by itself. You can close this.</p>}
        </div>
      </div>
    </div>
  );
}
