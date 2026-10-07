// Multiplayer (docs/RECIPE-FORMAT.md section 9): the Lobbies page, the "Play with friends" panel of a mashup, and
// the join sheet that walks a player from an invite link to the game.
import { useEffect, useState } from 'react';
import type { Ctx } from '../App';
import type { Mashup } from '../data/catalog';
import { GAME } from '../data/games';
import { appPlatform, copyText, inTauri, openUrl, platformName, prismDownload } from '../lib/api';
import { CONNECT_GAMES, DEFAULT_PORT, SERVER_ACTIVE, lanAddress, listLobbies, nearestRegion, parseInvite, validAddress, type HostedServer, type Lobby, type PublicLobby, type Target } from '../lib/lobbies';
import { Icon, MashupCover } from '../ui';
import { usePrivacy } from '../lib/privacy';
import { Section, gameName } from './shared';
import { day, list, t, tx, type Key } from '../i18n';

const missingOf = (ctx: Ctx, games: string[]) => games.filter((g) => !ctx.owned.has(g));

function Players({ n, max, state }: { n: number; max: number; state: PublicLobby['state'] }) {
  return (
    <span className={`players players-${state}`} title={t('lobby.playersOf', { count: n, max })}>
      <i style={{ ['--p' as string]: `${Math.min(100, (n / Math.max(1, max)) * 100)}%` }} />
      <b>{n}</b>/{max}
    </span>
  );
}

function LobbyRow({ ctx, l, i }: { ctx: Ctx; l: PublicLobby; i: number }) {
  const miss = missingOf(ctx, l.games);
  const [host, guest] = [l.games[0], l.games[1]];
  const known = ctx.catalog.find((m) => m.id === l.mashup.id);
  // A mashup that does not run on this system (a Windows-only one on a Mac): shown, but it cannot be joined here.
  const only = ctx.elsewhere.get(l.mashup.id);
  const platforms = list((only ?? []).map(platformName));
  return (
    <div className="lobby" style={{ ['--i' as string]: i }}>
      <MashupCover host={known?.host ?? host} guest={known?.guest ?? guest} className="qcover" />
      <div className="qinfo">
        <b>{l.mashup.name}</b>
        <span>
          {t('lobby.hostedBy', { host: l.host })} · {l.games.map(gameName).join(' + ')} · v{l.mashup.version}
        </span>
      </div>
      <Players n={l.players} max={l.maxPlayers} state={l.state} />
      {only ? (
        <button className="act act-busy" disabled title={t('lobby.onlyTitle', { platforms })}>{t('lobby.only', { platforms })}</button>
      ) : miss.length ? (
        <button className="act act-miss" onClick={() => known && ctx.open(known)}>{t('card.needs', { games: miss.map(gameName).join(' + ') })}</button>
      ) : l.state === 'open' ? (
        <button className="act act-get" onClick={() => ctx.join(l.id, true)}>{t('lobby.join')}</button>
      ) : (
        <button className="act act-busy" disabled>{l.state === 'full' ? t('lobby.full') : t('lobby.starting')}</button>
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
  const { list: lobbies, error } = useLobbies(ctx);
  const [link, setLink] = useState('');
  const id = parseInvite(link);
  const players = (lobbies ?? []).reduce((s, l) => s + l.players, 0);

  return (
    <div className="page">
      <section className="together-hero">
        <span className="eyebrow">{t('lobbies.eyebrow')}</span>
        <h1>
          {t('lobbies.title1')} <span className="chrome">{t('lobbies.title2')}</span>
        </h1>
        <p>{t('lobbies.lede')}</p>
        <form
          className="invite-field"
          onSubmit={(e) => {
            e.preventDefault();
            if (id) ctx.join(id);
          }}
        >
          <Icon name="link" size={16} />
          <input value={link} onChange={(e) => setLink(e.target.value)} placeholder={t('lobbies.paste')} spellCheck={false} />
          <button className="act act-get" disabled={!id}>{t('lobby.join')}</button>
        </form>
      </section>

      {ctx.hosted && (
        <Section title={t('lobbies.yours')} sub={t('lobbies.yoursSub')}>
          <HostCard ctx={ctx} />
        </Section>
      )}

      <HostedWorlds ctx={ctx} />

      <Section
        title={t('lobbies.public')}
        sub={t('lobbies.publicSub')}
        aside={lobbies && <span className="live-pill"><Icon name="live" size={10} /> {t('lobbies.openCount', { count: lobbies.length })} · {t('lobbies.playing', { count: players })}</span>}
      >
        {error && <div className="empty">{t('lobbies.unreachable', { error })}</div>}
        {!error && lobbies === null && <div className="empty">{t('lobbies.looking')}</div>}
        {!error && lobbies?.length === 0 && <div className="empty">{t('lobbies.none')}</div>}
        <div className="queue">{lobbies?.map((l, i) => <LobbyRow key={l.id} ctx={ctx} l={l} i={i} />)}</div>
      </Section>
    </div>
  );
}

/** "7 h 42 min", "12 min", "under a minute". */
function timeLeft(ms: number) {
  if (ms < 60_000) return t('time.underMinute');
  const min = Math.floor(ms / 60_000);
  return min >= 60 ? t('time.hoursMin', { h: String(Math.floor(min / 60)), m: String(min % 60).padStart(2, '0') }) : t('time.min', { count: min });
}

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
    s.state === 'queued' ? `${t('srv.queued')}${s.position ? ` · #${s.position}` : ''}`
    : s.state === 'starting' ? `${t('srv.starting')}${s.etaS ? ` · ${t('srv.about', { count: Math.max(1, Math.round(s.etaS / 60)) })}` : ''}`
    : s.state === 'running' ? t('srv.ready')
    : s.state === 'failed' ? t('srv.failed')
    : t('srv.stopped');
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
          <small>{t('srv.hostedOn', { region: region ?? '' })}</small>
        </div>
        {s.state === 'running' && <Players n={s.players ?? 0} max={max} state={(s.players ?? 0) >= max ? 'full' : 'open'} />}
      </div>
      {(s.state === 'queued' || s.state === 'starting') && <i className="srv-bar" />}
      {s.state === 'running' && (
        <p className="host-hint">
          {t('srv.addressHint')}{' '}
          {left !== null && (
            <span className={`srv-left ${warn ? 'on' : ''}`}>
              <Icon name="clock" size={12} /> {warn ? t('srv.stopsIn', { time: timeLeft(left) }) : t('srv.left', { time: timeLeft(left), hours: ctx.hosting?.limits.hours ?? 8 })}
            </span>
          )}
        </p>
      )}
      {(s.state === 'stopped' || s.state === 'failed') && s.worldUntil && <p className="host-hint">{t('srv.keptUntil', { day: day(s.worldUntil) })}</p>}
      <div className="host-actions">
        {SERVER_ACTIVE.includes(s.state) ? (
          <button className="act act-ghost" disabled={busy} onClick={() => act(ctx.stopServer)}>
            <Icon name="x" size={14} /> {t('srv.stop')}
          </button>
        ) : (
          <button className="act act-ghost" disabled={busy} onClick={() => act(() => ctx.startServer(s.region ?? 'eu-west-1'))}>
            <Icon name="server" size={14} /> {t('srv.startAgain')}
          </button>
        )}
        {s.state !== 'queued' && (
          <button className="act act-ghost" disabled={busy} onClick={() => act(() => ctx.downloadWorld(s.lobby ?? ctx.hosted!.lobby.id))}>
            <Icon name="download" size={14} /> {t('srv.download')}
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
    <Section title={t('worlds.title')} sub={t('worlds.sub')}>
      <div className="queue">
        {list.map((w, i) => {
          const known = ctx.catalog.find((m) => m.id === w.mashupId);
          return (
            <div key={w.lobby} className="lobby" style={{ ['--i' as string]: i }}>
              <MashupCover host={known?.host ?? 'minecraft'} guest={known?.guest} cover={known?.cover} className="qcover" />
              <div className="qinfo">
                <b>{w.name || known?.name || w.mashupId}</b>
                <span>
                  {t('worlds.hosted', { day: day(w.startedAt * 1000) })}
                  {w.worldUntil ? ` · ${t('worlds.kept', { day: day(w.worldUntil * 1000) })}` : ''}
                </span>
              </div>
              <button className="act act-get" onClick={() => ctx.downloadWorld(w.lobby)}>
                <Icon name="download" size={14} /> {t('srv.download')}
              </button>
              <button className="act act-ghost" onClick={() => ctx.forgetWorld(w.lobby)} title={t('worlds.forget')} aria-label={t('worlds.forget')}>
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
          <small>{l.mode === 'public' ? t('host.public') : t('host.inviteOnly')} · v{l.mashup.version}</small>
        </div>
        <Players n={l.players} max={l.maxPlayers} state={l.state} />
      </div>
      <div className="invite-row">
        <code>{l.url.replace(/^https:\/\//, '')}</code>
        <button
          className="act act-get"
          onClick={async () => {
            if (await copyText(l.url)) {
              setCopied(true);
              setTimeout(() => setCopied(false), 1800);
            }
          }}
        >
          <Icon name={copied ? 'check' : 'copy'} size={14} /> {copied ? t('host.copied') : t('host.copy')}
        </button>
      </div>
      {srv && <ServerPanel ctx={ctx} s={srv} />}
      {ctx.serverError && (
        <div className="join-error">
          <span>{ctx.serverError}</span>
          <div className="host-actions">
            <button className="act act-get" onClick={() => ctx.startServer(srv?.region ?? (ctx.hosting ? nearestRegion(ctx.hosting) : 'eu-west-1'))}>{t('common.tryAgain')}</button>
          </div>
        </div>
      )}
      {!srv && !ctx.serverError && l.state === 'waiting' && <p className="host-hint">{t('host.waiting')} {mc ? tx('host.waitingMc', { cmd: '/publish true survival 25565' }) : t('host.waitingOther')}</p>}
      {!mc && !srv && (
        <div className="stepper" title={t('host.playersTitle')}>
          <span>{t('host.players')}</span>
          <button onClick={() => ctx.hostUpdate(Math.max(1, l.players - 1))} aria-label={t('host.oneLess')}>−</button>
          <b>{l.players}</b>
          <button onClick={() => ctx.hostUpdate(Math.min(l.maxPlayers, l.players + 1))} aria-label={t('host.oneMore')}>+</button>
        </div>
      )}
      <div className="host-actions">
        <button className="act act-ghost" onClick={() => ctx.closeHost()} title={srv && SERVER_ACTIVE.includes(srv.state) ? t('host.closeTitle') : undefined}>
          <Icon name="x" size={14} /> {t('host.close')}
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

const SIGF_STEPS = ['together.sigf1', 'together.sigf2', 'together.sigf3'] as const;
const OWN_STEPS = ['together.own2', 'together.own3'] as const;

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
  const [where, setWhere] = useState<'own' | 'sigf'>('sigf');
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
      <h4>{t('together.title')}</h4>
      {mine ? (
        <HostCard ctx={ctx} />
      ) : inTauri && (!m.version || !m.recipeUrl) ? (
        <p className="host-hint">{t('together.notLive')}</p>
      ) : !ready ? (
        <p className="host-hint">{t('together.getFirst')}</p>
      ) : outdated ? (
        <p className="host-hint">
          {tx('together.outdated', { have: inst.version!, latest: m.version!, update: <a onClick={() => ctx.get(m)}>{t('card.update')}</a> })}
        </p>
      ) : !open ? (
        <button className="act act-get together-open" onClick={() => setOpen(true)} disabled={!!ctx.hosted} title={ctx.hosted ? t('together.otherLobby') : undefined}>
          <Icon name="people" size={15} /> {t('together.title')}
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
            await ctx.host(m, { mode, maxPlayers: onSigf ? cap : max, name: name.trim(), targets, region: onSigf ? region : undefined });
            setBusy(false);
            setOpen(false);
          }}
        >
          {onSigf ? (
            <>
              {/* The default when SIGF can host: one button, nothing to configure. */}
              <p className="together-lede">
                <Icon name="server" size={14} /> {t('together.sigfLede', { count: cap, hours: ctx.hosting?.limits.hours ?? 8 })}
              </p>
              <ol className="together-steps">
                {SIGF_STEPS.map((k) => <li key={k}>{t(k)}</li>)}
              </ol>
            </>
          ) : (
            <>
              <p className="together-lede">{t('together.ownLede')}</p>
              <ol className="together-steps">
                <li>{games.includes('minecraft') ? tx('together.own1Mc', { cmd: <code>/publish true survival 25565</code> }) : t('together.own1')}</li>
                {OWN_STEPS.map((k) => <li key={k}>{t(k)}</li>)}
              </ol>
              {games.map((g) => (
                <label key={g} className={`field ${addr[g] && !validAddress(addr[g]) ? 'bad' : ''}`}>
                  <small>{games.length > 1 ? t('together.gameAddress', { game: GAME[g]?.short ?? g }) : t('together.yourAddress')}</small>
                  <input value={addr[g] ?? ''} onChange={(e) => setAddr((a) => ({ ...a, [g]: e.target.value }))} spellCheck={false} placeholder={t('together.addressPlaceholder')} />
                </label>
              ))}
              {!games.every((g) => addr[g]?.trim()) && (
                <button type="button" className="act act-ghost" onClick={() => void fillLan()}>
                  <Icon name="link" size={13} /> {t('together.fill')}
                </button>
              )}
              <p className="host-hint">{t('together.lanHint')}</p>
            </>
          )}
          {!loadName() && (
            <label className="field">
              <small>{t('together.name')}</small>
              <input value={name} onChange={(e) => setName(e.target.value)} maxLength={32} placeholder={t('together.namePlaceholder')} />
            </label>
          )}
          {onSigf && ctx.hosting && ctx.hosting.regions.length > 1 && (
            <div className="seg">
              {ctx.hosting.regions.map((r) => (
                <button key={r.id} type="button" className={region === r.id ? 'on' : ''} disabled={!r.available} title={r.available ? undefined : t('together.opensTomorrow')} onClick={() => setRegion(r.id)}>
                  {r.label}{r.available ? '' : ` · ${t('together.tomorrow')}`}
                </button>
              ))}
            </div>
          )}
          <label className="together-public">
            <input type="checkbox" checked={mode === 'public'} onChange={(e) => setMode(e.target.checked ? 'public' : 'invite')} /> {t('together.public')}
          </label>
          <div className="host-actions">
            {canHost && (
              <a className="together-switch" onClick={() => setWhere(onSigf ? 'own' : 'sigf')}>
                {onSigf ? t('together.useOwn') : t('together.useSigf')}
              </a>
            )}
            <button type="button" className="act act-ghost" onClick={() => setOpen(false)}>{t('common.cancel')}</button>
            <button className="act act-get" disabled={!nameOk || !addrOk || busy}>
              {busy ? t('together.starting') : onSigf ? t('together.startServer') : t('together.create')}
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

const STEPS: { id: Joining['step']; label: Key }[] = [
  { id: 'lobby', label: 'joinSheet.stepLobby' },
  { id: 'install', label: 'joinSheet.stepInstall' },
  { id: 'launch', label: 'joinSheet.stepLaunch' },
  { id: 'done', label: 'joinSheet.stepDone' },
];

/** What joining will do, before anything happens: the mashup, who made it, the games it changes, where it connects. */
function JoinConfirm({ ctx, l, install, onConfirm, onClose }: { ctx: Ctx; l: Lobby; install: boolean; onConfirm: () => void; onClose: () => void }) {
  const known = ctx.catalog.find((m) => m.id === l.mashup.id);
  const author = known?.by.name;
  return (
    <div className="join-confirm">
      <dl className="join-facts">
        <div><dt>{t('joinSheet.mashup')}</dt><dd>{l.mashup.name} <span className="muted">v{l.mashup.version}</span></dd></div>
        <div><dt>{t('detail.madeBy')}</dt><dd>{author ?? <span className="muted">{t('joinSheet.unknownAuthor')}</span>}</dd></div>
        <div><dt>{t('joinSheet.invitedBy')}</dt><dd>{l.host}</dd></div>
        <div>
          <dt>{install ? t('joinSheet.changes') : t('joinSheet.starts')}</dt>
          <dd>{l.games.map(gameName).join(' + ')}</dd>
        </div>
        <div>
          <dt>{t('joinSheet.connects')}</dt>
          <dd className="join-addr">
            {l.targets.length ? l.targets.map((t) => <code key={t.game}>{l.targets.length > 1 ? `${gameName(t.game)}: ` : ''}{t.address}</code>) : <span className="muted">{t('joinSheet.noAddress')}</span>}
          </dd>
        </div>
      </dl>
      <p className="host-hint">
        {install
          ? t('joinSheet.installHint')
          : t('joinSheet.installedHint')}
      </p>
      <div className="host-actions">
        <button className="act act-ghost" onClick={onClose}>{t('common.cancel')}</button>
        <button className="act act-get" onClick={onConfirm} disabled={!l.targets.length} autoFocus>{install ? t('joinSheet.installJoin') : t('lobby.join')}</button>
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
        <button className="detail-close" onClick={onClose} aria-label={t('common.close')}><Icon name="x" size={16} /></button>
        <div className="join-body">
          <span className="eyebrow">{l ? t('joinSheet.invites', { host: l.host }) : t('joinSheet.invite')}</span>
          <h2>{l?.mashup.name ?? t('joinSheet.opening')}</h2>
          {l && <p className="muted">v{l.mashup.version} · {t('joinSheet.players', { n: l.players, count: l.maxPlayers })} · {l.games.map(gameName).join(' + ')}</p>}
          {j.step === 'confirm' && l ? (
            <JoinConfirm ctx={ctx} l={l} install={!!j.install} onConfirm={() => onConfirm(l)} onClose={onClose} />
          ) : j.step === 'error' ? (
            <div className="join-error">
              <span>{j.error}</span>
              <div className="host-actions">
                {j.prism && <button className="act act-ghost" onClick={() => void appPlatform().then((p) => openUrl(prismDownload(p)))}>{t('joinSheet.getPrism')} <Icon name="ext" size={12} /></button>}
                <button className="act act-get" onClick={() => ctx.join(j.id)}>{t('common.tryAgain')}</button>
              </div>
            </div>
          ) : (
            <ol className="join-steps">
              {STEPS.map((s, i) => (
                <li key={s.id} className={i < at || j.step === 'done' ? 'done' : i === at ? 'on' : ''}>
                  <i>{i < at || j.step === 'done' ? <Icon name="check" size={12} /> : null}</i>
                  <span>{t(s.label)}</span>
                  {s.id === 'install' && i === at && <small>{pct}%</small>}
                  {s.id === 'install' && i === at && <em className="bar" style={{ ['--p' as string]: `${pct}%` }} />}
                </li>
              ))}
            </ol>
          )}
          {j.step === 'done' && <p className="host-hint">{t('joinSheet.doneHint')}</p>}
        </div>
      </div>
    </div>
  );
}
