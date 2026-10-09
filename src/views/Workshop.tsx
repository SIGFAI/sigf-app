// The Steam Workshop part of the game page (docs/WORKSHOP.md) and the pieces every tab shares: the page context, item
// previews, error text, the add-to-library menu, libraries (rows, sheet, import) across sources (docs/GAME-HUB.md
// section 6). The game page itself, the hub and the Mods tab are in views/Game.tsx. Nothing is subscribed or installed
// without a click.
import { createContext, useContext, useEffect, useRef, useState } from 'react';
import type { Ctx } from '../App';
import type { GameInfo } from './Game';
import { GAME, GAMES, steamHero } from '../data/games';
import { copyText, launchGame, openUrl } from '../lib/api';
import { imageOk, usePrivacy } from '../lib/privacy';
import { installErrorText } from '../i18n/errors';
import {
  MAX_SHARE, STEAM_BLOCKING, STEAM_CLOSED, WorkshopError, browse, deleteLibrary, fmtBytes, getCollection, getItems, isCode, isSubscribed,
  newLibrary, parsePasted, readState, saveLibrary, shareLink, statusOf, steamUrl, subscribe, subscribedIds, unsubscribe, useItems, useLibraries,
  useWorkshop, wsIdOf, type BrowsePage, type Library, type Sort, type Status, type WorkshopItem,
} from '../lib/workshop';
import {
  ModsError, SOURCE_NAME, applyLibrary, fromWorkshop, gameKeyOfAppid, getMod, libGame, modGameOf, modStatus, refHave, refItem, removeLibrary,
  sourceOfRef, useMods, useRefItems, type ModItem, type ModStatus,
} from '../lib/mods';
import { GameArt, Icon, fmtCount } from '../ui';
import { Section } from './shared';
import { date, day, getLocale, t, type Key } from '../i18n';

// ---------- Shared bits ----------

const SORTS: { id: Sort; label: Key }[] = [
  { id: 'trend', label: 'ws.sortTrend' },
  { id: 'top', label: 'ws.sortTop' },
  { id: 'new', label: 'ws.sortNew' },
  { id: 'updated', label: 'ws.sortUpdated' },
];

export const hash = (s: string) => [...s].reduce((h, c) => (h * 31 + c.charCodeAt(0)) >>> 0, 7);
export const bytes = (n?: number) => fmtBytes(n, getLocale());
const thisYear = (s: number) => new Date(s * 1000).getFullYear() === new Date().getFullYear();
/** A source's time (unix s): "Oct 7" this year, "Mar 10, 2014" before. */
export const when = (s: number) => (thisYear(s) ? day(s * 1000) : date(new Date(s * 1000).toISOString()));
/** A source's time, short: "Oct 7" this year, "2014" before. */
export const whenShort = (s: number) => (thisYear(s) ? day(s * 1000) : String(new Date(s * 1000).getFullYear()));

/** The rating as a whole percent, when Steam has votes for it. */
const scorePct = (it: WorkshopItem) => (typeof it.score === 'number' && (it.votesUp ?? 1) + (it.votesDown ?? 0) > 0 ? Math.round(it.score * 100) : null);

/** A Workshop item's state on this PC in words (the subscribe button, requirements). */
function statusLabel(s: Status): string {
  switch (s.kind) {
    case 'subscribing': return t('ws.subscribing');
    case 'unsubscribing': return t('ws.removing');
    case 'downloading': return t('ws.downloading');
    case 'subscribed': return t('ws.queued');
    case 'installed': return s.needsUpdate ? t('ws.needsUpdate') : t('ws.installed');
    default: return t('ws.notSubscribed');
  }
}

/** Any item's state on this PC in words (library rows): Workshop items as Steam says, the others as the engine does. */
export function modStatusLabel(s: ModStatus, ws: boolean): string {
  switch (s.kind) {
    case 'planning': return ws ? t('ws.subscribing') : t('mods.preparing');
    case 'installing': return t(s.phase === 'download' ? 'ws.downloading' : s.phase === 'verify' ? 'mods.checking' : 'mods.installing');
    case 'nxm': return t('mods.waitingNexus');
    case 'removing': return t('ws.removing');
    case 'installed': return t('ws.installed');
    case 'failed': return t('mods.failed');
    default: return ws ? t('ws.notSubscribed') : t('mods.notInstalled');
  }
}

/** The latest value, for callbacks that must stay the same function across renders. */
export function useLatest<T>(v: T) {
  const ref = useRef(v);
  ref.current = v;
  return ref;
}

/** A game's name for a game key (`lethal`, `steam:<appid>`): the scan's, our catalog's, the mods map's. */
export function keyName(ctx: Ctx, key: string): string {
  const appid = key.startsWith('steam:') ? key.slice(6) : null;
  const scanned = ctx.scan?.games.find((g) => (appid ? g.store === 'steam' && g.storeId === appid : g.canon === key));
  return scanned?.name ?? GAME[key]?.name ?? modGameOf(key)?.name ?? (appid ? t('ws.steamApp', { appid }) : key);
}

/** Whether a game (key) is on this PC. */
export function keyOnPc(ctx: Ctx, key: string): boolean {
  const appid = key.startsWith('steam:') ? key.slice(6) : null;
  return !!ctx.scan?.games.some((g) => (appid ? g.store === 'steam' && g.storeId === appid : g.canon === key));
}

/** What the core or a proxy said, for players. */
export function errorText(e: unknown, game: string): string {
  if (e instanceof WorkshopError) {
    switch (e.code) {
      case 'steam_not_running': return t('ws.err.steam_not_running');
      case 'not_owned': return t('ws.err.not_owned', { game });
      case 'not_steam_game': return t('ws.err.not_steam_game', { game });
      case 'helper_missing': return t('ws.err.helper_missing');
      case 'init_failed': return t('ws.err.init_failed');
      case 'search_unavailable': return t('ws.soonTitle');
      case 'not_found': return t('ws.badLink');
      case 'network': return t('ws.unreachable', { error: e.message });
      case 'library': return t('ws.err.library', { message: e.message });
      default: return t('ws.err.failed', { message: e.message });
    }
  }
  if (e instanceof ModsError) {
    switch (e.code) {
      case 'install': return installErrorText(e.detail);
      case 'not_on_pc': return t('mods.err.notOnPc', { game });
      case 'nexus_unavailable': return t('nx.soonTitle');
      case 'needs_nexus_login': return t('mods.err.needsNexus');
      case 'nxm_unknown': return t('mods.err.nxmUnknown');
      case 'nxm_expired': return t('mods.err.nxmExpired');
      case 'rate_limited': return t('mods.err.rateLimited');
      case 'link_only': return t('mods.err.linkOnly');
      case 'no_mc_version': return t('mods.err.noMcVersion', { profile: e.message });
      case 'cancelled': return t('mods.err.cancelled');
      case 'not_found': return t('mods.err.notFound');
      case 'network': return t('mods.err.network', { error: e.message });
      case 'source_unavailable':
      case 'no_source': return t('mods.soon');
      default: return t('mods.err.failed', { message: e.message });
    }
  }
  return t('ws.err.failed', { message: e instanceof Error ? e.message : String(e) });
}

/** Per game page: the game, who to tell about errors, how to open an item. */
export type Page = {
  ctx: Ctx; g: GameInfo;
  /** The game's Steam app id ('' when it is not a Steam game on this PC). */
  appid: string;
  /** The game's name. */
  game: string;
  openItem: (it: WorkshopItem) => void;
  openMod: (it: ModItem) => void;
  /** Steam refusals (not running...) show as a banner on the page; others as a toast. */
  fail: (e: unknown) => void;
  addTo: (lib: Library | null, refs: string[]) => void;
  /** The Nexus Mods account sheet (log in, "Mod manager download" links). */
  openNexus: () => void;
};
export const PageCtx = createContext<Page | null>(null);
export const usePage = () => useContext(PageCtx)!;

export function useEscape(f: () => void) {
  const ref = useRef(f);
  ref.current = f;
  useEffect(() => {
    const k = (e: KeyboardEvent) => {
      if (e.key === 'Escape') {
        e.stopPropagation();
        ref.current();
      }
    };
    window.addEventListener('keydown', k, true);
    return () => window.removeEventListener('keydown', k, true);
  }, []);
}

type Previewable = { title?: string; preview?: string; icon?: string; id?: string; ref?: string };
/** An item's picture, or a generated tile with its title when there is none (or pictures are off). */
export function Preview({ item, className = '' }: { item?: Previewable; className?: string }) {
  const privacy = usePrivacy();
  const [broken, setBroken] = useState(false);
  const src = item?.preview ?? item?.icon;
  if (src && !broken && imageOk(src, privacy)) {
    return (
      <div className={`art ws-prev ${className}`}>
        <img src={src} alt="" draggable={false} loading="lazy" onError={() => setBroken(true)} />
      </div>
    );
  }
  return (
    <div className={`art art-gen ws-prev ${className}`} style={{ ['--h' as string]: hash(item?.ref ?? item?.id ?? '0') % 360 }}>
      <span>{item?.title ?? ''}</span>
    </div>
  );
}

/** A game's wide key art for a page header. */
export function GameHero({ ctx, gameKey, appid }: { ctx: Ctx; gameKey: string; appid?: string }) {
  const g = ctx.scan?.games.find((x) => (appid ? x.store === 'steam' && x.storeId === appid : x.canon === gameKey)) ?? ctx.scan?.games.find((x) => x.canon === gameKey);
  const canon = gameKey.startsWith('steam:') ? g?.canon ?? null : gameKey;
  const sid = appid || (canon ? GAME[canon]?.steam?.[0] : undefined);
  return (
    <GameArt hero id={canon} name={g?.name ?? keyName(ctx, gameKey)} wide={sid ? steamHero(sid) : undefined} heroLocal={g?.heroLocal} wideLocal={g?.wideLocal} />
  );
}

/** Steam is closed, the helper is missing...: what happened and the one thing to do. */
export function SteamBanner({ error, game, onRetry }: { error: unknown; game: string; onRetry: () => void }) {
  const closed = isCode(error, STEAM_CLOSED);
  return (
    <div className="join-error ws-banner">
      <span><Icon name="restore" size={15} /> {errorText(error, game)}</span>
      <div className="host-actions">
        {closed && <button className="act act-get" onClick={() => void launchGame('steam://open/main')}>{t('ws.openSteamClient')}</button>}
        <button className="act act-ghost" onClick={onRetry}>{t('common.tryAgain')}</button>
      </div>
    </div>
  );
}

export { STEAM_BLOCKING };

// ---------- Subscribe button ----------

function SubButton({ appid, item, big = false }: { appid: string; item: WorkshopItem; big?: boolean }) {
  useWorkshop();
  const page = usePage();
  const s = statusOf(appid, item.id);
  const cls = `act ws-sub ${big ? 'act-big' : ''}`;
  const sub = () => subscribe(appid, [item.id]).catch(page.fail);
  const unsub = () => unsubscribe(appid, [item.id]).catch(page.fail);
  const stop = (f: () => void) => (e: React.MouseEvent) => {
    e.stopPropagation();
    f();
  };
  switch (s.kind) {
    case 'subscribing':
    case 'unsubscribing':
      return (
        <button className={`${cls} act-busy ws-indet`} onClick={stop(() => {})}>
          <span>{statusLabel(s)}</span>
        </button>
      );
    case 'downloading':
      return (
        <button className={`${cls} act-busy ${s.pct === null ? 'ws-indet' : ''}`} onClick={stop(() => {})} style={{ ['--p' as string]: `${Math.round(s.pct ?? 0)}%` }}>
          <span>{statusLabel(s)}</span> {s.pct !== null && <small>{Math.round(s.pct)}%</small>}
        </button>
      );
    case 'installed':
    case 'subscribed':
      return (
        <button className={`${cls} ws-have`} onClick={stop(unsub)} title={t('ws.unsubscribe')}>
          <span className="ws-have-on"><Icon name="check" size={14} /> {statusLabel(s)}</span>
          <span className="ws-have-off"><Icon name="x" size={13} /> {t('ws.unsubscribe')}</span>
        </button>
      );
    case 'failed':
      return (
        <button className={`${cls} act-miss`} onClick={stop(sub)} title={s.error}>
          <Icon name="restore" size={14} /> {t('ws.retry')}
        </button>
      );
    default:
      return (
        <button className={`${cls} act-get`} onClick={stop(sub)}>
          <Icon name="plus" size={14} /> {t('ws.subscribe')}
        </button>
      );
  }
}

// ---------- Add to library ----------

export function AddMenu({ refs, onClose, up = false }: { refs: string[]; onClose: () => void; up?: boolean }) {
  const page = usePage();
  const libs = (useLibraries() ?? []).filter((l) => libGame(l) === page.g.key);
  const ref = useRef<HTMLDivElement>(null);
  useEffect(() => {
    const away = (e: MouseEvent) => !ref.current?.contains(e.target as Node) && onClose();
    const id = setTimeout(() => window.addEventListener('mousedown', away));
    return () => {
      clearTimeout(id);
      window.removeEventListener('mousedown', away);
    };
  }, []);
  useEscape(onClose);
  return (
    <div className={`ws-menu ${up ? 'ws-menu-up' : ''}`} ref={ref} onClick={(e) => e.stopPropagation()}>
      <small>{t('ws.addToLib')}</small>
      {libs.map((l) => {
        const has = refs.every((id) => l.items.includes(id));
        return (
          <button key={l.id} disabled={has} onClick={() => { page.addTo(l, refs); onClose(); }}>
            <span>{l.name}</span>
            {has ? <Icon name="check" size={13} /> : <em>{t('ws.libItems', { count: l.items.length })}</em>}
          </button>
        );
      })}
      <button className="ws-menu-new" onClick={() => { page.addTo(null, refs); onClose(); }}>
        <Icon name="plus" size={13} /> <span>{t('ws.newLibrary')}</span>
      </button>
    </div>
  );
}

/** The stack button on a card: add this item to one of the game's libraries. */
export function AddButton({ refs }: { refs: string[] }) {
  const [menu, setMenu] = useState(false);
  return (
    <div className="ws-add">
      <button
        className={`ws-icon ${menu ? 'on' : ''}`}
        title={t('ws.addToLib')}
        aria-label={t('ws.addToLib')}
        onClick={(e) => {
          e.stopPropagation();
          setMenu(!menu);
        }}
      >
        <Icon name="stack" size={15} />
      </button>
      {menu && <AddMenu refs={refs} onClose={() => setMenu(false)} up />}
    </div>
  );
}

// ---------- Workshop item card ----------

function ItemCard({ item, i }: { item: WorkshopItem; i: number }) {
  const page = usePage();
  const score = scorePct(item);
  return (
    <article className="ws-card" style={{ ['--i' as string]: i % 30 }} onClick={() => page.openItem(item)}>
      <div className="ws-thumb">
        <Preview item={item} />
        {item.kind === 'collection' && <span className="chip ws-thumb-chip">{t('ws.collection')}</span>}
        {!!item.sizeBytes && <span className="ws-size">{bytes(item.sizeBytes)}</span>}
      </div>
      <div className="ws-card-body">
        <h3 title={item.title}>{item.title}</h3>
        <div className="ws-meta">
          <span title={t('ws.subsCount', { count: item.subs })}><Icon name="people" size={12} />{fmtCount(item.subs)}</span>
          {score !== null && (
            <span className="ws-score" title={t('ws.ratingTitle', { pct: score })}>
              <i style={{ ['--p' as string]: `${score}%` }} />{score}%
            </span>
          )}
          {item.updated > 0 && <span title={`${t('ws.updated')} ${when(item.updated)}`}>{whenShort(item.updated)}</span>}
        </div>
        <div className="ws-card-foot">
          <SubButton appid={page.appid} item={item} />
          <AddButton refs={[item.id]} />
        </div>
      </div>
    </article>
  );
}

export function Skeletons({ n = 12 }: { n?: number }) {
  return (
    <>
      {Array.from({ length: n }, (_, i) => (
        <div key={i} className="ws-card ws-skel" style={{ ['--i' as string]: i }}>
          <div className="ws-thumb" />
          <div className="ws-card-body"><b /><i /><span /></div>
        </div>
      ))}
    </>
  );
}

// ---------- Browse ----------

function Browse({ onImport, onSubscribed }: { onImport: () => void; onSubscribed: () => void }) {
  const page = usePage();
  const [sort, setSort] = useState<Sort>('trend');
  const [q, setQ] = useState('');
  const [query, setQuery] = useState('');
  const [tag, setTag] = useState<string | null>(null);
  const [res, setRes] = useState<BrowsePage | null>(null);
  const [error, setError] = useState<unknown>(null);
  const [more, setMore] = useState(false);
  const [tags, setTags] = useState<string[]>([]);
  const [retry, setRetry] = useState(0);
  const run = useRef(0);
  const items = res?.items ?? null;
  const next = res?.next ?? null;
  const foot = useRef<HTMLDivElement>(null);

  // Typing settles for 350 ms before the Workshop is asked.
  useEffect(() => {
    const id = setTimeout(() => setQuery(q), 350);
    return () => clearTimeout(id);
  }, [q]);

  useEffect(() => {
    const n = ++run.current;
    setRes(null);
    setError(null);
    browse(page.appid, { sort, q: query, tag }).then(
      (p) => {
        if (n !== run.current) return;
        setRes(p);
        // Subscribed / installed state for what is shown (one helper call per page of results).
        if (p.items.length) readState(page.appid, p.items.map((i) => i.id)).catch(() => {});
        // Tags to filter by: the most common ones on the first unfiltered page (cheap, no extra request).
        if (!tag && !query) {
          const count = new Map<string, number>();
          for (const it of p.items) for (const tg of it.tags) count.set(tg, (count.get(tg) ?? 0) + 1);
          setTags([...count.entries()].filter(([, c]) => c > 1 && c < p.items.length).sort((a, b) => b[1] - a[1]).slice(0, 9).map(([tg]) => tg));
        }
      },
      (e) => n === run.current && setError(e),
    );
  }, [page.appid, sort, query, tag, retry]);

  const loadMore = async () => {
    if (!next || more) return;
    const n = run.current;
    setMore(true);
    try {
      const p = await browse(page.appid, { sort, q: query, tag, cursor: next });
      if (n !== run.current) return;
      setRes((cur) => {
        const seen = new Set((cur?.items ?? []).map((i) => i.id));
        return { items: [...(cur?.items ?? []), ...p.items.filter((i) => !seen.has(i.id))], next: p.next, total: cur?.total ?? p.total };
      });
      if (p.items.length) readState(page.appid, p.items.map((i) => i.id)).catch(() => {});
    } catch (e) {
      page.fail(e);
    } finally {
      setMore(false);
    }
  };

  // Infinite scroll: the next page loads as the foot of the grid comes near.
  useEffect(() => {
    const el = foot.current;
    if (!el || !next) return;
    const io = new IntersectionObserver((es) => es.some((x) => x.isIntersecting) && void loadMore(), { rootMargin: '600px 0px' });
    io.observe(el);
    return () => io.disconnect();
  }, [next, more, items?.length]);

  if (error instanceof WorkshopError && error.code === 'search_unavailable') {
    return (
      <div className="ws-soon">
        <span className="ws-soon-icon"><Icon name="search" size={22} /></span>
        <h3>{t('ws.soonTitle')}</h3>
        <p>{t('ws.soonBody')}</p>
        <div className="host-actions">
          <button className="act act-get" onClick={onImport}><Icon name="link" size={14} /> {t('ws.importAction')}</button>
          <button className="act act-ghost" onClick={onSubscribed}>{t('ws.tabSubscribed')}</button>
        </div>
      </div>
    );
  }

  return (
    <>
      <div className="ws-toolbar">
        <label className="search ws-search">
          <Icon name="search" size={15} />
          <input value={q} onChange={(e) => setQ(e.target.value)} placeholder={t('ws.search', { game: page.game })} spellCheck={false} />
          {q && <button className="ws-clear" onClick={() => setQ('')} aria-label={t('common.close')}><Icon name="x" size={12} /></button>}
        </label>
        <div className="seg">
          {SORTS.map((s) => (
            <button key={s.id} className={sort === s.id ? 'on' : ''} onClick={() => setSort(s.id)}>{t(s.label)}</button>
          ))}
        </div>
        {res && <span className="ws-count">{t('ws.results', { count: res.total })}</span>}
      </div>
      {tags.length > 0 && (
        <div className="ws-tags">
          <button className={!tag ? 'on' : ''} onClick={() => setTag(null)}>{t('ws.allTags')}</button>
          {tags.map((tg) => (
            <button key={tg} className={tag === tg ? 'on' : ''} onClick={() => setTag(tag === tg ? null : tg)}>{tg}</button>
          ))}
        </div>
      )}
      {error ? (
        <SteamBanner error={error} game={page.game} onRetry={() => setRetry((r) => r + 1)} />
      ) : items?.length === 0 ? (
        <div className="empty">{query || tag ? t('ws.noResults') : t('ws.noItems')}</div>
      ) : (
        <div className="ws-grid">
          {items ? items.map((it, i) => <ItemCard key={it.id} item={it} i={i} />) : <Skeletons />}
          {more && <Skeletons n={6} />}
        </div>
      )}
      <div className="ws-foot" ref={foot}>
        {next && !more && <button className="act act-ghost" onClick={() => void loadMore()}>{t('ws.loadMore')}</button>}
      </div>
    </>
  );
}

// ---------- Subscribed ----------

function Subscribed() {
  const page = usePage();
  useWorkshop();
  const [items, setItems] = useState<WorkshopItem[] | null>(null);
  const [error, setError] = useState<unknown>(null);
  const [n, setN] = useState(0);
  useEffect(() => {
    let live = true;
    setError(null);
    readState(page.appid, null)
      .then((list) => getItems(list.filter((s) => s.subscribed).map((s) => s.id)))
      .then((got) => live && setItems(got), (e) => live && setError(e));
    return () => { live = false; };
  }, [page.appid, n]);
  // Unsubscribed from a card here: it leaves the list.
  const shown = items?.filter((i) => isSubscribed(page.appid, i.id) || statusOf(page.appid, i.id).kind !== 'none');
  const total = (shown ?? []).reduce((s, i) => s + (i.sizeBytes ?? 0), 0);
  return (
    <>
      <div className="ws-toolbar">
        <p className="ws-sub-lede">{t('ws.subscribedSub', { game: page.game })}</p>
        {shown && shown.length > 0 && <span className="ws-count">{t('ws.libItems', { count: shown.length })}{total ? ` · ${bytes(total)}` : ''}</span>}
      </div>
      {error ? (
        <SteamBanner error={error} game={page.game} onRetry={() => setN(n + 1)} />
      ) : shown?.length === 0 ? (
        <div className="empty">{t('ws.subscribedNone', { game: page.game })}</div>
      ) : (
        <div className="ws-grid">{shown ? shown.map((it, i) => <ItemCard key={it.id} item={it} i={i} />) : <Skeletons n={6} />}</div>
      )}
    </>
  );
}

/** The Workshop tab (Steam games, Windows): browse and what is subscribed, plus Steam's own page. */
export function WorkshopTab({ onImport }: { onImport: () => void }) {
  const page = usePage();
  const [part, setPart] = useState<'browse' | 'subscribed'>('browse');
  const subCount = useWorkshop(() => subscribedIds(page.appid)?.length);
  return (
    <>
      <div className="ws-subnav">
        <div className="seg">
          <button className={part === 'browse' ? 'on' : ''} onClick={() => setPart('browse')}>{t('ws.tabBrowse')}</button>
          <button className={part === 'subscribed' ? 'on' : ''} onClick={() => setPart('subscribed')}>
            {t('ws.tabSubscribed')}{subCount !== undefined && <em>{subCount}</em>}
          </button>
        </div>
        <span className="ws-subnav-hint">{t('ws.tabHint')}</span>
        <button className="act act-ghost" onClick={() => void launchGame(`steam://url/SteamWorkshopPage/${page.appid}`)}>{t('ws.openSteam')} <Icon name="ext" size={12} /></button>
      </div>
      <div className="ws-panel" key={part}>
        {part === 'browse' ? <Browse onImport={onImport} onSubscribed={() => setPart('subscribed')} /> : <Subscribed />}
      </div>
    </>
  );
}

// ---------- Libraries ----------

function libSize(lib: Library) {
  return lib.items.reduce((s, r) => s + (refItem(r)?.sizeBytes ?? 0), 0);
}

/** Library actions with their busy state and toasts; shared by the row and the sheet. */
function useLibActions(ctx: Ctx, lib: Library, game: string, fail: (e: unknown) => void) {
  const all = useLibraries() ?? [];
  const [busy, setBusy] = useState<'apply' | 'remove' | null>(null);
  const apply = async () => {
    setBusy('apply');
    try {
      const r = await applyLibrary(lib, ctx.workshopOn);
      const parts = [t('ws.libApplied', { name: lib.name, count: r.added })];
      if (r.waiting) parts.push(t('mods.libWaiting', { count: r.waiting }));
      if (r.failed) parts.push(t('mods.libFailed', { count: r.failed }));
      ctx.flash(parts.join(' · '));
    } catch (e) {
      fail(e);
    } finally {
      setBusy(null);
    }
  };
  const remove = async () => {
    setBusy('remove');
    try {
      const kept = await removeLibrary(lib, all);
      ctx.flash(kept ? `${t('ws.libRemoved', { name: lib.name })} · ${t('ws.libKept', { count: kept })}` : t('ws.libRemoved', { name: lib.name }));
    } catch (e) {
      fail(e);
    } finally {
      setBusy(null);
    }
  };
  const share = async () => {
    const ok = await copyText(shareLink(lib));
    ctx.flash(ok ? t('ws.linkCopied', { game }) : shareLink(lib));
  };
  return { busy, apply, remove, share };
}

function ApplyButton({ lib, busy, apply, remove }: { lib: Library; busy: 'apply' | 'remove' | null; apply: () => void; remove: () => void }) {
  useWorkshop();
  useMods();
  if (busy === 'apply') {
    const done = lib.items.filter((r) => refHave(r, lib.appid)).length;
    return (
      <button className="act act-busy" style={{ ['--p' as string]: `${(done / Math.max(1, lib.items.length)) * 100}%` }} onClick={(e) => e.stopPropagation()}>
        <span>{t('ws.applying', { done, total: lib.items.length })}</span>
      </button>
    );
  }
  if (busy === 'remove') return <button className="act act-busy ws-indet" onClick={(e) => e.stopPropagation()}><span>{t('ws.removing')}</span></button>;
  if (lib.applied) {
    return (
      <button className="act act-ghost" onClick={(e) => { e.stopPropagation(); remove(); }} title={t('ws.removeTitle')}>
        <Icon name="restore" size={14} /> {t('ws.remove')}
      </button>
    );
  }
  return (
    <button className="act act-get" disabled={!lib.items.length} onClick={(e) => { e.stopPropagation(); apply(); }}>
      <Icon name="download" size={14} /> {t('ws.apply')}
    </button>
  );
}

function Mosaic({ refs }: { refs: string[] }) {
  const four = refs.slice(0, 4);
  return (
    <div className={`ws-mosaic n${four.length}`}>
      {four.map((r) => <Preview key={r} item={refItem(r) ?? { ref: r, title: '' }} />)}
      {!four.length && <Icon name="stack" size={20} />}
    </div>
  );
}

/** The sources a library mixes, as small dots with names (Workshop, Thunderstore, Nexus...). */
function SourceDots({ refs }: { refs: string[] }) {
  const set = [...new Set(refs.map(sourceOfRef))];
  if (set.length < 2) return null;
  return (
    <span className="lib-srcs" title={set.map((s) => SOURCE_NAME[s]).join(' · ')}>
      {set.map((s) => <i key={s} className={`src-dot src-${s}`} />)}
    </span>
  );
}

export function LibraryRow({ ctx, lib, i, onOpen, withGame = false, fail }: { ctx: Ctx; lib: Library; i: number; onOpen: () => void; withGame?: boolean; fail: (e: unknown) => void }) {
  const game = keyName(ctx, libGame(lib));
  const { busy, apply, remove, share } = useLibActions(ctx, lib, game, fail);
  const size = libSize(lib);
  const from = lib.source?.kind === 'collection' ? t('ws.fromCollection') : lib.source?.kind === 'link' ? t('ws.fromLink') : null;
  return (
    <div className="lobby ws-lib" style={{ ['--i' as string]: i }} onClick={onOpen}>
      <Mosaic refs={lib.items} />
      <div className="qinfo">
        <b>{lib.name}</b>
        <span>
          {withGame && <>{game} · </>}
          {t('ws.libItems', { count: lib.items.length })}
          {size > 0 && ` · ${bytes(size)}`}
          {from && ` · ${from}`}
          <SourceDots refs={lib.items} />
        </span>
      </div>
      <span className={`ws-state ${lib.applied ? 'on' : ''}`}><i />{lib.applied ? t('ws.applied') : t('ws.notApplied')}</span>
      <div className="ws-lib-actions">
        <button className="ws-icon" title={t('ws.share')} aria-label={t('ws.share')} onClick={(e) => { e.stopPropagation(); void share(); }}><Icon name="link" size={15} /></button>
        <ApplyButton lib={lib} busy={busy} apply={() => void apply()} remove={() => void remove()} />
      </div>
    </div>
  );
}

export function Libraries({ onEdit, onImport, onNew }: { onEdit: (l: Library) => void; onImport: () => void; onNew: () => void }) {
  const page = usePage();
  const all = useLibraries();
  const libs = (all ?? []).filter((l) => libGame(l) === page.g.key);
  // Titles and previews for the mosaics.
  useRefItems(libs.flatMap((l) => l.items.slice(0, 4)));
  useEffect(() => {
    if (page.appid && page.ctx.workshopOn) readState(page.appid, null).catch(() => {});
  }, [page.appid]);
  return (
    <Section
      title={t('ws.tabLibraries')}
      sub={t('mods.libsSub')}
      aside={
        <div className="host-actions ws-head-actions">
          <button className="act act-ghost" onClick={onImport}><Icon name="link" size={14} /> {t('ws.importAction')}</button>
          <button className="act act-get" onClick={onNew}><Icon name="plus" size={14} /> {t('ws.newLibrary')}</button>
        </div>
      }
    >
      {all === null ? (
        <div className="empty">{t('ws.loadingLibs')}</div>
      ) : libs.length === 0 ? (
        <div className="empty">{t('mods.libsEmpty', { game: page.game })}</div>
      ) : (
        <div className="queue">{libs.map((l, i) => <LibraryRow key={l.id} ctx={page.ctx} lib={l} i={i} onOpen={() => onEdit(l)} fail={page.fail} />)}</div>
      )}
    </Section>
  );
}

// ---------- Library sheet ----------

export function LibrarySheet({ lib: start, onClose }: { lib: Library; onClose: () => void }) {
  const page = usePage();
  useWorkshop();
  const libs = useLibraries() ?? [];
  const lib = libs.find((l) => l.id === start.id) ?? start;
  const [name, setName] = useState(lib.name);
  const items = useRefItems(lib.items);
  const [sure, setSure] = useState(false);
  const { busy, apply, remove, share } = useLibActions(page.ctx, lib, page.game, page.fail);
  useEscape(onClose);
  // Keyed on the set of ids: reordering does not ask Steam again.
  const ws = lib.items.map(wsIdOf).filter((x): x is string => !!x);
  const idSet = [...ws].sort().join();
  useEffect(() => {
    if (ws.length && lib.appid && page.ctx.workshopOn) readState(lib.appid, ws).catch(() => {});
  }, [lib.appid, idSet]);

  const save = (patch: Partial<Library>) => saveLibrary({ ...lib, ...patch }).catch(page.fail);
  const rename = () => {
    const n = name.trim();
    if (n && n !== lib.name) void save({ name: n.slice(0, 80) });
    else setName(lib.name);
  };
  const move = (i: number, d: -1 | 1) => {
    const next = [...lib.items];
    const j = i + d;
    if (j < 0 || j >= next.length) return;
    [next[i], next[j]] = [next[j], next[i]];
    void save({ items: next });
  };
  const drop = (id: string) => void save({ items: lib.items.filter((x) => x !== id), addedByUs: lib.addedByUs.filter((x) => x !== id) });
  const size = items.reduce((s, it) => s + (it?.sizeBytes ?? 0), 0);
  const have = lib.items.filter((r) => refHave(r, lib.appid)).length;
  const open = (r: string, it?: ModItem) => {
    const id = wsIdOf(r);
    if (id) void getItems([id]).then(([x]) => x && page.openItem(x));
    else if (it) page.openMod(it);
  };

  return (
    <div className="scrim scrim-center" onClick={onClose}>
      <div className="picker ws-sheet" onClick={(e) => e.stopPropagation()}>
        <button className="detail-close" onClick={onClose} aria-label={t('common.close')}><Icon name="x" size={16} /></button>
        <span className="eyebrow">{t('ws.libraryOf', { game: page.game })}</span>
        <input
          className="ws-name"
          value={name}
          onChange={(e) => setName(e.target.value)}
          onBlur={rename}
          onKeyDown={(e) => e.key === 'Enter' && e.currentTarget.blur()}
          aria-label={t('ws.libName')}
          maxLength={80}
          spellCheck={false}
        />
        <p className="ws-sheet-sub">
          <span className={`ws-state ${lib.applied ? 'on' : ''}`}><i />{lib.applied ? t('ws.applied') : t('ws.notApplied')}</span>
          {t('ws.libItems', { count: lib.items.length })}
          {size > 0 && ` · ${bytes(size)}`}
          {lib.items.length > 0 && ` · ${t('mods.alreadyHave', { count: have })}`}
        </p>

        <ol className="ws-list">
          {lib.items.map((r, i) => {
            const it = items[i];
            const isWs = !!wsIdOf(r);
            const src = sourceOfRef(r);
            const st = modStatus(r, lib.appid);
            return (
              <li key={r} style={{ ['--i' as string]: i }}>
                <span className="ws-list-n">{String(i + 1).padStart(2, '0')}</span>
                <button className="ws-list-item" onClick={() => open(r, it)}>
                  <Preview item={it ?? { ref: r, title: '' }} />
                  <span>
                    <b>{it?.title ?? r}</b>
                    <small>
                      <span className={`src-tag src-${src}`}><i className={`src-dot src-${src}`} />{SOURCE_NAME[src]}</span>
                      {' · '}
                      {st.kind === 'installed' ? <em className="ok"><Icon name="check" size={11} /> {modStatusLabel(st, isWs)}</em>
                        : st.kind === 'installing' ? <em>{modStatusLabel(st, isWs)} {Math.round(st.pct)}%</em>
                        : <em className={st.kind === 'none' || st.kind === 'failed' ? 'dim' : ''}>{modStatusLabel(st, isWs)}</em>}
                      {it?.sizeBytes ? ` · ${bytes(it.sizeBytes)}` : ''}
                      {lib.addedByUs.includes(r) && lib.applied ? ` · ${t('ws.addedByLib')}` : ''}
                    </small>
                  </span>
                </button>
                <span className="ws-list-tools">
                  <button onClick={() => move(i, -1)} disabled={i === 0} title={t('ws.moveUp')} aria-label={t('ws.moveUp')}><Icon name="up" size={14} /></button>
                  <button onClick={() => move(i, 1)} disabled={i === lib.items.length - 1} title={t('ws.moveDown')} aria-label={t('ws.moveDown')}><Icon name="down" size={14} /></button>
                  <button onClick={() => drop(r)} title={t('ws.removeItem')} aria-label={t('ws.removeItem')}><Icon name="x" size={14} /></button>
                </span>
              </li>
            );
          })}
        </ol>
        {lib.items.length === 0 && <div className="empty ws-list-empty">{t('mods.libEmptyItems')}</div>}

        <div className="ws-sheet-foot">
          {sure ? (
            <span className="ws-sure">
              <button className="act ws-danger" onClick={() => void deleteLibrary(lib.id).then(onClose, page.fail)}>{t('ws.deleteConfirm')}</button>
              <button className="act act-ghost" onClick={() => setSure(false)}>{t('common.cancel')}</button>
              <small>{lib.applied ? t('mods.deleteHintApplied') : t('mods.deleteHint')}</small>
            </span>
          ) : (
            <button className="pv-link" onClick={() => setSure(true)}><Icon name="trash" size={13} /> {t('ws.delete')}</button>
          )}
          {!sure && (
            <>
              <button className="act act-ghost" onClick={() => void share()} disabled={!lib.items.length}><Icon name="link" size={14} /> {t('ws.share')}</button>
              <ApplyButton lib={lib} busy={busy} apply={() => void apply()} remove={() => void remove()} />
            </>
          )}
        </div>
      </div>
    </div>
  );
}

// ---------- Import sheet ----------

/** `ids`: every ref of the list, in order (a shared link may name items the proxies cannot resolve: they are kept). */
type Resolved = { appid: string; key: string; name: string; ids: string[]; items: ModItem[]; source: Library['source'] };

export function ImportSheet({ ctx, initial, onClose, onSaved, fail }: {
  ctx: Ctx; initial?: string; onClose: () => void; onSaved: (lib: Library) => void; fail: (e: unknown) => void;
}) {
  const [text, setText] = useState(initial ?? '');
  const [state, setState] = useState<'idle' | 'looking' | 'bad'>('idle');
  const [got, setGot] = useState<Resolved | null>(null);
  const [name, setName] = useState('');
  const [error, setError] = useState<string | null>(null);
  const [saving, setSaving] = useState(false);
  useWorkshop();
  useMods();
  useEscape(onClose);
  const run = useRef(0);

  const look = async (s: string) => {
    const p = parsePasted(s);
    setGot(null);
    setError(null);
    if (!p) return setState(s.trim() ? 'bad' : 'idle');
    const n = ++run.current;
    setState('looking');
    try {
      let r: Resolved;
      if (p.kind === 'share') {
        const { appid, game, ids } = p.share;
        const key = game ?? gameKeyOfAppid(appid, ctx.scan?.games.find((g) => g.store === 'steam' && g.storeId === appid)?.canon);
        const ws = ids.map(wsIdOf).filter((x): x is string => !!x);
        const wsItems = new Map((await getItems(ws)).map((w) => [w.id, fromWorkshop(w, key)]));
        const others = await Promise.all(ids.filter((r) => !wsIdOf(r)).map((r) => getMod(r).catch(() => null)));
        const byRef = new Map(others.filter((x): x is ModItem => !!x).map((x) => [x.ref, x]));
        const items = ids.map((r) => (wsIdOf(r) ? wsItems.get(wsIdOf(r)!) : byRef.get(r))).filter((x): x is ModItem => !!x);
        r = { appid: appid || (GAME[key]?.steam?.[0] ?? ''), key, name: p.share.name || t('ws.sharedName'), ids, items, source: { kind: 'link' } };
      } else {
        const c = await getCollection(p.id);
        const appid = c.collection.appid || c.items[0]?.appid || '';
        const key = gameKeyOfAppid(appid, ctx.scan?.games.find((g) => g.store === 'steam' && g.storeId === appid)?.canon);
        r = { appid, key, name: c.collection.title, ids: c.items.map((i) => i.id), items: c.items.map((w) => fromWorkshop(w, key)), source: { kind: 'collection', id: p.id } };
      }
      if (n !== run.current) return;
      setGot(r);
      setName(r.name);
      setState('idle');
      const ws = r.ids.map(wsIdOf).filter((x): x is string => !!x);
      if (ws.length && r.appid && ctx.workshopOn && keyOnPc(ctx, r.key)) readState(r.appid, ws).catch(() => {});
    } catch (e) {
      if (n !== run.current) return;
      setState('idle');
      setError(errorText(e, ''));
    }
  };
  useEffect(() => {
    if (initial) void look(initial);
  }, []);

  const owned = got ? keyOnPc(ctx, got.key) : false;
  const game = got ? keyName(ctx, got.key) : '';
  const have = got ? got.ids.filter((r) => refHave(r, got.appid)) : [];
  const toGet = got ? got.items.filter((i) => !refHave(i.ref, got.appid)).reduce((s, i) => s + (i.sizeBytes ?? 0), 0) : 0;
  // Workshop items need the Steam helper (Windows): elsewhere they stay in the library, unapplied.
  const wsOnly = got ? got.ids.every((r) => !!wsIdOf(r)) : false;
  const canApply = owned && (!wsOnly || ctx.workshopOn);

  const save = async (andApply: boolean) => {
    if (!got) return;
    setSaving(true);
    try {
      const canon = got.key.startsWith('steam:') ? undefined : got.key;
      const lib = newLibrary(got.appid, name.trim().slice(0, 80) || got.name, got.ids, got.source, canon);
      await saveLibrary(lib);
      ctx.flash(t('ws.saved', { name: lib.name }));
      onSaved(lib);
      if (andApply) void applyLibrary(lib, ctx.workshopOn).then(
        (r) => ctx.flash(t('ws.libApplied', { name: lib.name, count: r.added })),
        fail,
      );
    } catch (e) {
      fail(e);
    } finally {
      setSaving(false);
    }
  };

  return (
    <div className="scrim scrim-center" onClick={onClose}>
      <div className="picker ws-sheet ws-import" onClick={(e) => e.stopPropagation()}>
        <button className="detail-close" onClick={onClose} aria-label={t('common.close')}><Icon name="x" size={16} /></button>
        <span className="eyebrow">{t('game.eyebrowLibraries')}</span>
        <h2>{t('ws.importTitle')}</h2>
        <p className="hint ws-import-lede">{t('mods.importLede')}</p>
        <form className="invite-field" onSubmit={(e) => { e.preventDefault(); void look(text); }}>
          <Icon name="link" size={16} />
          <input
            autoFocus={!initial}
            value={text}
            onChange={(e) => { setText(e.target.value); setState('idle'); }}
            onPaste={(e) => { const s = e.clipboardData.getData('text'); setTimeout(() => void look(s)); }}
            placeholder={t('ws.pastePlaceholder')}
            spellCheck={false}
          />
          <button className="act act-get" disabled={!text.trim() || state === 'looking'}>{t('ws.lookUp')}</button>
        </form>
        {state === 'bad' && <p className="ws-note warn">{t('ws.badLink')}</p>}
        {state === 'looking' && <div className="ws-import-skel"><i /><i /><i /><i /><i /><i /></div>}
        {error && <div className="join-error ws-note"><span>{error}</span></div>}

        {got && (
          <div className="ws-import-got">
            <div className="ws-import-head">
              <div className="ws-import-game"><GameHero ctx={ctx} gameKey={got.key} appid={got.appid} /></div>
              <div>
                <small>{game}</small>
                <input className="ws-name ws-name-sm" value={name} onChange={(e) => setName(e.target.value)} aria-label={t('ws.libName')} maxLength={80} spellCheck={false} />
                <span>
                  {t('ws.libItems', { count: got.ids.length })}
                  {owned && have.length > 0 && ` · ${t('mods.alreadyHave', { count: have.length })}`}
                  {owned && toGet > 0 && ` · ${t('ws.toDownload', { size: bytes(toGet) })}`}
                  <SourceDots refs={got.ids} />
                </span>
              </div>
            </div>
            <div className="ws-import-grid">
              {got.items.slice(0, 11).map((it) => (
                <div key={it.ref} className={`ws-import-it ${refHave(it.ref, got.appid) ? 'have' : ''}`} title={it.title}>
                  <Preview item={it} />
                  {refHave(it.ref, got.appid) && <i><Icon name="check" size={11} /></i>}
                </div>
              ))}
              {got.items.length > 11 && <div className="ws-import-more">{t('ws.moreItems', { count: got.items.length - 11 })}</div>}
            </div>
            {got.ids.length >= MAX_SHARE && got.source?.kind === 'link' && <p className="ws-note">{t('ws.capped', { count: MAX_SHARE })}</p>}
            {!owned && (
              <div className="trust trust-warn">
                <Icon name="restore" size={16} />
                <span>{ctx.scan ? t('mods.notOnPc', { game }) : t('join.stillScanning')}</span>
              </div>
            )}
            {owned && got.ids.length === 0 && <p className="ws-note warn">{t('ws.importEmpty')}</p>}
            <div className="host-actions">
              <button className="act act-ghost" onClick={onClose}>{t('common.cancel')}</button>
              <button className="act act-ghost" disabled={!owned || saving || !got.ids.length} onClick={() => void save(false)}>{t('ws.save')}</button>
              <button className="act act-get" disabled={!canApply || saving || !got.ids.length} onClick={() => void save(true)}>
                <Icon name="download" size={14} /> {t('ws.saveApply')}
              </button>
            </div>
            <p className="ws-note">{t('mods.importSafe')}</p>
          </div>
        )}
      </div>
    </div>
  );
}

// ---------- Workshop item sheet ----------

export function ItemSheet({ item, onClose }: { item: WorkshopItem; onClose: () => void }) {
  const page = usePage();
  useWorkshop();
  const [menu, setMenu] = useState(false);
  const requires = item.requires ?? [];
  const req = useItems(requires);
  useEscape(onClose);
  useEffect(() => {
    if (requires.length) readState(page.appid, requires).catch(() => {});
  }, [item.id]);
  const score = scorePct(item);
  const missingReq = requires.filter((id) => !isSubscribed(page.appid, id));
  return (
    <div className="scrim" onClick={onClose}>
      <aside className="detail ws-detail" onClick={(e) => e.stopPropagation()}>
        <button className="detail-close" onClick={onClose} aria-label={t('common.close')}><Icon name="x" size={16} /></button>
        <div className="ws-detail-cover">
          <Preview item={item} className="ws-detail-bg" />
          <Preview item={item} className="ws-detail-img" />
        </div>
        <div className="detail-body">
          <div className="card-pair">{page.game}{item.kind === 'collection' && <span className="chip">{t('ws.collection')}</span>}</div>
          <h1>{item.title}</h1>
          <div className="byline">
            {item.author && <span>{t('ws.by', { author: item.author })}</span>}
            <a onClick={() => void openUrl(item.url || steamUrl(item.id))}>{t('ws.viewOnSteam')} <Icon name="ext" size={11} /></a>
          </div>
          <div className="detail-cta">
            {page.ctx.workshopOn && page.appid ? <SubButton appid={page.appid} item={item} big /> : <span className="act act-soon act-big" title={t('mods.wsWindowsTitle')}>{t('mods.wsWindows')}</span>}
            <div className="ws-add">
              <button className={`act act-ghost act-big ws-addbig ${menu ? 'on' : ''}`} onClick={() => setMenu(!menu)}>
                <Icon name="stack" size={15} /> {t('ws.addToLib')}
              </button>
              {menu && <AddMenu refs={[item.id, ...missingReq.filter((id) => !!req[requires.indexOf(id)])]} onClose={() => setMenu(false)} />}
            </div>
          </div>

          {requires.length > 0 && (
            <>
              <h4>{t('ws.requires')}</h4>
              <p className="ws-req-hint">{t('ws.requiresSub')}</p>
              <div className="needs">
                {requires.map((id, i) => req[i] ?? ({ id, title: id } as WorkshopItem)).map((r) => {
                  const st = statusOf(page.appid, r.id);
                  const ok = st.kind === 'installed' || st.kind === 'subscribed';
                  return (
                    <div key={r.id} className={`need ws-need ${ok ? 'have' : 'lack'}`} onClick={() => r.url && page.openItem(r)}>
                      <Preview item={r} />
                      <div>
                        <b>{r.title}</b>
                        <span>{ok ? <><Icon name="check" size={12} /> {statusLabel(st)}</> : statusLabel(st)}</span>
                      </div>
                    </div>
                  );
                })}
              </div>
              {missingReq.length > 0 && page.ctx.workshopOn && (
                <button className="act act-ghost ws-req-all" onClick={() => void subscribe(page.appid, missingReq).catch(page.fail)}>
                  <Icon name="plus" size={14} /> {t('ws.subscribeAllReq', { count: missingReq.length })}
                </button>
              )}
            </>
          )}

          <div className="facts">
            <div><small>{t('ws.subscribers')}</small>{fmtCount(item.subs)}</div>
            <div><small>{t('ws.favorites')}</small>{fmtCount(item.favs)}</div>
            {score !== null && <div><small>{t('ws.rating')}</small><span className="ws-score ws-score-lg"><i style={{ ['--p' as string]: `${score}%` }} />{t('ws.ratingTitle', { pct: score })}</span></div>}
            {!!item.sizeBytes && <div><small>{t('ws.size')}</small>{bytes(item.sizeBytes)}</div>}
            {item.updated > 0 && <div><small>{t('ws.updated')}</small>{when(item.updated)}</div>}
            {item.created > 0 && <div><small>{t('ws.created')}</small>{when(item.created)}</div>}
          </div>

          {item.tags.length > 0 && (
            <>
              <h4>{t('ws.tags')}</h4>
              <div className="ws-chips">{item.tags.map((tg) => <span key={tg} className="chip">{tg}</span>)}</div>
            </>
          )}

          <h4>{t('ws.description')}</h4>
          <p className="ws-desc">{item.description || t('ws.noDescription')}</p>
        </div>
      </aside>
    </div>
  );
}

/** The game key of a Steam app id (the scan's canonical id first): Workshop links and `ctx.workshop` go through it. */
export const steamGameKey = (ctx: Ctx, appid: string) =>
  gameKeyOfAppid(appid, ctx.scan?.games.find((g) => g.store === 'steam' && g.storeId === appid)?.canon ?? GAMES.find((g) => g.steam?.includes(appid))?.id);

