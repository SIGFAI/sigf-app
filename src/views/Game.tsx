// The game hub (docs/GAME-HUB.md section 7): every game on this PC with its mods and libraries, and one page per game
// with Play, the mods installed, and tabs: Mods (every source merged), Mashups, Servers, Libraries, Workshop (Steam
// games on Windows). Nothing is installed without a click; Nexus downloads for free accounts wait for the player's own
// "Mod manager download" click on the site.
import { useCallback, useEffect, useMemo, useRef, useState } from 'react';
import type { Ctx, GameNav, GameTab } from '../App';
import { GAME } from '../data/games';
import { launchGame, openUrl, type Game } from '../lib/api';
import { STEAM_BLOCKING, isCode, newLibrary, parsePasted, readState, saveLibrary, subscribedIds, useLibraries, useWorkshop, type Library, type WorkshopItem } from '../lib/workshop';
import {
  ModsError, SOURCE_NAME, SOURCE_SHORT, canInstall, cancelNxm, clearFailed, nexusSetKey, viaNexusClick, getMod, libGame, libRef, mergePages, modStatus, nexusLogin, nexusLogout, useNxmState,
  previewPlan, searchSource, setNxmHandler, sourceOfRef, sourcesOf, uninstallMod, installMod, refItem, useInstalledMods, useModGames, useMods, useNexus,
  useRefItems, type Hit, type InstallPlan, type ModGame, type ModItem, type ModSort, type Source,
} from '../lib/mods';
import { GameArt, Icon, STORE_LABEL, fmtCount } from '../ui';
import { Card, Section } from './shared';
import { HostCard, HostedWorlds, LobbyRow, useLobbies } from './Lobbies';
import {
  AddButton, AddMenu, GameHero, ImportSheet, ItemSheet, Libraries, LibraryRow, LibrarySheet, PageCtx, Preview, Skeletons, SteamBanner, WorkshopTab,
  bytes, errorText, keyName, modStatusLabel, steamGameKey, useEscape, useLatest, usePage, when, whenShort, type Page,
} from './Workshop';
import { McProfileBar } from './McProfile';
import { useMcProfileKey } from '../lib/mcprofile';
import { t, type Key } from '../i18n';

// ---------- The game ----------

/** One game as the page sees it. `key`: its canonical id, or `steam:<appid>` for a Steam game SIGF does not map. */
export type GameInfo = {
  key: string; canon: string | null; name: string;
  /** Its Steam app id when it has one ('' otherwise). */
  appid: string;
  /** On this PC (any store), and on this PC through Steam (the Workshop needs that). */
  onPc: boolean; steamOnPc: boolean;
  scanned?: Game;
  mod?: ModGame;
  /** The Workshop tab: a Steam game on this PC, on Windows. */
  workshop: boolean;
};

export function resolveGame(ctx: Ctx, key: string, mod?: ModGame): GameInfo {
  const games = ctx.scan?.games ?? [];
  const steamId = key.startsWith('steam:') ? key.slice(6) : null;
  const canon = steamId ? null : key;
  const steamScan = games.find((g) => g.store === 'steam' && (steamId ? g.storeId === steamId : g.canon === canon));
  const scanned = steamScan ?? (canon ? games.find((g) => g.canon === canon) : undefined);
  const appid = steamId ?? steamScan?.storeId ?? mod?.steam ?? (canon ? GAME[canon]?.steam?.[0] : undefined) ?? '';
  return {
    key, canon: canon ?? scanned?.canon ?? null, name: scanned?.name ?? (canon ? GAME[canon]?.name : undefined) ?? mod?.name ?? keyName(ctx, key),
    appid, onPc: !!scanned, steamOnPc: !!steamScan, scanned, mod, workshop: !!steamScan && ctx.workshopOn,
  };
}

/** The key of a scanned game. */
const keyOf = (g: Game) => g.canon ?? (g.store === 'steam' ? `steam:${g.storeId}` : `${g.store}:${g.storeId}`);

const SORTS: { id: ModSort; label: Key }[] = [
  { id: 'popular', label: 'mods.sortPopular' },
  { id: 'updated', label: 'mods.sortUpdated' },
  { id: 'new', label: 'mods.sortNew' },
];

const NSFW_KEY = 'mods-nsfw';
const loadNsfw = () => {
  try {
    return localStorage.getItem(NSFW_KEY) === '1';
  } catch {
    return false;
  }
};

/** Why a mod cannot be installed by the app, in words (`why`: from canInstall, else the item's own). */
function whyText(it: ModItem, why = canInstall(it).why ?? it.why): string {
  switch (why) {
    case 'distribution_off': return t('mods.why.distribution_off');
    case 'needs_nexus_login': return t('mods.why.needs_nexus_login');
    case 'no_target': return t('mods.why.no_target', { game: GAME[it.game]?.short ?? it.game });
    case 'use_minecraft_flow': return t('mods.why.use_minecraft_flow');
    default: return t('mods.why.other', { source: SOURCE_NAME[it.source] });
  }
}

// ---------- Install button ----------

function ModButton({ it, big = false }: { it: ModItem; big?: boolean }) {
  const page = usePage();
  useMods();
  useWorkshop();
  const st = modStatus(it.ref, page.appid);
  const ws = it.source === 'ws';
  const cls = `act ws-sub ${big ? 'act-big' : ''}`;
  const stop = (f: () => void) => (e: React.MouseEvent) => {
    e.stopPropagation();
    f();
  };
  const install = () => {
    clearFailed(it.ref);
    installMod(it.ref, page.g.canon ?? page.g.key, { appid: page.appid }).then(
      (r) => {
        if (r.kind === 'link') page.ctx.flash(t('mods.openedPage', { source: SOURCE_NAME[it.source] }));
        else if (r.kind === 'nxm') page.openMod(it);
        else if (!ws) page.ctx.flash(t('mods.installedToast', { name: it.title }));
      },
      (e) => (e instanceof ModsError && e.code === 'needs_nexus_login' ? page.openNexus() : page.fail(e)),
    );
  };
  const uninstall = async () => {
    try {
      try {
        await uninstallMod(it.ref, { appid: page.appid });
      } catch (e) {
        if (!(e instanceof ModsError && e.detail?.kind === 'tampered')) throw e;
        if (!confirm(t('confirm.tampered', { count: Math.max(1, e.detail.files.length) }))) return;
        await uninstallMod(it.ref, { appid: page.appid, force: true });
      }
      if (!ws) page.ctx.flash(t('mods.removedToast', { name: it.title }));
    } catch (e) {
      page.fail(e);
    }
  };
  const can = canInstall(it);
  if (!can.ok && st.kind === 'none') {
    // Nexus with a target: what is missing is the player's Nexus key, so the sheet's big button sets it up.
    if (big && can.why === 'needs_nexus_login') {
      return (
        <button className="act act-get act-big ws-sub" onClick={stop(page.openNexus)} title={whyText(it)}>
          <i className="src-dot src-nx" /> {t('mods.setupNexus')}
        </button>
      );
    }
    if (big) {
      return (
        <button className="act act-ghost act-big ws-sub" title={whyText(it)} onClick={stop(() => void openUrl(it.url))}>
          <span>{t('mods.openOn', { source: SOURCE_SHORT[it.source] })}</span> <Icon name="ext" size={12} />
        </button>
      );
    }
    // A card: no primary button, a small way out to the source's page.
    return (
      <button className="mod-weblink" title={`${t('mods.openOn', { source: SOURCE_NAME[it.source] })}: ${whyText(it)}`} onClick={stop(() => void openUrl(it.url))}>
        {SOURCE_SHORT[it.source]} <Icon name="ext" size={11} />
      </button>
    );
  }
  const nexusClick = !ws && viaNexusClick(it);
  switch (st.kind) {
    case 'planning':
    case 'removing':
      return <button className={`${cls} act-busy ws-indet`} onClick={stop(() => {})}><span>{modStatusLabel(st, ws)}</span></button>;
    case 'installing':
      return (
        <button className={`${cls} act-busy`} onClick={stop(() => {})} style={{ ['--p' as string]: `${Math.round(st.pct)}%` }}>
          <span>{modStatusLabel(st, ws)}</span> <small>{Math.round(st.pct)}%</small>
        </button>
      );
    case 'nxm':
      return (
        <button className={`${cls} act-busy ws-indet mod-wait`} onClick={stop(() => page.openMod(it))} title={t('nx.waitTitle')}>
          <span>{t('mods.waitingNexus')}</span>
        </button>
      );
    case 'installed':
      return (
        <button className={`${cls} ws-have`} onClick={stop(() => void uninstall())} title={ws ? t('ws.unsubscribe') : t('mods.uninstall')}>
          <span className="ws-have-on"><Icon name="check" size={14} /> {t('ws.installed')}</span>
          <span className="ws-have-off"><Icon name="x" size={13} /> {ws ? t('ws.unsubscribe') : t('mods.uninstall')}</span>
        </button>
      );
    case 'failed':
      return (
        <button className={`${cls} act-miss`} onClick={stop(install)} title={st.error}>
          <Icon name="restore" size={14} /> {t('ws.retry')}
        </button>
      );
    default:
      return (
        <button className={`${cls} act-get`} onClick={stop(install)} disabled={ws && !page.ctx.workshopOn} title={nexusClick ? t('mods.installViaNexusHint') : undefined}>
          <Icon name={ws ? 'plus' : 'download'} size={14} /> <span>{ws ? t('ws.subscribe') : nexusClick ? t(big ? 'mods.installViaNexus' : 'mods.installViaNexusShort') : t('mods.install')}</span>
        </button>
      );
  }
}

// ---------- Mod card ----------

function SourceBadge({ s, also }: { s: Source; also?: ModItem[] }) {
  return (
    <span className={`src-badge src-${s}`} title={also?.length ? t('mods.alsoOn', { sources: also.map((a) => SOURCE_NAME[a.source]).join(', ') }) : SOURCE_NAME[s]}>
      <i className={`src-dot src-${s}`} />
      {SOURCE_SHORT[s]}
      {!!also?.length && <em>+{also.length}</em>}
    </span>
  );
}

function ModCard({ it, i }: { it: Hit; i: number }) {
  const page = usePage();
  useMods();
  const web = !canInstall(it).ok && modStatus(it.ref, page.appid).kind === 'none';
  return (
    <article className={`ws-card mod-card ${web ? 'mod-card-web' : ''}`} style={{ ['--i' as string]: i % 30 }} onClick={() => page.openMod(it)}>
      <div className="ws-thumb">
        <Preview item={it} />
        <SourceBadge s={it.source} also={it.also} />
        {it.nsfw && <span className="chip mod-nsfw">18+</span>}
        {!!it.sizeBytes && <span className="ws-size">{bytes(it.sizeBytes)}</span>}
      </div>
      <div className="ws-card-body">
        <h3 title={it.title}>{it.title}</h3>
        {it.author && <span className="mod-by">{t('ws.by', { author: it.author })}</span>}
        <div className="ws-meta">
          {typeof it.downloads === 'number' && <span title={t('mods.downloadsCount', { count: it.downloads })}><Icon name="download" size={12} />{fmtCount(it.downloads)}</span>}
          {!!it.likes && <span title={t('mods.likesCount', { count: it.likes })}><Icon name="heart" size={12} />{fmtCount(it.likes)}</span>}
          {!!it.updated && <span title={`${t('ws.updated')} ${when(it.updated)}`}>{whenShort(it.updated)}</span>}
        </div>
        <div className="ws-card-foot">
          <ModButton it={it} />
          <AddButton refs={[libRef(it.ref)]} />
        </div>
      </div>
    </article>
  );
}

// ---------- Mods tab ----------

type SourceState = { state: 'loading' | 'ok' | 'soon' | 'error'; next: string | null; total?: number; error?: string };
type Filter = 'all' | 'installed' | Source;

/** The short reason on the chip of a source none of whose mods installs here. */
const CHIP_WHY: Record<string, Key> = {
  needs_nexus_login: 'mods.chip.needs_nexus_login', no_target: 'mods.chip.no_target', distribution_off: 'mods.chip.distribution_off',
  use_minecraft_flow: 'mods.chip.use_minecraft_flow', no_file: 'mods.chip.other',
};

/** A card the app can act on: installable here, or already on its way / on the PC. */
const actionable = (it: ModItem, appid: string) => canInstall(it).ok || modStatus(it.ref, appid).kind !== 'none';

/** When the card's own source only links out but the same mod installs from another source, that one leads. */
function installableFirst(h: Hit): Hit {
  if (canInstall(h).ok || !h.also?.length) return h;
  const k = h.also.findIndex((a) => canInstall(a).ok);
  if (k < 0) return h;
  const { also, ...self } = h;
  return { ...also[k], also: [self as ModItem, ...also.filter((_, j) => j !== k)] };
}

function NexusPill() {
  const page = usePage();
  const { status, error } = useNexus();
  const soon = error?.code === 'nexus_unavailable';
  return (
    <button className={`nx-pill ${status?.loggedIn ? 'on' : ''}`} onClick={page.openNexus} title={t('nx.title')}>
      <i className="src-dot src-nx" />
      {status === null ? SOURCE_SHORT.nx : soon ? `${SOURCE_SHORT.nx} · ${t('mods.soonShort')}` : status.loggedIn ? `${SOURCE_SHORT.nx} · ${status.name ?? t('nx.signedIn')}` : t('nx.login')}
    </button>
  );
}

function ModsTab({ onWorkshop }: { onWorkshop: () => void }) {
  const page = usePage();
  const games = useModGames();
  const g = page.g;
  const sources = useMemo(() => sourcesOf(g.mod, g.workshop && !!page.appid), [g.mod, g.workshop, page.appid]);
  const [sort, setSort] = useState<ModSort>('popular');
  const [q, setQ] = useState('');
  const [query, setQuery] = useState('');
  const [filter, setFilter] = useState<Filter>('all');
  const [nsfw, setNsfw] = useState(loadNsfw);
  const [hits, setHits] = useState<Hit[] | null>(null);
  const [states, setStates] = useState<Partial<Record<Source, SourceState>>>({});
  const [more, setMore] = useState(false);
  const [retry, setRetry] = useState(0);
  // Installable only (the default); link-only mods wait under "Show N more on the web".
  const [only, setOnly] = useState(true);
  const [web, setWeb] = useState(false);
  // Pages in a row that brought no installable card: past 2, scrolling stops asking (the button stays).
  const [dry, setDry] = useState(0);
  useNexus();
  useMods();
  const run = useRef(0);
  const seen = useRef(new Map<string, Hit>());
  const foot = useRef<HTMLDivElement>(null);
  const installed = useInstalledMods(g.canon ?? g.key);
  const installedRefs = (installed ?? []).map((r) => r.ref);
  const installedItems = useRefItems(filter === 'installed' ? installedRefs : []);
  const game = g.canon ?? g.key;
  // Minecraft: a new profile is a new search (only mods for its version and loader).
  const mcKey = useMcProfileKey(game);

  useEffect(() => {
    const id = setTimeout(() => setQuery(q), 350);
    return () => clearTimeout(id);
  }, [q]);

  const ask = filter === 'all' || filter === 'installed' ? sources : [filter];
  useEffect(() => {
    if (filter === 'installed' || !ask.length) return;
    const n = ++run.current;
    setHits(null);
    setDry(0);
    seen.current = new Map();
    setStates((s) => ({ ...s, ...Object.fromEntries(ask.map((x) => [x, { ...(s[x] ?? {}), state: 'loading', next: null }])) }));
    void Promise.allSettled(ask.map((s) => searchSource(game, s, { q: query, sort }, page.appid))).then((res) => {
      if (n !== run.current) return;
      const next: Partial<Record<Source, SourceState>> = {};
      const lists: ModItem[][] = [];
      res.forEach((r, i) => {
        const s = ask[i];
        if (r.status === 'fulfilled') {
          next[s] = { state: 'ok', next: r.value.next, total: r.value.total };
          lists.push(r.value.items);
        } else {
          const e = r.reason;
          next[s] = { state: e instanceof ModsError && (e.code === 'source_unavailable' || e.code === 'no_source') ? 'soon' : 'error', next: null, error: errorText(e, page.game) };
        }
      });
      setStates((cur) => ({ ...cur, ...next }));
      setHits(mergePages(lists, sort, seen.current));
    });
  }, [game, sort, query, filter, retry, sources.join(), mcKey]);

  const loadMore = async () => {
    const want = ask.filter((s) => states[s]?.state === 'ok' && states[s]?.next);
    if (!want.length || more) return;
    const n = run.current;
    setMore(true);
    const res = await Promise.allSettled(want.map((s) => searchSource(game, s, { q: query, sort, cursor: states[s]!.next }, page.appid)));
    if (n !== run.current) return setMore(false);
    const lists: ModItem[][] = [];
    const upd: Partial<Record<Source, SourceState>> = {};
    res.forEach((r, i) => {
      const s = want[i];
      if (r.status === 'fulfilled') {
        lists.push(r.value.items);
        upd[s] = { ...states[s]!, next: r.value.next };
      } else upd[s] = { ...states[s]!, next: null };
    });
    setStates((cur) => ({ ...cur, ...upd }));
    const add = mergePages(lists, sort, seen.current);
    setHits((cur) => [...(cur ?? []), ...add]);
    setDry((d) => (add.some((h) => actionable(installableFirst(h), page.appid)) ? 0 : d + 1));
    setMore(false);
  };
  const hasNext = ask.some((s) => states[s]?.state === 'ok' && !!states[s]?.next);
  useEffect(() => {
    const el = foot.current;
    if (!el || !hasNext || filter === 'installed' || (only && !web && dry >= 2)) return;
    const io = new IntersectionObserver((es) => es.some((x) => x.isIntersecting) && void loadMore(), { rootMargin: '600px 0px' });
    io.observe(el);
    return () => io.disconnect();
  }, [hasNext, more, hits?.length, filter, only, web, dry]);

  const toggleNsfw = () => {
    setNsfw(!nsfw);
    try {
      localStorage.setItem(NSFW_KEY, nsfw ? '0' : '1');
    } catch {}
  };

  if (games === null) return <div className="ws-grid"><Skeletons n={8} /></div>;
  if (!sources.length) {
    return (
      <div className="ws-soon">
        <span className="ws-soon-icon"><Icon name="workshop" size={22} /></span>
        <h3>{t('mods.noSourcesTitle', { game: page.game })}</h3>
        <p>{g.steamOnPc && !page.ctx.workshopOn ? t('mods.noSourcesMac') : t('mods.noSourcesBody')}</p>
        {g.workshop && <div className="host-actions"><button className="act act-get" onClick={onWorkshop}><Icon name="workshop" size={14} /> {t('mods.openWorkshopTab')}</button></div>}
      </div>
    );
  }

  const allSoon = sources.every((s) => states[s]?.state === 'soon');
  const shownRaw: Hit[] | null = filter === 'installed'
    ? installed === null ? null : installedRefs.map((r, i) => installedItems[i] ?? stub(r, installed.find((x) => x.ref === r)!.name, game))
    : hits?.map(installableFirst) ?? null;
  const hidden = (shownRaw ?? []).filter((h) => h.nsfw).length;
  const shown = shownRaw?.filter((h) => nsfw || !h.nsfw) ?? null;
  const browsing = filter !== 'installed';
  const split = browsing && only;
  const main = shown && split ? shown.filter((h) => actionable(h, page.appid)) : shown;
  const webOnly = shown && split ? shown.filter((h) => !actionable(h, page.appid)) : [];

  // Per source, from what is loaded (each card and its twins): how many install here, and why the others do not.
  const per: Partial<Record<Source, { n: number; k: number; why: Record<string, number> }>> = {};
  for (const h of hits ?? []) {
    for (const it of [h, ...(h.also ?? [])]) {
      const p = (per[it.source] ??= { n: 0, k: 0, why: {} });
      p.n++;
      const c = canInstall(it);
      if (c.ok) p.k++;
      else p.why[c.why ?? 'other'] = (p.why[c.why ?? 'other'] ?? 0) + 1;
    }
  }
  /** A source none of whose loaded mods installs here: the reason most of them give. */
  const linkOnly = (s: Source): string | null => {
    const p = per[s];
    if (states[s]?.state !== 'ok' || !p || p.n === 0 || p.k > 0) return null;
    return Object.entries(p.why).sort((a, b) => b[1] - a[1])[0]?.[0] ?? 'other';
  };
  /** The installable count of a source: exact once every page is in, else its total when all loaded ones install. */
  const instCount = (s: Source): { n: number; plus: boolean } => {
    const st = states[s];
    const p = per[s];
    if (!st || st.state !== 'ok' || !p) return { n: 0, plus: false };
    if (!st.next) return { n: p.k, plus: false };
    return p.k === p.n && typeof st.total === 'number' ? { n: st.total, plus: false } : { n: p.k, plus: p.k > 0 };
  };
  const counts = ask.map(instCount);
  const total = counts.reduce((s, c) => s + c.n, 0);
  const totalPlus = counts.some((c) => c.plus);

  return (
    <>
      {game === 'minecraft' && <McProfileBar />}
      <div className="ws-toolbar">
        <label className="search ws-search">
          <Icon name="search" size={15} />
          <input value={q} onChange={(e) => setQ(e.target.value)} placeholder={t('mods.search', { game: page.game })} spellCheck={false} disabled={filter === 'installed'} />
          {q && <button className="ws-clear" onClick={() => setQ('')} aria-label={t('common.close')}><Icon name="x" size={12} /></button>}
        </label>
        <div className="seg">
          {SORTS.map((s) => (
            <button key={s.id} className={sort === s.id ? 'on' : ''} onClick={() => setSort(s.id)} disabled={filter === 'installed'}>{t(s.label)}</button>
          ))}
        </div>
        {sources.includes('nx') && <NexusPill />}
        {browsing && hits && (total > 0 || !only) && (
          <span className="ws-count">{only ? t(totalPlus ? 'mods.installableCountPlus' : 'mods.installableCount', { count: total }) : t('ws.results', { count: ask.reduce((s, x) => s + (states[x]?.total ?? 0), 0) })}</span>
        )}
      </div>
      <div className="src-row">
        <button className={`src-chip src-inst ${only && browsing ? 'on' : ''}`} disabled={!browsing} onClick={() => setOnly(!only)} title={t('mods.installableTitle')} aria-pressed={only}>
          <Icon name="download" size={12} /> {t('mods.installableFilter')}{browsing && hits && <em>{fmtCount(total)}{totalPlus ? '+' : ''}</em>}
        </button>
        <span className="src-sep" />
        <button className={`src-chip ${filter === 'all' ? 'on' : ''}`} onClick={() => setFilter('all')}>{t('mods.allSources')}</button>
        <button className={`src-chip ${filter === 'installed' ? 'on' : ''}`} onClick={() => setFilter('installed')}>
          <Icon name="check" size={12} /> {t('mods.installedFilter')}{installed && installed.length > 0 && <em>{installed.length}</em>}
        </button>
        <span className="src-sep" />
        {sources.map((s) => {
          const st = states[s];
          // A Steam game without a Workshop: no empty chip.
          if (s === 'ws' && st?.state === 'ok' && st.total === 0 && !query && filter !== 'ws') return null;
          const soon = st?.state === 'soon';
          const why = linkOnly(s);
          const c = instCount(s);
          const sample = (hits ?? []).flatMap((h) => [h, ...(h.also ?? [])]).find((x) => x.source === s);
          const click = () => {
            if (st?.state === 'error') return setRetry((r) => r + 1);
            // Nothing from Nexus installs until the player's Nexus key is set: the chip opens that.
            if (why === 'needs_nexus_login') return page.openNexus();
            if (why) setWeb(true);
            setFilter(filter === s ? 'all' : s);
          };
          return (
            <button
              key={s}
              className={`src-chip ${filter === s ? 'on' : ''} ${soon ? 'soon' : ''} ${st?.state === 'error' ? 'err' : ''} ${why ? 'web' : ''}`}
              disabled={soon}
              title={soon ? t('mods.sourceSoonTitle', { source: SOURCE_NAME[s] }) : st?.state === 'error' ? st.error : why && sample ? whyText(sample, why) : undefined}
              onClick={click}
            >
              <i className={`src-dot src-${s}`} />
              {SOURCE_NAME[s]}
              {soon ? <small>{t('mods.soonShort')}</small>
                : st?.state === 'error' ? <Icon name="restore" size={11} />
                : why ? <small className="why">{t(CHIP_WHY[why] ?? 'mods.chip.other')}</small>
                : only && st?.state === 'ok' ? <em>{fmtCount(c.n)}{c.plus ? '+' : ''}</em>
                : typeof st?.total === 'number' && <em>{fmtCount(st.total)}</em>}
            </button>
          );
        })}
        {(hidden > 0 || nsfw) && (
          <button className={`src-chip src-nsfw ${nsfw ? 'on' : ''}`} onClick={toggleNsfw} title={t('mods.nsfwTitle')}>
            {nsfw ? t('mods.nsfwHide') : t('mods.nsfwShow', { count: hidden })}
          </button>
        )}
      </div>

      {filter !== 'installed' && allSoon ? (
        <div className="ws-soon">
          <span className="ws-soon-icon"><Icon name="search" size={22} /></span>
          <h3>{t('mods.allSoonTitle')}</h3>
          <p>{t('mods.allSoonBody', { game: page.game })}</p>
        </div>
      ) : shown?.length === 0 ? (
        <div className="empty">
          {filter === 'installed' ? t('mods.installedNone', { game: page.game }) : query ? t('ws.noResults') : t('mods.noItems')}
        </div>
      ) : (
        <>
          {main?.length === 0 ? (
            <div className="mod-none-here">
              <b>{t('mods.noneInstallableTitle', { game: page.game })}</b>
              <span>{t('mods.noneInstallableBody', { count: webOnly.length })}</span>
            </div>
          ) : (
            <div className="ws-grid">
              {main ? main.map((h, i) => <ModCard key={h.ref} it={h} i={i} />) : <Skeletons />}
              {more && !(split && web) && <Skeletons n={6} />}
            </div>
          )}
          {webOnly.length > 0 && (
            <div className="mod-web">
              <button className={`mod-web-toggle ${web ? 'on' : ''}`} onClick={() => setWeb(!web)} aria-expanded={web}>
                <Icon name="ext" size={13} />
                <span>{web ? t('mods.webHide') : t('mods.webShow', { count: webOnly.length })}</span>
                <small>{t('mods.webSub')}</small>
                <Icon name="down" size={13} />
              </button>
              {web && (
                <div className="ws-grid">
                  {webOnly.map((h, i) => <ModCard key={h.ref} it={h} i={i} />)}
                  {more && <Skeletons n={6} />}
                </div>
              )}
            </div>
          )}
        </>
      )}
      <div className="ws-foot" ref={foot}>
        {hasNext && !more && filter !== 'installed' && <button className="act act-ghost" onClick={() => void loadMore()}>{t('ws.loadMore')}</button>}
      </div>
    </>
  );
}

/** An installed mod the proxy did not describe (yet): its name from the engine's record. */
const stub = (ref: string, name: string, game: string): ModItem => ({ ref, source: sourceOfRef(ref), game, title: name, summary: '', tags: [], url: '', installable: true });

// ---------- Mod sheet ----------

/** The Nexus wait: what to click on the site, and that the app picks it up. */
function NxmPanel({ it, filesUrl }: { it: ModItem; filesUrl: string }) {
  const page = usePage();
  const nxm = useNxmState();
  return (
    <div className="nxm-wait">
      <div className="nxm-top">
        <span className="nxm-ring"><i className="src-dot src-nx" /></span>
        <div>
          <b>{t('nx.waitTitle')}</b>
          <small>{t('nx.waitSub')}</small>
        </div>
      </div>
      <ol className="together-steps nxm-steps">
        <li>{t('nx.step1')}</li>
        <li>{t('nx.step2')}</li>
        <li>{t('nx.step3')}</li>
      </ol>
      {nxm && nxm.supported && !nxm.enabled && (
        <div className="trust trust-warn nxm-handler">
          <Icon name="link" size={15} />
          <span>{nxm.other ? t('nx.handlerOther') : t('nx.handlerOff')}</span>
          <button className="act act-get" onClick={() => void setNxmHandler(true).catch(page.fail)}>{t('nx.turnOn')}</button>
        </div>
      )}
      {nxm && !nxm.supported && <p className="ws-note warn">{t('nx.unsupported')}</p>}
      <div className="host-actions">
        <button className="act act-ghost" onClick={() => void openUrl(filesUrl)}>{t('nx.openFiles')} <Icon name="ext" size={12} /></button>
        <button className="act act-ghost" onClick={() => cancelNxm(it.ref)}>{t('common.cancel')}</button>
      </div>
    </div>
  );
}

function ModSheet({ item, onClose }: { item: ModItem; onClose: () => void }) {
  const page = usePage();
  useMods();
  useWorkshop();
  const [full, setFull] = useState<ModItem>(item);
  const [plan, setPlan] = useState<InstallPlan | null>(null);
  const [menu, setMenu] = useState(false);
  const game = page.g.canon ?? page.g.key;
  const st = modStatus(item.ref, page.appid);
  const can = canInstall(full);
  useEscape(onClose);
  useEffect(() => {
    let live = true;
    getMod(item.ref).then((x) => live && setFull({ ...item, ...x, game: item.game || x.game }), () => {});
    // What an Install brings along (dependencies), for sources that resolve them.
    if (item.installable && item.source !== 'ws' && item.source !== 'nx') previewPlan(item.ref, game).then((p) => live && setPlan(p), () => {});
    return () => { live = false; };
  }, [item.ref]);
  const it = full;
  const also = (item as Hit).also ?? [];
  const deps = plan?.deps ?? [];
  useRefItems(deps.map((d) => d.ref));
  const missing = plan?.missing ?? [];
  const files = it.files ?? [];
  const pick = (fileId: string) => {
    clearFailed(it.ref);
    installMod(it.ref, game, { file: fileId, appid: page.appid }).then(
      (r) => r.kind === 'installed' && page.ctx.flash(t('mods.installedToast', { name: it.title })),
      (e) => (e instanceof ModsError && e.code === 'needs_nexus_login' ? page.openNexus() : page.fail(e)),
    );
  };

  return (
    <div className="scrim" onClick={onClose}>
      <aside className="detail ws-detail" onClick={(e) => e.stopPropagation()}>
        <button className="detail-close" onClick={onClose} aria-label={t('common.close')}><Icon name="x" size={16} /></button>
        <div className="ws-detail-cover">
          <Preview item={it} className="ws-detail-bg" />
          <Preview item={it} className="ws-detail-img" />
        </div>
        <div className="detail-body">
          <div className="card-pair"><SourceBadge s={it.source} />{page.game}{it.nsfw && <span className="chip">18+</span>}</div>
          <h1>{it.title}</h1>
          <div className="byline">
            {it.author && <span>{t('ws.by', { author: it.author })}</span>}
            {it.url && <a onClick={() => void openUrl(it.url)}>{t('mods.viewOn', { source: SOURCE_NAME[it.source] })} <Icon name="ext" size={11} /></a>}
            {also.map((a) => <a key={a.ref} onClick={() => void openUrl(a.url)}>{SOURCE_NAME[a.source]} <Icon name="ext" size={11} /></a>)}
          </div>
          <div className="detail-cta">
            <ModButton it={it} big />
            <div className="ws-add">
              <button className={`act act-ghost act-big ws-addbig ${menu ? 'on' : ''}`} onClick={() => setMenu(!menu)}>
                <Icon name="stack" size={15} /> {t('ws.addToLib')}
              </button>
              {menu && <AddMenu refs={[libRef(it.ref)]} onClose={() => setMenu(false)} />}
            </div>
          </div>

          {st.kind === 'nxm' && <NxmPanel it={it} filesUrl={st.filesUrl} />}
          {st.kind === 'installing' && (
            <div className="mod-progress">
              <div><b>{modStatusLabel(st, it.source === 'ws')}</b><small>{Math.round(st.pct)}%</small></div>
              <em className="bar" style={{ ['--p' as string]: `${st.pct}%` }} />
              <small>{t('mods.progressHint')}</small>
            </div>
          )}
          {st.kind === 'failed' && <div className="join-error mod-failed"><span>{st.error}</span></div>}
          {can.ok && st.kind === 'none' && it.source === 'nx' && viaNexusClick(it) && <p className="ws-note mod-nxhint">{t('mods.installViaNexusHint')}</p>}
          {!can.ok && st.kind === 'none' && (
            <div className="trust trust-warn mod-why">
              <Icon name="ext" size={15} />
              <span>{whyText(it)}</span>
            </div>
          )}
          {st.kind === 'installed' && it.source !== 'ws' && (
            <div className="trust">
              <Icon name="restore" size={15} />
              <span>{t('mods.restoreHint', { game: page.game })}</span>
            </div>
          )}

          {deps.length > 0 && (
            <>
              <h4>{t('mods.alsoInstalls')}</h4>
              <div className="needs">
                {deps.map((d) => {
                  const ds = modStatus(d.ref);
                  const di = refItem(d.ref);
                  return (
                    <div key={d.ref} className={`need ws-need ${ds.kind === 'installed' ? 'have' : ''}`} onClick={() => di && page.openMod(di)}>
                      <Preview item={di ?? { ref: d.ref, title: d.name }} />
                      <div>
                        <b>{d.name}</b>
                        <span>{ds.kind === 'installed' ? <><Icon name="check" size={12} /> {t('mods.depHave')}</> : t('mods.depVersion', { version: d.version })}</span>
                      </div>
                    </div>
                  );
                })}
              </div>
            </>
          )}

          {missing.length > 0 && (
            <>
              <h4>{t('mods.missingDeps')}</h4>
              <p className="ws-req-hint">{t('mods.missingDepsHint')}</p>
              <div className="needs">
                {missing.map((m) => (
                  <div key={m} className="need ws-need lack">
                    <Preview item={{ ref: m, title: m }} />
                    <div><b>{m}</b><span>{t('mods.missingDeps')}</span></div>
                  </div>
                ))}
              </div>
            </>
          )}

          <div className="facts">
            {typeof it.downloads === 'number' && <div><small>{t('mods.downloads')}</small>{fmtCount(it.downloads)}</div>}
            {typeof it.likes === 'number' && <div><small>{it.source === 'nx' ? t('mods.endorsements') : t('mods.likes')}</small>{fmtCount(it.likes)}</div>}
            {it.version && <div><small>{t('mods.version')}</small>{it.version}</div>}
            {!!it.sizeBytes && <div><small>{t('ws.size')}</small>{bytes(it.sizeBytes)}</div>}
            {!!it.updated && <div><small>{t('ws.updated')}</small>{when(it.updated)}</div>}
            {!!it.created && <div><small>{t('ws.created')}</small>{when(it.created)}</div>}
          </div>

          {files.length > 0 && (it.source === 'nx' || it.source === 'cf' || it.source === 'mio' || files.length > 1) && (
            <>
              <h4>{t('mods.files')}</h4>
              <div className="mod-files">
                {files.map((f) => (
                  <div key={f.id} className="mod-file">
                    <div>
                      <b>{f.name}{f.primary && <span className="chip">{t('mods.mainFile')}</span>}</b>
                      <small>{[f.version && `v${f.version}`, f.sizeBytes && bytes(f.sizeBytes), f.updated && when(f.updated)].filter(Boolean).join(' · ')}</small>
                    </div>
                    {can.ok && st.kind !== 'installed' && st.kind !== 'installing' && (
                      <button className="act act-ghost" onClick={() => pick(f.id)}><Icon name="download" size={13} /> {t('mods.install')}</button>
                    )}
                  </div>
                ))}
              </div>
            </>
          )}

          {it.tags.length > 0 && (
            <>
              <h4>{t('ws.tags')}</h4>
              <div className="ws-chips">{it.tags.map((tg) => <span key={tg} className="chip">{tg}</span>)}</div>
            </>
          )}

          <h4>{t('ws.description')}</h4>
          <p className="ws-desc">{it.description || it.summary || t('ws.noDescription')}</p>
        </div>
      </aside>
    </div>
  );
}

// ---------- Nexus account ----------

function NexusSheet({ onClose }: { onClose: () => void }) {
  const page = usePage();
  const { status, error } = useNexus();
  const [busy, setBusy] = useState(false);
  useEscape(onClose);
  useMods();
  const soon = error?.code === 'nexus_unavailable';
  const nxm = useNxmState();
  // "Use my Nexus API key": open by itself while SSO sign-in is not available.
  const [keyOpen, setKeyOpen] = useState(false);
  const [key, setKey] = useState('');
  const [keyBusy, setKeyBusy] = useState(false);
  const [keyErr, setKeyErr] = useState<string | null>(null);
  const keyShape = /^[A-Za-z0-9+/=_-]{16,512}$/.test(key.trim());
  const saveKey = async () => {
    if (!keyShape || keyBusy) return;
    setKeyBusy(true);
    setKeyErr(null);
    try {
      const s = await nexusSetKey(key);
      setKey('');
      setKeyOpen(false);
      page.ctx.flash(t('nx.loggedInToast', { name: s.name ?? SOURCE_SHORT.nx }));
    } catch (e) {
      setKeyErr(e instanceof ModsError && e.code === 'needs_nexus_login' ? t('nx.keyInvalid') : errorText(e, page.game));
    } finally {
      setKeyBusy(false);
    }
  };
  const login = async () => {
    setBusy(true);
    try {
      const s = await nexusLogin();
      page.ctx.flash(t('nx.loggedInToast', { name: s.name ?? SOURCE_SHORT.nx }));
    } catch (e) {
      if (!(e instanceof ModsError && e.code === 'nexus_unavailable')) page.fail(e);
    } finally {
      setBusy(false);
    }
  };
  return (
    <div className="scrim scrim-center" onClick={onClose}>
      <div className="picker ws-sheet nx-sheet" onClick={(e) => e.stopPropagation()}>
        <button className="detail-close" onClick={onClose} aria-label={t('common.close')}><Icon name="x" size={16} /></button>
        <span className="eyebrow">{SOURCE_NAME.nx}</span>
        <h2>{t('nx.title')}</h2>
        <p className="hint ws-import-lede">{t('nx.lede')}</p>
        {status?.loggedIn ? (
          <div className="nx-acct">
            <span className="nx-avatar">{(status.name ?? '?').slice(0, 1).toUpperCase()}</span>
            <div>
              <b>{status.name ?? t('nx.signedIn')}</b>
              <small>{status.premium ? t('nx.premium') : t('nx.free')}</small>
            </div>
            <button className="act act-ghost" onClick={() => void nexusLogout().catch(page.fail)}>{t('nx.logout')}</button>
          </div>
        ) : (
          <div className="nx-acct">
            <span className="nx-avatar off"><i className="src-dot src-nx" /></span>
            <div>
              <b>{t('nx.notSignedIn')}</b>
              <small>{soon ? t('nx.ssoSoonSub') : t('nx.notSignedInSub')}</small>
            </div>
            {!soon && (
              <button className={`act act-get ${busy ? 'act-busy ws-indet' : ''}`} disabled={busy || status === null} onClick={() => void login()}>
                <span>{busy ? t('nx.waitingBrowser') : t('nx.login')}</span>
              </button>
            )}
          </div>
        )}
        {!status?.loggedIn && (
          <div className={`nx-key ${keyOpen || soon ? 'open' : ''}`}>
            {!(keyOpen || soon) ? (
              <button className="nx-key-open" onClick={() => setKeyOpen(true)}>
                <Icon name="link" size={14} /> {t('nx.keyUse')}
              </button>
            ) : (
              <form onSubmit={(e) => { e.preventDefault(); void saveKey(); }}>
                <b>{t('nx.keyUse')}</b>
                <small>{soon ? t('nx.keySoonSub') : t('nx.keySub')}</small>
                <ol className="together-steps nx-key-steps">
                  <li>
                    {t('nx.keyStep1')}{' '}
                    <a className="pv-link" onClick={() => void openUrl('https://www.nexusmods.com/users/myaccount?tab=api')}>{t('nx.keyOpenPage')} <Icon name="ext" size={11} /></a>
                  </li>
                  <li>{t('nx.keyStep2')}</li>
                </ol>
                <div className="nx-key-row">
                  <input
                    type="password" value={key} onChange={(e) => { setKey(e.target.value); setKeyErr(null); }} placeholder={t('nx.keyPlaceholder')}
                    spellCheck={false} autoComplete="off" autoCorrect="off" autoCapitalize="off" aria-label={t('nx.keyPlaceholder')} maxLength={600}
                  />
                  <button type="submit" className={`act act-get ${keyBusy ? 'act-busy ws-indet' : ''}`} disabled={!keyShape || keyBusy}>
                    <span>{keyBusy ? t('nx.keyChecking') : t('nx.keySave')}</span>
                  </button>
                </div>
                {keyErr ? <p className="ws-note warn">{keyErr}</p> : <p className="ws-note">{t('nx.keyPrivacy')}</p>}
              </form>
            )}
          </div>
        )}
        {(status?.loggedIn || !soon) && <p className="ws-note">{status?.premium ? t('nx.premiumNote') : t('nx.freeNote')}</p>}
        {nxm?.supported === false ? (
          <p className="ws-note warn">{t('nx.unsupported')}</p>
        ) : (
          <label className="nx-toggle">
            <input type="checkbox" checked={!!nxm?.enabled} disabled={!nxm} onChange={(e) => void setNxmHandler(e.target.checked).catch(page.fail)} />
            <span>
              <b>{t('nx.handler')}</b>
              <small>{nxm?.other && !nxm.enabled ? t('nx.handlerOther') : t('nx.handlerSub')}</small>
            </span>
          </label>
        )}
      </div>
    </div>
  );
}

// ---------- Mashups and servers ----------

const ofGame = (ctx: Ctx, canon: string | null) => (canon ? ctx.catalog.filter((m) => m.needs.includes(canon) || m.guest === canon || m.host === canon) : []);

function MashupsTab() {
  const page = usePage();
  const { ctx, g } = page;
  const list = ofGame(ctx, g.canon);
  return (
    <Section title={t('game.tabMashups')} sub={t('game.mashupsSub', { game: page.game })}>
      {list.length === 0 ? (
        <div className="empty">
          {t('game.mashupsNone', { game: page.game })}{' '}
          {g.canon && <a className="pv-link" onClick={() => { ctx.setPair([g.canon, null]); ctx.go('build'); }}>{t('game.buildOne')}</a>}
        </div>
      ) : (
        <div className="grid">{list.map((m, i) => <Card key={m.id} ctx={ctx} m={m} i={i} />)}</div>
      )}
    </Section>
  );
}

function ServersTab() {
  const page = usePage();
  const { ctx, g } = page;
  const { list, error } = useLobbies(ctx);
  const lobbies = (list ?? []).filter((l) => !!g.canon && l.games.includes(g.canon));
  const mine = !!ctx.hosted && !!g.canon && ctx.hosted.lobby.games.includes(g.canon);
  const hostable = ofGame(ctx, g.canon).sort((a, b) => Number(!!b.server) - Number(!!a.server));
  const players = lobbies.reduce((s, l) => s + l.players, 0);
  return (
    <>
      {mine && (
        <Section title={t('lobbies.yours')} sub={t('lobbies.yoursSub')}>
          <HostCard ctx={ctx} />
        </Section>
      )}
      <Section
        title={t('game.lobbiesTitle')}
        sub={t('game.lobbiesSub', { game: page.game })}
        aside={list && <span className="live-pill"><Icon name="live" size={10} /> {t('lobbies.openCount', { count: lobbies.length })} · {t('lobbies.playing', { count: players })}</span>}
      >
        {error && <div className="empty">{t('lobbies.unreachable', { error })}</div>}
        {!error && list === null && <div className="empty">{t('lobbies.looking')}</div>}
        {!error && list && lobbies.length === 0 && <div className="empty">{t('game.lobbiesNone', { game: page.game })}</div>}
        <div className="queue">{lobbies.map((l, i) => <LobbyRow key={l.id} ctx={ctx} l={l} i={i} />)}</div>
      </Section>
      <HostedWorlds ctx={ctx} game={g.canon ?? undefined} />
      <Section title={t('game.hostTitle')} sub={ctx.hosting ? t('game.hostSub', { hours: ctx.hosting.limits.hours }) : t('game.hostSubOwn')}>
        {hostable.length === 0 ? (
          <div className="empty">{t('game.hostNone', { game: page.game })}</div>
        ) : (
          <div className="grid grid-sm">{hostable.slice(0, 6).map((m, i) => <Card key={m.id} ctx={ctx} m={m} i={i} />)}</div>
        )}
      </Section>
    </>
  );
}

// ---------- The game page ----------

const TABS: { id: GameTab; label: Key }[] = [
  { id: 'mods', label: 'game.tabMods' },
  { id: 'mashups', label: 'game.tabMashups' },
  { id: 'servers', label: 'game.tabServers' },
  { id: 'libraries', label: 'ws.tabLibraries' },
  { id: 'workshop', label: 'game.tabWorkshop' },
];

function GamePage({ ctx, g, nav, banner, setBanner, fail, onImport, onHub }: {
  ctx: Ctx; g: GameInfo; nav: GameNav;
  banner: unknown; setBanner: (e: unknown) => void;
  fail: (e: unknown) => void; onImport: () => void; onHub: () => void;
}) {
  const appid = g.workshop ? g.appid : '';
  const [tab, setTab] = useState<GameTab>(() => nav.tab ?? (nav.lib || nav.link ? 'libraries' : 'mods'));
  const all = useLibraries();
  const libs = useMemo(() => (all ?? []).filter((l) => libGame(l) === g.key), [all, g.key]);
  const [item, setItem] = useState<WorkshopItem | null>(null);
  const [mod, setMod] = useState<ModItem | null>(null);
  const [nexus, setNexus] = useState(false);
  const [editing, setEditing] = useState<Library | null>(() => libs.find((l) => l.id === nav.lib) ?? null);
  const [sureAll, setSureAll] = useState(false);
  const [restoring, setRestoring] = useState(false);
  const subCount = useWorkshop(() => (appid ? subscribedIds(appid)?.length : undefined));
  const installed = useInstalledMods(g.canon ?? g.key) ?? [];
  const latest = useLatest({ ctx, libs });
  const mashups = ofGame(ctx, g.canon).length;
  const tabs = TABS.filter((x) => x.id !== 'workshop' || g.workshop);

  // A new arrival (library link, "open the Workshop") moves to its tab.
  useEffect(() => {
    if (nav.tab) setTab(nav.tab);
    else if (nav.lib || nav.link) setTab('libraries');
  }, [nav.n]);

  useEffect(() => {
    if (appid) readState(appid, null).then(() => setBanner(null), setBanner);
  }, [appid]);

  const addTo = useCallback((lib: Library | null, refs: string[]) => {
    const { flash } = latest.current.ctx;
    if (lib) {
      void saveLibrary({ ...lib, items: [...new Set([...lib.items, ...refs])], applied: false })
        .then(() => flash(t('ws.added', { name: lib.name })), fail);
    } else {
      const n = latest.current.libs.length + 1;
      const made = newLibrary(g.appid, n > 1 ? t('ws.newLibNameN', { n }) : t('ws.newLibName'), refs, undefined, g.canon ?? undefined);
      void saveLibrary(made).then(() => {
        flash(t('ws.added', { name: made.name }));
        setEditing(made);
      }, fail);
    }
  }, [g.key, fail]);
  const openNexus = useCallback(() => setNexus(true), []);
  const page = useMemo<Page>(() => ({ ctx, g, appid, game: g.name, openItem: setItem, openMod: setMod, fail, addTo, openNexus }), [ctx, g, appid, fail, addTo, openNexus]);

  const restoreAll = async () => {
    setRestoring(true);
    let n = 0;
    for (const r of installed) {
      try {
        await uninstallMod(r.ref);
        n++;
      } catch (e) {
        fail(e);
      }
    }
    setRestoring(false);
    setSureAll(false);
    ctx.flash(t('game.restoredAll', { count: n, game: g.name }));
  };
  const launch = g.scanned?.launch;
  const store = g.canon ? GAME[g.canon]?.store : undefined;
  const sourceNames = g.mod ? sourcesOf(g.mod, false).map((s) => SOURCE_NAME[s]) : [];

  return (
    <PageCtx.Provider value={page}>
      <div className="page ws-page">
        <section className="ws-hero">
          <div className="ws-hero-art" aria-hidden><GameHero ctx={ctx} gameKey={g.key} appid={g.appid} /></div>
          <div className="ws-hero-copy">
            <button className="ws-back" onClick={onHub}><Icon name="back" size={14} /> {t('ws.allGames')}</button>
            <span className="eyebrow">{g.scanned ? STORE_LABEL[g.scanned.store] : t('game.notOnPcShort')}{sourceNames.length > 0 && ` · ${sourceNames.join(' · ')}`}</span>
            <h1>{g.name}</h1>
            <p className="ws-stats">
              <span><b>{installed.length}</b> {t('game.statMods', { count: installed.length })}</span>
              {subCount !== undefined && <span><b>{subCount}</b> {t('ws.statSubscribed', { count: subCount })}</span>}
              <span><b>{libs.length}</b> {t('ws.statLibraries', { count: libs.length })}</span>
            </p>
          </div>
          <div className="ws-hero-actions">
            {sureAll ? (
              <div className="restore-all">
                <span>{t('game.restoreAllAsk', { count: installed.length })}</span>
                <button className={`act ws-danger ${restoring ? 'act-busy ws-indet' : ''}`} disabled={restoring} onClick={() => void restoreAll()}><span>{restoring ? t('ws.removing') : t('game.restoreAll')}</span></button>
                <button className="act act-ghost" disabled={restoring} onClick={() => setSureAll(false)}>{t('common.cancel')}</button>
              </div>
            ) : (
              <>
                {installed.length > 0 && (
                  <button className="act act-ghost" onClick={() => setSureAll(true)} title={t('game.restoreAllTitle', { game: g.name })}>
                    <Icon name="restore" size={14} /> {t('game.restoreAll')}
                  </button>
                )}
                {launch ? (
                  <button className="act act-play act-hero" onClick={() => { void launchGame(launch); ctx.flash(t('toast.launching', { name: g.name })); }} title={installed.length ? t('game.playTitleMods', { count: installed.length }) : t('lib.launchVanilla')}>
                    <Icon name="play" size={14} /> {t('card.play')}
                  </button>
                ) : !g.onPc && store ? (
                  <button className="act act-ghost" onClick={() => void openUrl(store)}>{t('game.getGame')} <Icon name="ext" size={12} /></button>
                ) : null}
              </>
            )}
          </div>
        </section>

        <nav className="ws-tabs" role="tablist">
          {tabs.map(({ id, label }) => (
            <button key={id} role="tab" aria-selected={tab === id} className={tab === id ? 'on' : ''} onClick={() => setTab(id)}>
              {t(label)}
              {id === 'mods' && installed.length > 0 && <em>{installed.length}</em>}
              {id === 'mashups' && mashups > 0 && <em>{mashups}</em>}
              {id === 'libraries' && libs.length > 0 && <em>{libs.length}</em>}
              {id === 'workshop' && subCount !== undefined && subCount > 0 && <em>{subCount}</em>}
            </button>
          ))}
        </nav>

        {!g.onPc && ctx.scan && (
          <div className="trust trust-warn game-missing">
            <Icon name="restore" size={16} />
            <span>{t('mods.notOnPc', { game: g.name })}</span>
          </div>
        )}
        {banner !== null && <SteamBanner error={banner} game={g.name} onRetry={() => readState(appid, null).then(() => setBanner(null), setBanner)} />}

        <div className="ws-panel" key={tab}>
          {tab === 'mods' && <ModsTab onWorkshop={() => setTab('workshop')} />}
          {tab === 'mashups' && <MashupsTab />}
          {tab === 'servers' && <ServersTab />}
          {tab === 'libraries' && <Libraries onEdit={setEditing} onImport={onImport} onNew={() => addTo(null, [])} />}
          {tab === 'workshop' && g.workshop && <WorkshopTab onImport={onImport} />}
        </div>
      </div>

      {item && <ItemSheet item={item} onClose={() => setItem(null)} />}
      {mod && <ModSheet key={mod.ref} item={mod} onClose={() => setMod(null)} />}
      {editing && <LibrarySheet lib={editing} onClose={() => setEditing(null)} />}
      {nexus && <NexusSheet onClose={() => setNexus(false)} />}
    </PageCtx.Provider>
  );
}

// ---------- The hub: every game, every library ----------

function Hub({ ctx, fail, onImport }: { ctx: Ctx; fail: (e: unknown) => void; onImport: (text: string) => void }) {
  const libs = useLibraries();
  const games = useModGames();
  const installed = useInstalledMods(null);
  const [text, setText] = useState('');
  useRefItems((libs ?? []).flatMap((l) => l.items.slice(0, 4)));
  const sorted = useMemo(() => [...(libs ?? [])].sort((a, b) => libGame(a).localeCompare(libGame(b)) || b.updated - a.updated), [libs]);
  // One card per game, whatever the store; games with mods or mod sources first.
  const list = useMemo(() => {
    const seen = new Set<string>();
    const out: { g: Game; key: string; mods: number; libs: number; sources: Source[] }[] = [];
    for (const g of ctx.scan?.games ?? []) {
      const key = keyOf(g);
      if (seen.has(key)) continue;
      seen.add(key);
      const mg = g.canon ? games?.find((x) => x.game === g.canon) : undefined;
      const sources = sourcesOf(mg, g.store === 'steam' && ctx.workshopOn);
      out.push({ g, key, mods: (installed ?? []).filter((r) => r.game === key).length, libs: (libs ?? []).filter((l) => libGame(l) === key).length, sources });
    }
    const weight = (x: (typeof out)[number]) => (x.mods ? 4 : 0) + (x.libs ? 2 : 0) + (x.sources.some((s) => s !== 'ws') ? 1 : 0);
    return out.sort((a, b) => weight(b) - weight(a));
  }, [ctx.scan, games, installed, libs, ctx.workshopOn]);

  return (
    <div className="page">
      <section className="together-hero">
        <span className="eyebrow">{t('game.hubEyebrow')}</span>
        <h1>{t('game.hubTitle1')}<br /><span className="chrome">{t('game.hubTitle2')}</span></h1>
        <p>{t('game.hubLede')}</p>
        <form className="invite-field" onSubmit={(e) => { e.preventDefault(); if (text.trim()) onImport(text); }}>
          <Icon name="link" size={16} />
          <input value={text} onChange={(e) => setText(e.target.value)} placeholder={t('ws.pastePlaceholder')} spellCheck={false} />
          <button className="act act-get" disabled={!parsePasted(text)}>{t('ws.importAction')}</button>
        </form>
      </section>

      <Section title={t('game.gamesTitle')} sub={t('game.gamesSub')}>
        {!ctx.scan ? <div className="empty">{t('lib.scanning')}</div> : list.length === 0 ? <div className="empty">{t('game.noGames')}</div> : (
          <div className="ws-games">
            {list.map(({ g, key, mods, libs: n, sources }, i) => (
              <button key={key} className="ws-game" style={{ ['--i' as string]: i }} onClick={() => ctx.game(key)}>
                <GameArt id={g.canon} name={g.name} src={g.artWide} wide={g.artWide} local={g.wideLocal} wideLocal={g.wideLocal} />
                <span>
                  <b>{g.name}</b>
                  <small>
                    {mods > 0 || n > 0
                      ? [mods > 0 && t('game.modsN', { count: mods }), n > 0 && t('ws.statLibrariesN', { count: n })].filter(Boolean).join(' · ')
                      : sources.length === 1 && sources[0] === 'ws' ? SOURCE_NAME.ws : sources.length ? t('game.sourcesN', { count: sources.length }) : t('game.noSourcesShort')}
                  </small>
                  {sources.length > 0 && (
                    <i className="game-srcs" title={sources.map((s) => SOURCE_NAME[s]).join(' · ')}>
                      {sources.map((s) => <i key={s} className={`src-dot src-${s}`} />)}
                    </i>
                  )}
                </span>
              </button>
            ))}
          </div>
        )}
      </Section>

      <Section title={t('ws.libsAll')} sub={t('ws.libsAllSub')}>
        {libs === null ? (
          <div className="empty">{t('ws.loadingLibs')}</div>
        ) : sorted.length === 0 ? (
          <div className="empty">{t('mods.libsNone')}</div>
        ) : (
          <div className="queue">
            {sorted.map((l, i) => <LibraryRow key={l.id} ctx={ctx} lib={l} i={i} withGame fail={fail} onOpen={() => ctx.game(libGame(l), { lib: l.id })} />)}
          </div>
        )}
      </Section>
    </div>
  );
}

/**
 * The Games view: a game's page when `nav.key` is set, else the hub. `nav.link`: a library link to import (asked
 * first); `nav.lib`: a library to open; `nav.tab`: the tab to show. App remounts the view per game.
 */
export function Games({ ctx, nav }: { ctx: Ctx; nav: GameNav }) {
  const games = useModGames();
  const [banner, setBanner] = useState<unknown>(null);
  const [importing, setImporting] = useState<{ text?: string; n?: number } | null>(null);
  // Each link that arrives (with the view, or while it is open) opens the import sheet once.
  const [seen, setSeen] = useState<number>();
  if (nav.link && nav.link.n !== seen) {
    setSeen(nav.link.n);
    setImporting({ text: nav.link.text, n: nav.link.n });
  }
  const g = nav.key ? resolveGame(ctx, nav.key, games?.find((x) => x.game === nav.key)) : null;
  const latest = useLatest(ctx);
  /** Steam refusals (not running...) show as a banner on a game page; others as a toast. */
  const fail = useCallback((e: unknown) => (g?.workshop && isCode(e, STEAM_BLOCKING) ? setBanner(e) : latest.current.flash(errorText(e, g?.name ?? ''))), [g?.key, g?.workshop]);
  const onSaved = (lib: Library) => {
    setImporting(null);
    ctx.game(libGame(lib), { tab: 'libraries' });
  };

  return (
    <>
      {g ? (
        <GamePage key={g.key} ctx={ctx} g={g} nav={nav} banner={banner} setBanner={setBanner} fail={fail} onImport={() => setImporting({})} onHub={() => ctx.game(null)} />
      ) : (
        <Hub ctx={ctx} fail={fail} onImport={(text) => setImporting({ text })} />
      )}
      {importing && <ImportSheet key={importing.n ?? 'typed'} ctx={ctx} initial={importing.text} onClose={() => setImporting(null)} onSaved={onSaved} fail={fail} />}
    </>
  );
}

export { steamGameKey };
