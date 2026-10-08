// Steam Workshop (docs/WORKSHOP.md): a hub (every library, every Steam game) and a page per game (browse, subscribed,
// libraries), the item sheet, the library sheet and the import sheet. Nothing is subscribed without a click.
import { createContext, useCallback, useContext, useEffect, useMemo, useRef, useState, type ReactNode } from 'react';
import type { Ctx, WorkshopNav } from '../App';
import { GAMES, steamHero } from '../data/games';
import { copyText, launchGame, openUrl, type Game } from '../lib/api';
import { imageOk, usePrivacy } from '../lib/privacy';
import {
  MAX_SHARE, STEAM_BLOCKING, STEAM_CLOSED, WorkshopError, applyLibrary, browse, cachedItem, deleteLibrary, fmtBytes, getCollection, getItems, isCode,
  isSubscribed, newLibrary, parsePasted, readState, removeLibrary, saveLibrary, shareLink, statusOf, steamUrl, subscribe, subscribedIds, unsubscribe,
  useItems, useLibraries, useWorkshop, type BrowsePage, type Library, type Sort, type Status, type WorkshopItem,
} from '../lib/workshop';
import { GameArt, Icon, fmtCount } from '../ui';
import { Section } from './shared';
import { date, day, getLocale, t, type Key } from '../i18n';

// ---------- Shared bits ----------

type Tab = 'browse' | 'subscribed' | 'libraries';
const TABS: { id: Tab; label: Key }[] = [
  { id: 'browse', label: 'ws.tabBrowse' },
  { id: 'subscribed', label: 'ws.tabSubscribed' },
  { id: 'libraries', label: 'ws.tabLibraries' },
];
const SORTS: { id: Sort; label: Key }[] = [
  { id: 'trend', label: 'ws.sortTrend' },
  { id: 'top', label: 'ws.sortTop' },
  { id: 'new', label: 'ws.sortNew' },
  { id: 'updated', label: 'ws.sortUpdated' },
];

const hash = (s: string) => [...s].reduce((h, c) => (h * 31 + c.charCodeAt(0)) >>> 0, 7);
const bytes = (n?: number) => fmtBytes(n, getLocale());
const thisYear = (s: number) => new Date(s * 1000).getFullYear() === new Date().getFullYear();
/** A Steam time (unix s): "Oct 7" this year, "Mar 10, 2014" before. */
const when = (s: number) => (thisYear(s) ? day(s * 1000) : date(new Date(s * 1000).toISOString()));
/** A Steam time, short: "Oct 7" this year, "2014" before. */
const whenShort = (s: number) => (thisYear(s) ? day(s * 1000) : String(new Date(s * 1000).getFullYear()));

/** The rating as a whole percent, when Steam has votes for it. */
const scorePct = (it: WorkshopItem) => (typeof it.score === 'number' && (it.votesUp ?? 1) + (it.votesDown ?? 0) > 0 ? Math.round(it.score * 100) : null);

/** An item's state on this PC in words (the subscribe button, library rows, requirements). */
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

/** The latest value, for callbacks that must stay the same function across renders. */
function useLatest<T>(v: T) {
  const ref = useRef(v);
  ref.current = v;
  return ref;
}

/** The scanned Steam game with this app id, if it is on this PC. */
const steamGame = (ctx: Ctx, appid: string): Game | undefined => ctx.scan?.games.find((g) => g.store === 'steam' && g.storeId === appid);
/** A game's name for an app id: the scan's, our catalog's, else "Steam app <id>". */
const gameTitle = (ctx: Ctx, appid: string) =>
  steamGame(ctx, appid)?.name ?? GAMES.find((g) => g.steam?.includes(appid))?.name ?? t('ws.steamApp', { appid });

/** What the core said, for players. */
function errorText(e: unknown, game: string): string {
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
  return t('ws.err.failed', { message: e instanceof Error ? e.message : String(e) });
}

/** Per game page: who to tell about errors, how to open an item. */
type Page = {
  ctx: Ctx; appid: string; game: string;
  openItem: (it: WorkshopItem) => void;
  /** Steam refusals (not running...) show as a banner on the page; others as a toast. */
  fail: (e: unknown) => void;
  addTo: (lib: Library | null, ids: string[]) => void;
};
const PageCtx = createContext<Page | null>(null);
const usePage = () => useContext(PageCtx)!;

function useEscape(f: () => void) {
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

/** An item's preview picture, or a generated tile with its title when there is none (or pictures are off). */
function Preview({ item, className = '' }: { item?: WorkshopItem; className?: string }) {
  const privacy = usePrivacy();
  const [broken, setBroken] = useState(false);
  const src = item?.preview;
  if (src && !broken && imageOk(src, privacy)) {
    return (
      <div className={`art ws-prev ${className}`}>
        <img src={src} alt="" draggable={false} loading="lazy" onError={() => setBroken(true)} />
      </div>
    );
  }
  return (
    <div className={`art art-gen ws-prev ${className}`} style={{ ['--h' as string]: hash(item?.id ?? '0') % 360 }}>
      <span>{item?.title ?? ''}</span>
    </div>
  );
}

/** The game's wide key art for the page header. */
function GameHero({ ctx, appid }: { ctx: Ctx; appid: string }) {
  const g = steamGame(ctx, appid);
  return (
    <GameArt
      hero
      id={g?.canon ?? null}
      name={g?.name ?? gameTitle(ctx, appid)}
      wide={steamHero(appid)}
      heroLocal={g?.heroLocal}
      wideLocal={g?.wideLocal}
    />
  );
}

/** Steam is closed, the helper is missing...: what happened and the one thing to do. */
function SteamBanner({ error, game, onRetry }: { error: unknown; game: string; onRetry: () => void }) {
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

function AddMenu({ appid, ids, onClose, up = false }: { appid: string; ids: string[]; onClose: () => void; up?: boolean }) {
  const page = usePage();
  const libs = (useLibraries() ?? []).filter((l) => l.appid === appid);
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
        const has = ids.every((id) => l.items.includes(id));
        return (
          <button key={l.id} disabled={has} onClick={() => { page.addTo(l, ids); onClose(); }}>
            <span>{l.name}</span>
            {has ? <Icon name="check" size={13} /> : <em>{t('ws.libItems', { count: l.items.length })}</em>}
          </button>
        );
      })}
      <button className="ws-menu-new" onClick={() => { page.addTo(null, ids); onClose(); }}>
        <Icon name="plus" size={13} /> <span>{t('ws.newLibrary')}</span>
      </button>
    </div>
  );
}

// ---------- Item card ----------

function ItemCard({ item, i }: { item: WorkshopItem; i: number }) {
  const page = usePage();
  const [menu, setMenu] = useState(false);
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
            {menu && <AddMenu appid={page.appid} ids={[item.id]} onClose={() => setMenu(false)} up />}
          </div>
        </div>
      </div>
    </article>
  );
}

function Skeletons({ n = 12 }: { n?: number }) {
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

function Browse({ onImport, onTab }: { onImport: () => void; onTab: (t: Tab) => void }) {
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
          <button className="act act-ghost" onClick={() => onTab('subscribed')}>{t('ws.tabSubscribed')}</button>
          <button className="act act-ghost" onClick={() => onTab('libraries')}>{t('ws.tabLibraries')}</button>
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
    <Section
      title={t('ws.tabSubscribed')}
      sub={t('ws.subscribedSub', { game: page.game })}
      aside={shown && shown.length > 0 && <span className="ws-count">{t('ws.libItems', { count: shown.length })}{total ? ` · ${bytes(total)}` : ''}</span>}
    >
      {error ? (
        <SteamBanner error={error} game={page.game} onRetry={() => setN(n + 1)} />
      ) : shown?.length === 0 ? (
        <div className="empty">{t('ws.subscribedNone', { game: page.game })}</div>
      ) : (
        <div className="ws-grid">{shown ? shown.map((it, i) => <ItemCard key={it.id} item={it} i={i} />) : <Skeletons n={6} />}</div>
      )}
    </Section>
  );
}

// ---------- Libraries ----------

function libSize(lib: Library) {
  return lib.items.reduce((s, id) => s + (cachedItem(id)?.sizeBytes ?? 0), 0);
}

/** Library actions with their busy state and toasts; shared by the row and the sheet. */
function useLibActions(ctx: Ctx, lib: Library, game: string, fail: (e: unknown) => void) {
  const all = useLibraries() ?? [];
  const [busy, setBusy] = useState<'apply' | 'remove' | null>(null);
  const apply = async () => {
    setBusy('apply');
    const before = lib.items.filter((id) => isSubscribed(lib.appid, id)).length;
    try {
      await applyLibrary(lib);
      ctx.flash(t('ws.libApplied', { name: lib.name, count: Math.max(0, lib.items.length - before) }));
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
  if (busy === 'apply') {
    const done = lib.items.filter((id) => statusOf(lib.appid, id).kind === 'installed').length;
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

function Mosaic({ ids }: { ids: string[] }) {
  const four = ids.slice(0, 4);
  return (
    <div className={`ws-mosaic n${four.length}`}>
      {four.map((id) => <Preview key={id} item={cachedItem(id) ?? { id, title: '' } as WorkshopItem} />)}
      {!four.length && <Icon name="stack" size={20} />}
    </div>
  );
}

function LibraryRow({ ctx, lib, i, onOpen, withGame = false, fail }: { ctx: Ctx; lib: Library; i: number; onOpen: () => void; withGame?: boolean; fail: (e: unknown) => void }) {
  const game = gameTitle(ctx, lib.appid);
  const { busy, apply, remove, share } = useLibActions(ctx, lib, game, fail);
  const size = libSize(lib);
  const from = lib.source?.kind === 'collection' ? t('ws.fromCollection') : lib.source?.kind === 'link' ? t('ws.fromLink') : null;
  return (
    <div className="lobby ws-lib" style={{ ['--i' as string]: i }} onClick={onOpen}>
      <Mosaic ids={lib.items} />
      <div className="qinfo">
        <b>{lib.name}</b>
        <span>
          {withGame && <>{game} · </>}
          {t('ws.libItems', { count: lib.items.length })}
          {size > 0 && ` · ${bytes(size)}`}
          {from && ` · ${from}`}
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

function Libraries({ onEdit, onImport, onNew }: { onEdit: (l: Library) => void; onImport: () => void; onNew: () => void }) {
  const page = usePage();
  const all = useLibraries();
  const libs = (all ?? []).filter((l) => l.appid === page.appid);
  // Titles and previews for the mosaics.
  useItems(libs.flatMap((l) => l.items.slice(0, 4)));
  useEffect(() => {
    readState(page.appid, null).catch(() => {});
  }, [page.appid]);
  return (
    <Section
      title={t('ws.tabLibraries')}
      sub={t('ws.libsSub')}
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
        <div className="empty">{t('ws.libsEmpty', { game: page.game })}</div>
      ) : (
        <div className="queue">{libs.map((l, i) => <LibraryRow key={l.id} ctx={page.ctx} lib={l} i={i} onOpen={() => onEdit(l)} fail={page.fail} />)}</div>
      )}
    </Section>
  );
}

// ---------- Library sheet ----------

function LibrarySheet({ lib: start, onClose }: { lib: Library; onClose: () => void }) {
  const page = usePage();
  useWorkshop();
  const libs = useLibraries() ?? [];
  const lib = libs.find((l) => l.id === start.id) ?? start;
  const [name, setName] = useState(lib.name);
  const items = useItems(lib.items);
  const [sure, setSure] = useState(false);
  const { busy, apply, remove, share } = useLibActions(page.ctx, lib, page.game, page.fail);
  useEscape(onClose);
  // Keyed on the set of ids: reordering does not ask Steam again.
  const idSet = [...lib.items].sort().join();
  useEffect(() => {
    readState(lib.appid, lib.items).catch(() => {});
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
  const have = lib.items.filter((id) => isSubscribed(lib.appid, id)).length;

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
          {lib.items.length > 0 && ` · ${t('ws.alreadySub', { count: have })}`}
        </p>

        <ol className="ws-list">
          {lib.items.map((id, i) => {
            const it = items[i];
            const st = statusOf(lib.appid, id);
            return (
              <li key={id} style={{ ['--i' as string]: i }}>
                <span className="ws-list-n">{String(i + 1).padStart(2, '0')}</span>
                <button className="ws-list-item" onClick={() => it && page.openItem(it)}>
                  <Preview item={it ?? ({ id, title: '' } as WorkshopItem)} />
                  <span>
                    <b>{it?.title ?? id}</b>
                    <small>
                      {st.kind === 'installed' ? <em className="ok"><Icon name="check" size={11} /> {statusLabel(st)}</em>
                        : st.kind === 'downloading' ? <em>{statusLabel(st)}{st.pct !== null ? ` ${Math.round(st.pct)}%` : ''}</em>
                        : <em className={st.kind === 'none' || st.kind === 'failed' ? 'dim' : ''}>{statusLabel(st)}</em>}
                      {it?.sizeBytes ? ` · ${bytes(it.sizeBytes)}` : ''}
                      {lib.addedByUs.includes(id) && lib.applied ? ` · ${t('ws.addedByLib')}` : ''}
                    </small>
                  </span>
                </button>
                <span className="ws-list-tools">
                  <button onClick={() => move(i, -1)} disabled={i === 0} title={t('ws.moveUp')} aria-label={t('ws.moveUp')}><Icon name="up" size={14} /></button>
                  <button onClick={() => move(i, 1)} disabled={i === lib.items.length - 1} title={t('ws.moveDown')} aria-label={t('ws.moveDown')}><Icon name="down" size={14} /></button>
                  <button onClick={() => drop(id)} title={t('ws.removeItem')} aria-label={t('ws.removeItem')}><Icon name="x" size={14} /></button>
                </span>
              </li>
            );
          })}
        </ol>
        {lib.items.length === 0 && <div className="empty ws-list-empty">{t('ws.libEmptyItems')}</div>}

        <div className="ws-sheet-foot">
          {sure ? (
            <span className="ws-sure">
              <button className="act ws-danger" onClick={() => void deleteLibrary(lib.id).then(onClose, page.fail)}>{t('ws.deleteConfirm')}</button>
              <button className="act act-ghost" onClick={() => setSure(false)}>{t('common.cancel')}</button>
              <small>{lib.applied ? t('ws.deleteHintApplied') : t('ws.deleteHint')}</small>
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

/** `ids`: every id of the list, in order (a shared link may name items the proxy cannot resolve: they are kept). */
type Resolved = { appid: string; name: string; ids: string[]; items: WorkshopItem[]; source: Library['source'] };

function ImportSheet({ ctx, initial, onClose, onSaved, fail }: {
  ctx: Ctx; initial?: string; onClose: () => void; onSaved: (lib: Library) => void; fail: (e: unknown) => void;
}) {
  const [text, setText] = useState(initial ?? '');
  const [state, setState] = useState<'idle' | 'looking' | 'bad'>('idle');
  const [got, setGot] = useState<Resolved | null>(null);
  const [name, setName] = useState('');
  const [error, setError] = useState<string | null>(null);
  const [saving, setSaving] = useState(false);
  useWorkshop();
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
        const items = await getItems(p.share.ids);
        r = { appid: p.share.appid, name: p.share.name || t('ws.sharedName'), ids: p.share.ids, items, source: { kind: 'link' } };
      } else {
        const c = await getCollection(p.id);
        r = { appid: c.collection.appid || c.items[0]?.appid || '', name: c.collection.title, ids: c.items.map((i) => i.id), items: c.items, source: { kind: 'collection', id: p.id } };
      }
      if (n !== run.current) return;
      setGot(r);
      setName(r.name);
      setState('idle');
      if (steamGame(ctx, r.appid)) readState(r.appid, r.items.map((i) => i.id)).catch(() => {});
    } catch (e) {
      if (n !== run.current) return;
      setState('idle');
      setError(errorText(e, ''));
    }
  };
  useEffect(() => {
    if (initial) void look(initial);
  }, []);

  const owned = got ? !!steamGame(ctx, got.appid) : false;
  const game = got ? gameTitle(ctx, got.appid) : '';
  const have = got ? got.items.filter((i) => isSubscribed(got.appid, i.id)) : [];
  const toGet = got ? got.items.filter((i) => !isSubscribed(got.appid, i.id)).reduce((s, i) => s + (i.sizeBytes ?? 0), 0) : 0;

  const save = async (andApply: boolean) => {
    if (!got) return;
    setSaving(true);
    try {
      const lib = newLibrary(got.appid, name.trim().slice(0, 80) || got.name, got.ids, got.source);
      await saveLibrary(lib);
      ctx.flash(t('ws.saved', { name: lib.name }));
      onSaved(lib);
      if (andApply) void applyLibrary(lib).then(
        () => ctx.flash(t('ws.libApplied', { name: lib.name, count: Math.max(0, got.ids.length - have.length) })),
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
        <span className="eyebrow">{t('ws.eyebrow')}</span>
        <h2>{t('ws.importTitle')}</h2>
        <p className="hint ws-import-lede">{t('ws.importLede')}</p>
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
              <div className="ws-import-game"><GameHero ctx={ctx} appid={got.appid} /></div>
              <div>
                <small>{game}</small>
                <input className="ws-name ws-name-sm" value={name} onChange={(e) => setName(e.target.value)} aria-label={t('ws.libName')} maxLength={80} spellCheck={false} />
                <span>
                  {t('ws.libItems', { count: got.ids.length })}
                  {owned && have.length > 0 && ` · ${t('ws.alreadySub', { count: have.length })}`}
                  {owned && toGet > 0 && ` · ${t('ws.toDownload', { size: bytes(toGet) })}`}
                </span>
              </div>
            </div>
            <div className="ws-import-grid">
              {got.items.slice(0, 11).map((it) => (
                <div key={it.id} className={`ws-import-it ${isSubscribed(got.appid, it.id) ? 'have' : ''}`} title={it.title}>
                  <Preview item={it} />
                  {isSubscribed(got.appid, it.id) && <i><Icon name="check" size={11} /></i>}
                </div>
              ))}
              {got.items.length > 11 && <div className="ws-import-more">{t('ws.moreItems', { count: got.items.length - 11 })}</div>}
            </div>
            {got.ids.length >= MAX_SHARE && got.source?.kind === 'link' && <p className="ws-note">{t('ws.capped', { count: MAX_SHARE })}</p>}
            {!owned && (
              <div className="trust trust-warn">
                <Icon name="restore" size={16} />
                <span>{ctx.scan ? t('ws.notOwned', { game }) : t('join.stillScanning')}</span>
              </div>
            )}
            {owned && got.ids.length === 0 && <p className="ws-note warn">{t('ws.importEmpty')}</p>}
            <div className="host-actions">
              <button className="act act-ghost" onClick={onClose}>{t('common.cancel')}</button>
              <button className="act act-ghost" disabled={!owned || saving || !got.ids.length} onClick={() => void save(false)}>{t('ws.save')}</button>
              <button className="act act-get" disabled={!owned || saving || !got.ids.length} onClick={() => void save(true)}>
                <Icon name="download" size={14} /> {t('ws.saveApply')}
              </button>
            </div>
            <p className="ws-note">{t('ws.importSafe')}</p>
          </div>
        )}
      </div>
    </div>
  );
}

// ---------- Item sheet ----------

function ItemSheet({ item, onClose }: { item: WorkshopItem; onClose: () => void }) {
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
            <SubButton appid={page.appid} item={item} big />
            <div className="ws-add">
              <button className={`act act-ghost act-big ws-addbig ${menu ? 'on' : ''}`} onClick={() => setMenu(!menu)}>
                <Icon name="stack" size={15} /> {t('ws.addToLib')}
              </button>
              {menu && <AddMenu appid={page.appid} ids={[item.id, ...missingReq.filter((id) => !!cachedItem(id))]} onClose={() => setMenu(false)} />}
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
              {missingReq.length > 0 && (
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

// ---------- The game page ----------

function GamePage({ ctx, appid, game, openLib, tab, setTab, banner, setBanner, fail, onImport, onHub }: {
  ctx: Ctx; appid: string; game: string; openLib?: string;
  tab: Tab; setTab: (t: Tab) => void;
  banner: unknown; setBanner: (e: unknown) => void;
  fail: (e: unknown) => void; onImport: () => void; onHub: () => void;
}) {
  const owned = !!steamGame(ctx, appid);
  const all = useLibraries();
  const libs = useMemo(() => (all ?? []).filter((l) => l.appid === appid), [all, appid]);
  const [item, setItem] = useState<WorkshopItem | null>(null);
  const [editing, setEditing] = useState<Library | null>(() => libs.find((l) => l.id === openLib) ?? null);
  const subCount = useWorkshop(() => subscribedIds(appid)?.length);
  const latest = useLatest({ ctx, libs });

  useEffect(() => {
    if (owned) readState(appid, null).then(() => setBanner(null), setBanner);
  }, [appid, owned]);

  const addTo = useCallback((lib: Library | null, ids: string[]) => {
    const { flash } = latest.current.ctx;
    if (lib) {
      void saveLibrary({ ...lib, items: [...new Set([...lib.items, ...ids])], applied: lib.applied && ids.every((id) => isSubscribed(appid, id)) })
        .then(() => flash(t('ws.added', { name: lib.name })), fail);
    } else {
      const n = latest.current.libs.length + 1;
      const made = newLibrary(appid, n > 1 ? t('ws.newLibNameN', { n }) : t('ws.newLibName'), ids);
      void saveLibrary(made).then(() => {
        flash(t('ws.added', { name: made.name }));
        setEditing(made);
      }, fail);
    }
  }, [appid, fail]);
  const page = useMemo<Page>(() => ({ ctx, appid, game, openItem: setItem, fail, addTo }), [ctx, appid, game, fail, addTo]);

  if (!owned && ctx.scan) {
    return (
      <div className="page">
        <button className="ws-back" onClick={onHub}><Icon name="back" size={14} /> {t('ws.allGames')}</button>
        <div className="empty">{t('ws.notOwned', { game })}</div>
      </div>
    );
  }

  return (
    <PageCtx.Provider value={page}>
      <div className="page ws-page">
        <section className="ws-hero">
          <div className="ws-hero-art" aria-hidden><GameHero ctx={ctx} appid={appid} /></div>
          <div className="ws-hero-copy">
            <button className="ws-back" onClick={onHub}><Icon name="back" size={14} /> {t('ws.allGames')}</button>
            <span className="eyebrow">{t('ws.eyebrow')}</span>
            <h1>{game}</h1>
            <p className="ws-stats">
              {subCount !== undefined && <span><b>{subCount}</b> {t('ws.statSubscribed', { count: subCount })}</span>}
              <span><b>{libs.length}</b> {t('ws.statLibraries', { count: libs.length })}</span>
            </p>
          </div>
          <div className="ws-hero-actions">
            <button className="act act-ghost" onClick={onImport}><Icon name="link" size={14} /> {t('ws.importAction')}</button>
            <button className="act act-ghost" onClick={() => void launchGame(`steam://url/SteamWorkshopPage/${appid}`)}>{t('ws.openSteam')} <Icon name="ext" size={12} /></button>
          </div>
        </section>

        <nav className="ws-tabs" role="tablist">
          {TABS.map(({ id, label }) => (
            <button key={id} role="tab" aria-selected={tab === id} className={tab === id ? 'on' : ''} onClick={() => setTab(id)}>
              {t(label)}
              {id === 'subscribed' && subCount !== undefined && <em>{subCount}</em>}
              {id === 'libraries' && libs.length > 0 && <em>{libs.length}</em>}
            </button>
          ))}
        </nav>

        {banner !== null && <SteamBanner error={banner} game={game} onRetry={() => readState(appid, null).then(() => setBanner(null), setBanner)} />}

        <div className="ws-panel" key={tab}>
          {tab === 'browse' && <Browse onImport={onImport} onTab={setTab} />}
          {tab === 'subscribed' && <Subscribed />}
          {tab === 'libraries' && <Libraries onEdit={setEditing} onImport={onImport} onNew={() => addTo(null, [])} />}
        </div>
      </div>

      {item && <ItemSheet item={item} onClose={() => setItem(null)} />}
      {editing && <LibrarySheet lib={editing} onClose={() => setEditing(null)} />}
    </PageCtx.Provider>
  );
}

// ---------- The hub: every library, every Steam game ----------

function Hub({ ctx, fail, onImport }: { ctx: Ctx; fail: (e: unknown) => void; onImport: (text: string) => void }) {
  const libs = useLibraries();
  const steam = (ctx.scan?.games ?? []).filter((g) => g.store === 'steam');
  const [text, setText] = useState('');
  useItems((libs ?? []).flatMap((l) => l.items.slice(0, 4)));
  // Libraries sorted by game, then most recently changed.
  const sorted = useMemo(() => [...(libs ?? [])].sort((a, b) => a.appid.localeCompare(b.appid) || b.updated - a.updated), [libs]);
  // A library opens on its game's page, where its items can be opened and added to.
  const open = (l: Library) => (steamGame(ctx, l.appid) ? ctx.workshop(l.appid, undefined, l.id) : ctx.flash(t('ws.notOwned', { game: gameTitle(ctx, l.appid) })));

  return (
    <div className="page">
      <section className="together-hero">
        <span className="eyebrow">{t('ws.eyebrow')}</span>
        <h1>{t('ws.hubTitle1')}<br /><span className="chrome">{t('ws.hubTitle2')}</span></h1>
        <p>{t('ws.hubLede')}</p>
        <form className="invite-field" onSubmit={(e) => { e.preventDefault(); if (text.trim()) onImport(text); }}>
          <Icon name="link" size={16} />
          <input value={text} onChange={(e) => setText(e.target.value)} placeholder={t('ws.pastePlaceholder')} spellCheck={false} />
          <button className="act act-get" disabled={!parsePasted(text)}>{t('ws.importAction')}</button>
        </form>
      </section>

      <Section title={t('ws.libsAll')} sub={t('ws.libsAllSub')}>
        {libs === null ? (
          <div className="empty">{t('ws.loadingLibs')}</div>
        ) : sorted.length === 0 ? (
          <div className="empty">{t('ws.libsNone')}</div>
        ) : (
          <div className="queue">
            {sorted.map((l, i) => <LibraryRow key={l.id} ctx={ctx} lib={l} i={i} withGame fail={fail} onOpen={() => open(l)} />)}
          </div>
        )}
      </Section>

      <Section title={t('ws.games')} sub={t('ws.gamesSub')}>
        {!ctx.scan ? <div className="empty">{t('lib.scanning')}</div> : steam.length === 0 ? <div className="empty">{t('ws.noSteam')}</div> : (
          <div className="ws-games">
            {steam.map((g, i) => {
              const n = (libs ?? []).filter((l) => l.appid === g.storeId).length;
              return (
                <button key={g.key} className="ws-game" style={{ ['--i' as string]: i }} onClick={() => ctx.workshop(g.storeId)}>
                  <GameArt id={g.canon} name={g.name} src={g.artWide} wide={g.artWide} local={g.wideLocal} wideLocal={g.wideLocal} />
                  <span>
                    <b>{g.name}</b>
                    <small>{n ? t('ws.statLibrariesN', { count: n }) : t('ws.browseShort')}</small>
                  </span>
                </button>
              );
            })}
          </div>
        )}
      </Section>
    </div>
  );
}

/**
 * The Workshop view: a game's page when `nav.appid` is set, else the hub. `nav.link`: a library link to import (asked
 * first), shown over the game's libraries; `nav.lib`: a library to open. App remounts the view per game.
 */
export function Workshop({ ctx, nav }: { ctx: Ctx; nav: WorkshopNav }): ReactNode {
  const { appid } = nav;
  const [tab, setTab] = useState<Tab>(nav.lib ? 'libraries' : 'browse');
  const [banner, setBanner] = useState<unknown>(null);
  const [importing, setImporting] = useState<{ text?: string; n?: number } | null>(null);
  // Each link that arrives (with the view, or while it is open) opens the import sheet once.
  const [seen, setSeen] = useState<number>();
  if (nav.link && nav.link.n !== seen) {
    setSeen(nav.link.n);
    setImporting({ text: nav.link.text, n: nav.link.n });
    setTab('libraries');
  }
  const game = appid ? gameTitle(ctx, appid) : '';
  const latest = useLatest(ctx);
  /** Steam refusals (not running...) show as a banner on a game page; others as a toast. */
  const fail = useCallback((e: unknown) => (appid && isCode(e, STEAM_BLOCKING) ? setBanner(e) : latest.current.flash(errorText(e, game))), [appid, game]);
  const openImport = useCallback((text?: string) => setImporting({ text }), []);
  const openBlank = useCallback(() => setImporting({}), []);
  const onSaved = (lib: Library) => {
    setImporting(null);
    if (lib.appid === appid) setTab('libraries');
    else ctx.workshop(lib.appid);
  };

  return (
    <>
      {appid ? (
        <GamePage
          key={appid} ctx={ctx} appid={appid} game={game} openLib={nav.lib}
          tab={tab} setTab={setTab} banner={banner} setBanner={setBanner}
          fail={fail} onImport={openBlank} onHub={() => ctx.workshop(null)}
        />
      ) : (
        <Hub ctx={ctx} fail={fail} onImport={openImport} />
      )}
      {importing && <ImportSheet key={importing.n ?? 'typed'} ctx={ctx} initial={importing.text} onClose={() => setImporting(null)} onSaved={onSaved} fail={fail} />}
    </>
  );
}
