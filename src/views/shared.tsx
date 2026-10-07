import type { Ctx, Phase } from '../App';
import { isCommunity, type Mashup } from '../data/catalog';
import { GAME } from '../data/games';
import { Avatar, Icon, MashupCover, fmtCount, shownDownloads } from '../ui';
import { inTauri } from '../lib/api';
import { t, type Key } from '../i18n';

const PHASE_LABEL: Record<Exclude<Phase, 'ready'>, Key> = { download: 'phase.download', verify: 'phase.verify', build: 'phase.build', install: 'phase.install' };

export const missing = (ctx: Ctx, m: Mashup) => m.needs.filter((g) => !ctx.owned.has(g));
export const gameName = (id?: string) => (id ? GAME[id]?.short ?? id : '');

export function ActionButton({ ctx, m, big = false }: { ctx: Ctx; m: Mashup; big?: boolean }) {
  const inst = ctx.installs[m.id];
  const miss = missing(ctx, m);
  const cls = `act ${big ? 'act-big' : ''}`;
  if (miss.length) {
    return (
      <button className={`${cls} act-miss`} onClick={(e) => { e.stopPropagation(); ctx.open(m); }}>
        {t('card.needs', { games: miss.map(gameName).join(' + ') })}
      </button>
    );
  }
  if (!inst && inTauri && !m.recipeUrl) {
    return (
      <span className={`${cls} act-soon`} title={t('card.soonTitle')}>
        {t('card.soon')}
      </span>
    );
  }
  if (!inst) {
    return (
      <button className={`${cls} act-get`} onClick={(e) => { e.stopPropagation(); ctx.get(m); }}>
        {t('card.get')} <small>{t('common.mb', { mb: m.sizeMb })}</small>
      </button>
    );
  }
  if (inst.phase !== 'ready') {
    const label = t(PHASE_LABEL[inst.phase]);
    return (
      <button className={`${cls} act-busy`} onClick={(e) => e.stopPropagation()} style={{ ['--p' as string]: `${Math.round(inst.pct)}%` }}>
        <span>{label}</span> <small>{Math.round(inst.pct)}%</small>
      </button>
    );
  }
  // A newer version is published: Play would start the old one (e.g. a fix the player is waiting for), so update first.
  if (inst.real && inst.version && m.version && inst.version !== m.version) {
    return (
      <button className={`${cls} act-get`} title={t('card.updateTitle', { have: inst.version, latest: m.version })} onClick={(e) => { e.stopPropagation(); ctx.get(m); }}>
        {t('card.update')} <small>v{m.version}</small>
      </button>
    );
  }
  return (
    <button className={`${cls} act-play`} onClick={(e) => { e.stopPropagation(); ctx.play(m); }}>
      <Icon name="play" size={big ? 18 : 14} /> {t('card.play')}
    </button>
  );
}

/** Catalog text may carry `**bold**`: render it, never show the asterisks. */
export function bold(text: string) {
  return text.split(/\*\*(.+?)\*\*/g).map((part, i) => (i % 2 ? <b key={i}>{part}</b> : part));
}

export function Card({ ctx, m, i = 0 }: { ctx: Ctx; m: Mashup; i?: number }) {
  const miss = missing(ctx, m);
  return (
    <article className={`card ${miss.length ? 'card-dim' : ''}`} onClick={() => ctx.open(m)} style={{ ['--i' as string]: i }}>
      <MashupCover host={m.host} guest={m.guest} cover={m.cover} clip={m.clip} />
      {m.clip && <video className="card-clip" src={m.clip} muted loop playsInline preload="none" onMouseEnter={(e) => e.currentTarget.play().catch(() => {})} onMouseLeave={(e) => { e.currentTarget.pause(); e.currentTarget.currentTime = 0; }} />}
      <div className="card-tags">
        {m.kind === 'passthrough' && <span className="chip chip-prism">{t('card.crossover')}</span>}
        {m.fresh && <span className="chip">{t('card.new')}</span>}
        {m.status === 'beta' && <span className="chip">{t('card.beta')}</span>}
      </div>
      <div className="card-body">
        <div className="card-pair">
          {m.subtitle ?? <>{gameName(m.host)}{m.guest && <> <b>×</b> {gameName(m.guest)}</>}</>}
        </div>
        <h3>{m.name}</h3>
        <p>{bold(m.tagline)}</p>
        <div className="card-foot">
          <span className="meta">
            {m.avatar ? <Avatar src={m.avatar} /> : m.by.agent && <i className="dot-agent" title={t('card.byAgent')} />}
            {isCommunity(m) ? <span title={t('card.communityTitle')}>{t('detail.byCommunity', { author: m.by.name })}</span> : m.by.agent ? m.by.name.replace(/^SIGF agent /, '$') : m.by.name}
            {shownDownloads(m.downloads) && <span className="card-dl" title={t('card.downloads', { count: m.downloads })}><Icon name="download" size={11} />{fmtCount(m.downloads)}</span>}
          </span>
          <ActionButton ctx={ctx} m={m} />
        </div>
      </div>
    </article>
  );
}

export function Section({ title, sub, children, aside }: { title: string; sub?: string; children: React.ReactNode; aside?: React.ReactNode }) {
  return (
    <section className="section">
      <header>
        <div>
          <h2>{title}</h2>
          {sub && <p>{sub}</p>}
        </div>
        {aside}
      </header>
      {children}
    </section>
  );
}
