import type { Ctx } from '../App';
import type { Mashup } from '../data/catalog';
import { GAME } from '../data/games';
import { Avatar, Icon, MashupCover, fmtCount } from '../ui';
import { inTauri } from '../lib/api';

export const missing = (ctx: Ctx, m: Mashup) => m.needs.filter((g) => !ctx.owned.has(g));
export const gameName = (id?: string) => (id ? GAME[id]?.short ?? id : '');

export function ActionButton({ ctx, m, big = false }: { ctx: Ctx; m: Mashup; big?: boolean }) {
  const inst = ctx.installs[m.id];
  const miss = missing(ctx, m);
  const cls = `act ${big ? 'act-big' : ''}`;
  if (miss.length) {
    return (
      <button className={`${cls} act-miss`} onClick={(e) => { e.stopPropagation(); ctx.open(m); }}>
        Needs {miss.map(gameName).join(' + ')}
      </button>
    );
  }
  if (!inst && inTauri && !m.recipeUrl) {
    return (
      <span className={`${cls} act-soon`} title="Built and listed, the installable release is not published yet">
        Coming soon
      </span>
    );
  }
  if (!inst) {
    return (
      <button className={`${cls} act-get`} onClick={(e) => { e.stopPropagation(); ctx.get(m); }}>
        Get <small>{m.sizeMb} MB</small>
      </button>
    );
  }
  if (inst.phase !== 'ready') {
    const label = inst.phase === 'download' ? 'Downloading' : inst.phase === 'verify' ? 'Verifying' : 'Installing';
    return (
      <button className={`${cls} act-busy`} onClick={(e) => e.stopPropagation()} style={{ ['--p' as string]: `${Math.round(inst.pct)}%` }}>
        <span>{label}</span> <small>{Math.round(inst.pct)}%</small>
      </button>
    );
  }
  // A newer version is published: Play would start the old one (e.g. a fix the player is waiting for), so update first.
  if (inst.real && inst.version && m.version && inst.version !== m.version) {
    return (
      <button className={`${cls} act-get`} title={`You have v${inst.version}; v${m.version} is out`} onClick={(e) => { e.stopPropagation(); ctx.get(m); }}>
        Update <small>v{m.version}</small>
      </button>
    );
  }
  return (
    <button className={`${cls} act-play`} onClick={(e) => { e.stopPropagation(); ctx.play(m); }}>
      <Icon name="play" size={big ? 18 : 14} /> Play
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
        {m.kind === 'passthrough' && <span className="chip chip-prism">Real crossover</span>}
        {m.fresh && <span className="chip">New</span>}
        {m.status === 'beta' && <span className="chip">Beta</span>}
      </div>
      <div className="card-body">
        <div className="card-pair">
          {m.subtitle ?? <>{gameName(m.host)}{m.guest && <> <b>×</b> {gameName(m.guest)}</>}</>}
        </div>
        <h3>{m.name}</h3>
        <p>{bold(m.tagline)}</p>
        <div className="card-foot">
          <span className="meta">
            {m.avatar ? <Avatar src={m.avatar} /> : m.by.agent && <i className="dot-agent" title="Built by an AI agent" />}
            {m.by.agent ? m.by.name.replace(/^SIGF agent /, '$') : m.by.name}{m.plays > 0 && ` · ${fmtCount(m.plays)}`}
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
