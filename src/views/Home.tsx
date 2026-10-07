import { useMemo, useState } from 'react';
import type { Ctx } from '../App';
import { GAME } from '../data/games';
import { GameArt, Icon } from '../ui';
import { imageOk, usePrivacy } from '../lib/privacy';
import { Card, Section, gameName, missing } from './shared';
import { t, tx } from '../i18n';

type Filter = 'playable' | 'all' | 'crossover';

function Slot({ ctx, slot }: { ctx: Ctx; slot: 0 | 1 }) {
  const id = ctx.pair[slot];
  const g = id ? GAME[id] : null;
  const scanned = id ? ctx.scan?.games.find((x) => x.canon === id) : null;
  return (
    <button className={`slot ${id ? 'slot-full' : ''}`} onClick={() => ctx.pick(slot)}>
      {id ? (
        <>
          <GameArt id={id} name={g?.name ?? id} src={scanned?.art} wide={scanned?.artWide} local={scanned?.artLocal} wideLocal={scanned?.wideLocal} />
          <span className="slot-label">
            <small>{slot === 0 ? t('mix.host') : t('mix.guest')}</small>
            {g?.short ?? id}
          </span>
        </>
      ) : (
        <span className="slot-empty">
          <Icon name="plus" size={26} />
          <small>{slot === 0 ? t('mix.hostGame') : t('mix.guestGame')}</small>
          <em>{slot === 0 ? t('mix.hostHint') : t('mix.guestHint')}</em>
        </span>
      )}
    </button>
  );
}

export function Home({ ctx, query }: { ctx: Ctx; query: string }) {
  const [filter, setFilter] = useState<Filter>('playable');
  const privacy = usePrivacy();
  const [a, b] = ctx.pair;

  const pairHits = useMemo(() => {
    if (!a && !b) return [];
    return ctx.catalog.filter((m) => {
      const sides = [m.host, m.guest];
      return (!a || sides.includes(a)) && (!b || sides.includes(b));
    });
  }, [a, b]);

  const q = query.trim().toLowerCase();
  const list = ctx.catalog.filter((m) => {
    if (q) return [m.name, m.tagline, m.by.name, gameName(m.host), GAME[m.guest ?? '']?.name ?? m.guest].join(' ').toLowerCase().includes(q);
    if (filter === 'playable') return missing(ctx, m).length === 0;
    if (filter === 'crossover') return m.kind === 'passthrough';
    return true;
  });
  // Community and library mashups lead; what our own AI built gets its own section below them.
  const community = list.filter((m) => !m.by.agent);
  const byAi = list.filter((m) => m.by.agent);
  const oneAway = ctx.catalog.filter((m) => missing(ctx, m).length === 1);
  const playableCount = ctx.catalog.filter((m) => missing(ctx, m).length === 0).length;
  const live = ctx.agents.filter((x) => x.status === 'building' || x.live).slice(0, 8);

  const bgA = a && ctx.scan?.games.find((x) => x.canon === a);

  return (
    <div className="page">
      <section className="board">
        <div className="board-bg" aria-hidden>
          {a && <GameArt id={a} name={a} src={bgA ? bgA.art : undefined} local={bgA ? bgA.heroLocal ?? bgA.artLocal : undefined} />}
        </div>
        <div className="board-copy">
          <span className="eyebrow">{t('mix.eyebrow')}</span>
          <h1>
            {t('mix.title1')}<br />
            <span className="chrome">{t('mix.title2')}</span>
          </h1>
          <p>{t('mix.lede')}</p>
          {!ctx.scan ? (
            <div className="scanline">{t('mix.scanning')}</div>
          ) : (
            <div className="scanline done">
              {tx('mix.scanned', { games: <b>{ctx.scan.games.length}</b>, playable: <b>{playableCount}</b>, ms: ctx.scan.millis })}
            </div>
          )}
        </div>
        <div className="board-slots">
          <Slot ctx={ctx} slot={0} />
          <button className="x" onClick={() => ctx.setPair([b, a])} title={t('mix.swap')}>
            <span>×</span>
            <Icon name="swap" size={14} />
          </button>
          <Slot ctx={ctx} slot={1} />
        </div>
        {(a || b) && (
          <div className="board-result">
            {pairHits.length > 0 ? (
              <>
                <span className="eyebrow">{t('mix.pairHits', { count: pairHits.length, pair: [a, b].filter(Boolean).map((x) => gameName(x!)).join(' × ') })}</span>
                <div className="pair-list">
                  {pairHits.map((m) => (
                    <button key={m.id} className="pair-hit" onClick={() => ctx.open(m)}>
                      <b>{m.name}</b>
                      <span>{m.kind === 'passthrough' ? t('card.crossover') : t('mix.mashup')} · {m.by.name}</span>
                    </button>
                  ))}
                  {a && b && (
                    <button className="pair-hit pair-build" onClick={() => ctx.go('build')}>
                      <b>{t('mix.buildOwn')}</b>
                      <span>{t('mix.buildOwnSub')}</span>
                    </button>
                  )}
                </div>
              </>
            ) : (
              <div className="pair-none">
                <span>{t('mix.nobody', { pair: [a, b].filter(Boolean).map((x) => gameName(x!)).join(' × ') })}</span>
                <button className="act act-get" onClick={() => ctx.go('build')}>
                  <Icon name="build" size={15} /> {t('mix.buildIt')}
                </button>
              </div>
            )}
          </div>
        )}
      </section>

      <Section
        title={q ? t('mix.results', { query }) : t('mix.mashups')}
        sub={q ? undefined : filter === 'playable' ? t('mix.playableSub') : undefined}
        aside={
          !q && (
            <div className="seg">
              {([['playable', t('mix.filterPlayable', { count: playableCount })], ['crossover', t('mix.filterCrossover')], ['all', t('mix.filterAll')]] as const).map(([f, l]) => (
                <button key={f} className={filter === f ? 'on' : ''} onClick={() => setFilter(f)}>{l}</button>
              ))}
            </div>
          )
        }
      >
        {community.length ? (
          <div className="grid">{community.map((m, i) => <Card key={m.id} ctx={ctx} m={m} i={i} />)}</div>
        ) : (
          <div className="empty">{byAi.length ? t('mix.emptyCommunity') : t('mix.empty')}</div>
        )}
      </Section>

      {byAi.length > 0 && (
        <Section title={t('mix.byAi')} sub={t('mix.byAiSub')}>
          <div className="grid grid-sm">{byAi.map((m, i) => <Card key={m.id} ctx={ctx} m={m} i={i} />)}</div>
        </Section>
      )}

      {live.length > 0 && !q && (
        <Section title={t('mix.building')} sub={t('mix.buildingSub')} aside={<span className="live-pill"><Icon name="live" size={10} /> {t('mix.liveFrom')}</span>}>
          <div className="rail-row">
            {live.map((x) => (
              <a key={x.ticker} className="agent" href={`https://sigf.ai/agent/${x.ticker}`} target="_blank" rel="noreferrer">
                <div className="agent-img">{imageOk(x.image, privacy) ? <img src={x.image} alt="" /> : <span>${x.ticker}</span>}</div>
                <div>
                  <b>{x.name}</b>
                  <span>{gameName(x.host)} × {x.guest}</span>
                  <em className={x.status === 'building' ? 'on' : ''}>{x.status === 'building' ? t('mix.statusBuilding') : x.status}</em>
                </div>
              </a>
            ))}
          </div>
        </Section>
      )}

      {oneAway.length > 0 && !q && (
        <Section title={t('mix.oneAway')} sub={t('mix.oneAwaySub')}>
          <div className="grid grid-sm">{oneAway.map((m, i) => <Card key={m.id} ctx={ctx} m={m} i={i} />)}</div>
        </Section>
      )}
    </div>
  );
}
